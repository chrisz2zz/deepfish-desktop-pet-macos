pub mod context;
pub mod risk;
pub mod sandbox;
pub mod session;
pub mod tools;
pub mod web;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

use crate::chat;
use crate::settings::Settings;
use crate::AppState;

use session::{AgentMessage, Session, SessionStore, SessionSummary, ToolCall};

/// How long a risky tool waits for the user before it is treated as denied.
/// Background runs can sit here a long time while the pet shows a notice bubble.
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_REPLY_TOKENS: u32 = 400;
const SUMMARY_TOKENS: u32 = 320;

/// One message to append to a session. Keeps call sites readable while the
/// persistence layer keeps a flat `push` signature.
struct NewMessage<'a> {
    role: &'a str,
    content: String,
    tool_calls: Vec<ToolCall>,
    tool_call_id: Option<String>,
    name: Option<String>,
    display: Option<String>,
}

impl<'a> NewMessage<'a> {
    fn text(role: &'a str, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            name: None,
            display: None,
        }
    }

    fn assistant(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            tool_calls,
            ..Self::text("assistant", content)
        }
    }

    fn tool(call: &ToolCall, content: impl Into<String>, display: impl Into<String>) -> Self {
        Self {
            role: "tool",
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(call.id.clone()),
            name: Some(call.name.clone()),
            display: Some(display.into()),
        }
    }
}

/// How often debounced session changes reach the disk. Short enough that a
/// crash loses at most the last moment of a conversation.
const FLUSH_INTERVAL: Duration = Duration::from_millis(700);

/// Writes the session file in the background so a long tool-heavy run does not
/// rewrite the whole document on every message.
fn spawn_flusher(store: Arc<Mutex<SessionStore>>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(FLUSH_INTERVAL);
        let mut guard = store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Err(error) = guard.flush() {
            eprintln!("无法写入会话：{error}");
        }
    });
}

/// An approval card the renderer has not answered yet.
type PendingApproval = (String, tokio::sync::oneshot::Sender<bool>);

struct CompactionPlan {
    expected: usize,
    end: usize,
    prompt: String,
}

/// The agent harness: owns conversation memory, decides when to compress it,
/// and drives the tool loop. Runs are keyed by session so switching or closing
/// the chat panel never interrupts work that is already in flight.
pub struct AgentRuntime {
    store: Arc<Mutex<SessionStore>>,
    runs: Mutex<HashMap<String, u64>>,
    /// Pending approvals keyed by tool call id. The runtime awaits the receiver
    /// while the renderer shows an approval card.
    approvals: Mutex<HashMap<String, PendingApproval>>,
}

impl AgentRuntime {
    pub fn new(directory: &Path) -> Self {
        let store = Arc::new(Mutex::new(SessionStore::load(directory)));
        spawn_flusher(Arc::clone(&store));
        Self {
            store,
            runs: Mutex::new(HashMap::new()),
            approvals: Mutex::new(HashMap::new()),
        }
    }

