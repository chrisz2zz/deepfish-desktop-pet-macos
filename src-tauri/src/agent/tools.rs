use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use super::risk;
use super::session::ToolCall;
use crate::reminders::{format_due, resolve_due, ReminderStore};

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

/// Everything a tool needs besides its arguments. Owned so the loop can hand it
/// to a blocking worker thread.
#[derive(Clone, Default)]
pub struct ToolContext {
    pub root: PathBuf,
    pub api_key: Option<String>,
    /// DeepSeek server-side search/fetch, both opt-in in the settings window.
    pub search: bool,
    pub fetch: bool,
    /// Wrap `run_command` in the Seatbelt sandbox (on by default).
    pub sandbox: bool,
    /// `auto`, `netease`, `qq` or `browser` for `play_music`.
    pub music_player: String,
}

impl ToolContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        root: PathBuf,
        api_key: Option<String>,
        search: bool,
        fetch: bool,
        sandbox: bool,
        music_player: String,
    ) -> Self {
        Self {
            root,
            api_key,
            search,
            fetch,
            sandbox,
            music_player,
        }
    }
}

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
        "add_reminder" => "⏰ 设置提醒".to_string(),
        "list_reminders" => "📋 查看提醒".to_string(),
        "cancel_reminder" => "🗑 取消提醒".to_string(),
        "play_music" => "🎵 播放音乐".to_string(),
        "focus_session" => "🍅 专注计时".to_string(),
        "list_dir" => "📁 列目录".to_string(),
        "read_file" => "📄 读文件".to_string(),
        "write_file" => "💾 写文件".to_string(),
        "run_command" => "⌨ 执行命令".to_string(),
        "web_search" => "🔎 联网搜索".to_string(),
        "web_fetch" => "🌐 抓取网页".to_string(),
        other => format!("🛠 {other}"),
    }
}

pub fn specs(context: &ToolContext) -> Value {
    let behaviors: Vec<&str> = BEHAVIORS.iter().map(|(id, _)| *id).collect();
    let mut tools = json!([
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
                "name": "add_reminder",
                "description": "为用户设置一个到点提醒（喝水、开会、休息等）。用户说“提醒我……”时使用。时间用相对分钟数，或本地时间如“2026-10-28 20:30”/“20:30”。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "text": { "type": "string", "description": "提醒内容，尽量简短" },
                        "minutes": { "type": "integer", "description": "多少分钟后提醒（1-43200）" },
                        "at": { "type": "string", "description": "本地提醒时间；与 minutes 二选一" }
                    },
                    "required": ["text"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "list_reminders",
                "description": "列出还没触发的提醒，按时间排序。回答“我有哪些提醒”时使用。",
                "parameters": { "type": "object", "properties": {} }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "cancel_reminder",
                "description": "取消一个提醒。需要先用 list_reminders 拿到 id。",
                "parameters": {
                    "type": "object",
                    "properties": { "id": { "type": "string", "description": "提醒 id" } },
                    "required": ["id"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "play_music",
                "description": "用系统默认浏览器打开音乐搜索页播放音乐，适合用户说“放首歌”“来点背景音乐”时使用。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "歌名、歌手或氛围，如“lofi”“周杰伦”" },
                        "site": {
                            "type": "string",
                            "enum": ["netease", "bilibili", "youtube", "spotify"],
                            "description": "浏览器回退时的音乐站点，默认 netease"
                        },
                        "player": {
                            "type": "string",
                            "enum": ["auto", "netease", "qq", "browser"],
                            "description": "播放方式：auto 优先本机客户端，默认 auto"
                        }
                    }
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "focus_session",
                "description": "开始或结束一次专注计时（番茄钟）。专注期间桌宠会安静陪伴，结束时提醒休息。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["start", "stop"] },
                        "minutes": { "type": "integer", "description": "专注分钟数 5-120；不填则用设置里的值" }
                    },
                    "required": ["action"]
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
                "description": run_command_description(),
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
    ]);
    let list = tools.as_array_mut().expect("tool list is an array");
    if context.search {
        list.push(json!({
            "type": "function",
            "function": {
                "name": "web_search",
                "description": "用 DeepSeek 搜索接口查询实时网页，返回标题、链接和摘要。回答时效性问题前先搜索，并在回复里带上来源链接。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "搜索词，支持 site:/lang:/after: 等语法" },
                        "count": { "type": "integer", "description": "返回条数 1-20，默认 5" },
                        "need_content": { "type": "boolean", "description": "是否同时返回网页正文，默认 false" }
                    },
                    "required": ["query"]
                }
            }
        }));
    }
    if context.fetch {
        list.push(json!({
            "type": "function",
            "function": {
                "name": "web_fetch",
                "description": "抓取单个网页并返回 Markdown 正文，适合读取搜索结果的链接。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": { "type": "string", "description": "完整的 http/https 链接" }
                    },
                    "required": ["url"]
                }
            }
        }));
    }
    tools
}

