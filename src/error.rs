//! Error types for SoundShared.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum SoundSharedError {
    #[error("Audio error: {0}")]
    Audio(#[from] AudioError),

    #[error("Network error: {0}")]
    Network(#[from] NetworkError),

    #[error("GUI error: {0}")]
    Gui(#[from] GuiError),

    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Channel error: {0}")]
    Channel(String),

    #[error("Generic error: {0}")]
    Other(String),
}

#[derive(Error, Debug)]
pub enum AudioError {
    #[error("Failed to open audio device: {0}")]
    DeviceOpen(String),

    #[error("Failed to build audio stream: {0}")]
    StreamBuild(String),

    #[error("Audio stream error: {0}")]
    Stream(String),

    #[error("Decoding error: {0}")]
    Decode(String),

    #[error("Unsupported audio format: {0}")]
    UnsupportedFormat(String),

    #[error("Resampling error: {0}")]
    Resample(String),

    #[error("Mixer error: {0}")]
    Mixer(String),

    #[error("Engine error: {0}")]
    Engine(String),
}

#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("Server bind error: {0}")]
    Bind(String),

    #[error("WebSocket error: {0}")]
    WebSocket(String),

    #[error("Broadcast error: {0}")]
    Broadcast(String),

    #[error("Server not running")]
    NotRunning,

    #[error("Invalid address: {0}")]
    InvalidAddress(String),
}

#[derive(Error, Debug)]
pub enum GuiError {
    #[error("eframe error: {0}")]
    Eframe(String),

    #[error("File dialog error: {0}")]
    FileDialog(String),

    #[error("State error: {0}")]
    State(String),
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Config file not found: {0}")]
    NotFound(String),

    #[error("Config parse error: {0}")]
    Parse(String),

    #[error("Config write error: {0}")]
    Write(String),
}

impl From<&str> for SoundSharedError {
    fn from(s: &str) -> Self {
        SoundSharedError::Other(s.to_string())
    }
}

impl From<String> for SoundSharedError {
    fn from(s: String) -> Self {
        SoundSharedError::Other(s)
    }
}

pub type Result<T> = std::result::Result<T, SoundSharedError>;
