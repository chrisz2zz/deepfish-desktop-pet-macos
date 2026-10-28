use std::time::Duration;

use serde_json::{json, Value};

use crate::settings::Settings;

pub const SYSTEM_PROMPT: &str = "你是桌面宠物大肥鱼，一只蓝色、聪明但偶尔装傻的二次元小鲸鱼女仆。\n用简短自然的中文回答，通常不超过 120 字。\n语气温柔、有一点机灵的吐槽，不要自称 DeepSeek 官方角色。\n遇到严肃问题时优先准确和有帮助，不要为了卖萌牺牲事实。";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(25);
const STREAM_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug)]
pub struct ChatRequest {
    pub endpoint: String,
    pub authorization: Option<String>,
    pub body: Value,
}

/// One streamed fragment of a tool call. OpenAI compatible endpoints split the
/// name and the JSON arguments over several events, so fragments are merged.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolCallDelta {
    pub index: usize,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct ChatDelta {
    pub text: Option<String>,
    pub tool_calls: Vec<ToolCallDelta>,
}

#[derive(Debug, Default)]
pub struct StreamOutcome {
    pub content: String,
    pub tool_calls: Vec<AssembledToolCall>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct AssembledToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

pub fn normalize_endpoint(endpoint: &str) -> String {
    endpoint.trim().trim_end_matches('/').to_string()
}

pub fn build_request(
    settings: &Settings,
    api_key: Option<&str>,
    body: Value,
) -> Result<ChatRequest, String> {
    let endpoint = normalize_endpoint(&settings.endpoint);
    if !endpoint.to_ascii_lowercase().starts_with("https://") {
        return Err("API 地址必须使用 HTTPS".to_string());
    }

    Ok(ChatRequest {
        endpoint,
        authorization: api_key
            .filter(|key| !key.is_empty())
            .map(|key| format!("Bearer {key}")),
        body,
    })
}

/// Builds an OpenAI compatible chat body. `tools` is the JSON schema array that
/// enables function calling; providers that do not support it ignore the field.
pub fn payload(
    settings: &Settings,
    messages: Vec<Value>,
    tools: Option<Value>,
    stream: bool,
    max_tokens: u32,
) -> Value {
    let mut body = json!({
        "model": settings.model,
        "messages": messages,
        "temperature": 0.8,
        "max_tokens": max_tokens,
        "stream": stream
    });
    if let Some(tools) = tools {
        body["tools"] = tools;
        body["tool_choice"] = json!("auto");
    }
    body
}

fn error_detail(payload: &Value, status: reqwest::StatusCode) -> String {
    payload
        .pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| payload.get("message").and_then(Value::as_str))
        .map_or_else(|| format!("HTTP {}", status.as_u16()), str::to_string)
}

