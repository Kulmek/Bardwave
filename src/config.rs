//! Configuration management for SoundShared

use crate::error::{ConfigError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Network server configuration
    pub server: ServerConfig,

    /// Audio engine configuration
    pub audio: AudioConfig,

    /// Logging configuration
    pub logging: LoggingConfig,

    /// Persistent sound library and group configuration
    #[serde(default)]
    pub library: LibraryConfig,
}

/// Network server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Port to bind the HTTP/WebSocket server to
    #[serde(default = "default_port")]
    pub port: u16,

    /// Host to bind to (0.0.0.0 for all interfaces)
    #[serde(default = "default_host")]
    pub host: String,
}

/// Audio engine configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    /// Audio buffer size in samples (per channel)
    #[serde(default = "default_buffer_size")]
    pub buffer_size: usize,

    /// Sample rate for audio processing
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,

    /// Number of output channels (2 for stereo)
    #[serde(default = "default_channels")]
    pub channels: u16,

    /// Master volume (0.0 to 1.0)
    #[serde(default = "default_master_volume")]
    pub master_volume: f32,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Enable file logging in addition to stdout
    #[serde(default)]
    pub file_logging: bool,
}

/// Persistent library + groups configuration
///
/// This mirrors the in-memory sound library model (`crate::gui::groups::Library`)
/// so a user's groups, sound paths, per-sound volumes and group membership
/// survive restarts. Everything is optional in the file so configs written by
/// older versions (which had no `[library]` section) still load.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct LibraryConfig {
    /// Sound groups, in insertion order
    #[serde(default)]
    pub groups: Vec<GroupConfig>,

    /// Registered sounds, in insertion order
    #[serde(default)]
    pub sounds: Vec<SoundConfigEntry>,
}

/// A single sound group (persisted form)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupConfig {
    /// Group identifier (UUID string)
    pub id: String,

    /// Human-readable group name
    pub name: String,
}

/// A single registered sound (persisted form)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SoundConfigEntry {
    /// Absolute or user-supplied path to the audio file
    pub path: String,

    /// Display name; falls back to the file name when empty
    #[serde(default)]
    pub name: String,

    /// Per-sound volume, 0.0..=1.0
    #[serde(default = "default_sound_volume")]
    pub volume: f32,

    /// Whether the sound restarts from the beginning when it ends
    #[serde(default)]
    pub looped: bool,

    /// Group this sound belongs to, if any (UUID string)
    #[serde(default)]
    pub group: Option<String>,
}

fn default_port() -> u16 {
    8080
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_buffer_size() -> usize {
    1024
}

fn default_sample_rate() -> u32 {
    48000
}

fn default_channels() -> u16 {
    2
}

fn default_master_volume() -> f32 {
    0.8
}

fn default_log_level() -> String {
    "info".to_string()
}

/// Default per-sound volume used when a `SoundConfigEntry` omits `volume`
pub fn default_sound_volume() -> f32 {
    1.0
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            audio: AudioConfig::default(),
            logging: LoggingConfig::default(),
            library: LibraryConfig::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
            host: default_host(),
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            buffer_size: default_buffer_size(),
            sample_rate: default_sample_rate(),
            channels: default_channels(),
            master_volume: default_master_volume(),
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            file_logging: false,
        }
    }
}

impl Config {
    /// Load configuration from config.toml next to the executable
    /// Falls back to defaults if file doesn't exist or is invalid
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;

        if !config_path.exists() {
            tracing::info!("Config file not found at {:?}, using defaults", config_path);
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| ConfigError::NotFound(format!("{:?}: {}", config_path, e)))?;

        let config: Config = toml::from_str(&content)
            .map_err(|e| ConfigError::Parse(format!("{:?}: {}", config_path, e)))?;

