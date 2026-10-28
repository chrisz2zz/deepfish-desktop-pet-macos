use std::path::{Path, PathBuf};

use serde_json::Value;

use super::session::ToolCall;

/// How dangerous a tool call is. The agent loop only runs `Safe` calls on its
/// own, asks the user about `NeedsApproval`, and refuses `Blocked` outright.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Risk {
    Safe,
    NeedsApproval,
    Blocked,
}

impl Risk {
    pub fn as_str(self) -> &'static str {
        match self {
            Risk::Safe => "safe",
            Risk::NeedsApproval => "approval",
            Risk::Blocked => "blocked",
        }
    }
}

/// A tool call after classification: what the user is asked to approve and how
/// risky it is.
pub struct Plan {
    pub risk: Risk,
    pub summary: String,
}

impl Plan {
    fn safe(summary: impl Into<String>) -> Self {
        Self {
            risk: Risk::Safe,
            summary: summary.into(),
        }
    }

    fn approval(summary: impl Into<String>) -> Self {
        Self {
            risk: Risk::NeedsApproval,
            summary: summary.into(),
        }
    }

    fn blocked(summary: impl Into<String>) -> Self {
        Self {
            risk: Risk::Blocked,
            summary: summary.into(),
        }
    }
}

/// Files stay inside one root so a confused model cannot roam the disk.
/// `DEEPFISH_TOOL_ROOT` exists for tests and for users who want a narrower or
/// wider sandbox than their home directory.
pub fn tool_root() -> PathBuf {
    std::env::var_os("DEEPFISH_TOOL_ROOT")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Expands `~`, resolves `..` lexically and rejects anything outside the root.
/// Lexical resolution keeps the check working for files that do not exist yet.
pub fn resolve(root: &Path, raw: &str) -> Result<PathBuf, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("缺少路径".to_string());
    }
    if raw.contains('\0') {
        return Err("路径包含非法字符".to_string());
    }
    let expanded = match raw.strip_prefix('~') {
        Some("") => root.to_path_buf(),
        Some(rest) => root.join(rest.trim_start_matches('/')),
        None => PathBuf::from(raw),
    };
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        root.join(expanded)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    if !normalized.starts_with(root) {
        return Err(format!("路径超出允许范围（仅限 {}）", root.display()));
    }
    Ok(normalized)
}

/// Paths that hold credentials. Reading them is never automatic; the user has
/// to say yes, because a chat message must not be able to leak a key.
const SENSITIVE: &[&str] = &[
    ".ssh",
    ".aws",
    ".gnupg",
    ".kube",
    ".docker/config.json",
    ".netrc",
    ".npmrc",
    ".pypirc",
    ".config/gh",
    ".config/gcloud",
    ".config/op",
    ".config/hub",
    "library/keychains",
    "library/application support/com.deepfish.desktop-pet",
    "library/cookies",
    "library/safari",
];

pub fn is_sensitive(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return true;
    };
    let text = relative.to_string_lossy().to_lowercase();
    SENSITIVE.iter().any(|needle| {
        text == *needle || text.starts_with(&format!("{needle}/")) || text.ends_with(&format!("/{needle}"))
    })
}

/// Commands that must never run, no matter what the user approves.
/// Refused even with approval: system-destroying tools, credential readers and
/// anything that can phone home with what it just read.
const BLOCKED_HEADS: &[&str] = &[
    "sudo", "su", "doas", "visudo", "shutdown", "reboot", "halt", "diskutil", "mkfs", "dd",
    "csrutil", "nvram", "kextload", "kextunload", "tccutil", "security", "launchctl", "spctl",
    "fdisk", "newfs", "chflags", "dscl", "pfctl", "systemsetup", "sparsebundle", "osascript",
    "curl", "wget", "nc", "ncat", "telnet", "ssh", "scp", "sftp", "rsync", "ftp",
];