pub fn message_content(payload: &Value) -> String {
    payload
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn assemble(index: usize, id: Option<&str>, name: Option<&str>) -> AssembledToolCall {
    AssembledToolCall {
        id: id.map_or_else(|| format!("call_{index}"), str::to_string),
        name: name.unwrap_or_default().to_string(),
        arguments: String::new(),
    }
}

/// Extracts text and tool-call fragments from one `choices[0].delta` payload,
/// tolerating providers that stream a whole message or plain text instead.
pub fn parse_delta(payload: &str) -> Option<ChatDelta> {
    let value: Value = serde_json::from_str(payload).ok()?;
    let choice = value.pointer("/choices/0")?;

    let text = choice
        .pointer("/delta/content")
        .and_then(Value::as_str)
        .or_else(|| choice.pointer("/message/content").and_then(Value::as_str))
        .or_else(|| choice.get("text").and_then(Value::as_str))
        .filter(|text| !text.is_empty())
        .map(str::to_string);

    let mut tool_calls = Vec::new();
    let streamed = choice
        .pointer("/delta/tool_calls")
        .or_else(|| choice.pointer("/message/tool_calls"))
        .and_then(Value::as_array);
    for item in streamed.into_iter().flatten() {
        tool_calls.push(ToolCallDelta {
            index: item.get("index").and_then(Value::as_u64).unwrap_or(0) as usize,
            id: item.get("id").and_then(Value::as_str).map(str::to_string),
            name: item
                .pointer("/function/name")
                .and_then(Value::as_str)
                .map(str::to_string),
            arguments: item
                .pointer("/function/arguments")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }

    if text.is_none() && tool_calls.is_empty() {
        return None;
    }
    Some(ChatDelta { text, tool_calls })
}

/// Some providers (deepseek-flash among them) leak their internal tool-call
/// markup as plain text when tools are disabled. Everything from the first
/// marker on is control junk, so the reply keeps only the prose before it.
pub fn strip_tool_markup(text: &str) -> String {
    const MARKERS: [&str; 5] = [
        "<｜｜DSML｜｜",
        "<|DSML|",
        "｜｜DSML｜｜",
        "<|tool_call|",
        "<|tool▁calls▁begin|",
    ];
    let cut = MARKERS
        .iter()
        .filter_map(|marker| text.find(marker))
        .min()
        .unwrap_or(text.len());
    text[..cut].trim().to_string()
}

/// Merges one streamed fragment into the assembled call list. Arguments and
/// names arrive split over many events, and calls may start out of order.
pub fn merge_tool_delta(calls: &mut Vec<AssembledToolCall>, fragment: ToolCallDelta) {
    while calls.len() <= fragment.index {
        let index = calls.len();
        calls.push(assemble(index, None, None));
    }
    let call = &mut calls[fragment.index];
    if let Some(id) = fragment.id {
        call.id = id;
    }
    if let Some(name) = fragment.name {
        call.name.push_str(&name);
    }
    if let Some(arguments) = fragment.arguments {
        call.arguments.push_str(&arguments);
    }
}

/// Incremental server-sent event parser. Chunks can split multi-byte
/// characters, so bytes are buffered until a complete line arrives.
#[derive(Default)]
pub struct SseBuffer {
    buffer: Vec<u8>,
}

/// A single SSE line that never ends would otherwise grow this buffer for as
/// long as the server keeps streaming, so it is capped.
const MAX_PENDING_LINE: usize = 512 * 1024;

impl SseBuffer {
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(chunk);
        if self.buffer.len() > MAX_PENDING_LINE {
            self.buffer.clear();
            return Vec::new();
        }
        let mut payloads = Vec::new();
        while let Some(index) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=index).collect();
            let Ok(text) = String::from_utf8(line) else {
                continue;
            };
            if let Some(data) = text.trim_end().strip_prefix("data:") {
                let data = data.trim();
                if !data.is_empty() {
                    payloads.push(data.to_string());
                }
            }
        }
        payloads
    }
}

async fn post(request: ChatRequest, timeout: Duration) -> Result<reqwest::Response, String> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|error| format!("无法创建网络客户端：{error}"))?;

    let mut pending = client.post(&request.endpoint).json(&request.body);
    if let Some(authorization) = request.authorization {
        pending = pending.header("authorization", authorization);
    }

    let response = pending
        .send()
        .await
        .map_err(|error| format!("网络请求失败：{error}"))?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let payload: Value = response.json().await.unwrap_or(Value::Null);
    Err(error_detail(&payload, status))
}

/// Single non-streamed completion, used by context compaction and the legacy
/// one-shot chat command.
pub async fn completion(
    settings: &Settings,
    api_key: Option<&str>,
    body: Value,
) -> Result<Value, String> {
    let request = build_request(settings, api_key, body)?;
    let response = post(request, REQUEST_TIMEOUT).await?;
    response
        .json()
        .await
        .map_err(|error| format!("无法解析模型响应：{error}"))
}

