//! Configuration management for SoundShared

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::error::{ConfigError, Result};

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Network server configuration
    pub server: ServerConfig,

    /// Audio engine configuration
    pub audio: AudioConfig,

    /// Logging configuration
    pub logging: LoggingConfig,
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

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            audio: AudioConfig::default(),
            logging: LoggingConfig::default(),
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
        
        let config_dir = exe_path.parent()
            .ok_or_else(|| ConfigError::NotFound("Executable has no parent directory".to_string()))?;
        
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
}