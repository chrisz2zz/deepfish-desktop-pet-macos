//! Music playback that prefers a player the user already has installed.
//!
//! NetEase Cloud Music exposes a public search endpoint, so the pet can resolve
//! a query to a song id and hand it to the client through `orpheus://`. QQ
//! Music has no reliable search deep link, so it is simply launched. When
//! neither client exists the old browser search page is used.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Player {
    Auto,
    Netease,
    Qq,
    Browser,
}

impl Player {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "auto" => Ok(Self::Auto),
            "netease" | "cloudmusic" | "163" => Ok(Self::Netease),
            "qq" | "qqmusic" => Ok(Self::Qq),
            "browser" | "web" => Ok(Self::Browser),
            other => Err(format!("不认识的播放器：{other}")),
        }
    }
}

/// Plays something and returns a sentence the pet can show the user.
pub fn play(player: &str, site: &str, query: &str) -> Result<String, String> {
    let preference = Player::parse(player)?;
    let query = query.trim();
    if query.chars().count() > 80 {
        return Err("搜索词太长".to_string());
    }

    let chosen = match preference {
        Player::Auto => {
            if netease_exe().is_some() {
                Player::Netease
            } else if qq_exe().is_some() {
                Player::Qq
            } else {
                Player::Browser
            }
        }
        Player::Netease if netease_exe().is_some() => Player::Netease,
        Player::Qq if qq_exe().is_some() => Player::Qq,
        Player::Netease | Player::Qq => Player::Browser,
        Player::Browser => Player::Browser,
    };

    match chosen {
        Player::Netease => play_netease(query),
        Player::Qq => play_qq(query),
        Player::Auto | Player::Browser => play_browser(site, query),
    }
}

struct Song {
    id: i64,
    name: String,
    artist: String,
}

fn play_netease(query: &str) -> Result<String, String> {
    let mut chosen = None;
    if !query.is_empty() {
        if let Some(song) = netease_song(query) {
            let link = format!("orpheus://song/{}/?autoplay=1", song.id);
            // A scheme call that silently does nothing still leaves the launch
            // below, so both paths are attempted.
            let _ = open_scheme(&link);
            chosen = Some(song);
        }
    }
    launch_netease()?;
    Ok(match chosen {
        Some(song) if song.artist.is_empty() => {
            format!("已打开本机网易云音乐，并尝试播放《{}》。", song.name)
        }
        Some(song) => format!(
            "已打开本机网易云音乐，并尝试播放《{}》- {}。",
            song.name, song.artist
        ),
        None if query.is_empty() => "已打开本机网易云音乐。".to_string(),
        None => format!("已打开本机网易云音乐，搜一下「{query}」就能听。"),
    })
}

fn play_qq(query: &str) -> Result<String, String> {
    launch_qq()?;
    Ok(if query.is_empty() {
        "已打开本机 QQ 音乐。".to_string()
    } else {
        format!("已打开本机 QQ 音乐，搜一下「{query}」就能听。")
    })
}

fn play_browser(site: &str, query: &str) -> Result<String, String> {
    let url = browser_url(site, query)?;
    crate::open_external_url(&url)?;
    Ok(format!("没有找到本机音乐客户端，已在浏览器打开：{url}"))
}

