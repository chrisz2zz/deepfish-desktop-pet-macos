use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::settings::Settings;

pub const SYSTEM_PROMPT: &str = "你是桌面宠物大肥鱼，一只蓝色、聪明但偶尔装傻的二次元小鲸鱼女仆。\n用简短自然的中文回答，通常不超过 120 字。\n语气温柔、有一点机灵的吐槽，不要自称 DeepSeek 官方角色。\n遇到严肃问题时优先准确和有帮助，不要为了卖萌牺牲事实。";

const CONTEXT_WINDOW: usize = 10;
const MAX_TOKENS: u32 = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug)]
pub struct ChatRequest {
    pub endpoint: String,
    pub authorization: Option<String>,
    pub body: Value,
}

pub fn normalize_endpoint(endpoint: &str) -> String {
    endpoint.trim().trim_end_matches('/').to_string()
}

pub fn build_request(
    settings: &Settings,
    api_key: Option<&str>,
    messages: &[ChatMessage],
) -> Result<ChatRequest, String> {
    let endpoint = normalize_endpoint(&settings.endpoint);
    if !endpoint.to_ascii_lowercase().starts_with("https://") {
        return Err("API 地址必须使用 HTTPS".to_string());
    }

    let mut payload = vec![json!({ "role": "system", "content": SYSTEM_PROMPT })];
    let start = messages.len().saturating_sub(CONTEXT_WINDOW);
    payload.extend(
        messages[start..]
            .iter()
            .map(|message| json!({ "role": message.role, "content": message.content })),
    );

    Ok(ChatRequest {
        endpoint,
        authorization: api_key
            .filter(|key| !key.is_empty())
            .map(|key| format!("Bearer {key}")),
        body: json!({
            "model": settings.model,
            "messages": payload,
            "temperature": 0.8,
            "max_tokens": MAX_TOKENS
        }),
    })
}

pub async fn chat(
    settings: &Settings,
    api_key: Option<&str>,
    messages: &[ChatMessage],
) -> Result<String, String> {
    let request = build_request(settings, api_key, messages)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
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
    let payload: Value = response.json().await.unwrap_or(Value::Null);

    if !status.is_success() {
        let detail = payload
            .pointer("/error/message")
            .and_then(Value::as_str)
            .or_else(|| payload.get("message").and_then(Value::as_str))
            .map(str::to_string)
            .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
        return Err(detail);
    }

    let content = payload
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if content.is_empty() {
        return Err("模型没有返回可显示的内容".to_string());
    }
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        Settings::default()
    }

    fn messages(count: usize) -> Vec<ChatMessage> {
        (0..count)
            .map(|index| ChatMessage {
                role: "user".to_string(),
                content: format!("消息 {index}"),
            })
            .collect()
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
        assert!(build_request(&custom, Some("key"), &[]).is_err());
    }

    #[test]
    fn keeps_only_the_last_ten_messages() {
        let request = build_request(&settings(), None, &messages(14)).unwrap();
        let payload = request.body["messages"].as_array().unwrap();
        assert_eq!(payload.len(), CONTEXT_WINDOW + 1);
        assert_eq!(payload[0]["role"], "system");
        assert_eq!(payload[1]["content"], "消息 4");
        assert_eq!(payload[10]["content"], "消息 13");
    }

    #[test]
    #[ignore = "requires network access to the public free model"]
    fn answers_with_the_free_model() {
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: "只回复两个字：你好".to_string(),
        }];
        let reply = tauri::async_runtime::block_on(chat(&settings(), None, &messages))
            .expect("live chat request");
        assert!(!reply.trim().is_empty());
    }

    #[test]
    fn omits_authorization_without_a_key() {
        let request = build_request(&settings(), None, &[]).unwrap();
        assert!(request.authorization.is_none());

        let request = build_request(&settings(), Some("secret"), &[]).unwrap();
        assert_eq!(request.authorization.as_deref(), Some("Bearer secret"));
    }
}
