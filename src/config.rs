//! Configuration management for Voisu for Windows (`voisu-win`).
//!
//! Handles configuration schema, persistence to `%APPDATA%\voisu\config.json`,
//! and the interactive setup CLI wizard.

use crate::core::types::{DeliveryMode, DprPolicy, InteractionMode, TriggerKey};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Default clipboard restoration timeout in milliseconds.
pub const DEFAULT_CLIPBOARD_RESTORE_TIMEOUT_MS: u32 = 200;
pub const MIN_CLIPBOARD_RESTORE_TIMEOUT_MS: u32 = 50;
pub const MAX_CLIPBOARD_RESTORE_TIMEOUT_MS: u32 = 1000;

/// Persisted user preferences and provider credentials.
/// Matches schema definition in `schema.md §1.6` and `schema.md §2`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    /// Deepgram API secret token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deepgram_api_key: Option<String>,

    /// Groq API secret token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groq_api_key: Option<String>,

    /// Key used to trigger dictation.
    #[serde(default)]
    pub trigger_key: TriggerKey,

    /// Interaction mode (Hybrid, PushToTalk, Toggle).
    #[serde(default)]
    pub interaction_mode: InteractionMode,

    /// Text delivery strategy to target application.
    #[serde(default)]
    pub delivery_mode: DeliveryMode,

    /// Deep Punctuation & Formatting (DPR) policy.
    #[serde(default)]
    pub dpr_policy: DprPolicy,

    /// Specific audio input device ID (None = system default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_device_id: Option<String>,

    /// Hardware capture sample rate override in Hz (None = auto-detect hardware default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_sample_rate: Option<u32>,

    /// Maximum wait time (ms) for target app to consume clipboard before restoring prior content.
    #[serde(default = "default_clipboard_restore_timeout")]
    pub clipboard_restore_timeout_ms: u32,

    /// Custom domain terms, acronyms, or proper nouns.
    #[serde(default)]
    pub custom_dictionary: Vec<String>,
}

fn default_clipboard_restore_timeout() -> u32 {
    DEFAULT_CLIPBOARD_RESTORE_TIMEOUT_MS
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            deepgram_api_key: None,
            groq_api_key: None,
            trigger_key: TriggerKey::default(),
            interaction_mode: InteractionMode::default(),
            delivery_mode: DeliveryMode::default(),
            dpr_policy: DprPolicy::default(),
            audio_device_id: None,
            native_sample_rate: None,
            clipboard_restore_timeout_ms: DEFAULT_CLIPBOARD_RESTORE_TIMEOUT_MS,
            custom_dictionary: Vec::new(),
        }
    }
}

impl AppConfig {
    /// Resolve standard config directory: `%APPDATA%\voisu` on Windows.
    pub fn config_dir() -> PathBuf {
        if let Some(config_dir) = dirs::config_dir() {
            config_dir.join("voisu")
        } else {
            PathBuf::from(".voisu")
        }
    }

