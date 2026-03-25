//! User settings storage.
//!
//! Stored as JSON in `~/Notes/.p2p/settings.json`.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::CoreError;

const SETTINGS_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceSettings {
    #[serde(default = "default_theme_mode")]
    pub mode: ThemeMode,

    #[serde(default = "default_accent")]
    pub accent: String,
}

/// Application settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,

    /// Display name for this peer (shown to others).
    #[serde(default = "default_display_name")]
    pub display_name: String,

    /// Custom relay server URLs (in addition to N0 defaults).
    #[serde(default)]
    pub custom_relays: Vec<String>,

    /// UI appearance preference.
    #[serde(default)]
    pub appearance: AppearanceSettings,

    /// Editor font size.
    #[serde(default = "default_font_size")]
    pub font_size: u32,

    /// Whether to auto-save (always true, but can be toggled).
    #[serde(default = "default_true")]
    pub auto_save: bool,

    /// Auto-save interval in seconds.
    #[serde(default = "default_save_interval")]
    pub save_interval_secs: u32,

    /// Large document warning threshold (words).
    #[serde(default = "default_large_doc_threshold")]
    pub large_doc_warning_words: u32,

    /// Idle document timeout in seconds (0 = disabled).
    #[serde(default)]
    pub idle_doc_timeout_secs: u32,
}

fn default_display_name() -> String {
    whoami::fallible::hostname().unwrap_or_else(|_| "Unknown".to_string())
}
fn default_schema_version() -> u32 {
    SETTINGS_SCHEMA_VERSION
}
fn default_theme_mode() -> ThemeMode {
    ThemeMode::System
}
fn default_accent() -> String {
    "amber".to_string()
}
fn default_font_size() -> u32 {
    16
}
fn default_true() -> bool {
    true
}
fn default_save_interval() -> u32 {
    5
}
fn default_large_doc_threshold() -> u32 {
    10000
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: default_schema_version(),
            display_name: default_display_name(),
            custom_relays: vec![],
            appearance: AppearanceSettings::default(),
            font_size: default_font_size(),
            auto_save: true,
            save_interval_secs: default_save_interval(),
            large_doc_warning_words: default_large_doc_threshold(),
            idle_doc_timeout_secs: 0,
        }
    }
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            mode: default_theme_mode(),
            accent: default_accent(),
        }
    }
}

fn parse_theme_mode(value: Option<&Value>) -> ThemeMode {
    match value.and_then(Value::as_str) {
        Some("light") => ThemeMode::Light,
        Some("dark") => ThemeMode::Dark,
        _ => ThemeMode::System,
    }
}

fn normalize_accent(value: &str) -> String {
    match value {
        "amber" | "slate" | "clay" | "olive" => value.to_string(),
        _ => default_accent(),
    }
}

fn parse_accent(value: Option<&Value>) -> String {
    match value.and_then(Value::as_str) {
        Some(valid) => normalize_accent(valid),
        _ => default_accent(),
    }
}

impl AppSettings {
    pub fn normalized(mut self) -> Self {
        self.schema_version = default_schema_version();
        self.appearance.accent = normalize_accent(&self.appearance.accent);
        self
    }

    fn from_value(value: Value) -> Self {
        let defaults = Self::default();
        let default_display_name = defaults.display_name.clone();
        let default_custom_relays = defaults.custom_relays.clone();
        let default_font_size = defaults.font_size;
        let default_auto_save = defaults.auto_save;
        let default_save_interval = defaults.save_interval_secs;
        let default_large_doc_warning_words = defaults.large_doc_warning_words;
        let default_idle_doc_timeout_secs = defaults.idle_doc_timeout_secs;
        let obj = match value {
            Value::Object(map) => map,
            _ => return defaults,
        };

        let appearance_value = obj.get("appearance");
        let legacy_theme = obj.get("theme");

        let appearance = match appearance_value {
            Some(Value::Object(map)) => AppearanceSettings {
                mode: parse_theme_mode(map.get("mode")),
                accent: parse_accent(map.get("accent")),
            },
            _ => AppearanceSettings {
                mode: parse_theme_mode(legacy_theme),
                accent: default_accent(),
            },
        };

        Self {
            schema_version: default_schema_version(),
            display_name: obj
                .get("displayName")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or(default_display_name),
            custom_relays: obj
                .get("customRelays")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                        .collect()
                })
                .unwrap_or(default_custom_relays),
            appearance,
            font_size: obj
                .get("fontSize")
                .and_then(Value::as_u64)
                .map(|v| v as u32)
                .unwrap_or(default_font_size),
            auto_save: obj
                .get("autoSave")
                .and_then(Value::as_bool)
                .unwrap_or(default_auto_save),
            save_interval_secs: obj
                .get("saveIntervalSecs")
                .and_then(Value::as_u64)
                .map(|v| v as u32)
                .unwrap_or(default_save_interval),
            large_doc_warning_words: obj
                .get("largeDocWarningWords")
                .and_then(Value::as_u64)
                .map(|v| v as u32)
                .unwrap_or(default_large_doc_warning_words),
            idle_doc_timeout_secs: obj
                .get("idleDocTimeoutSecs")
                .and_then(Value::as_u64)
                .map(|v| v as u32)
                .unwrap_or(default_idle_doc_timeout_secs),
        }
        .normalized()
    }
}