    fn approvals(&self) -> MutexGuard<'_, HashMap<String, PendingApproval>> {
        self.approvals
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn request_approval(
        &self,
        session_id: &str,
        call_id: &str,
    ) -> tokio::sync::oneshot::Receiver<bool> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.approvals()
            .insert(call_id.to_string(), (session_id.to_string(), sender));
        receiver
    }

    fn forget_approval(&self, call_id: &str) {
        self.approvals().remove(call_id);
    }

    /// Returns whether a pending approval was waiting for this decision.
    pub fn decide(&self, call_id: &str, allow: bool) -> bool {
        match self.approvals().remove(call_id) {
            Some((_, sender)) => sender.send(allow).is_ok(),
            None => false,
        }
    }

    /// Cancelling a run must also unblock any tool that is waiting for the user.
    fn deny_pending_approvals(&self, session_id: &str) {
        let mut approvals = self.approvals();
        let stale: Vec<String> = approvals
            .iter()
            .filter(|(_, (owner, _))| owner == session_id)
            .map(|(call_id, _)| call_id.clone())
            .collect();
        for call_id in stale {
            if let Some((_, sender)) = approvals.remove(&call_id) {
                let _ = sender.send(false);
            }
        }
    }

    /// Writes any debounced session changes. Called on exit and after a run.
    pub fn flush(&self) {
        let mut store = self.store();
        if let Err(error) = store.flush() {
            eprintln!("无法写入会话：{error}");
        }
    }

    fn store(&self) -> MutexGuard<'_, SessionStore> {
        self.store
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn runs(&self) -> MutexGuard<'_, HashMap<String, u64>> {
        self.runs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn sessions(&self) -> Vec<SessionSummary> {
        self.store().list()
    }

    pub fn open(&self, id: Option<&str>) -> Result<Session, String> {
        self.store().open(id)
    }

    pub fn exists(&self, id: &str) -> bool {
        self.store().get(id).is_some()
    }

    pub fn create(&self, title: Option<String>) -> Result<Session, String> {
        self.store().create(title)
    }

    pub fn rename(&self, id: &str, title: &str) -> Result<(), String> {
        self.store().rename(id, title)
    }

    pub fn delete(&self, id: &str) -> Result<Vec<SessionSummary>, String> {
        self.runs().remove(id);
        self.deny_pending_approvals(id);
        self.store().delete(id)
    }

    fn push_message(&self, id: &str, message: NewMessage<'_>) -> Result<AgentMessage, String> {
        let mut store = self.store();
        let saved = store.get_mut(id)?.push(
            message.role,
            message.content,
            message.tool_calls,
            message.tool_call_id,
            message.name,
            message.display,
        );
        store.set_active(id);
        // Messages are the hot path, so they only mark the store dirty; the
        // flusher thread writes them out shortly after.
        store.touch();
        Ok(saved)
    }

    fn context(&self, id: &str, tools: bool) -> Result<Vec<Value>, String> {
        let store = self.store();
        let session = store.get(id).ok_or_else(|| "会话不存在".to_string())?;
        Ok(context::build_messages(session, tools))
    }

    fn begin(&self, id: &str, request_id: u64) {
        self.runs().insert(id.to_string(), request_id);
    }

    fn finish(&self, id: &str, request_id: u64) {
        let mut runs = self.runs();
        if runs.get(id) == Some(&request_id) {
            runs.remove(id);
        }
    }

    pub fn cancel(&self, id: &str) {
        self.runs().remove(id);
        self.deny_pending_approvals(id);
    }

    fn is_cancelled(&self, id: &str, request_id: u64) -> bool {
        self.runs().get(id) != Some(&request_id)
    }

    fn compaction_plan(&self, id: &str) -> Result<Option<CompactionPlan>, String> {
        let store = self.store();
        let session = store.get(id).ok_or_else(|| "会话不存在".to_string())?;
        let Some(end) = context::plan_compaction(session) else {
            return Ok(None);
        };
        let prompt = context::compaction_prompt(
            session.summary.as_deref(),
            &session.messages[session.compacted..end],
        );
        Ok(Some(CompactionPlan {
            expected: session.compacted,
            end,
            prompt,
        }))
    }

    fn digest(&self, id: &str, end: usize) -> Result<String, String> {
        let store = self.store();
        let session = store.get(id).ok_or_else(|| "会话不存在".to_string())?;
        let end = end.min(session.messages.len());
        Ok(context::fallback_summary(
            &session.messages[session.compacted.min(end)..end],
        ))
    }

    fn apply_compaction(
        &self,
        id: &str,
        expected: usize,
        end: usize,
        summary: String,
    ) -> Result<bool, String> {
        let mut store = self.store();
        let session = store.get_mut(id)?;
        if session.compacted != expected {
            return Ok(false);
        }
        session.apply_compaction(summary, end);
        store.save()?;
        Ok(true)
    }
}

fn emit(app: &AppHandle, session_id: &str, request_id: u64, mut payload: Value) {
    if let Some(map) = payload.as_object_mut() {
        map.insert("sessionId".to_string(), json!(session_id));
        map.insert("requestId".to_string(), json!(request_id));
    }
    let _ = app.emit("agent-event", payload);
}

fn require_key(settings: &Settings, api_key: Option<&str>) -> Result<(), String> {
    if settings.provider == "deepseek" && api_key.is_none() {
        return Err("请先在设置中填写 DeepSeek API Key".to_string());
    }
    Ok(())
}