    /// Resolve standard config file path: `%APPDATA%\voisu\config.json`.
    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.json")
    }

    /// Check if config file exists on disk.
    pub fn exists() -> bool {
        Self::config_path().exists()
    }

    /// Load configuration from standard location or return default if missing.
    pub fn load() -> Result<Self, io::Error> {
        let path = Self::config_path();
        Self::load_from_path(&path)
    }

    /// Load configuration from specific file path.
    pub fn load_from_path(path: &Path) -> Result<Self, io::Error> {
        if !path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Config file not found at: {}", path.display()),
            ));
        }
        let data = fs::read_to_string(path)?;
        let mut config: Self = serde_json::from_str(&data)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        config.validate_and_clamp();
        Ok(config)
    }

    /// Save configuration to standard location `%APPDATA%\voisu\config.json`.
    pub fn save(&self) -> Result<(), io::Error> {
        let path = Self::config_path();
        self.save_to_path(&path)
    }

    /// Save configuration to a specific path.
    pub fn save_to_path(&self, path: &Path) -> Result<(), io::Error> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Clamps fields to valid schema bounds.
    pub fn validate_and_clamp(&mut self) {
        self.clipboard_restore_timeout_ms = self.clipboard_restore_timeout_ms.clamp(
            MIN_CLIPBOARD_RESTORE_TIMEOUT_MS,
            MAX_CLIPBOARD_RESTORE_TIMEOUT_MS,
        );
    }

    /// Checks if at least one STT provider API key is configured.
    pub fn has_active_provider(&self) -> bool {
        self.deepgram_api_key
            .as_ref()
            .is_some_and(|k| !k.trim().is_empty())
            || self
                .groq_api_key
                .as_ref()
                .is_some_and(|k| !k.trim().is_empty())
    }

    /// Interactive CLI wizard for initial configuration setup.
    pub fn run_interactive_setup() -> Result<Self, io::Error> {
        println!("============================================================");
        println!("      Voisu for Windows — Configuration Setup Wizard        ");
        println!("============================================================");
        println!("This wizard will configure your STT cloud providers and hotkeys.\n");

        let mut current = Self::load().unwrap_or_default();

        // 1. Deepgram API Key
        print!(
            "Enter Deepgram API Key [{}]: ",
            current.deepgram_api_key.as_deref().unwrap_or("none")
        );
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim();
        if !trimmed.is_empty() {
            current.deepgram_api_key = Some(trimmed.to_string());
        }

        // 2. Groq API Key
        print!(
            "Enter Groq API Key [{}]: ",
            current.groq_api_key.as_deref().unwrap_or("none")
        );
        io::stdout().flush()?;
        input.clear();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim();
        if !trimmed.is_empty() {
            current.groq_api_key = Some(trimmed.to_string());
        }

        // 3. Trigger Key
        println!("\nSelect Trigger Key:");
        println!("  1. CapsLock (Default — recommended for fastest access)");
        println!("  2. RightAlt");
        println!("  3. F8");
        print!("Choose [1-3, default 1]: ");
        io::stdout().flush()?;
        input.clear();
        io::stdin().read_line(&mut input)?;
        match input.trim() {
            "2" => current.trigger_key = TriggerKey::RightAlt,
            "3" => current.trigger_key = TriggerKey::F8,
            _ => current.trigger_key = TriggerKey::CapsLock,
        }

        // 4. Interaction Mode
        println!("\nSelect Interaction Mode:");
        println!("  1. Hybrid (Hold to talk >400ms, Tap to toggle on/off) [Default]");
        println!("  2. PushToTalk (Hold while speaking, release to finish)");
        println!("  3. Toggle (Tap to start, tap again to stop)");
        print!("Choose [1-3, default 1]: ");
        io::stdout().flush()?;
        input.clear();
        io::stdin().read_line(&mut input)?;
        match input.trim() {
            "2" => current.interaction_mode = InteractionMode::PushToTalk,
            "3" => current.interaction_mode = InteractionMode::Toggle,
            _ => current.interaction_mode = InteractionMode::Hybrid,
        }

        current.validate_and_clamp();
        let target_path = Self::config_path();
        current.save()?;

        println!(
            "\nConfiguration successfully saved to: {}",
            target_path.display()
        );
        println!("Run 'voisu-win doctor' to verify hardware and cloud connectivity.");
        println!("Run 'voisu-win run' to launch the dictation service.");

        Ok(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_roundtrip() {
        let config = AppConfig::default();
        let json = serde_json::to_string(&config).expect("Serialization failed");
        let decoded: AppConfig = serde_json::from_str(&json).expect("Deserialization failed");
        assert_eq!(config, decoded);
    }

    #[test]
    fn test_custom_config_serialization() {
        let config = AppConfig {
            deepgram_api_key: Some("dg_test_key_123".to_string()),
            groq_api_key: Some("gsk_test_key_456".to_string()),
            trigger_key: TriggerKey::CapsLock,
            interaction_mode: InteractionMode::Hybrid,
            delivery_mode: DeliveryMode::SmartClipboard,
            dpr_policy: DprPolicy::Adaptive,
            audio_device_id: None,
            native_sample_rate: Some(48000),
            clipboard_restore_timeout_ms: 250,
            custom_dictionary: vec!["Kubernetes".to_string(), "gRPC".to_string()],
        };

        let json = serde_json::to_string_pretty(&config).unwrap();
        assert!(json.contains("dg_test_key_123"));
        assert!(json.contains("gsk_test_key_456"));
        assert!(json.contains("CapsLock"));
        assert!(json.contains("Kubernetes"));

        let deserialized: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
        assert!(deserialized.has_active_provider());
    }

    #[test]
    fn test_timeout_clamping() {
        let mut config = AppConfig {
            clipboard_restore_timeout_ms: 20, // below minimum 50
            ..Default::default()
        };
        config.validate_and_clamp();
        assert_eq!(
            config.clipboard_restore_timeout_ms,
            MIN_CLIPBOARD_RESTORE_TIMEOUT_MS
        );

        config.clipboard_restore_timeout_ms = 5000; // above maximum 1000
        config.validate_and_clamp();
        assert_eq!(
            config.clipboard_restore_timeout_ms,
            MAX_CLIPBOARD_RESTORE_TIMEOUT_MS
        );
    }

    #[test]
    fn test_file_persistence() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_file = temp_dir.path().join("config.json");

        let config = AppConfig {
            deepgram_api_key: Some("test_secret".into()),
            ..Default::default()
        };
        config.save_to_path(&config_file).unwrap();

        assert!(config_file.exists());
        let loaded = AppConfig::load_from_path(&config_file).unwrap();
        assert_eq!(loaded.deepgram_api_key.as_deref(), Some("test_secret"));
    }
}