fn netease_song(query: &str) -> Option<Song> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(6))
        .build()
        .ok()?;
    let response = client
        .get("https://music.163.com/api/search/get/web")
        .query(&[
            ("s", query),
            ("type", "1"),
            ("offset", "0"),
            ("limit", "5"),
        ])
        .header(
            reqwest::header::USER_AGENT,
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36",
        )
        .header(reqwest::header::REFERER, "https://music.163.com/")
        .send()
        .ok()?;
    let value: Value = response.json().ok()?;
    let song = value.pointer("/result/songs/0")?;
    Some(Song {
        id: song.get("id")?.as_i64()?,
        name: song.get("name")?.as_str()?.to_string(),
        artist: song
            .pointer("/artists/0/name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

fn launch_netease() -> Result<(), String> {
    launch_app(netease_exe().ok_or_else(|| "没有找到本机网易云音乐".to_string())?)
}

fn launch_qq() -> Result<(), String> {
    launch_app(qq_exe().ok_or_else(|| "没有找到本机 QQ 音乐".to_string())?)
}

fn launch_app(path: PathBuf) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(&path).spawn();

    #[cfg(all(not(target_os = "macos"), target_os = "windows"))]
    let result = {
        use std::os::windows::process::CommandExt as _;
        let mut command = Command::new(&path);
        // The player is a GUI app; never flash a console window for it.
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        command.spawn()
    };

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let result = Command::new(&path).spawn();

    result
        .map(|_| ())
        .map_err(|error| format!("无法启动 {}：{error}", path.display()))
}

fn open_scheme(url: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let operation: Vec<u16> = "open\0".encode_utf16().collect();
        let target: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: both strings are NUL-terminated and outlive the call.
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        result as isize > 32
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = url;
        false
    }
}

fn browser_url(site: &str, query: &str) -> Result<String, String> {
    let site = site.trim().to_ascii_lowercase();
    let site = if site.is_empty() {
        "netease".to_string()
    } else {
        site
    };
    let query = encode_query(query);
    let url = match site.as_str() {
        "netease" => {
            if query.is_empty() {
                "https://music.163.com/".to_string()
            } else {
                format!("https://music.163.com/#/search/m/?s={query}&type=1")
            }
        }
        "bilibili" => {
            if query.is_empty() {
                "https://www.bilibili.com/".to_string()
            } else {
                format!("https://search.bilibili.com/all?keyword={query}")
            }
        }
        "youtube" => {
            if query.is_empty() {
                "https://music.youtube.com/".to_string()
            } else {
                format!("https://music.youtube.com/search?q={query}")
            }
        }
        "spotify" => {
            if query.is_empty() {
                "https://open.spotify.com/".to_string()
            } else {
                format!("https://open.spotify.com/search/{query}")
            }
        }
        other => return Err(format!("不支持的音乐站点：{other}")),
    };
    Ok(url)
}

/// Percent-encodes a search query without pulling in a URL crate.
fn encode_query(input: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for byte in input.trim().as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(*byte));
            }
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

/// Extracts the executable from a file-association command such as
/// `"C:\...\QQMusic.exe" /play "%1"`.
fn quoted_exe(command: &str) -> Option<String> {
    let trimmed = command.trim();
    if let Some(rest) = trimmed.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some(rest[..end].to_string());
    }
    trimmed.split_whitespace().next().map(str::to_string)
}

#[cfg(target_os = "windows")]
fn netease_exe() -> Option<PathBuf> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    let mut candidates = Vec::new();
    for key in [
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\cloudmusic.exe",
        "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\App Paths\\cloudmusic.exe",
    ] {
        for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            if let Some(path) = registry_string(hive, key) {
                candidates.push(PathBuf::from(path));
            }
        }
    }
    for root in install_roots() {
        candidates.push(root.join("Netease\\CloudMusic\\cloudmusic.exe"));
        candidates.push(root.join("Programs\\Netease\\CloudMusic\\cloudmusic.exe"));
    }
    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(target_os = "windows")]
