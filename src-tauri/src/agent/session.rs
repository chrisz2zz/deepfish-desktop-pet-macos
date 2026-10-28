use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const MAX_SESSIONS: usize = 40;
const MAX_MESSAGES: usize = 400;
const TITLE_LIMIT: usize = 18;

static SESSION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

impl ToolCall {
    pub fn new(id: impl Into<String>, name: impl Into<String>, arguments: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMessage {
    pub id: u64,
    pub role: String,
    pub content: String,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Short label rendered as a chip for tool results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub messages: Vec<AgentMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Number of leading messages already folded into `summary`.
    #[serde(default)]
    pub compacted: usize,
    #[serde(default)]
    pub next_message_id: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub updated_at: i64,
    pub message_count: usize,
    pub preview: String,
}

impl Session {
    pub fn new(title: Option<String>) -> Self {
        let now = now_ms();
        Self {
            id: format!(
                "s{now}-{}",
                SESSION_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ),
            title: title
                .map(|title| title.trim().to_string())
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| "新对话".to_string()),
            created_at: now,
            updated_at: now,
            messages: Vec::new(),
            summary: None,
            compacted: 0,
            next_message_id: 1,
        }
    }

    pub fn summary(&self) -> SessionSummary {
        let preview = self
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "user" || message.role == "assistant")
            .map(|message| truncate(&message.content, 36))
            .unwrap_or_default();
        SessionSummary {
            id: self.id.clone(),
            title: self.title.clone(),
            updated_at: self.updated_at,
            message_count: self.messages.len(),
            preview,
        }
    }

    pub fn push(
        &mut self,
        role: &str,
        content: String,
        tool_calls: Vec<ToolCall>,
        tool_call_id: Option<String>,
        name: Option<String>,
        display: Option<String>,
    ) -> AgentMessage {
        let id = self.next_message_id;
        self.next_message_id += 1;
        let message = AgentMessage {
            id,
            role: role.to_string(),
            content,
            created_at: now_ms(),
            tool_calls,
            tool_call_id,
            name,
            display,
        };
        if role == "user" && self.messages.iter().all(|item| item.role != "user") {
            self.title = truncate(&message.content, TITLE_LIMIT);
        }
        self.messages.push(message.clone());
        self.updated_at = message.created_at;
        self.trim();
        message
    }

    pub fn apply_compaction(&mut self, summary: String, compacted: usize) {
        self.summary = Some(summary);
        self.compacted = compacted.min(self.messages.len());
        self.updated_at = now_ms();
    }

    fn trim(&mut self) {
        if self.messages.len() > MAX_MESSAGES {
            let drop = self.messages.len() - MAX_MESSAGES;
            self.messages.drain(..drop);
            self.compacted = self.compacted.saturating_sub(drop);
        }
    }
}

