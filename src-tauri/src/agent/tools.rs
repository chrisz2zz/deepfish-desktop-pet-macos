use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use super::risk;
use super::session::ToolCall;

/// Hard caps so one tool call cannot flood the model's context or block the UI.
const MAX_READ_LINES: usize = 400;
const MAX_READ_BYTES: usize = 200_000;
const MAX_WRITE_BYTES: usize = 200_000;
const MAX_LIST_ENTRIES: usize = 200;
const MAX_OUTPUT_CHARS: usize = 6_000;
const DEFAULT_TIMEOUT_MS: u64 = 15_000;
/// Long builds and installs are allowed to run in the background.
const MAX_TIMEOUT_MS: u64 = 600_000;

/// Behaviours the model may trigger. Mirrors the catalogue in
/// `src/shared/behaviors.js`; anything else is rejected.
const BEHAVIORS: &[(&str, &str)] = &[
    ("walk", "散步"),
    ("wash", "洗碗"),
    ("work", "加班"),
    ("coffee", "喝咖啡"),
    ("toy", "抱玩偶"),
    ("sleep", "睡觉"),
    ("sit", "坐好"),
    ("feed", "吃饭"),
    ("pat", "被摸头"),
    ("think", "思考"),
    ("smug", "得意"),
    ("ciallo", "卖萌"),
    ("shy", "害羞"),
    ("stretch", "伸懒腰"),
    ("cry", "哭"),
    ("shock", "震惊"),
    ("angry", "生气"),
    ("dizzy", "转晕"),
];

pub enum ToolEffect {
    None,
    RenameSession(String),
}

pub struct ToolOutcome {
    pub content: String,
    pub display: String,
    pub effect: ToolEffect,
}

impl ToolOutcome {
    /// The risk classifier refused this call; the model gets told why so it can
    /// pick a different approach instead of retrying the same thing.
    pub fn refused(name: &str, summary: String) -> Self {
        Self {
            content: format!("该操作被安全策略禁止，未执行：{summary}。请不要重试，改用其它方式或直接告诉用户。"),
            display: format!("⛔ 已阻止 · {}", label(name)),
            effect: ToolEffect::None,
        }
    }

    /// The per-question tool budget is spent; nothing ran.
    pub fn budget_exhausted(name: &str, limit: usize) -> Self {
        Self {
            content: format!(
                "本次问答的工具调用次数已达上限（{limit} 次），这条没有执行。请直接总结已有的结果或告诉用户需要再问一次。"
            ),
            display: format!("⏹ 已超工具额度 · {}", label(name)),
            effect: ToolEffect::None,
        }
    }

    /// The user rejected the approval card.
    pub fn denied(name: &str, summary: &str) -> Self {
        Self {
            content: format!("用户拒绝了这次操作：{summary}。不要重复请求，换个方式或直接说明你做不到。"),
            display: format!("🚫 用户拒绝 · {}", label(name)),
            effect: ToolEffect::None,
        }
    }
}

pub fn label(name: &str) -> String {
    match name {
        "get_time" => "🕒 查看时间".to_string(),
        "pet_behavior" => "🐾 桌宠动作".to_string(),
        "pet_say" => "💬 说句话".to_string(),
        "open_settings" => "⚙ 打开设置".to_string(),
        "rename_session" => "✏ 重命名会话".to_string(),
        "list_dir" => "📁 列目录".to_string(),
        "read_file" => "📄 读文件".to_string(),
        "write_file" => "💾 写文件".to_string(),
        "run_command" => "⌨ 执行命令".to_string(),
        other => format!("🛠 {other}"),
    }
}

