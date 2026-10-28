//! DeepSeek's server-side web tools: `POST /v1/tools/search` and
//! `POST /v1/tools/fetch`. Both are plain JSON APIs billed per request, so they
//! stay behind two opt-in settings switches.

use std::fmt::Write as _;
use std::time::Duration;

use serde_json::{json, Value};

use super::session::truncate;

pub const SEARCH_URL: &str = "https://api.deepseek.com/v1/tools/search";
pub const FETCH_URL: &str = "https://api.deepseek.com/v1/tools/fetch";

const TIMEOUT: Duration = Duration::from_secs(20);
/// Keeps one search or one page from eating the whole context budget.
const MAX_TOOL_CHARS: usize = 4_000;
/// The API's long summary is up to ~800 characters; a per-hit cap keeps several
/// results readable inside the budget instead of one page-long entry.
const MAX_SUMMARY_CHARS: usize = 300;
const MAX_COUNT: u32 = 20;

pub fn search(api_key: &str, query: &str, count: u32, need_content: bool) -> Result<String, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("搜索词不能为空".to_string());
    }
    let body = json!({
        "query": truncate(query, 128),
        "count": count.clamp(1, MAX_COUNT),
        "need_content": need_content,
    });
    let value = post(SEARCH_URL, api_key, body)?;
    Ok(format_search(&value))
}

pub fn fetch(api_key: &str, url: &str) -> Result<String, String> {
    let url = url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("只支持 http/https 链接".to_string());
    }
    if url.len() > 8_192 {
        return Err("链接太长".to_string());
    }
    let value = post(FETCH_URL, api_key, json!({ "url": url }))?;
    Ok(format_page(&value))
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|error| format!("无法创建网络客户端：{error}"))
}

fn post(url: &str, api_key: &str, body: Value) -> Result<Value, String> {
    let response = client()?
        .post(url)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .map_err(|error| format!("请求失败：{error}"))?;
    let status = response.status();
    let payload: Value = response.json().unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(payload);
    }
    Err(describe_error(status, &payload))
}

/// Turns the OpenAI-compatible error body into something worth showing the model.
fn describe_error(status: reqwest::StatusCode, payload: &Value) -> String {
    let detail = payload
        .pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let hint = match status.as_u16() {
        401 | 403 => "API Key 无效或没有权限".to_string(),
        402 => "DeepSeek 账号余额不足".to_string(),
        404 => "没有抓取到可用的网页正文".to_string(),
        429 => "请求过于频繁（QPS 超限）".to_string(),
        500..=599 => "DeepSeek 服务暂时不可用".to_string(),
        _ => "请求不合法".to_string(),
    };
    if detail.is_empty() {
        format!("{hint}（HTTP {}）", status.as_u16())
    } else {
        format!("{hint}（HTTP {}）：{detail}", status.as_u16())
    }
}

/// One line per hit plus the long summary the API is designed for. The model
/// gets URLs so it can cite them, and the user can open them.
pub fn format_search(payload: &Value) -> String {
    let results = payload
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if results.is_empty() {
        return "没有找到相关网页。".to_string();
    }
    let mut text = String::new();
    for (index, item) in results.iter().enumerate() {
        let title = item
            .get("title")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("(无标题)");
        let url = item.get("url").and_then(Value::as_str).unwrap_or("");
        let summary = item
            .get("summary")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .or_else(|| item.get("snippet").and_then(Value::as_str))
            .unwrap_or("");
        let date = item.get("date").and_then(Value::as_str).unwrap_or("");
        let date = date.split('T').next().unwrap_or(date);
        let _ = writeln!(text, "{}. {title}\n   {url}", index + 1);
        if !date.is_empty() {
            let _ = writeln!(text, "   发布：{date}");
        }
        if !summary.trim().is_empty() {
            let _ = writeln!(text, "   {}", truncate(summary.trim(), MAX_SUMMARY_CHARS));
        }
        if text.len() > MAX_TOOL_CHARS {
            text.push_str("…（结果过多，已截断）");
            break;
        }
    }
    truncate(&text, MAX_TOOL_CHARS)
}