/// Read-only utilities that are safe on their own.
const SAFE_HEADS: &[&str] = &[
    "ls", "pwd", "cat", "head", "tail", "wc", "file", "stat", "du", "df", "tree", "which",
    "whoami", "date", "uname", "sw_vers", "echo", "printf", "basename", "dirname", "realpath",
    "sort", "uniq", "cut", "tr", "jq", "grep", "rg", "find", "diff", "cal", "hostname", "id",
    "printenv",
];

const SAFE_GIT_SUBCOMMANDS: &[&str] = &[
    "status", "diff", "log", "show", "branch", "remote", "rev-parse", "ls-files", "blame",
    "shortlog", "describe", "cat-file", "rev-list", "whatchanged",
];

/// Shell syntax that can hide a second command or write a file.
const SHELL_WRITES: &[&str] = &[">", ">>", "<", "$(", "`", "&&", "||", ";"];

fn shell_words(command: &str) -> Vec<String> {
    command
        .split_whitespace()
        .map(|word| word.trim_matches(|c| c == '"' || c == '\'').to_string())
        .filter(|word| !word.is_empty())
        .collect()
}

fn head_of(segment: &str) -> String {
    shell_words(segment)
        .first()
        .map(|word| word.rsplit('/').next().unwrap_or(word).to_string())
        .unwrap_or_default()
}

/// `grep`/`rg`/`find` are read-only unless asked to write or to run something.
fn argument_is_risky(head: &str, words: &[String]) -> bool {
    let risky = |needle: &str| words.iter().any(|word| word.starts_with(needle));
    match head {
        "find" => [
            "-delete", "-exec", "-execdir", "-ok", "-okdir", "-fprint", "-fls",
        ]
        .iter()
        .any(|flag| risky(flag)),
        "rg" => ["--pre", "--hostname-bin", "--sort-files="]
            .iter()
            .any(|flag| risky(flag)),
        _ => false,
    }
}

fn segment_is_safe(root: &Path, segment: &str) -> bool {
    let words = shell_words(segment);
    let Some(first) = words.first() else {
        return true;
    };
    let head = first.rsplit('/').next().unwrap_or(first).to_string();
    if !SAFE_HEADS.contains(&head.as_str()) && head != "git" && head != "node" {
        return false;
    }
    if argument_is_risky(&head, &words) {
        return false;
    }
    match head.as_str() {
        "git" => match words.get(1) {
            Some(subcommand) if SAFE_GIT_SUBCOMMANDS.contains(&subcommand.as_str()) => {}
            _ => return false,
        },
        "node" => {
            if words.len() > 2 || !matches!(words[1].as_str(), "-v" | "--version") {
                return false;
            }
        }
        _ => {}
    }
    // Reading a credential file through `cat` is still a leak.
    words[1..].iter().all(|word| {
        let looks_like_path = word.contains('/') || word.starts_with('.') || word.starts_with('~');
        if !looks_like_path {
            return true;
        }
        resolve(root, word).is_ok_and(|path| !is_sensitive(root, &path))
    })
}

/// Classifies a shell command. Pipelines of read-only commands stay safe; any
/// redirection, substitution or unknown program needs explicit approval.
pub fn command_risk(root: &Path, command: &str) -> Risk {
    let command = command.trim();
    if command.is_empty() {
        return Risk::Blocked;
    }
    if command.contains('\0') {
        return Risk::Blocked;
    }
    let words = shell_words(command);
    if words.is_empty() {
        return Risk::Blocked;
    }
    let head = head_of(command);
    if BLOCKED_HEADS.contains(&head.as_str()) {
        return Risk::Blocked;
    }
    // `rm -rf /` and friends are refused even though `rm` is not reachable
    // through the safe list.
    if command.contains("rm -rf /") || command.contains("rm -fr /") {
        return Risk::Blocked;
    }
    if SHELL_WRITES.iter().any(|token| command.contains(token)) {
        return Risk::NeedsApproval;
    }
    let segments: Vec<&str> = command
        .split('|')
        .flat_map(|part| part.split("||"))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    if segments.is_empty() {
        return Risk::Blocked;
    }
    if segments.iter().all(|segment| {
        let head = head_of(segment);
        !BLOCKED_HEADS.contains(&head.as_str()) && segment_is_safe(root, segment)
    }) {
        return Risk::Safe;
    }
    Risk::NeedsApproval
}

