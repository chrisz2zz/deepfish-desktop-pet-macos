use std::fmt::Write as _;

use serde_json::{json, Value};

use crate::chat::SYSTEM_PROMPT;

use super::session::{truncate, AgentMessage, Session};

/// Token budget for the history of one request. Estimates only, but sized so the
/// system prompt, tool schema and reply still fit inside an 8k context window
/// while multi-step tasks keep enough earlier turns to stay coherent.
pub const CONTEXT_BUDGET: usize = 6000;
/// After folding messages the history should shrink well below the budget.
const COMPACT_TARGET: usize = 2400;
/// Upper bound on messages even when they are short, so long tool chains cannot
/// grow the request without touching the token budget first.
const MAX_CONTEXT_MESSAGES: usize = 80;
const MAX_TOOL_CHARS: usize = 600;

const TOOL_GUIDE: &str = "\n你可以调用工具来操作桌宠，并在用户主目录内查看、读取、写入文件或执行命令，不要编造工具结果；\n\
复杂请求先想清楚步骤，一次只调用一个工具，看到结果后再决定下一步；\n\
只读命令（ls/cat/grep/git status 等）会自动执行，其它命令和所有写文件操作都要用户点「允许」，\n\
被拒绝或被安全策略阻止时不要重试同样的调用，换个方式或直接说明做不到；\n\
如果提供了 web_search / web_fetch，遇到时效性或不确定的信息先搜索，引用来源时给出链接，不要编造网页内容；\n\
路径一律写成 ~ 或 ~/ 开头的相对形式；不要读取 .ssh、钥匙串、密钥等凭据文件；\n\
工具执行完后用一两句话回复用户，不要重复工具输出。";

pub const COMPACTION_INSTRUCTION: &str = "把上面的对话压缩成一段不超过 200 字的中文摘要，保留用户的目标、关键结论、未完成事项与称呼偏好；不要编造内容，只输出摘要本身。";