        tracing::info!("Loaded configuration from {:?}", config_path);
        Ok(config)
    }

    /// Save configuration to config.toml next to the executable
    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path()?;
        let content = toml::to_string_pretty(self)
            .map_err(|e| ConfigError::Write(format!("Serialization failed: {}", e)))?;

        std::fs::write(&config_path, content)
            .map_err(|e| ConfigError::Write(format!("{:?}: {}", config_path, e)))?;

        tracing::info!("Saved configuration to {:?}", config_path);
        Ok(())
    }

    /// Get the path to config.toml (next to executable)
    fn config_path() -> Result<PathBuf> {
        let exe_path = std::env::current_exe()
            .map_err(|e| ConfigError::NotFound(format!("Failed to get executable path: {}", e)))?;

        let config_dir = exe_path.parent().ok_or_else(|| {
            ConfigError::NotFound("Executable has no parent directory".to_string())
        })?;

        Ok(config_dir.join("config.toml"))
    }

    /// Get the socket address for the server
    pub fn server_addr(&self) -> String {
        format!("{}:{}", self.server.host, self.server.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.audio.buffer_size, 1024);
        assert_eq!(config.audio.sample_rate, 48000);
        assert_eq!(config.audio.channels, 2);
        assert_eq!(config.audio.master_volume, 0.8);
        assert_eq!(config.logging.level, "info");
    }

    #[test]
    fn test_server_addr() {
        let config = Config::default();
        assert_eq!(config.server_addr(), "0.0.0.0:8080");
    }

    #[test]
    fn test_default_library_is_empty() {
        let config = Config::default();
        assert!(config.library.groups.is_empty());
        assert!(config.library.sounds.is_empty());
        assert_eq!(default_sound_volume(), 1.0);
    }

    /// Backwards compatibility: a config.toml written before the `[library]`
    /// section existed must still parse, defaulting the library to empty.
    #[test]
    fn test_legacy_config_without_library_section_parses() {
        let legacy = r#"
[server]
port = 9000
host = "127.0.0.1"

[audio]
buffer_size = 512
sample_rate = 44100
channels = 2
master_volume = 0.5

[logging]
level = "debug"
file_logging = true
"#;

        let config: Config = toml::from_str(legacy).expect("legacy config must parse");
        assert_eq!(config.server.port, 9000);
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.audio.buffer_size, 512);
        assert_eq!(config.logging.level, "debug");
        assert!(config.logging.file_logging);
        assert!(config.library.groups.is_empty());
        assert!(config.library.sounds.is_empty());
    }

    /// A document that carries only empty sections plus `[library]` must parse
    /// with every individual setting at its default.
    #[test]
    fn test_config_with_only_library_parses_with_defaults() {
        let text = r#"
[server]
[audio]
[logging]

[[library.groups]]
id = "6f1e1b0e-0f8e-4a8e-9a2c-2b2a0e9d1f22"
name = "Combat"
"#;

        let config: Config = toml::from_str(text).expect("library-only config must parse");
        assert_eq!(config.server.port, default_port());
        assert_eq!(config.audio.master_volume, default_master_volume());
        assert_eq!(config.logging.level, default_log_level());
        assert_eq!(config.library.groups.len(), 1);
        assert_eq!(config.library.groups[0].name, "Combat");
        assert!(config.library.sounds.is_empty());
    }

    #[test]
    fn test_library_config_roundtrip() {
        let config = Config {
            library: LibraryConfig {
                groups: vec![
                    GroupConfig {
                        id: "6f1e1b0e-0f8e-4a8e-9a2c-2b2a0e9d1f22".to_string(),
                        name: "Combat".to_string(),
                    },
                    GroupConfig {
                        id: "1c2d3e4f-5a6b-4c7d-8e9f-0a1b2c3d4e5f".to_string(),
                        name: "Ambience".to_string(),
                    },
                ],
                sounds: vec![
                    SoundConfigEntry {
                        path: r"C:\fake\hit.wav".to_string(),
                        name: "Hit".to_string(),
                        volume: 0.75,
                        looped: true,
                        group: Some("6f1e1b0e-0f8e-4a8e-9a2c-2b2a0e9d1f22".to_string()),
                    },
                    SoundConfigEntry {
                        path: r"C:\fake\wind.ogg".to_string(),
                        name: String::new(),
                        volume: 1.0,
                        looped: false,
                        group: None,
                    },
                ],
            },
            ..Config::default()
        };

        let text = toml::to_string_pretty(&config).expect("serialize");
        let back: Config = toml::from_str(&text).expect("deserialize");

        assert_eq!(back.library.groups, config.library.groups);
        assert_eq!(back.library.sounds, config.library.sounds);
        assert_eq!(back.server.port, config.server.port);
    }

    /// `volume` in a stored entry must default to 1.0 when omitted, and
    /// `group`/`name` must default to None/empty.
    #[test]
    fn test_sound_entry_defaults_when_fields_omitted() {
        let text = r#"
[server]
port = 8080
host = "0.0.0.0"

[audio]
[logging]

[[library.sounds]]
path = 'C:\fake\hit.wav'

[[library.groups]]
id = "6f1e1b0e-0f8e-4a8e-9a2c-2b2a0e9d1f22"
name = "Combat"
"#;

        let config: Config = toml::from_str(text).expect("parse");
        assert_eq!(config.library.sounds.len(), 1);
        let entry = &config.library.sounds[0];
        assert_eq!(entry.volume, 1.0);
        assert_eq!(entry.name, "");
        assert_eq!(entry.group, None);
        // Configs written before the repeat toggle existed must load as "off".
        assert!(!entry.looped);
    }
}