fn qq_exe() -> Option<PathBuf> {
    use windows_sys::Win32::System::Registry::{
        HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
    };

    let mut candidates = Vec::new();
    for key in [
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\QQMusic.exe",
        "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\App Paths\\QQMusic.exe",
    ] {
        for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            if let Some(path) = registry_string(hive, key) {
                candidates.push(PathBuf::from(path));
            }
        }
    }
    for root in install_roots() {
        for base in ["Tencent\\QQMusic", "Programs\\Tencent\\QQMusic"] {
            let directory = root.join(base);
            candidates.push(directory.join("QQMusic.exe"));
            // Versioned installs nest the binary one level deeper.
            if let Ok(entries) = std::fs::read_dir(&directory) {
                for entry in entries.flatten() {
                    candidates.push(entry.path().join("QQMusic.exe"));
                }
            }
        }
    }
    // The file association knows where the player lives even when the install
    // directory uses an unexpected layout.
    if let Some(command) = registry_string(HKEY_CLASSES_ROOT, "QQMusic.mp3\\shell\\open\\command") {
        if let Some(exe) = quoted_exe(&command) {
            candidates.push(PathBuf::from(exe));
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(target_os = "windows")]
fn install_roots() -> Vec<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
        .iter()
        .filter_map(|name| std::env::var_os(name).map(PathBuf::from))
        .collect()
}

#[cfg(target_os = "windows")]
fn registry_string(
    hive: windows_sys::Win32::System::Registry::HKEY,
    key: &str,
) -> Option<String> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, RRF_RT_REG_SZ};

    let key: Vec<u16> = key.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buffer = vec![0u16; 1024];
    let mut size = u32::try_from(buffer.len() * 2).ok()?;
    // SAFETY: `key` is NUL-terminated, the buffer is writable, and `size` is its
    // capacity in bytes as required by RegGetValueW.
    let status = unsafe {
        RegGetValueW(
            hive,
            key.as_ptr(),
            std::ptr::null(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != 0 {
        return None;
    }
    let length = buffer.iter().position(|value| *value == 0).unwrap_or(buffer.len());
    let value = String::from_utf16_lossy(&buffer[..length]);
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

#[cfg(target_os = "macos")]
fn netease_exe() -> Option<PathBuf> {
    app_candidates(&["网易云音乐.app", "NeteaseMusic.app", "NetEaseMusic.app"])
}

#[cfg(target_os = "macos")]
fn qq_exe() -> Option<PathBuf> {
    app_candidates(&["QQ音乐.app", "QQMusic.app"])
}

#[cfg(target_os = "macos")]
fn app_candidates(names: &[&str]) -> Option<PathBuf> {
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join("Applications"));
    }
    roots
        .iter()
        .flat_map(|root| names.iter().map(move |name| root.join(name)))
        .find(|path| path.is_dir())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn netease_exe() -> Option<PathBuf> {
    None
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn qq_exe() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_player_preferences() {
        assert_eq!(Player::parse("auto").unwrap(), Player::Auto);
        assert_eq!(Player::parse("").unwrap(), Player::Auto);
        assert_eq!(Player::parse("Netease").unwrap(), Player::Netease);
        assert_eq!(Player::parse("163").unwrap(), Player::Netease);
        assert_eq!(Player::parse("QQMusic").unwrap(), Player::Qq);
        assert_eq!(Player::parse("browser").unwrap(), Player::Browser);
        assert!(Player::parse("foobar").is_err());
    }

    #[test]
    fn builds_browser_fallback_urls() {
        assert_eq!(
            browser_url("netease", "周杰伦").unwrap(),
            "https://music.163.com/#/search/m/?s=%E5%91%A8%E6%9D%B0%E4%BC%A6&type=1"
        );
        assert_eq!(
            browser_url("spotify", "lofi beats").unwrap(),
            "https://open.spotify.com/search/lofi%20beats"
        );
        assert_eq!(browser_url("youtube", "").unwrap(), "https://music.youtube.com/");
        assert!(browser_url("unknown", "x").is_err());
    }

    #[test]
    fn extracts_the_executable_from_a_shell_command() {
        assert_eq!(
            quoted_exe(r#""C:\Program Files (x86)\Tencent\QQMusic\QQMusic.exe" /play "%1""#),
            Some(r"C:\Program Files (x86)\Tencent\QQMusic\QQMusic.exe".to_string())
        );
        assert_eq!(
            quoted_exe("cloudmusic.exe %1"),
            Some("cloudmusic.exe".to_string())
        );
        assert_eq!(quoted_exe("   "), None);
    }
}