#[tauri::command]
pub fn agent_sessions(state: State<'_, AppState>) -> Vec<SessionSummary> {
    state.agent.sessions()
}

#[tauri::command]
pub fn agent_open_session(
    state: State<'_, AppState>,
    session_id: Option<String>,
) -> Result<Session, String> {
    state.agent.open(session_id.as_deref())
}

#[tauri::command]
pub fn agent_new_session(
    state: State<'_, AppState>,
    title: Option<String>,
) -> Result<Session, String> {
    state.agent.create(title)
}

#[tauri::command]
pub fn agent_rename_session(
    state: State<'_, AppState>,
    session_id: String,
    title: String,
) -> Result<(), String> {
    state.agent.rename(&session_id, &title)
}

#[tauri::command]
pub fn agent_delete_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<SessionSummary>, String> {
    state.agent.delete(&session_id)
}

#[tauri::command]
pub fn agent_cancel(state: State<'_, AppState>, session_id: String) {
    state.agent.cancel(&session_id);
}

/// Resolves a pending approval card. Returns false when the call already timed
/// out or the session was cancelled, so the renderer can stop waiting.
#[tauri::command]
pub fn agent_approval(state: State<'_, AppState>, call_id: String, allow: bool) -> bool {
    state.agent.decide(&call_id, allow)
}

#[tauri::command]
pub async fn agent_compact(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<bool, String> {
    let (settings, api_key) = crate::chat_config(&state);
    require_key(&settings, api_key.as_deref())?;
    compact(&app, &state.agent, &settings, api_key.as_deref(), &session_id).await
}

#[tauri::command]
pub async fn agent_send(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    request_id: u64,
    text: String,
) -> Result<(), String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("消息不能为空".to_string());
    }
    let (settings, api_key) = crate::chat_config(&state);
    require_key(&settings, api_key.as_deref())?;
    if !state.agent.exists(&session_id) {
        return Err("会话不存在".to_string());
    }
    run(&app, &state.agent, settings, api_key, session_id, request_id, text).await
}

