//! Seatbelt sandbox around `run_command`.
//!
//! Approved commands still run with the user's privileges, so they are wrapped
//! in a macOS sandbox profile: reads are limited to system paths plus the parts
//! of the home directory that do not hold credentials, writes are limited to
//! the command's working directory plus temp/cache directories, and networking
//! is granted per call. The profile is applied by the system `sandbox-exec`
//! helper, so nothing is linked into the app and the restriction disappears
//! together with the child process.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::risk;

/// The system helper that applies a Seatbelt profile before `exec`.
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// Seatbelt is macOS-only, so other platforms keep the previous behaviour
/// instead of refusing to run anything.
pub const SUPPORTED: bool = cfg!(target_os = "macos");

/// Locations every toolchain has to read, and whose binaries we let it run.
const SYSTEM_READ: &[&str] = &[
    "/bin",
    "/sbin",
    "/usr",
    "/System",
    "/Library",
    "/Applications",
    "/private/etc",
    "/private/var/db",
    "/private/var/select",
    "/dev",
    // Homebrew, MacPorts and anything else installed under /opt.
    "/opt",
];

/// Package-manager and build caches, which need to stay writable or installs
/// and builds fail with `EACCES`.
const CACHE_WRITE: &[&str] = &[
    "library/caches",
    ".cache",
    ".npm",
    ".yarn",
    ".pnpm-store",
    ".cargo",
    ".rustup",
    ".gradle",
    ".m2",
    ".bun",
    ".deno",
    ".local/share/pnpm",
];

pub struct Policy<'a> {
    /// Home directory the sensitive-path list is relative to.
    pub home: &'a Path,
    pub cwd: &'a Path,
    /// Off for commands the risk classifier considers read-only.
    pub network: bool,
}

/// A command line ready for `std::process::Command`.
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// Wraps `command` so it runs inside the sandbox.
pub fn wrap(command: &str, policy: &Policy<'_>) -> Result<Launch, String> {
    wrap_with(Path::new(SANDBOX_EXEC), command, policy)
}

fn wrap_with(program: &Path, command: &str, policy: &Policy<'_>) -> Result<Launch, String> {
    if !program.is_file() {
        return Err(format!("找不到系统沙箱工具 {}", program.display()));
    }
    let profile = profile(policy);
    Ok(Launch {
        program: program.to_path_buf(),
        args: vec![
            "-p".to_string(),
            profile,
            "/bin/sh".to_string(),
            "-c".to_string(),
            command.to_string(),
        ],
    })
}

/// The rule set, starting from Apple's own baseline (`system.sb`) so that
/// `dyld` and the shell can start at all, then denying everything else.
fn profile(policy: &Policy<'_>) -> String {
    let mut profile = String::from(
        "(version 1)\n\
         (import \"system.sb\")\n\
         (deny default)\n\
         (allow mach*)\n\
         (allow ipc*)\n\
         (allow signal (target others))\n\
         (allow process-fork)\n\
         (allow sysctl*)\n\
         (allow system*)\n\
         (allow file-read-metadata)\n\
         (allow file-write-data (literal \"/dev/null\") (literal \"/dev/tty\") (subpath \"/dev/fd\"))\n",
    );

    let mut readable = system_paths(SYSTEM_READ);
    readable.extend(canonical(policy.cwd));
    readable.extend(visible_home_paths(policy.home));
    for path in &readable {
        rule(&mut profile, "file-read*", path);
        // Running a binary needs `process-exec`; restricting it to the same
        // paths keeps the sandbox from spawning anything it imported.
        rule(&mut profile, "process-exec", path);
    }

    // `$TMPDIR` rather than the whole `/private/var/folders` tree: the latter
    // would hand over every other user's temporary directories as well.
    let mut writable = system_paths(&["/private/tmp"]);
    writable.extend(canonical(policy.cwd));
    writable.extend(canonical(&std::env::temp_dir()));
    writable.extend(
        CACHE_WRITE
            .iter()
            .filter_map(|name| canonical(&policy.home.join(name))),
    );
    for path in &writable {
        rule(&mut profile, "file-write*", path);
    }

    if policy.network {
        profile.push_str("(allow network*)\n");
    }
    profile
}

fn rule(profile: &mut String, operation: &str, path: &Path) {
    let _ = writeln!(profile, "(allow {operation} (subpath \"{}\"))", escape(path));
}

fn system_paths(paths: &[&str]) -> Vec<PathBuf> {
    paths
        .iter()
        .filter_map(|path| canonical(Path::new(path)))
        .collect()
}

/// Drops paths that do not exist: an unknown subpath makes `sandbox-exec`
/// abort instead of simply matching nothing.
fn canonical(path: &Path) -> Option<PathBuf> {
    path.canonicalize().ok()
}

/// Everything in the home directory except the credential paths in
/// [`risk`]. A directory that only *contains* a sensitive entry is opened one
/// level deeper instead of being allowed wholesale.
fn visible_home_paths(home: &Path) -> Vec<PathBuf> {
    let mut allowed = Vec::new();
    expand(home, home, &mut allowed);
    allowed
}

