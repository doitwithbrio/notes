//! User settings storage.
//!
//! Stored as JSON in `~/Notes/.p2p/settings.json`.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// Application settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// Display name for this peer (shown to others).
    #[serde(default = "default_display_name")]
    pub display_name: String,

    /// Custom relay server URLs (in addition to N0 defaults).
    #[serde(default)]
    pub custom_relays: Vec<String>,

    /// UI theme preference.
    #[serde(default = "default_theme")]
    pub theme: String,

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
fn default_theme() -> String {
    "system".to_string()
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
            display_name: default_display_name(),
            custom_relays: vec![],
            theme: default_theme(),
            font_size: default_font_size(),
            auto_save: true,
            save_interval_secs: default_save_interval(),
            large_doc_warning_words: default_large_doc_threshold(),
            idle_doc_timeout_secs: 0,
        }
    }
}

impl AppSettings {
    /// Load settings from disk, or return defaults if not found.
    pub async fn load(base_dir: &Path) -> Self {
        let path = base_dir.join(".p2p").join("settings.json");
        match tokio::fs::read_to_string(&path).await {
            Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save settings to disk.
    pub async fn save(&self, base_dir: &Path) -> Result<(), CoreError> {
        let path = base_dir.join(".p2p").join("settings.json");
        tokio::fs::create_dir_all(path.parent().unwrap()).await?;
        let json = serde_json::to_string_pretty(self)?;
        tokio::fs::write(&path, json).await?;
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
        assert_eq!(settings.theme, "system");
        assert!(settings.auto_save);
    }

    #[test]
    fn test_serde_roundtrip() {
        let settings = AppSettings::default();
        let json = serde_json::to_string(&settings).unwrap();
        let loaded: AppSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.font_size, settings.font_size);
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