/// Drives one user question to completion. The caller is responsible for
/// cleanup and for the terminal events, so an early `?` here can never leave a
/// session marked as running.
#[allow(clippy::too_many_arguments)]
async fn run_loop(
    app: &AppHandle,
    runtime: &AgentRuntime,
    settings: &Settings,
    api_key: Option<&str>,
    session_id: &str,
    request_id: u64,
    text: String,
) -> Result<(), String> {
    let user_message = runtime.push_message(session_id, NewMessage::text("user", text))?;
    emit(
        app,
        session_id,
        request_id,
        json!({ "type": "message", "message": user_message }),
    );
    runtime.begin(session_id, request_id);
    emit(
        app,
        session_id,
        request_id,
        json!({ "type": "status", "state": "running", "step": 0 }),
    );

    let mut step = 0usize;
    let mut tool_calls_used = 0usize;
    let loop_limit = settings.agent_loop_limit as usize;
    let call_limit = settings.tool_call_limit as usize;
    let mut budget_notes: Vec<String> = Vec::new();

    loop {
        if runtime.is_cancelled(session_id, request_id) {
            break;
        }
        if step > loop_limit {
            break;
        }
        // The last pass runs without tools so the model has to wrap up in words,
        // and so does every pass once the tool budget is spent.
        let use_tools = step < loop_limit && tool_calls_used < call_limit;
        let messages = runtime.context(session_id, use_tools)?;
        // DeepSeek's web tools need its own key, so they are only registered when
        // the user switched them on and the provider is DeepSeek.
        let deepseek = settings.provider == "deepseek";
        let tool_context = tools::ToolContext::new(
            risk::tool_root(),
            api_key.map(str::to_string),
            deepseek && settings.deepseek_search,
            deepseek && settings.deepseek_fetch,
            settings.sandboxed_commands,
        );
        let tools = use_tools.then(|| tools::specs(&tool_context));
        let body = chat::payload(settings, messages, tools, true, MAX_REPLY_TOKENS);
        let outcome = chat::stream_completion(
            settings,
            api_key,
            body,
            || runtime.is_cancelled(session_id, request_id),
            |delta| {
                emit(
                    app,
                    session_id,
                    request_id,
                    json!({ "type": "delta", "text": delta }),
                );
            },
        )
        .await;

        let outcome = match outcome {
            Ok(outcome) => outcome,
            // A cancelled run stops quietly; a real failure is reported by the
            // caller after it has cleaned up.
            Err(_) if runtime.is_cancelled(session_id, request_id) => return Ok(()),
            Err(error) => return Err(error),
        };

        let calls: Vec<ToolCall> = outcome
            .tool_calls
            .iter()
            .map(|call| ToolCall::new(call.id.clone(), call.name.clone(), call.arguments.clone()))
            .collect();
        // Guards against control markup leaking into the bubble on tool-free passes.
        let content = chat::strip_tool_markup(&outcome.content);
        let content = if content.is_empty() && !outcome.content.trim().is_empty() {
            "（这轮模型只输出了工具调用标记，没有有效回复，可以再问一次）".to_string()
        } else {
            content
        };
        let message =
            runtime.push_message(session_id, NewMessage::assistant(content, calls.clone()))?;
        emit(
            app,
            session_id,
            request_id,
            json!({ "type": "message", "message": message }),
        );

        if calls.is_empty() {
            break;
        }

        let root = tool_context.root.clone();
        for call in &calls {
            if runtime.is_cancelled(session_id, request_id) {
                break;
            }
            // Every emitted call needs a tool message, so over-budget calls are
            // answered without running anything.
            if tool_calls_used >= call_limit {
                if !budget_notes.iter().any(|note| note.contains("工具调用次数")) {
                    budget_notes.push(format!(
                        "本次问答的工具调用次数已达上限（{call_limit} 次），剩余的调用没有执行。"
                    ));
                }
                let refused = tools::ToolOutcome::budget_exhausted(&call.name, call_limit);
                let tool_message = runtime.push_message(
                    session_id,
                    NewMessage::tool(call, refused.content, refused.display),
                )?;
                emit(
                    app,
                    session_id,
                    request_id,
                    json!({ "type": "message", "message": tool_message }),
                );
                continue;
            }
            tool_calls_used += 1;
            let plan = risk::plan(&root, call, settings.sandboxed_commands);
            let blocked = matches!(plan.risk, risk::Risk::Blocked);
            let mut denied = false;
            if plan.risk != risk::Risk::Safe {
                emit(
                    app,
                    session_id,
                    request_id,
                    json!({
                        "type": "approval",
                        "callId": call.id,
                        "tool": call.name,
                        "display": tools::label(&call.name),
                        "risk": plan.risk.as_str(),
                        "summary": plan.summary,
                        "timeout": APPROVAL_TIMEOUT.as_secs(),
                    }),
                );
            }
            if plan.risk == risk::Risk::NeedsApproval {
                let receiver = runtime.request_approval(session_id, &call.id);
                // A dropped sender (session deleted, run cancelled) or a timeout
                // both mean "no consent".
                denied = !matches!(
                    tokio::time::timeout(APPROVAL_TIMEOUT, receiver).await,
                    Ok(Ok(true))
                );
                runtime.forget_approval(&call.id);
            }

            let result = if blocked {
                tools::ToolOutcome::refused(&call.name, plan.summary)
            } else if denied {
                tools::ToolOutcome::denied(&call.name, &plan.summary)
            } else {
                emit(
                    app,
                    session_id,
                    request_id,
                    json!({ "type": "tool", "status": "running", "name": call.name, "display": tools::label(&call.name) }),
                );
                let tool_app = app.clone();
                let tool_context = tool_context.clone();
                let tool_call = call.clone();
                match tauri::async_runtime::spawn_blocking(move || {
                    tools::execute(&tool_app, &tool_context, &tool_call)
                })
                .await
                {
                    Ok(outcome) => outcome,
                    Err(error) => tools::ToolOutcome::refused(
                        &call.name,
                        format!("工具执行线程异常：{error}"),
                    ),
                }
            };
            emit(
                app,
                session_id,
                request_id,
                json!({
                    "type": "approval-result",
                    "callId": call.id,
                    "decision": if blocked { "blocked" } else if denied { "denied" } else { "done" }
                }),
            );
            if let tools::ToolEffect::RenameSession(title) = result.effect {
                let _ = runtime.rename(session_id, &title);
                emit(
                    app,
                    session_id,
                    request_id,
                    json!({ "type": "renamed", "title": title }),
                );
            }
            let tool_message = runtime.push_message(
                session_id,
                NewMessage::tool(call, result.content, result.display),
            )?;
            emit(
                app,
                session_id,
                request_id,
                json!({ "type": "message", "message": tool_message }),
            );
        }

        step += 1;
        if step >= loop_limit && tool_calls_used < call_limit {
            budget_notes.push(format!(
                "本次问答已达循环上限（{loop_limit} 轮），正在收尾。"
            ));
        }
        emit(
            app,
            session_id,
            request_id,
            json!({ "type": "status", "state": "running", "step": step }),
        );
    }

    for note in &budget_notes {
        emit(
            app,
            session_id,
            request_id,
            json!({ "type": "note", "level": "warn", "text": note }),
        );
    }
    Ok(())
}

