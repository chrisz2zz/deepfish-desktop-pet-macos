//! One-shot reminders the pet keeps for the user.
//!
//! Everything lives in one pretty-printed JSON document next to the settings so
//! the pet can survive restarts without a database. The scheduler is a simple
//! polling thread; reminders are removed as soon as they are handed to the
//! renderer, which means a reminder fires exactly once even across restarts.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Local, NaiveDateTime, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};

const MAX_REMINDERS: usize = 200;
const MAX_TEXT: usize = 200;
const MAX_AHEAD_MS: i64 = 90 * 24 * 60 * 60 * 1000;
const MINUTE_MS: i64 = 60_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub id: String,
    pub text: String,
    pub due_at: i64,
    pub created_at: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RemindersFile {
    version: u32,
    reminders: Vec<Reminder>,
}

pub struct ReminderStore {
    path: PathBuf,
    data: Mutex<RemindersFile>,
}

impl ReminderStore {
    pub fn load(directory: &Path) -> Self {
        let path = directory.join("reminders.json");
        let data = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<RemindersFile>(&text).ok())
            .unwrap_or_default();
        Self {
            path,
            data: Mutex::new(data),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RemindersFile> {
        self.data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn add(&self, text: &str, due_at: i64) -> Result<Reminder, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("提醒内容不能为空".to_string());
        }
        if text.chars().count() > MAX_TEXT {
            return Err(format!("提醒内容不能超过 {MAX_TEXT} 个字"));
        }
        let now = now_ms();
        if due_at <= now {
            return Err("提醒时间必须晚于当前时间".to_string());
        }
        if due_at - now > MAX_AHEAD_MS {
            return Err("提醒时间最多只能设在 90 天以内".to_string());
        }

        let mut data = self.lock();
        if data.reminders.len() >= MAX_REMINDERS {
            return Err("提醒太多了，先清理一些吧".to_string());
        }
        let reminder = Reminder {
            id: format!("r{now}-{}", data.reminders.len() + 1),
            text: text.to_string(),
            due_at,
            created_at: now,
        };
        data.reminders.push(reminder.clone());
        data.reminders.sort_by_key(|item| item.due_at);
        data.version = 1;
        self.persist(&data)?;
        Ok(reminder)
    }

    pub fn list(&self) -> Vec<Reminder> {
        let mut items = self.lock().reminders.clone();
        items.sort_by_key(|item| item.due_at);
        items
    }

    pub fn remove(&self, id: &str) -> bool {
        let mut data = self.lock();
        let before = data.reminders.len();
        data.reminders.retain(|item| item.id != id);
        if data.reminders.len() == before {
            return false;
        }
        let _ = self.persist(&data);
        true
    }

    /// Removes and returns every reminder whose time has come. Removing them is
    /// what guarantees a reminder is delivered only once.
    pub fn take_due(&self, now: i64) -> Vec<Reminder> {
        let mut data = self.lock();
        let mut due = Vec::new();
        data.reminders.retain(|item| {
            if item.due_at <= now {
                due.push(item.clone());
                false
            } else {
                true
            }
        });
        if !due.is_empty() {
            let _ = self.persist(&data);
        }
        due.sort_by_key(|item| item.due_at);
        due
    }

    fn persist(&self, data: &RemindersFile) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("无法创建提醒目录：{error}"))?;
        }
        let json = serde_json::to_string_pretty(data)
            .map_err(|error| format!("无法序列化提醒：{error}"))?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, json).map_err(|error| format!("无法写入提醒：{error}"))?;
        fs::rename(&temporary, &self.path).map_err(|error| format!("无法保存提醒：{error}"))
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as i64)
        .unwrap_or(0)
}

/// Resolves the tool/command arguments into an absolute timestamp. `minutes` is
/// relative, `at` accepts a local date-time or a bare clock time (today, or
/// tomorrow when that time has already passed).
pub fn resolve_due(minutes: Option<u32>, at: Option<&str>) -> Result<i64, String> {
    if let Some(minutes) = minutes {
        let minutes = minutes.clamp(1, 43_200);
        return Ok(now_ms() + i64::from(minutes) * MINUTE_MS);
    }
    if let Some(at) = at.map(str::trim).filter(|value| !value.is_empty()) {
        return parse_local_time(at).ok_or_else(|| {
            "看不懂这个时间，请用“2026-10-28 20:30”或“20:30”的格式".to_string()
        });
    }
    Err("请给出提醒时间（相对分钟数或本地时间）".to_string())
}

fn parse_local_time(text: &str) -> Option<i64> {
    let now = Local::now();
    for format in ["%Y-%m-%d %H:%M", "%Y-%m-%d %H:%M:%S", "%Y/%m/%d %H:%M"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(text, format) {
            return Local
                .from_local_datetime(&naive)
                .single()
                .map(|due| due.timestamp_millis());
        }
    }
    if let Ok(time) = NaiveTime::parse_from_str(text, "%H:%M") {
        let today = now.date_naive().and_time(time);
        let mut due = Local.from_local_datetime(&today).single()?;
        if due <= now {
            due += chrono::Duration::days(1);
        }
        return Some(due.timestamp_millis());
    }
    None
}

pub fn format_due(due_at: i64) -> String {
    Local
        .timestamp_millis_opt(due_at)
        .single()
        .map(|due| due.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "未知时间".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "deepfish-reminders-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn adds_lists_removes_and_reloads() {
        let dir = scratch("crud");
        let store = ReminderStore::load(&dir);
        let reminder = store.add("喝水", now_ms() + 60_000).unwrap();
        assert_eq!(store.list().len(), 1);

        let reloaded = ReminderStore::load(&dir);
        assert_eq!(reloaded.list().len(), 1);
        assert_eq!(reloaded.list()[0].text, "喝水");

        assert!(store.remove(&reminder.id));
        assert!(store.list().is_empty());
        assert!(!store.remove("missing"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_invalid_content_and_times() {
        let dir = scratch("invalid");
        let store = ReminderStore::load(&dir);
        assert!(store.add("   ", now_ms() + 60_000).is_err());
        assert!(store.add("过去", now_ms() - 1_000).is_err());
        assert!(store.add(&"长".repeat(MAX_TEXT + 1), now_ms() + 60_000).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn take_due_delivers_each_reminder_once() {
        let dir = scratch("due");
        let store = ReminderStore::load(&dir);
        store.add("早点", now_ms() + 60_000).unwrap();
        let late = store.add("晚点", now_ms() + 120_000).unwrap();

        let due = store.take_due(late.due_at - 1);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].text, "早点");
        assert_eq!(store.list().len(), 1, "只剩还没到点的那条");

        assert_eq!(store.take_due(late.due_at).len(), 1);
        assert!(store.list().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolves_relative_and_local_times() {
        let now = now_ms();
        let soon = resolve_due(Some(30), None).unwrap();
        assert!(soon - now >= 29 * MINUTE_MS);
        assert!(resolve_due(None, Some("20:30")).is_ok());
        assert!(resolve_due(None, Some("2026-01-02 03:04")).is_ok());
        assert!(resolve_due(None, Some("下午三点")).is_err());
        assert!(resolve_due(None, None).is_err());
    }
}