pub fn specs() -> Value {
    let behaviors: Vec<&str> = BEHAVIORS.iter().map(|(id, _)| *id).collect();
    json!([
        {
            "type": "function",
            "function": {
                "name": "get_time",
                "description": "获取本机当前的日期、星期和本地时间。",
                "parameters": { "type": "object", "properties": {} }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "pet_behavior",
                "description": "让桌宠大肥鱼做一个动作或表情。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "behavior": {
                            "type": "string",
                            "enum": behaviors,
                            "description": "动作标识"
                        }
                    },
                    "required": ["behavior"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "pet_say",
                "description": "让桌宠在头顶的气泡里说一句很短的话（不超过 40 字）。",
                "parameters": {
                    "type": "object",
                    "properties": { "text": { "type": "string" } },
                    "required": ["text"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "open_settings",
                "description": "打开桌宠的设置窗口。",
                "parameters": { "type": "object", "properties": {} }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "rename_session",
                "description": "给当前会话换一个更贴切的短标题。",
                "parameters": {
                    "type": "object",
                    "properties": { "title": { "type": "string" } },
                    "required": ["title"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "list_dir",
                "description": "列出用户主目录内的某个目录，返回名称、类型和大小。路径必须是 ~ 或 ~/ 开头的相对写法。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "目录路径，默认 \".\"" }
                    }
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "读取用户主目录内的文本文件（默认前 200 行）。不要读取密钥或凭据文件。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "文件路径" },
                        "offset": { "type": "integer", "description": "起始行号，从 1 开始" },
                        "limit": { "type": "integer", "description": "最多读取的行数" }
                    },
                    "required": ["path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "write_file",
                "description": "把内容写入用户主目录内的文件（覆盖或新建，父目录会自动创建）。这个操作需要用户审批。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "文件路径" },
                        "content": { "type": "string", "description": "完整的新内容" }
                    },
                    "required": ["path", "content"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "run_command",
                "description": "在用户主目录内执行一条 shell 命令并返回输出。只读命令会自动执行，其它命令需要用户审批；涉及 sudo、凭据或外网访问的命令一律禁止。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "要执行的 shell 命令" },
                        "cwd": { "type": "string", "description": "工作目录，默认 ~" },
                        "timeout_ms": { "type": "integer", "description": "超时毫秒数，默认 15000" }
                    },
                    "required": ["command"]
                }
            }
        }
    ])
}