impl AppSettings {
    /// Load settings from disk, or return defaults if not found.
    pub async fn load(base_dir: &Path) -> Self {
        let path = base_dir.join(".p2p").join("settings.json");
        match tokio::fs::read_to_string(&path).await {
            Ok(json) => serde_json::from_str::<Value>(&json)
                .map(Self::from_value)
                .unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save settings to disk.
    pub async fn save(&self, base_dir: &Path) -> Result<(), CoreError> {
        let path = base_dir.join(".p2p").join("settings.json");
        tokio::fs::create_dir_all(path.parent().unwrap()).await?;
        let json = serde_json::to_string_pretty(self)?;
        // Atomic write
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp_path = std::path::PathBuf::from(tmp);
        tokio::fs::write(&tmp_path, &json).await?;
        tokio::fs::rename(&tmp_path, &path).await?;
        Ok(())
    }

    /// Count words in a text string (simple whitespace split).
    pub fn count_words(text: &str) -> u32 {
        text.split_whitespace().count() as u32
    }

    /// Check if a document exceeds the large document warning threshold.
    pub fn is_large_document(&self, text: &str) -> bool {
        Self::count_words(text) >= self.large_doc_warning_words
    }

    /// Get the degradation level for a document based on word count.
    pub fn degradation_level(&self, text: &str) -> DegradationLevel {
        let words = Self::count_words(text);
        if words >= 25000 {
            DegradationLevel::PerformanceMode
        } else if words >= 20000 {
            DegradationLevel::BatchSync
        } else if words >= 15000 {
            DegradationLevel::ReducedFeatures
        } else if words >= self.large_doc_warning_words {
            DegradationLevel::Warning
        } else {
            DegradationLevel::Normal
        }
    }
}

/// Progressive degradation levels for large documents (per AGENTS.md §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DegradationLevel {
    /// Normal operation.
    Normal,
    /// 10k+ words: warning banner suggesting split.
    Warning,
    /// 15k+ words: disable collaboration decorations.
    ReducedFeatures,
    /// 20k+ words: batch-only sync (5s intervals).
    BatchSync,
    /// 25k+ words: performance mode, non-essential plugins disabled.
    PerformanceMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let settings = AppSettings::default();
        assert_eq!(settings.font_size, 16);
        assert_eq!(settings.appearance.mode, ThemeMode::System);
        assert_eq!(settings.appearance.accent, "amber");
        assert!(settings.auto_save);
    }

    #[test]
    fn test_serde_roundtrip() {
        let settings = AppSettings::default();
        let json = serde_json::to_string(&settings).unwrap();
        let loaded: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.font_size, settings.font_size);
        assert_eq!(loaded.appearance.accent, settings.appearance.accent);
    }

    #[test]
    fn test_load_legacy_theme_string() {
        let legacy = serde_json::json!({
            "displayName": "tim",
            "theme": "dark"
        });

        let loaded = AppSettings::from_value(legacy);
        assert_eq!(loaded.display_name, "tim");
        assert_eq!(loaded.appearance.mode, ThemeMode::Dark);
        assert_eq!(loaded.appearance.accent, "amber");
    }

    #[test]
    fn test_word_count() {
        assert_eq!(AppSettings::count_words("hello world"), 2);
        assert_eq!(AppSettings::count_words(""), 0);
        assert_eq!(AppSettings::count_words("  one  two  three  "), 3);
    }

    #[test]
    fn test_degradation_levels() {
        let settings = AppSettings::default();
        assert_eq!(
            settings.degradation_level("hello world"),
            DegradationLevel::Normal
        );

        let big = "word ".repeat(15000);
        assert_eq!(
            settings.degradation_level(&big),
            DegradationLevel::ReducedFeatures
        );

        let huge = "word ".repeat(25000);
        assert_eq!(
            settings.degradation_level(&huge),
            DegradationLevel::PerformanceMode
        );
    }
}