fn argument(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Decides what has to happen before a tool call runs, and what the approval
/// card should show.
pub fn plan(root: &Path, call: &ToolCall) -> Plan {
    let args: Value = serde_json::from_str(&call.arguments).unwrap_or(Value::Null);
    match call.name.as_str() {
        "list_dir" => {
            let raw = argument(&args, "path");
            let raw = if raw.is_empty() { ".".to_string() } else { raw };
            match resolve(root, &raw) {
                Ok(path) if is_sensitive(root, &path) => Plan::approval(format!(
                    "浏览敏感目录 {}",
                    display_path(root, &path)
                )),
                Ok(path) => Plan::safe(format!("浏览目录 {}", display_path(root, &path))),
                Err(error) => Plan::blocked(error),
            }
        }
        "read_file" => {
            let raw = argument(&args, "path");
            match resolve(root, &raw) {
                Ok(path) if is_sensitive(root, &path) => Plan::approval(format!(
                    "读取敏感文件 {}",
                    display_path(root, &path)
                )),
                Ok(path) => Plan::safe(format!("读取 {}", display_path(root, &path))),
                Err(error) => Plan::blocked(error),
            }
        }
        "write_file" => {
            let raw = argument(&args, "path");
            let bytes = argument(&args, "content").len();
            match resolve(root, &raw) {
                Ok(path) => Plan::approval(format!(
                    "写入 {}（{bytes} 字节）",
                    display_path(root, &path)
                )),
                Err(error) => Plan::blocked(error),
            }
        }
        "run_command" => {
            let command = argument(&args, "command");
            if command.is_empty() {
                return Plan::blocked("缺少要执行的命令".to_string());
            }
            let summary = format!("执行命令：{}", super::session::truncate(&command, 140));
            if let Ok(cwd) = resolve(root, &default_dot(&argument(&args, "cwd"))) {
                if is_sensitive(root, &cwd) {
                    return Plan::approval(format!(
                        "在敏感目录 {} 里{}",
                        display_path(root, &cwd),
                        summary
                    ));
                }
            }
            match command_risk(root, &command) {
                Risk::Safe => Plan::safe(summary),
                Risk::NeedsApproval => Plan::approval(summary),
                Risk::Blocked => Plan::blocked(format!("命令被安全策略禁止：{command}")),
            }
        }
        // Everything the pet can do to itself is harmless.
        _ => Plan::safe(call.name.clone()),
    }
}

fn default_dot(value: &str) -> String {
    if value.is_empty() {
        ".".to_string()
    } else {
        value.to_string()
    }
}

/// Shortens the home prefix so approval cards stay readable.
pub fn display_path(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(relative) if relative.as_os_str().is_empty() => "~".to_string(),
        Ok(relative) => format!("~/{}", relative.display()),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/Users/tester")
    }

    fn call(name: &str, arguments: &str) -> ToolCall {
        ToolCall::new("call_1", name, arguments)
    }

    #[test]
    fn resolves_paths_inside_the_root_only() {
        let root = home();
        assert_eq!(resolve(&root, ".").unwrap(), root);
        assert_eq!(resolve(&root, "~/notes").unwrap(), root.join("notes"));
        assert_eq!(
            resolve(&root, "/Users/tester/a/../b").unwrap(),
            root.join("b")
        );
        assert!(resolve(&root, "../etc/passwd").is_err());
        assert!(resolve(&root, "/etc/passwd").is_err());
        assert!(resolve(&root, "~/../../etc").is_err());
        assert!(resolve(&root, "  ").is_err());
    }

    #[test]
    fn flags_credential_files() {
        let root = home();
        assert!(is_sensitive(&root, &root.join(".ssh/id_ed25519")));
        assert!(is_sensitive(
            &root,
            &root.join("Library/Keychains/login.keychain-db")
        ));
        assert!(is_sensitive(
            &root,
            &root.join("Library/Application Support/com.deepfish.desktop-pet/settings.json")
        ));
        assert!(!is_sensitive(&root, &root.join("Documents/todo.md")));
        // Anything outside the root is treated as sensitive.
        assert!(is_sensitive(&root, Path::new("/etc/passwd")));
    }

    #[test]
    fn keeps_read_only_commands_safe() {
        let root = home();
        assert_eq!(command_risk(&root, "ls -la"), Risk::Safe);
        assert_eq!(command_risk(&root, "git status --short"), Risk::Safe);
        assert_eq!(command_risk(&root, "cat notes.md | grep todo"), Risk::Safe);
        assert_eq!(command_risk(&root, "find . -name '*.md'"), Risk::Safe);
        assert_eq!(command_risk(&root, "node --version"), Risk::Safe);
    }

    #[test]
    fn escalates_writes_unknown_programs_and_substitutions() {
        let root = home();
        assert_eq!(command_risk(&root, "ls > out.txt"), Risk::NeedsApproval);
        assert_eq!(command_risk(&root, "npm install"), Risk::NeedsApproval);
        assert_eq!(command_risk(&root, "echo $(whoami)"), Risk::NeedsApproval);
        assert_eq!(command_risk(&root, "sed -i 's/a/b/' f.txt"), Risk::NeedsApproval);
        assert_eq!(command_risk(&root, "find . -delete"), Risk::NeedsApproval);
        assert_eq!(command_risk(&root, "git push --force"), Risk::NeedsApproval);
        assert_eq!(command_risk(&root, "rm -rf build"), Risk::NeedsApproval);
        assert_eq!(command_risk(&root, "cat ~/.ssh/id_rsa"), Risk::NeedsApproval);
    }

    #[test]
    fn refuses_destructive_commands() {
        let root = home();
        assert_eq!(command_risk(&root, "sudo ls"), Risk::Blocked);
        assert_eq!(command_risk(&root, "security find-generic-password -w"), Risk::Blocked);
        assert_eq!(command_risk(&root, "shutdown -h now"), Risk::Blocked);
        assert_eq!(command_risk(&root, "rm -rf /"), Risk::Blocked);
        assert_eq!(command_risk(&root, ""), Risk::Blocked);
    }

    #[test]
    fn plans_carry_a_readable_summary() {
        let root = home();
        let planned = plan(&root, &call("read_file", r#"{"path":"notes/a.md"}"#));
        assert_eq!(planned.risk, Risk::Safe);
        assert_eq!(planned.summary, "读取 ~/notes/a.md");

        let planned = plan(&root, &call("write_file", r#"{"path":"~/new.txt","content":"hi"}"#));
        assert_eq!(planned.risk, Risk::NeedsApproval);
        assert!(planned.summary.contains("写入 ~/new.txt"));

        let planned = plan(&root, &call("read_file", r#"{"path":"/etc/passwd"}"#));
        assert_eq!(planned.risk, Risk::Blocked);

        let planned = plan(&root, &call("run_command", r#"{"command":"ls -la"}"#));
        assert_eq!(planned.risk, Risk::Safe);

        let planned = plan(&root, &call("run_command", r#"{"command":"npm test"}"#));
        assert_eq!(planned.risk, Risk::NeedsApproval);

        let planned = plan(&root, &call("pet_say", r#"{"text":"hi"}"#));
        assert_eq!(planned.risk, Risk::Safe);
    }
}