fn argument(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn failure(name: &str, message: impl Into<String>) -> ToolOutcome {
    ToolOutcome {
        content: message.into(),
        display: label(name),
        effect: ToolEffect::None,
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn list_dir(root: &Path, path: &Path) -> ToolOutcome {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => return failure("list_dir", format!("无法读取目录：{error}")),
    };
    let mut rows = Vec::new();
    for entry in entries.flatten().take(MAX_LIST_ENTRIES) {
        let metadata = entry.metadata().ok();
        let is_dir = metadata.as_ref().is_some_and(std::fs::Metadata::is_dir);
        let size = metadata.as_ref().map_or(0, std::fs::Metadata::len);
        let name = entry.file_name().to_string_lossy().to_string();
        rows.push(if is_dir {
            format!("{name}/")
        } else {
            format!("{name} ({})", human_size(size))
        });
    }
    rows.sort();
    if rows.is_empty() {
        return ToolOutcome {
            content: format!("目录 {} 是空的。", risk::display_path(root, path)),
            display: format!("📁 {}", risk::display_path(root, path)),
            effect: ToolEffect::None,
        };
    }
    ToolOutcome {
        content: format!(
            "{} 里有 {} 项：\n{}",
            risk::display_path(root, path),
            rows.len(),
            rows.join("\n")
        ),
        display: format!("📁 {} ({} 项)", risk::display_path(root, path), rows.len()),
        effect: ToolEffect::None,
    }
}

fn read_file(root: &Path, path: &Path, offset: usize, limit: usize) -> ToolOutcome {
    let name = "read_file";
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => {
            return failure(name, "这是一个目录，请用 list_dir。".to_string());
        }
        Ok(_) => {}
        Err(error) => return failure(name, format!("无法读取文件：{error}")),
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => return failure(name, format!("无法读取文件：{error}")),
    };
    if bytes.iter().take(8_000).any(|byte| *byte == 0) {
        return failure(name, "这是二进制文件，只能读文本。".to_string());
    }
    let sliced = &bytes[..bytes.len().min(MAX_READ_BYTES)];
    let text = String::from_utf8_lossy(sliced);
    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len();
    let start = offset.saturating_sub(1).min(total);
    let end = (start + limit.min(MAX_READ_LINES)).min(total);
    let mut body = String::new();
    for (index, line) in lines[start..end].iter().enumerate() {
        let _ = writeln!(body, "{}| {}", start + index + 1, line);
    }
    let truncated = end < total || sliced.len() < bytes.len();
    ToolOutcome {
        content: format!(
            "{}{}",
            body,
            if truncated {
                format!("…（仅显示第 {}-{} 行，共 {} 行）", start + 1, end, total)
            } else {
                String::new()
            }
        ),
        display: format!(
            "📄 {} ({}-{} / {} 行)",
            risk::display_path(root, path),
            start + 1,
            end,
            total
        ),
        effect: ToolEffect::None,
    }
}

fn write_file(root: &Path, path: &Path, content: &str) -> ToolOutcome {
    let name = "write_file";
    if content.len() > MAX_WRITE_BYTES {
        return failure(
            name,
            format!("内容太大（{} 字节，上限 {MAX_WRITE_BYTES}）。", content.len()),
        );
    }
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            return failure(name, format!("无法创建目录：{error}"));
        }
    }
    let temporary = path.with_extension("deepfish-tmp");
    let written = std::fs::File::create(&temporary)
        .and_then(|mut file| file.write_all(content.as_bytes()))
        .and_then(|()| std::fs::rename(&temporary, path));
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temporary);
        return failure(name, format!("写入失败：{error}"));
    }
    ToolOutcome {
        content: format!(
            "已写入 {} （{} 字节）。",
            risk::display_path(root, path),
            content.len()
        ),
        display: format!("💾 {}", risk::display_path(root, path)),
        effect: ToolEffect::None,
    }
}

fn drain(mut reader: impl std::io::Read) -> String {
    let mut kept = Vec::new();
    let mut buffer = [0u8; 8 * 1024];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                if kept.len() < MAX_OUTPUT_CHARS {
                    let room = MAX_OUTPUT_CHARS - kept.len();
                    kept.extend_from_slice(&buffer[..read.min(room)]);
                }
            }
        }
    }
    String::from_utf8_lossy(&kept).to_string()
}

fn run_command(root: &Path, command: &str, cwd: &Path, timeout: Duration) -> ToolOutcome {
    let name = "run_command";
    let mut child = match Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return failure(name, format!("无法启动命令：{error}")),
    };
    let stdout = child.stdout.take().map(|pipe| std::thread::spawn(move || drain(pipe)));
    let stderr = child.stderr.take().map(|pipe| std::thread::spawn(move || drain(pipe)));

    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                timed_out = true;
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break None,
        }
    };
    let out = stdout
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    let err = stderr
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();

    let code = status.and_then(|status| status.code());
    let mut report = String::new();
    let status_line = match (timed_out, code) {
        (true, _) => format!("超时 {} 秒已终止", timeout.as_secs()),
        (false, Some(code)) => format!("退出码 {code}"),
        (false, None) => "被信号终止".to_string(),
    };
    let _ = write!(
        report,
        "$ {command}\n（目录 {}，{status_line}）\n",
        risk::display_path(root, cwd)
    );
    if !out.trim().is_empty() {
        let _ = write!(report, "stdout:\n{}\n", out.trim_end());
    }
    if !err.trim().is_empty() {
        let _ = write!(report, "stderr:\n{}\n", err.trim_end());
    }
    if out.trim().is_empty() && err.trim().is_empty() {
        report.push_str("（没有输出）");
    }
    ToolOutcome {
        content: report,
        display: format!(
            "⌨ {}",
            super::session::truncate(command, 40)
        ),
        effect: ToolEffect::None,
    }
}

