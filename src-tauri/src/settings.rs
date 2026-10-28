use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Defaults for the agent loop guards. They keep a confused model from spinning
/// forever, and both are editable in the settings window.
pub const DEFAULT_AGENT_LOOP_LIMIT: u32 = 50;
pub const DEFAULT_TOOL_CALL_LIMIT: u32 = 10;
const MAX_AGENT_LOOP_LIMIT: u32 = 200;
const MAX_TOOL_CALL_LIMIT: u32 = 100;

/// DeepSeek's OpenAI-compatible chat endpoint. The `/v1` form appears in older
/// configs and in copy-pasted snippets, so it is rewritten on load.
pub const DEEPSEEK_ENDPOINT: &str = "https://api.deepseek.com/chat/completions";
const LEGACY_DEEPSEEK_ENDPOINT: &str = "https://api.deepseek.com/v1/chat/completions";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[allow(clippy::struct_excessive_bools)] // feature toggles are genuinely independent
pub struct Settings {
    pub provider: String,
    pub endpoint: String,
    pub model: String,
    pub always_on_top: bool,
    pub scale: f64,
    pub sound: bool,
    pub free_walk: bool,
    pub interactive_zones: bool,
    pub spicy_lines: bool,
    pub behavior_intensity: f64,
    /// How many model round-trips one user question may take.
    pub agent_loop_limit: u32,
    /// How many tool calls one user question may spend in total.
    pub tool_call_limit: u32,
    /// DeepSeek server-side web search (billed per request).
    pub deepseek_search: bool,
    /// DeepSeek server-side page fetch (billed per request).
    pub deepseek_fetch: bool,
    /// Kept in the local settings file only; never sent to the renderer.
    pub api_key: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: "pollinations".to_string(),
            endpoint: "https://text.pollinations.ai/openai".to_string(),
            model: "openai-fast".to_string(),
            always_on_top: true,
            scale: 1.0,
            sound: true,
            free_walk: true,
            interactive_zones: true,
            spicy_lines: true,
            behavior_intensity: 1.0,
            agent_loop_limit: DEFAULT_AGENT_LOOP_LIMIT,
            tool_call_limit: DEFAULT_TOOL_CALL_LIMIT,
            deepseek_search: false,
            deepseek_fetch: false,
            api_key: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicSettings {
    #[serde(flatten)]
    pub settings: Settings,
    pub has_api_key: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SettingsPatch {
    pub provider: Option<String>,
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub always_on_top: Option<bool>,
    pub scale: Option<f64>,
    pub sound: Option<bool>,
    pub free_walk: Option<bool>,
    pub interactive_zones: Option<bool>,
    pub spicy_lines: Option<bool>,
    pub behavior_intensity: Option<f64>,
    pub agent_loop_limit: Option<u32>,
    pub tool_call_limit: Option<u32>,
    pub deepseek_search: Option<bool>,
    pub deepseek_fetch: Option<bool>,
    pub api_key: Option<String>,
    pub clear_api_key: Option<bool>,
}

pub struct SettingsStore {
    path: PathBuf,
    pub data: Settings,
}

impl SettingsStore {
    pub fn load(directory: &Path) -> Self {
        let path = directory.join("settings.json");
        let mut data = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
            .unwrap_or_default();
        // Old builds shipped the `/v1` variant; fold it into the canonical URL
        // so users do not keep a stale address forever.
        let legacy_endpoint = data.endpoint.trim().trim_end_matches('/') == LEGACY_DEEPSEEK_ENDPOINT;
        if legacy_endpoint {
            data.endpoint = DEEPSEEK_ENDPOINT.to_string();
        }
        let store = Self { path, data };
        if legacy_endpoint {
            let _ = store.persist();
        }
        store
    }

    pub fn public_value(&self) -> PublicSettings {
        let mut settings = self.data.clone();
        let has_api_key = self.api_key().is_some();
        settings.api_key.clear();
        PublicSettings {
            settings,
            has_api_key,
        }
    }

    pub fn save(&mut self, patch: SettingsPatch) -> Result<PublicSettings, String> {
        if let Some(value) = patch.provider {
            self.data.provider = value;
        }
        if let Some(value) = patch.endpoint {
            self.data.endpoint = value;
        }
        if let Some(value) = patch.model {
            self.data.model = value;
        }
        if let Some(value) = patch.always_on_top {
            self.data.always_on_top = value;
        }
        if let Some(value) = patch.sound {
            self.data.sound = value;
        }
        if let Some(value) = patch.free_walk {
            self.data.free_walk = value;
        }
        if let Some(value) = patch.interactive_zones {
            self.data.interactive_zones = value;
        }
        if let Some(value) = patch.spicy_lines {
            self.data.spicy_lines = value;
        }
        if let Some(value) = patch.scale {
            self.data.scale = value;
        }
        if let Some(value) = patch.behavior_intensity {
            self.data.behavior_intensity = value;
        }
        if let Some(value) = patch.agent_loop_limit {
            self.data.agent_loop_limit = value;
        }
        if let Some(value) = patch.tool_call_limit {
            self.data.tool_call_limit = value;
        }
        if let Some(value) = patch.deepseek_search {
            self.data.deepseek_search = value;
        }
        if let Some(value) = patch.deepseek_fetch {
            self.data.deepseek_fetch = value;
        }

        self.data.scale = clamp(self.data.scale, 0.6, 1.15);
        self.data.behavior_intensity = clamp(self.data.behavior_intensity, 0.5, 1.8);
        self.data.agent_loop_limit = self.data.agent_loop_limit.clamp(1, MAX_AGENT_LOOP_LIMIT);
        self.data.tool_call_limit = self.data.tool_call_limit.clamp(1, MAX_TOOL_CALL_LIMIT);

        if let Some(key) = patch.api_key {
            let key = key.trim();
            if !key.is_empty() {
                self.data.api_key = key.to_string();
            }
        }
        if patch.clear_api_key == Some(true) {
            self.data.api_key.clear();
        }

        self.persist()?;
        Ok(self.public_value())
    }

    pub fn api_key(&self) -> Option<String> {
        let key = self.data.api_key.trim();
        (!key.is_empty()).then(|| key.to_string())
    }

    fn persist(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("无法创建设置目录：{error}"))?;
        }
        let json = serde_json::to_string_pretty(&self.data)
            .map_err(|error| format!("无法序列化设置：{error}"))?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, json).map_err(|error| format!("无法写入设置：{error}"))?;
        // The file holds the API key, so keep it private to the current user.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600));
        }
        fs::rename(&temporary, &self.path).map_err(|error| format!("无法保存设置：{error}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}

fn clamp(value: f64, minimum: f64, maximum: f64) -> f64 {
    if value.is_finite() {
        value.clamp(minimum, maximum)
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_electron_build() {
        let settings = Settings::default();
        assert_eq!(settings.provider, "pollinations");
        assert_eq!(settings.endpoint, "https://text.pollinations.ai/openai");
        assert_eq!(settings.model, "openai-fast");
        assert!(settings.always_on_top && settings.free_walk && settings.interactive_zones);
        assert!((settings.scale - 1.0).abs() < f64::EPSILON);
        assert!((settings.behavior_intensity - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn keeps_the_api_key_out_of_public_settings() {
        let dir = std::env::temp_dir().join(format!("deepfish-settings-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut store = SettingsStore::load(&dir);
        let saved = store
            .save(SettingsPatch {
                api_key: Some("  sk-test  ".to_string()),
                ..SettingsPatch::default()
            })
            .unwrap();
        assert!(saved.has_api_key);
        assert_eq!(store.api_key().as_deref(), Some("sk-test"));

        let text = fs::read_to_string(dir.join("settings.json")).unwrap();
        assert!(text.contains("sk-test"));
        assert!(!serde_json::to_string(&saved).unwrap().contains("sk-test"));

        let cleared = store
            .save(SettingsPatch {
                clear_api_key: Some(true),
                ..SettingsPatch::default()
            })
            .unwrap();
        assert!(!cleared.has_api_key);
        assert!(store.api_key().is_none());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.join("settings.json")).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn migrates_the_legacy_deepseek_endpoint() {
        let dir = std::env::temp_dir().join(format!("deepfish-legacy-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("settings.json"),
            r#"{"provider":"deepseek","endpoint":"https://api.deepseek.com/v1/chat/completions"}"#,
        )
        .unwrap();

        let store = SettingsStore::load(&dir);
        assert_eq!(store.data.endpoint, DEEPSEEK_ENDPOINT);
        let saved = fs::read_to_string(dir.join("settings.json")).unwrap();
        assert!(saved.contains(DEEPSEEK_ENDPOINT));
        assert!(!saved.contains("/v1/chat"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn keeps_agent_limits_within_range() {
        let dir = std::env::temp_dir().join(format!("deepfish-limits-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut store = SettingsStore::load(&dir);
        assert_eq!(store.data.agent_loop_limit, DEFAULT_AGENT_LOOP_LIMIT);
        assert_eq!(store.data.tool_call_limit, DEFAULT_TOOL_CALL_LIMIT);

        let saved = store
            .save(SettingsPatch {
                agent_loop_limit: Some(0),
                tool_call_limit: Some(0),
                ..SettingsPatch::default()
            })
            .unwrap();
        assert_eq!(saved.settings.agent_loop_limit, 1);
        assert_eq!(saved.settings.tool_call_limit, 1);

        let saved = store
            .save(SettingsPatch {
                agent_loop_limit: Some(9_999),
                tool_call_limit: Some(9_999),
                ..SettingsPatch::default()
            })
            .unwrap();
        assert_eq!(saved.settings.agent_loop_limit, MAX_AGENT_LOOP_LIMIT);
        assert_eq!(saved.settings.tool_call_limit, MAX_TOOL_CALL_LIMIT);

        let saved = store
            .save(SettingsPatch {
                agent_loop_limit: Some(12),
                tool_call_limit: Some(4),
                ..SettingsPatch::default()
            })
            .unwrap();
        assert_eq!(saved.settings.agent_loop_limit, 12);
        assert_eq!(saved.settings.tool_call_limit, 4);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn remembers_the_deepseek_web_tool_switches() {
        let dir = std::env::temp_dir().join(format!("deepfish-web-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut store = SettingsStore::load(&dir);
        assert!(!store.data.deepseek_search && !store.data.deepseek_fetch);

        let saved = store
            .save(SettingsPatch {
                deepseek_search: Some(true),
                ..SettingsPatch::default()
            })
            .unwrap();
        assert!(saved.settings.deepseek_search);
        assert!(!saved.settings.deepseek_fetch);

        let saved = store
            .save(SettingsPatch {
                deepseek_fetch: Some(true),
                ..SettingsPatch::default()
            })
            .unwrap();
        assert!(saved.settings.deepseek_search && saved.settings.deepseek_fetch);

        // Survives a reload from disk.
        let reloaded = SettingsStore::load(&dir);
        assert!(reloaded.data.deepseek_search && reloaded.data.deepseek_fetch);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn clamps_out_of_range_values() {
        assert!((clamp(0.2, 0.6, 1.15) - 0.6).abs() < f64::EPSILON);
        assert!((clamp(3.0, 0.6, 1.15) - 1.15).abs() < f64::EPSILON);
        assert!((clamp(f64::NAN, 0.6, 1.15) - 1.0).abs() < f64::EPSILON);
    }
}