/// The tool description has to be honest about the shell and the sandbox,
/// both of which differ per platform.
fn run_command_description() -> &'static str {
    if cfg!(target_os = "windows") {
        "在用户主目录内用 cmd /C 执行一条命令并返回输出。只读命令会自动执行，其它命令需要用户审批；涉及 sudo、凭据或外网访问的命令一律禁止。Windows 没有 Seatbelt 沙箱，命令会直接以当前用户权限运行，请只执行可以信任的命令。"
    } else if cfg!(target_os = "macos") {
        "在用户主目录内执行一条 shell 命令并返回输出。只读命令会自动执行，其它命令需要用户审批；涉及 sudo、凭据或外网访问的命令一律禁止。命令默认在系统沙箱内运行：只能写当前工作目录、临时目录和构建缓存，只读命令没有网络；越界访问会得到 Operation not permitted。"
    } else {
        "在用户主目录内执行一条 shell 命令并返回输出。只读命令会自动执行，其它命令需要用户审批；涉及 sudo、凭据或外网访问的命令一律禁止。当前平台没有 Seatbelt 沙箱，需审批的命令会以你的权限运行。"
    }
}

fn argument(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn run_web_search(context: &ToolContext, args: &Value) -> ToolOutcome {
    let name = "web_search";
    if !context.search {
        return failure(name, "联网搜索没有开启，请先在设置里打开 DeepSeek 搜索。");
    }
    let Some(key) = context.api_key.as_deref() else {
        return failure(name, "需要先在设置里填写 DeepSeek API Key。");
    };
    let query = argument(args, "query").unwrap_or_default();
    let count = args.get("count").and_then(Value::as_u64).unwrap_or(5) as u32;
    let need_content = args
        .get("need_content")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match super::web::search(key, &query, count, need_content) {
        Ok(text) => ToolOutcome {
            content: text,
            display: format!("🔎 {}", super::session::truncate(&query, 30)),
            effect: ToolEffect::None,
        },
        Err(error) => failure(name, format!("搜索失败：{error}")),
    }
}

fn run_web_fetch(context: &ToolContext, args: &Value) -> ToolOutcome {
    let name = "web_fetch";
    if !context.fetch {
        return failure(name, "网页抓取没有开启，请先在设置里打开 DeepSeek Fetch。");
    }
    let Some(key) = context.api_key.as_deref() else {
        return failure(name, "需要先在设置里填写 DeepSeek API Key。");
    };
    let url = argument(args, "url").unwrap_or_default();
    match super::web::fetch(key, &url) {
        Ok(text) => ToolOutcome {
            content: text,
            display: format!("🌐 {}", super::session::truncate(&url, 40)),
            effect: ToolEffect::None,
        },
        Err(error) => failure(name, format!("抓取失败：{error}")),
    }
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

/// Runs the command with the platform shell. Windows has no POSIX `sh`, so
/// `cmd.exe` takes over; the console code page is switched to UTF-8 first so
/// non-ASCII output survives the decode step below.
#[cfg(target_os = "windows")]
fn shell_launch(command: &str) -> super::sandbox::Launch {
    super::sandbox::Launch {
        program: std::env::var_os("COMSPEC").map_or_else(
            || PathBuf::from("cmd.exe"),
            PathBuf::from,
        ),
        args: Vec::new(),
        // `cmd` cannot parse Rust's standard argument quoting, so the whole
        // switch line is passed through verbatim.
        raw: Some(format!("/D /S /C \"chcp 65001>nul & {command}\"")),
    }
}

#[cfg(not(target_os = "windows"))]
fn shell_launch(command: &str) -> super::sandbox::Launch {
    super::sandbox::Launch {
        program: PathBuf::from("sh"),
        args: vec!["-c".to_string(), command.to_string()],
        raw: None,
    }
}

fn run_command(
    root: &Path,
    command: &str,
    cwd: &Path,
    timeout: Duration,
    sandboxed: bool,
) -> ToolOutcome {
    let name = "run_command";
    let sandboxed = sandboxed && super::sandbox::SUPPORTED;
    let launch = if sandboxed {
        // Read-only commands get no network at all; anything the user had to
        // approve keeps the connectivity it was approved for.
        let network = risk::command_risk(root, command) != risk::Risk::Safe;
        let policy = super::sandbox::Policy {
            home: root,
            cwd,
            network,
        };
        match super::sandbox::wrap(command, &policy) {
            Ok(launch) => launch,
            // Fail closed: an unavailable sandbox must not silently become an
            // unrestricted shell.
            Err(error) => {
                return failure(
                    name,
                    format!("{error}，命令未执行。请让主人手动在终端里运行：{command}"),
                )
            }
        }
    } else {
        shell_launch(command)
    };

    let mut process = Command::new(&launch.program);
    process
        .args(&launch.args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    if let Some(raw) = launch.raw.as_deref() {
        use std::os::windows::process::CommandExt as _;
        process.raw_arg(raw);
        // The pet has no console window; without this flag `cmd` would flash
        // one for every tool call.
        process.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }

    let mut child = match process.spawn() {
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
    let mode = if sandboxed { "，沙箱内" } else { "" };
    let _ = write!(
        report,
        "$ {command}\n（目录 {}，{status_line}{mode}）\n",
        risk::display_path(root, cwd)
    );
    if !out.trim().is_empty() {
        let _ = write!(report, "stdout:\n{}\n", out.trim_end());
    }
    if !err.trim().is_empty() {
        let _ = write!(report, "stderr:\n{}\n", err.trim_end());
    }
    if sandboxed && (err.contains("Operation not permitted") || err.contains("Permission denied")) {
        report.push_str("（沙箱拦截了越界访问：只允许读写当前目录、临时目录和构建缓存）\n");
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

pub fn execute(app: &AppHandle, ctx: &ToolContext, call: &ToolCall) -> ToolOutcome {
    let args: Value = serde_json::from_str(&call.arguments).unwrap_or(Value::Null);
    let root = ctx.root.as_path();
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
        "add_reminder" => {
            let Some(store) = app.try_state::<Arc<ReminderStore>>() else {
                return failure("add_reminder", "提醒服务还没有准备好。");
            };
            let text = argument(&args, "text").unwrap_or_default();
            let minutes = args
                .get("minutes")
                .and_then(Value::as_u64)
                .map(|value| value.clamp(1, 43_200) as u32);
            let at = argument(&args, "at");
            let due = match resolve_due(minutes, at.as_deref()) {
                Ok(due) => due,
                Err(error) => return failure("add_reminder", error),
            };
            match store.add(&text, due) {
                Ok(reminder) => ToolOutcome {
                    content: format!(
                        "已经记下：{}（{}）。到点我会提醒你。",
                        reminder.text,
                        format_due(reminder.due_at)
                    ),
                    display: format!("⏰ {}", super::session::truncate(&reminder.text, 16)),
                    effect: ToolEffect::None,
                },
                Err(error) => failure("add_reminder", error),
            }
        }
        "list_reminders" => {
            let Some(store) = app.try_state::<Arc<ReminderStore>>() else {
                return failure("list_reminders", "提醒服务还没有准备好。");
            };
            let items = store.list();
            if items.is_empty() {
                ToolOutcome {
                    content: "现在没有待触发的提醒。".to_string(),
                    display: label("list_reminders"),
                    effect: ToolEffect::None,
                }
            } else {
                let lines = items
                    .iter()
                    .take(20)
                    .map(|item| format!("- {}（{}）[{}]", item.text, format_due(item.due_at), item.id))
                    .collect::<Vec<_>>()
                    .join("\n");
                ToolOutcome {
                    content: format!("还有 {} 条提醒：\n{lines}", items.len()),
                    display: format!("📋 {} 条提醒", items.len()),
                    effect: ToolEffect::None,
                }
            }
        }
        "cancel_reminder" => {
            let Some(store) = app.try_state::<Arc<ReminderStore>>() else {
                return failure("cancel_reminder", "提醒服务还没有准备好。");
            };
            let id = argument(&args, "id").unwrap_or_default();
            if id.is_empty() {
                return failure("cancel_reminder", "缺少提醒 id。");
            }
            if store.remove(&id) {
                ToolOutcome {
                    content: format!("提醒 {id} 已取消。"),
                    display: label("cancel_reminder"),
                    effect: ToolEffect::None,
                }
            } else {
                failure("cancel_reminder", "没有找到这个提醒，可能已经触发或被取消了。")
            }
        }
        "play_music" => {
            let query = argument(&args, "query").unwrap_or_default();
            let site = argument(&args, "site").unwrap_or_else(|| "netease".to_string());
            let player = argument(&args, "player").unwrap_or_else(|| {
                if ctx.music_player.is_empty() {
                    "auto".to_string()
                } else {
                    ctx.music_player.clone()
                }
            });
            match crate::music::play(&player, &site, &query) {
                Ok(message) => ToolOutcome {
                    content: message,
                    display: if query.is_empty() {
                        label("play_music")
                    } else {
                        format!("🎵 {}", super::session::truncate(&query, 20))
                    },
                    effect: ToolEffect::None,
                },
                Err(error) => failure("play_music", error),
            }
        }
        "focus_session" => {
            let action = argument(&args, "action").unwrap_or_else(|| "start".to_string());
            if !matches!(action.as_str(), "start" | "stop") {
                return failure("focus_session", "action 只能是 start 或 stop。");
            }
            let minutes = args
                .get("minutes")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .min(120) as u32;
            if let Some(window) = app.get_webview_window(crate::PET_WINDOW_LABEL) {
                let _ = window.emit(
                    "focus-command",
                    json!({ "action": action, "minutes": minutes }),
                );
            }
            ToolOutcome {
                content: if action == "start" {
                    if minutes > 0 {
                        format!("专注计时已开始：{minutes} 分钟。")
                    } else {
                        "专注计时已开始，时长按设置里的专注时长。".to_string()
                    }
                } else {
                    "专注计时已结束，记得起来走走。".to_string()
                },
                display: label("focus_session"),
                effect: ToolEffect::None,
            }
        }
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
        "web_search" => run_web_search(ctx, &args),
        "web_fetch" => run_web_fetch(ctx, &args),
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
            run_command(
                root,
                &command,
                &cwd,
                Duration::from_millis(timeout),
                ctx.sandbox,
            )
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

    fn context(search: bool, fetch: bool) -> ToolContext {
        ToolContext::new(
            PathBuf::from("/tmp"),
            Some("sk-test".to_string()),
            search,
            fetch,
            true,
            "auto".to_string(),
        )
    }

    #[test]
    fn exposes_every_tool_with_a_schema() {
        let specs = specs(&context(false, false));
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
                "add_reminder",
                "list_reminders",
                "cancel_reminder",
                "play_music",
                "focus_session",
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

    #[test]
    fn registers_the_web_tools_only_when_enabled() {
        let names = |context: &ToolContext| -> Vec<String> {
            specs(context)
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item.pointer("/function/name").unwrap().as_str().unwrap().to_string())
                .collect()
        };
        assert!(!names(&context(false, false)).contains(&"web_search".to_string()));
        assert!(!names(&context(false, false)).contains(&"web_fetch".to_string()));
        assert!(names(&context(true, false)).contains(&"web_search".to_string()));
        assert!(!names(&context(true, false)).contains(&"web_fetch".to_string()));
        assert!(names(&context(false, true)).contains(&"web_fetch".to_string()));
    }

    #[test]
    fn web_tools_refuse_when_disabled() {
        let search = run_web_search(&context(false, false), &json!({ "query": "deepseek" }));
        assert!(search.content.contains("没有开启"), "{}", search.content);

        let fetch = run_web_fetch(&context(false, false), &json!({ "url": "https://example.com" }));
        assert!(fetch.content.contains("没有开启"), "{}", fetch.content);

        // Enabled but without a key: the tool explains what is missing instead
        // of trying an unauthenticated request.
        let keyless = ToolContext::new(
            PathBuf::from("/tmp"),
            None,
            true,
            true,
            true,
            "auto".to_string(),
        );
        let search = run_web_search(&keyless, &json!({ "query": "deepseek" }));
        assert!(search.content.contains("API Key"), "{}", search.content);
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

    #[cfg(target_os = "windows")]
    const ECHO_COMMAND: &str = "echo hi there";
    #[cfg(not(target_os = "windows"))]
    const ECHO_COMMAND: &str = "printf 'hi there'";

    #[cfg(target_os = "windows")]
    const RAW_COMMAND: &str = "echo raw";
    #[cfg(not(target_os = "windows"))]
    const RAW_COMMAND: &str = "printf 'raw'";

    #[cfg(target_os = "windows")]
    const SLEEP_COMMAND: &str = "ping -n 10 127.0.0.1 >nul";
    #[cfg(not(target_os = "windows"))]
    const SLEEP_COMMAND: &str = "sleep 5";

    #[test]
    fn runs_commands_and_reports_output() {
        let root = scratch("cmd");
        let ok = run_command(&root, ECHO_COMMAND, &root, Duration::from_secs(5), true);
        assert!(ok.content.contains("hi there"), "{}", ok.content);
        assert!(ok.content.contains("退出码 0"), "{}", ok.content);
        #[cfg(target_os = "macos")]
        assert!(ok.content.contains("沙箱内"), "{}", ok.content);

        let plain = run_command(&root, RAW_COMMAND, &root, Duration::from_secs(5), false);
        assert!(plain.content.contains("raw"), "{}", plain.content);
        assert!(!plain.content.contains("沙箱内"), "{}", plain.content);

        let failed = run_command(&root, "exit 3", &root, Duration::from_secs(5), true);
        assert!(failed.content.contains("退出码 3"), "{}", failed.content);

        let timed_out = run_command(&root, SLEEP_COMMAND, &root, Duration::from_millis(600), true);
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
        assert_eq!(label("add_reminder"), "⏰ 设置提醒");
        assert_eq!(label("mystery"), "🛠 mystery");
    }

}