/// Rough token estimate: one token per CJK character and one per four other
/// characters. Cheap, portable, and good enough for budget decisions.
pub fn estimate_tokens(text: &str) -> usize {
    let mut cjk = 0usize;
    let mut other = 0usize;
    for character in text.chars() {
        if ('\u{2E80}'..='\u{9FFF}').contains(&character)
            || ('\u{F900}'..='\u{FAFF}').contains(&character)
            || ('\u{FF00}'..='\u{FFEF}').contains(&character)
        {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    cjk + other.div_ceil(4)
}

pub fn message_tokens(message: &AgentMessage) -> usize {
    let calls: usize = message
        .tool_calls
        .iter()
        .map(|call| estimate_tokens(&call.name) + estimate_tokens(&call.arguments))
        .sum();
    estimate_tokens(&message.content) + calls + 4
}

pub fn history_tokens(session: &Session) -> usize {
    let summary = session.summary.as_deref().map_or(0, estimate_tokens);
    let body: usize = session
        .messages
        .iter()
        .skip(session.compacted)
        .map(message_tokens)
        .sum();
    summary + body
}

/// Returns the new `compacted` offset when the history no longer fits, keeping
/// tool results glued to the assistant call that produced them.
pub fn plan_compaction(session: &Session) -> Option<usize> {
    let total = history_tokens(session);
    if total <= CONTEXT_BUDGET {
        return None;
    }
    let count = session.messages.len();
    let mut end = session.compacted;
    let mut tokens = total;
    while end < count.saturating_sub(1) && tokens > COMPACT_TARGET {
        tokens = tokens.saturating_sub(message_tokens(&session.messages[end]));
        end += 1;
    }
    while end < count && session.messages[end].role == "tool" {
        end += 1;
    }
    if end <= session.compacted {
        None
    } else {
        Some(end)
    }
}

/// `tools == false` is the wrap-up pass: the model must answer in prose, and
/// some models otherwise leak their tool-call markup as plain text.
pub fn system_prompt(tools: bool) -> String {
    if tools {
        format!("{SYSTEM_PROMPT}{TOOL_GUIDE}")
    } else {
        format!(
            "{SYSTEM_PROMPT}（本轮不能再调用工具，请直接用自然语言回答，不要输出任何工具调用标记、伪代码或 XML。）"
        )
    }
}

fn to_openai(message: &AgentMessage) -> Value {
    match message.role.as_str() {
        "assistant" if !message.tool_calls.is_empty() => {
            let calls: Vec<Value> = message
                .tool_calls
                .iter()
                .map(|call| {
                    json!({
                        "id": call.id,
                        "type": "function",
                        "function": { "name": call.name, "arguments": call.arguments }
                    })
                })
                .collect();
            json!({ "role": "assistant", "content": message.content, "tool_calls": calls })
        }
        "tool" => json!({
            "role": "tool",
            "tool_call_id": message.tool_call_id.clone().unwrap_or_default(),
            "content": truncate(&message.content, MAX_TOOL_CHARS)
        }),
        role => json!({ "role": role, "content": message.content }),
    }
}

pub fn build_messages(session: &Session, tools: bool) -> Vec<Value> {
    let mut messages = vec![json!({ "role": "system", "content": system_prompt(tools) })];
    if let Some(summary) = session.summary.as_deref().filter(|text| !text.trim().is_empty()) {
        messages.push(json!({
            "role": "system",
            "content": format!("以下是这段会话更早内容的摘要，供参考：\n{summary}")
        }));
    }

    let history = &session.messages[session.compacted.min(session.messages.len())..];
    let mut slice = &history[history.len().saturating_sub(MAX_CONTEXT_MESSAGES)..];
    // Never start the window on an orphaned tool result.
    while slice.first().is_some_and(|message| message.role == "tool") {
        slice = &slice[1..];
    }
    messages.extend(slice.iter().map(to_openai));
    messages
}

pub fn compaction_prompt(summary: Option<&str>, folded: &[AgentMessage]) -> String {
    let mut prompt = String::new();
    if let Some(summary) = summary.filter(|text| !text.trim().is_empty()) {
        prompt.push_str("已有摘要：\n");
        prompt.push_str(summary);
        prompt.push_str("\n\n");
    }
    prompt.push_str("新增对话：\n");
    for message in folded {
        let role = match message.role.as_str() {
            "user" => "用户",
            "assistant" => "大肥鱼",
            "tool" => "工具结果",
            other => other,
        };
        let _ = writeln!(prompt, "{role}：{}", truncate(&message.content, 300));
    }
    prompt.push('\n');
    prompt.push_str(COMPACTION_INSTRUCTION);
    prompt
}

/// Deterministic digest used when the summarising request fails, so the context
/// still shrinks instead of growing without limit while offline.
pub fn fallback_summary(folded: &[AgentMessage]) -> String {
    let mut digest = String::new();
    for message in folded {
        if message.role == "tool" {
            continue;
        }
        let prefix = if message.role == "user" { "用户说" } else { "我回应" };
        let _ = write!(digest, "{prefix}：{}；", truncate(&message.content, 50));
        if estimate_tokens(&digest) > 150 {
            break;
        }
    }
    if digest.is_empty() {
        "（早期对话没有可保留的内容）".to_string()
    } else {
        format!("（早期对话摘要）{digest}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::session::ToolCall;

    fn session_with(messages: usize) -> Session {
        let mut session = Session::new(None);
        for index in 0..messages {
            let role = if index % 2 == 0 { "user" } else { "assistant" };
            session.push(role, format!("第 {index} 条消息，内容大概这么长，用来占一点上下文。"), vec![], None, None, None);
        }
        session
    }

    #[test]
    fn estimates_cjk_and_ascii_tokens() {
        assert_eq!(estimate_tokens("你好世界"), 4);
        assert_eq!(estimate_tokens("hello world"), 3);
        assert!(estimate_tokens("混合 mixed 文本") > 4);
    }

    #[test]
    fn keeps_short_sessions_intact() {
        let session = session_with(4);
        assert_eq!(plan_compaction(&session), None);
    }

    #[test]
    fn folds_old_messages_when_over_budget() {
        let session = session_with(300);
        let end = plan_compaction(&session).expect("compaction planned");
        assert!(end > 0);
        assert!(end < session.messages.len());
        let remaining: usize = session.messages[end..].iter().map(message_tokens).sum();
        assert!(remaining < history_tokens(&session));
    }

    #[test]
    fn never_folds_the_last_message() {
        let mut session = session_with(2);
        session.compacted = 0;
        session.messages[1].content = "长".repeat(CONTEXT_BUDGET * 4);
        let end = plan_compaction(&session).unwrap();
        assert_eq!(end, 1);
    }

    #[test]
    fn moves_the_boundary_past_trailing_tool_results() {
        let mut session = session_with(40);
        let call = ToolCall::new("call_1", "get_time", "{}");
        session.messages.truncate(20);
        session.push("assistant", String::new(), vec![call], None, None, None);
        session.push("tool", "现在是 10:00".to_string(), vec![], Some("call_1".into()), Some("get_time".into()), Some("🕒 查看时间".into()));
        for index in 0..220 {
            session.push(
                "user",
                format!("补内容 {index} 把上下文预算撑满").repeat(3),
                vec![],
                None,
                None,
                None,
            );
        }
        let end = plan_compaction(&session).unwrap();
        assert_ne!(session.messages[end].role, "tool");
    }

    #[test]
    fn builds_context_with_summary_and_tool_calls() {
        let mut session = session_with(3);
        session.summary = Some("之前聊过早起".to_string());
        let call = ToolCall::new("call_9", "get_time", "{}");
        session.push("assistant", String::new(), vec![call], None, None, None);
        session.push("tool", "10:00".to_string(), vec![], Some("call_9".into()), Some("get_time".into()), None);

        let messages = build_messages(&session, true);
        assert_eq!(messages[0]["role"], "system");
        assert!(messages[0]["content"].as_str().unwrap().contains("工具"));
        assert!(messages[1]["content"].as_str().unwrap().contains("早起"));
        let assistant = messages.iter().find(|message| message["role"] == "assistant" && message.get("tool_calls").is_some()).unwrap();
        assert_eq!(assistant["tool_calls"][0]["function"]["name"], "get_time");
        let tool = messages.last().unwrap();
        assert_eq!(tool["role"], "tool");
        assert_eq!(tool["tool_call_id"], "call_9");
    }

    #[test]
    fn fallback_summary_stays_short() {
        let session = session_with(6);
        let digest = fallback_summary(&session.messages);
        assert!(digest.starts_with("（早期对话摘要）"));
        assert!(estimate_tokens(&digest) <= 200);
    }

    #[test]
    fn compaction_prompt_mentions_previous_summary() {
        let session = session_with(2);
        let prompt = compaction_prompt(Some("旧摘要"), &session.messages);
        assert!(prompt.contains("旧摘要"));
        assert!(prompt.contains("用户："));
    }
}