/// Public entry point: runs the loop, then always clears the run and reports how
/// it ended (the renderer turns that into a background-task notice).
async fn run(
    app: &AppHandle,
    runtime: &AgentRuntime,
    settings: Settings,
    api_key: Option<String>,
    session_id: String,
    request_id: u64,
    text: String,
) -> Result<(), String> {
    let outcome = run_loop(
        app,
        runtime,
        &settings,
        api_key.as_deref(),
        &session_id,
        request_id,
        text,
    )
    .await;

    let cancelled = runtime.is_cancelled(&session_id, request_id);
    runtime.finish(&session_id, request_id);
    emit(
        app,
        &session_id,
        request_id,
        json!({ "type": "status", "state": "idle" }),
    );
    // One terminal event per run: the renderer turns it into a notice bubble
    // when the chat is closed, i.e. when the task ran in the background.
    emit(
        app,
        &session_id,
        request_id,
        json!({
            "type": "run-end",
            "outcome": if cancelled { "cancelled" } else if outcome.is_err() { "error" } else { "ok" },
            "message": outcome.as_ref().err(),
        }),
    );

    if let Err(error) = &outcome {
        emit(
            app,
            &session_id,
            request_id,
            json!({ "type": "error", "message": error }),
        );
    }

    if !cancelled {
        let _ = compact(app, runtime, &settings, api_key.as_deref(), &session_id).await;
    }
    runtime.flush();
    outcome
}

/// Folds the oldest turns into a summary. Falls back to a deterministic digest
/// when the summarising request fails, so the context never grows unbounded.
async fn compact(
    app: &AppHandle,
    runtime: &AgentRuntime,
    settings: &Settings,
    api_key: Option<&str>,
    session_id: &str,
) -> Result<bool, String> {
    let Some(plan) = runtime.compaction_plan(session_id)? else {
        return Ok(false);
    };
    let body = chat::payload(
        settings,
        vec![json!({ "role": "user", "content": plan.prompt })],
        None,
        false,
        SUMMARY_TOKENS,
    );
    let summarized = match chat::completion(settings, api_key, body).await {
        Ok(value) => chat::message_content(&value),
        Err(_) => String::new(),
    };
    let summary = if summarized.trim().is_empty() {
        runtime.digest(session_id, plan.end)?
    } else {
        summarized
    };
    let applied = runtime.apply_compaction(session_id, plan.expected, plan.end, summary.clone())?;
    if applied {
        emit(
            app,
            session_id,
            0,
            json!({ "type": "compacted", "compacted": plan.end, "summary": summary }),
        );
    }
    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "deepfish-runtime-{name}-{}",
            session::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn flushes_message_appends_in_the_background() {
        let dir = temp_dir("flush");
        let runtime = AgentRuntime::new(&dir);
        let session = runtime.create(None).unwrap();
        let path = dir.join("sessions.json");
        let before = std::fs::read_to_string(&path).unwrap();

        runtime
            .push_message(&session.id, NewMessage::text("user", "后台写入的内容"))
            .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

        // The flusher ticks every 700 ms, so this is comfortably inside the
        // window while still proving the write is debounced, not synchronous.
        std::thread::sleep(FLUSH_INTERVAL * 2);
        assert!(std::fs::read_to_string(&path).unwrap().contains("后台写入的内容"));

        runtime.flush();
        std::fs::remove_dir_all(dir).ok();
    }
}