pub fn execute(app: &AppHandle, root: &Path, call: &ToolCall) -> ToolOutcome {
    let args: Value = serde_json::from_str(&call.arguments).unwrap_or(Value::Null);
    match call.name.as_str() {
        "get_time" => {
            let now = chrono::Local::now();
            let weekday = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"]
                [now.format("%u").to_string().parse::<usize>().unwrap_or(1) - 1];
            let text = format!("现在是 {} {} {}", now.format("%Y-%m-%d"), weekday, now.format("%H:%M"));
            ToolOutcome {
                content: text,
                display: label("get_time"),
                effect: ToolEffect::None,
            }
        }
        "pet_behavior" => {
            let requested = argument(&args, "behavior").unwrap_or_default();
            match BEHAVIORS.iter().find(|(id, _)| *id == requested) {
                Some((id, name)) => {
                    if let Some(window) = app.get_webview_window(crate::PET_WINDOW_LABEL) {
                        let _ = window.emit("pet-command", *id);
                    }
                    ToolOutcome {
                        content: format!("已让桌宠执行「{name}」。"),
                        display: format!("🐾 {name}"),
                        effect: ToolEffect::None,
                    }
                }
                None => ToolOutcome {
                    content: format!(
                        "没有「{requested}」这个动作，可选：{}",
                        BEHAVIORS.iter().map(|(id, _)| *id).collect::<Vec<_>>().join("、")
                    ),
                    display: label("pet_behavior"),
                    effect: ToolEffect::None,
                },
            }
        }
        "pet_say" => {
            let text = argument(&args, "text").unwrap_or_default();
            if text.is_empty() {
                return ToolOutcome {
                    content: "缺少要说的话。".to_string(),
                    display: label("pet_say"),
                    effect: ToolEffect::None,
                };
            }
            if let Some(window) = app.get_webview_window(crate::PET_WINDOW_LABEL) {
                let _ = window.emit("pet-speak", json!({ "text": text }));
            }
            ToolOutcome {
                content: format!("已经在气泡里说了：{text}"),
                display: label("pet_say"),
                effect: ToolEffect::None,
            }
        }
        "open_settings" => {
            let content = match crate::create_settings_window(app) {
                Ok(()) => "设置窗口已打开。".to_string(),
                Err(error) => format!("打开设置失败：{error}"),
            };
            ToolOutcome {
                content,
                display: label("open_settings"),
                effect: ToolEffect::None,
            }
        }
        "rename_session" => match argument(&args, "title") {
            Some(title) => {
                let display = format!("✏ {}", super::session::truncate(&title, 12));
                ToolOutcome {
                    content: format!("会话标题已改为「{title}」。"),
                    display,
                    effect: ToolEffect::RenameSession(title),
                }
            }
            None => ToolOutcome {
                content: "缺少标题。".to_string(),
                display: label("rename_session"),
                effect: ToolEffect::None,
            },
        },
        "list_dir" => {
            let raw = argument(&args, "path").unwrap_or_else(|| ".".to_string());
            match risk::resolve(root, &raw) {
                Ok(path) => list_dir(root, &path),
                Err(error) => failure("list_dir", error),
            }
        }
        "read_file" => {
            let raw = argument(&args, "path").unwrap_or_default();
            let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(1).max(1) as usize;
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(MAX_READ_LINES as u64)
                .max(1) as usize;
            match risk::resolve(root, &raw) {
                Ok(path) => read_file(root, &path, offset, limit),
                Err(error) => failure("read_file", error),
            }
        }
        "write_file" => {
            let raw = argument(&args, "path").unwrap_or_default();
            let content = args
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            match risk::resolve(root, &raw) {
                Ok(path) => write_file(root, &path, &content),
                Err(error) => failure("write_file", error),
            }
        }
        "run_command" => {
            let command = argument(&args, "command").unwrap_or_default();
            if command.is_empty() {
                return failure("run_command", "缺少要执行的命令。");
            }
            let cwd = match risk::resolve(root, &argument(&args, "cwd").unwrap_or_else(|| ".".to_string())) {
                Ok(path) => path,
                Err(error) => return failure("run_command", error),
            };
            let timeout = args
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(DEFAULT_TIMEOUT_MS)
                .clamp(500, MAX_TIMEOUT_MS);
            run_command(root, &command, &cwd, Duration::from_millis(timeout))
        }
        other => ToolOutcome {
            content: format!("未知工具：{other}"),
            display: label(other),
            effect: ToolEffect::None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn exposes_every_tool_with_a_schema() {
        let specs = specs();
        let names: Vec<&str> = specs
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.pointer("/function/name").unwrap().as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "get_time",
                "pet_behavior",
                "pet_say",
                "open_settings",
                "rename_session",
                "list_dir",
                "read_file",
                "write_file",
                "run_command"
            ]
        );
        assert!(specs[1]["function"]["parameters"]["properties"]["behavior"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "walk"));
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("deepfish-tools-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_then_reads_a_file() {
        let root = scratch("rw");
        let path = risk::resolve(&root, "~/notes/todo.md").unwrap();
        let written = write_file(&root, &path, "第一行\n第二行\n");
        assert!(written.content.contains("已写入"), "{}", written.content);

        let read = read_file(&root, &path, 1, 200);
        assert!(read.content.contains("1| 第一行"), "{}", read.content);
        assert!(read.content.contains("2| 第二行"), "{}", read.content);
        // Line windows keep the original numbering.
        let tail = read_file(&root, &path, 2, 1);
        assert!(tail.content.contains("2| 第二行"), "{}", tail.content);
        assert!(!tail.content.contains("第一行"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn refuses_binary_and_oversized_writes() {
        let root = scratch("bin");
        let path = root.join("blob.bin");
        std::fs::write(&path, [0u8, 159, 146, 150]).unwrap();
        assert!(read_file(&root, &path, 1, 10).content.contains("二进制"));

        let big = write_file(&root, &root.join("big.txt"), &"a".repeat(MAX_WRITE_BYTES + 1));
        assert!(big.content.contains("太大"), "{}", big.content);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn lists_directory_entries() {
        let root = scratch("list");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("file.txt"), "hi").unwrap();
        let listing = list_dir(&root, &root);
        assert!(listing.content.contains("sub/"), "{}", listing.content);
        assert!(listing.content.contains("file.txt"), "{}", listing.content);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn runs_commands_and_reports_output() {
        let root = scratch("cmd");
        let ok = run_command(&root, "printf 'hi there'", &root, Duration::from_secs(5));
        assert!(ok.content.contains("hi there"), "{}", ok.content);
        assert!(ok.content.contains("退出码 0"), "{}", ok.content);

        let failed = run_command(&root, "exit 3", &root, Duration::from_secs(5));
        assert!(failed.content.contains("退出码 3"), "{}", failed.content);

        let timed_out = run_command(&root, "sleep 5", &root, Duration::from_millis(600));
        assert!(timed_out.content.contains("超时"), "{}", timed_out.content);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn behaviour_tools_validate_the_whitelist() {
        let app_free = ToolCall::new("1", "pet_behavior", r#"{"behavior":"nap"}"#);
        let args: Value = serde_json::from_str(&app_free.arguments).unwrap();
        assert_eq!(argument(&args, "behavior").as_deref(), Some("nap"));
        assert!(BEHAVIORS.iter().all(|(id, _)| *id != "nap"));
    }

    #[test]
    fn labels_unknown_tools() {
        assert_eq!(label("get_time"), "🕒 查看时间");
        assert_eq!(label("mystery"), "🛠 mystery");
    }
}