pub fn truncate(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    let head: String = trimmed.chars().take(limit).collect();
    format!("{head}…")
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct SessionsFile {
    version: u32,
    active: Option<String>,
    sessions: Vec<Session>,
}

/// Session history lives in one JSON document next to the settings so the agent
/// keeps its memory across restarts without a database dependency.
pub struct SessionStore {
    path: PathBuf,
    data: SessionsFile,
}

impl SessionStore {
    pub fn load(directory: &Path) -> Self {
        let path = directory.join("sessions.json");
        let data = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<SessionsFile>(&text).ok())
            .unwrap_or_default();
        Self { path, data }
    }

    pub fn persist(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("无法创建会话目录：{error}"))?;
        }
        let json = serde_json::to_string_pretty(&self.data)
            .map_err(|error| format!("无法序列化会话：{error}"))?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, json).map_err(|error| format!("无法写入会话：{error}"))?;
        fs::rename(&temporary, &self.path).map_err(|error| format!("无法保存会话：{error}"))
    }

    pub fn list(&self) -> Vec<SessionSummary> {
        let mut sessions: Vec<SessionSummary> = self.data.sessions.iter().map(Session::summary).collect();
        sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        sessions
    }

    pub fn set_active(&mut self, id: &str) {
        self.data.active = Some(id.to_string());
    }

    pub fn get(&self, id: &str) -> Option<&Session> {
        self.data.sessions.iter().find(|session| session.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Result<&mut Session, String> {
        self.data
            .sessions
            .iter_mut()
            .find(|session| session.id == id)
            .ok_or_else(|| "会话不存在".to_string())
    }

    pub fn open(&mut self, id: Option<&str>) -> Result<Session, String> {
        let target = id
            .map(str::to_string)
            .or_else(|| self.data.active.clone())
            .or_else(|| self.list().first().map(|summary| summary.id.clone()));

        if let Some(target) = target {
            if let Some(session) = self.get(&target) {
                let snapshot = session.clone();
                self.data.active = Some(target);
                return Ok(snapshot);
            }
        }
        self.create(None)
    }

    pub fn create(&mut self, title: Option<String>) -> Result<Session, String> {
        let session = Session::new(title);
        self.data.active = Some(session.id.clone());
        self.data.sessions.push(session.clone());
        while self.data.sessions.len() > MAX_SESSIONS {
            let oldest = self
                .data
                .sessions
                .iter()
                .enumerate()
                .min_by_key(|(_, item)| item.updated_at)
                .map(|(index, _)| index)
                .unwrap_or(0);
            self.data.sessions.remove(oldest);
        }
        self.persist()?;
        Ok(session)
    }

    pub fn rename(&mut self, id: &str, title: &str) -> Result<(), String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("标题不能为空".to_string());
        }
        let session = self.get_mut(id)?;
        session.title = truncate(title, 30);
        session.updated_at = now_ms();
        self.persist()
    }

    pub fn delete(&mut self, id: &str) -> Result<Vec<SessionSummary>, String> {
        self.data.sessions.retain(|session| session.id != id);
        if self.data.active.as_deref() == Some(id) {
            self.data.active = self.list().first().map(|summary| summary.id.clone());
        }
        self.persist()?;
        Ok(self.list())
    }

    pub fn save(&self) -> Result<(), String> {
        self.persist()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("deepfish-agent-{name}-{}", now_ms()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn creates_and_round_trips_sessions() {
        let dir = temp_dir("roundtrip");
        let mut store = SessionStore::load(&dir);
        let created = store.create(None).unwrap();
        store
            .get_mut(&created.id)
            .unwrap()
            .push("user", "帮我安排一下今天的工作".to_string(), vec![], None, None, None);
        store.save().unwrap();

        let mut reloaded = SessionStore::load(&dir);
        let session = reloaded.get(&created.id).unwrap();
        assert_eq!(session.messages.len(), 1);
        assert_eq!(session.title, "帮我安排一下今天的工作");
        assert_eq!(reloaded.open(None).unwrap().id, created.id);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn deletes_sessions_and_falls_back_to_the_next_one() {
        let dir = temp_dir("delete");
        let mut store = SessionStore::load(&dir);
        let first = store.create(Some("第一个".into())).unwrap();
        let second = store.create(Some("第二个".into())).unwrap();
        let remaining = store.delete(&second.id).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, first.id);
        assert_eq!(store.open(None).unwrap().id, first.id);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn trims_messages_and_keeps_the_compaction_offset_valid() {
        let mut session = Session::new(None);
        session.compacted = 5;
        for index in 0..(MAX_MESSAGES + 20) {
            session.push("user", format!("消息 {index}"), vec![], None, None, None);
        }
        assert_eq!(session.messages.len(), MAX_MESSAGES);
        assert_eq!(session.compacted, 0);
    }

    #[test]
    fn derives_a_title_from_the_first_user_message() {
        let mut session = Session::new(None);
        session.push("user", "帮我写一段很长的自我介绍文案，要温柔一点".to_string(), vec![], None, None, None);
        assert_eq!(session.title, truncate("帮我写一段很长的自我介绍文案，要温柔一点", TITLE_LIMIT));
    }
}