pub fn format_page(payload: &Value) -> String {
    let title = payload
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let content = payload
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if content.is_empty() {
        return "这个网页没有可提取的正文。".to_string();
    }
    let header = if title.is_empty() {
        String::new()
    } else {
        format!("{title}\n\n")
    };
    let body = truncate(content, MAX_TOOL_CHARS.saturating_sub(header.len()));
    format!("{header}{body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_search_hits_with_urls_and_dates() {
        let payload = json!({
            "results": [
                {
                    "title": "DeepSeek V4 发布",
                    "url": "https://example.com/a",
                    "summary": "长摘要内容",
                    "snippet": "短摘要",
                    "date": "2026-06-01T00:00:00+00:00",
                    "rerank_score": 2.4
                },
                { "title": "", "url": "https://example.com/b" }
            ]
        });
        let text = format_search(&payload);
        assert!(text.contains("1. DeepSeek V4 发布"));
        assert!(text.contains("https://example.com/a"));
        assert!(text.contains("发布：2026-06-01"));
        assert!(text.contains("长摘要内容"));
        assert!(!text.contains("短摘要"), "summary wins over snippet");
        assert!(text.contains("2. (无标题)"));
    }

    #[test]
    fn caps_each_summary_and_the_whole_result_list() {
        let long = "摘".repeat(900);
        let payload = json!({ "results": [{ "title": "T", "url": "https://e.com", "summary": long }] });
        let text = format_search(&payload);
        assert!(text.chars().count() < MAX_SUMMARY_CHARS + 80, "{}", text.chars().count());

        let many: Vec<Value> = (0..40)
            .map(|index| json!({ "title": format!("标题 {index}"), "url": "https://e.com", "summary": "摘".repeat(300) }))
            .collect();
        let text = format_search(&json!({ "results": many }));
        assert!(text.chars().count() <= MAX_TOOL_CHARS + 16);
        assert!(text.contains("已截断"));
    }

    #[test]
    fn handles_empty_and_oversized_results() {
        assert_eq!(format_search(&json!({ "results": [] })), "没有找到相关网页。");
        assert_eq!(format_search(&json!({})), "没有找到相关网页。");

        let long = "很长的正文".repeat(5_000);
        let text = format_page(&json!({ "title": "标题", "content": long }));
        assert!(text.starts_with("标题"));
        assert!(text.chars().count() <= MAX_TOOL_CHARS + 8);
    }

    #[test]
    fn rejects_unusable_urls_and_queries() {
        assert!(fetch("key", "file:///etc/passwd").is_err());
        assert!(fetch("key", "example.com").is_err());
        assert!(search("key", "   ", 5, false).is_err());
    }

    /// One real search plus one real fetch. Both are billed per request, so this
    /// stays opt-in via `--ignored`. The key is read from the app's own settings.
    #[test]
    #[ignore = "requires a real DeepSeek API key in the app settings"]
    fn live_search_and_fetch() {
        let home = std::env::var("HOME").expect("HOME");
        let path = format!("{home}/Library/Application Support/com.deepfish.desktop-pet/settings.json");
        let text = std::fs::read_to_string(path).expect("settings.json");
        let value: Value = serde_json::from_str(&text).expect("valid settings json");
        let key = value["apiKey"].as_str().expect("apiKey").to_string();

        let results = search(&key, "DeepSeek 官方文档", 3, false).expect("search ok");
        assert!(results.contains("http"), "{results}");
        assert!(results.chars().count() <= MAX_TOOL_CHARS + 16);

        let page = fetch(&key, "https://api-docs.deepseek.com/zh-cn/guides/search")
            .expect("fetch ok");
        assert!(page.chars().count() > 100, "page too small: {}", page.len());
    }

    #[test]
    fn explains_api_errors() {
        let error = describe_error(
            reqwest::StatusCode::PAYMENT_REQUIRED,
            &json!({ "error": { "message": "Insufficient Balance" } }),
        );
        assert!(error.contains("余额不足"));
        assert!(error.contains("Insufficient Balance"));
    }
}