fn expand(base: &Path, directory: &Path, allowed: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(base) else {
            continue;
        };
        let relative = relative.to_string_lossy().to_lowercase();
        if risk::SENSITIVE.contains(&relative.as_str()) {
            continue;
        }
        let nested = risk::SENSITIVE
            .iter()
            .any(|sensitive| sensitive.starts_with(&format!("{relative}/")));
        if nested {
            expand(base, &path, allowed);
        } else if let Some(path) = canonical(&path) {
            allowed.push(path);
        }
    }
}

fn escape(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy<'a>(home: &'a Path, cwd: &'a Path) -> Policy<'a> {
        Policy {
            home,
            cwd,
            network: false,
        }
    }

    #[test]
    fn hides_credential_paths_but_keeps_their_siblings() {
        let home = std::env::temp_dir().join(format!("deepfish-sbx-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".ssh")).unwrap();
        std::fs::write(home.join(".ssh").join("id_ed25519"), "secret").unwrap();
        std::fs::create_dir_all(home.join(".config/gh")).unwrap();
        std::fs::create_dir_all(home.join(".config/nvim")).unwrap();
        std::fs::create_dir_all(home.join("Library/Keychains")).unwrap();
        std::fs::create_dir_all(home.join("Library/Caches")).unwrap();
        std::fs::write(home.join("notes.txt"), "ok").unwrap();

        let visible = visible_home_paths(&home);
        let contains = |suffix: &str| visible.iter().any(|path| path.ends_with(suffix));

        assert!(contains("notes.txt"));
        assert!(contains(".config/nvim"));
        assert!(contains("Caches"));
        assert!(!contains(".ssh") && !contains("id_ed25519"));
        assert!(!contains("gh"));
        assert!(!contains("Keychains"));

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn network_is_only_granted_when_asked() {
        let home = Path::new("/tmp");
        let cwd = Path::new("/tmp");
        let quiet = profile(&policy(home, cwd));
        assert!(!quiet.contains("(allow network*)"));

        let loud = profile(&Policy {
            home,
            cwd,
            network: true,
        });
        assert!(loud.contains("(allow network*)"));
        assert!(loud.contains("(deny default)"));
    }

    #[test]
    fn fails_closed_without_the_helper() {
        let error = wrap_with(
            Path::new("/nonexistent/sandbox-exec"),
            "echo hi",
            &policy(Path::new("/tmp"), Path::new("/tmp")),
        )
        .err()
        .unwrap();
        assert!(error.contains("沙箱"));
    }

    /// Runs `command` through the generated profile. The temp directory is
    /// writable by design, so every test here works in `/Users/Shared` where a
    /// stray file would still be caught by the sandbox.
    #[cfg(target_os = "macos")]
    fn run(command: &str, policy: &Policy<'_>) -> std::process::Output {
        let launch = wrap(command, policy).unwrap();
        std::process::Command::new(&launch.program)
            .args(&launch.args)
            .current_dir(policy.cwd)
            .output()
            .unwrap()
    }

    #[cfg(target_os = "macos")]
    fn shared_home(tag: &str) -> PathBuf {
        let base = Path::new("/Users/Shared").join(format!("deepfish-sbx-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        base
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn confines_writes_and_credential_reads() {
        let base = shared_home("run");
        let home = base.join("home");
        let cwd = base.join("work");
        if std::fs::create_dir_all(home.join(".ssh")).is_err()
            || std::fs::create_dir_all(&cwd).is_err()
        {
            return;
        }
        std::fs::write(home.join(".ssh").join("id_ed25519"), "secret").unwrap();

        let out = run(
            "echo inside > inside.txt; echo outside > ../home/escaped.txt; cat ../home/.ssh/id_ed25519 > stolen.txt",
            &policy(&home, &cwd),
        );

        assert!(!out.status.success(), "越界访问应当让命令失败");
        assert!(cwd.join("inside.txt").is_file(), "工作目录内应当可写");
        assert!(!home.join("escaped.txt").exists(), "工作目录外必须拒绝写入");
        assert_eq!(
            std::fs::read_to_string(cwd.join("stolen.txt")).unwrap_or_default(),
            "",
            "凭据文件必须读不到"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn blocks_network_unless_the_call_was_approved() {
        let base = shared_home("net");
        let home = base.join("home");
        let cwd = base.join("work");
        if std::fs::create_dir_all(&home).is_err() || std::fs::create_dir_all(&cwd).is_err() {
            return;
        }

        let offline = run("ping -c 1 -W 1000 127.0.0.1", &policy(&home, &cwd));
        assert!(
            String::from_utf8_lossy(&offline.stderr).contains("not permitted"),
            "只读命令不应有网络"
        );

        let online = run(
            "ping -c 1 -W 1000 127.0.0.1",
            &Policy {
                home: &home,
                cwd: &cwd,
                network: true,
            },
        );
        assert!(online.status.success(), "审批过的命令应当可以联网");

        let _ = std::fs::remove_dir_all(&base);
    }
}

