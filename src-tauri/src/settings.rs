use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
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
        let data = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
            .unwrap_or_default();
        Self { path, data }
    }

    pub fn public_value(&self) -> PublicSettings {
        PublicSettings {
            settings: self.data.clone(),
            has_api_key: self.api_key().is_some(),
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

        self.data.scale = clamp(self.data.scale, 0.6, 1.15);
        self.data.behavior_intensity = clamp(self.data.behavior_intensity, 0.5, 1.8);

        if let Some(key) = patch.api_key {
            let key = key.trim();
            if !key.is_empty() {
                keychain::write(key)?;
            }
        }
        if patch.clear_api_key == Some(true) {
            keychain::clear()?;
        }

        self.persist()?;
        Ok(self.public_value())
    }

    pub fn api_key(&self) -> Option<String> {
        keychain::read()
    }

    fn persist(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("无法创建设置目录：{error}"))?;
        }
        let json = serde_json::to_string_pretty(&self.data)
            .map_err(|error| format!("无法序列化设置：{error}"))?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, json).map_err(|error| format!("无法写入设置：{error}"))?;
        fs::rename(&temporary, &self.path).map_err(|error| format!("无法保存设置：{error}"))
    }
}

fn clamp(value: f64, minimum: f64, maximum: f64) -> f64 {
    if value.is_finite() {
        value.clamp(minimum, maximum)
    } else {
        1.0
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod keychain {
    const SERVICE: &str = "deepfish-desktop-pet";
    const ACCOUNT: &str = "api-key";

    fn entry() -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, ACCOUNT)
            .map_err(|error| format!("无法访问系统钥匙串：{error}"))
    }

    pub fn read() -> Option<String> {
        entry()
            .ok()?
            .get_password()
            .ok()
            .filter(|value| !value.is_empty())
    }

    pub fn write(key: &str) -> Result<(), String> {
        entry()?
            .set_password(key)
            .map_err(|error| format!("无法保存 API Key：{error}"))
    }

    pub fn clear() -> Result<(), String> {
        match entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(format!("无法清除 API Key：{error}")),
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod keychain {
    pub fn read() -> Option<String> {
        None
    }

    pub fn write(_key: &str) -> Result<(), String> {
        Err("当前平台不支持系统钥匙串".to_string())
    }

    pub fn clear() -> Result<(), String> {
        Ok(())
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
        assert_eq!(settings.scale, 1.0);
        assert_eq!(settings.behavior_intensity, 1.0);
    }

    #[test]
    fn clamps_out_of_range_values() {
        assert_eq!(clamp(0.2, 0.6, 1.15), 0.6);
        assert_eq!(clamp(3.0, 0.6, 1.15), 1.15);
        assert_eq!(clamp(f64::NAN, 0.6, 1.15), 1.0);
    }
}