/// Streams a completion, forwarding text fragments to `on_text` and returning
/// the assembled reply plus any tool calls the model asked for.
pub async fn stream_completion<F, C>(
    settings: &Settings,
    api_key: Option<&str>,
    body: Value,
    mut should_stop: C,
    mut on_text: F,
) -> Result<StreamOutcome, String>
where
    F: FnMut(&str),
    C: FnMut() -> bool,
{
    let request = build_request(settings, api_key, body)?;
    let mut response = post(request, STREAM_TIMEOUT).await?;
    let event_stream = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("event-stream"));

    let mut outcome = StreamOutcome::default();
    let mut pending_calls: Vec<AssembledToolCall> = Vec::new();

    if !event_stream {
        let payload: Value = response
            .json()
            .await
            .map_err(|error| format!("无法解析模型响应：{error}"))?;
        if let Some(content) = parse_delta(&payload.to_string()).and_then(|delta| delta.text) {
            outcome.content.push_str(&content);
            on_text(&content);
        }
        if let Some(calls) = payload.pointer("/choices/0/message/tool_calls") {
            for (index, item) in calls.as_array().into_iter().flatten().enumerate() {
                let mut call = assemble(
                    index,
                    item.get("id").and_then(Value::as_str),
                    item.pointer("/function/name").and_then(Value::as_str),
                );
                call.arguments = item
                    .pointer("/function/arguments")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                pending_calls.push(call);
            }
        }
        outcome.tool_calls = pending_calls;
        if outcome.content.is_empty() && outcome.tool_calls.is_empty() {
            return Err("模型没有返回可显示的内容".to_string());
        }
        return Ok(outcome);
    }

    let mut parser = SseBuffer::default();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("读取流式响应失败：{error}"))?
    {
        if should_stop() {
            break;
        }
        for payload in parser.push(&chunk) {
            if payload.trim() == "[DONE]" {
                outcome.tool_calls = pending_calls;
                return Ok(outcome);
            }
            let Some(delta) = parse_delta(&payload) else {
                continue;
            };
            if let Some(text) = delta.text {
                outcome.content.push_str(&text);
                on_text(&text);
            }
            for fragment in delta.tool_calls {
                merge_tool_delta(&mut pending_calls, fragment);
            }
        }
    }

    outcome.tool_calls = pending_calls;
    if outcome.content.is_empty() && outcome.tool_calls.is_empty() {
        return Err("模型没有返回可显示的内容".to_string());
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        Settings::default()
    }

    #[test]
    fn normalizes_a_trailing_endpoint_slash() {
        assert_eq!(
            normalize_endpoint("https://api.example.com/v1/"),
            "https://api.example.com/v1"
        );
    }

    #[test]
    fn rejects_an_insecure_endpoint() {
        let mut custom = settings();
        custom.endpoint = "http://example.com".to_string();
        assert!(build_request(&custom, Some("key"), json!({})).is_err());
    }

    #[test]
    fn strips_leaked_tool_markup() {
        assert_eq!(strip_tool_markup("现在是 10:00。"), "现在是 10:00。");
        assert_eq!(
            strip_tool_markup("查完了。\n<｜｜DSML｜｜tool_calls>\n<｜｜DSML｜｜invoke name=\"get_time\">"),
            "查完了。"
        );
        assert_eq!(strip_tool_markup("<|DSML|>junk"), "");
        assert_eq!(strip_tool_markup(""), "");
    }

    #[test]
    fn bounds_a_line_that_never_ends() {
        let mut buffer = SseBuffer::default();
        assert!(buffer.push(&vec![b'x'; MAX_PENDING_LINE + 1]).is_empty());
        assert_eq!(buffer.buffer.len(), 0);
        // The parser keeps working afterwards.
        assert_eq!(buffer.push(b"data: {}\n"), vec!["{}".to_string()]);
    }

    #[test]
    fn parses_streamed_lines_across_chunks() {
        let mut parser = SseBuffer::default();
        assert!(parser.push(b"data: {\"a\":").is_empty());
        let payloads = parser.push(b"1}\n\ndata: [DONE]\n");
        assert_eq!(payloads, vec!["{\"a\":1}".to_string(), "[DONE]".to_string()]);
    }

    #[test]
    fn keeps_multibyte_characters_intact_across_chunks() {
        let mut parser = SseBuffer::default();
        let payload = "data: {\"t\":\"鱼\"}\n".as_bytes();
        let (head, tail) = payload.split_at(payload.len() - 4);
        assert!(parser.push(head).is_empty());
        assert_eq!(parser.push(tail), vec!["{\"t\":\"鱼\"}".to_string()]);
    }

    #[test]
    fn reads_text_from_common_shapes() {
        assert_eq!(
            parse_delta(r#"{"choices":[{"delta":{"content":"嗨"}}]}"#)
                .and_then(|delta| delta.text)
                .as_deref(),
            Some("嗨")
        );
        assert_eq!(
            parse_delta(r#"{"choices":[{"text":"嗨"}]}"#)
                .and_then(|delta| delta.text)
                .as_deref(),
            Some("嗨")
        );
        assert!(parse_delta(r#"{"choices":[{"delta":{"content":""}}]}"#).is_none());
        assert!(parse_delta("not json").is_none());
    }

    #[test]
    fn reads_tool_call_fragments() {
        let delta = parse_delta(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"get_time","arguments":"{\"tz\":"}}]}}]}"#,
        )
        .unwrap();
        assert_eq!(delta.tool_calls.len(), 1);
        assert_eq!(delta.tool_calls[0].index, 0);
        assert_eq!(delta.tool_calls[0].id.as_deref(), Some("call_1"));
        assert_eq!(delta.tool_calls[0].name.as_deref(), Some("get_time"));
        assert_eq!(delta.tool_calls[0].arguments.as_deref(), Some("{\"tz\":"));
    }

    #[test]
    fn merges_streamed_tool_call_fragments() {
        let mut calls: Vec<AssembledToolCall> = Vec::new();
        merge_tool_delta(
            &mut calls,
            ToolCallDelta {
                index: 0,
                id: Some("call_a".into()),
                name: Some("get_".into()),
                arguments: Some("{\"a\"".into()),
            },
        );
        merge_tool_delta(
            &mut calls,
            ToolCallDelta {
                index: 0,
                id: None,
                name: Some("time".into()),
                arguments: Some(":1}".into()),
            },
        );
        // A second call can start in the same stream.
        merge_tool_delta(
            &mut calls,
            ToolCallDelta {
                index: 1,
                id: Some("call_b".into()),
                name: Some("pet_behavior".into()),
                arguments: Some("{}".into()),
            },
        );
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "call_a");
        assert_eq!(calls[0].name, "get_time");
        assert_eq!(calls[0].arguments, "{\"a\":1}");
        assert_eq!(calls[1].name, "pet_behavior");
    }

    #[test]
    fn invents_an_id_for_calls_without_one() {
        let mut calls: Vec<AssembledToolCall> = Vec::new();
        merge_tool_delta(
            &mut calls,
            ToolCallDelta {
                index: 2,
                id: None,
                name: Some("get_time".into()),
                arguments: Some("{}".into()),
            },
        );
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[2].id, "call_2");
    }

    #[test]
    fn tool_payload_requests_auto_choice() {
        let body = payload(
            &settings(),
            vec![json!({ "role": "user", "content": "hi" })],
            Some(json!([{ "type": "function" }])),
            true,
            400,
        );
        assert_eq!(body["tool_choice"], json!("auto"));
        assert_eq!(body["stream"], json!(true));
        assert!(body["tools"].is_array());
    }

    #[test]
    fn omits_authorization_without_a_key() {
        let request = build_request(&settings(), None, json!({})).unwrap();
        assert!(request.authorization.is_none());

        let request = build_request(&settings(), Some("secret"), json!({})).unwrap();
        assert_eq!(request.authorization.as_deref(), Some("Bearer secret"));
    }

    #[test]
    #[ignore = "requires network access to the public free model"]
    fn streams_with_tools_and_tool_results() {
        let tool_spec = json!([{
            "type": "function",
            "function": {
                "name": "get_time",
                "description": "获取当前时间",
                "parameters": { "type": "object", "properties": {} }
            }
        }]);
        let messages = vec![
            json!({ "role": "user", "content": "现在几点了？" }),
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": { "name": "get_time", "arguments": "{}" }
                }]
            }),
            json!({ "role": "tool", "tool_call_id": "call_1", "content": "现在是 2026-10-28 周三 14:20" }),
        ];
        let body = payload(&settings(), messages, Some(tool_spec), true, 200);
        let outcome = tauri::async_runtime::block_on(stream_completion(
            &settings(),
            None,
            body,
            || false,
            |_| {},
        ))
        .expect("live tool request");
        assert!(!outcome.content.trim().is_empty() || !outcome.tool_calls.is_empty());
    }

    #[test]
    #[ignore = "requires network access to the public free model"]
    fn streams_from_the_free_model() {
        let body = payload(
            &settings(),
            vec![json!({ "role": "user", "content": "只回复两个字：你好" })],
            None,
            true,
            200,
        );
        let mut chunks = 0;
        let outcome = tauri::async_runtime::block_on(stream_completion(
            &settings(),
            None,
            body,
            || false,
            |_| chunks += 1,
        ))
        .expect("live streaming request");
        assert!(!outcome.content.trim().is_empty());
        assert!(chunks >= 1);
    }
}
