//! Shared types for GUI integration
//! These types are used by both the GUI and main.rs for communication

use crate::audio::SoundId;
use crossbeam_channel::Sender;
use std::fmt::Debug;

/// Commands from GUI to audio engine
#[derive(Debug, Clone)]
pub enum AudioCommand {
    /// Load a sound file and return its SoundInfo
    LoadSound { path: String, response: Sender<anyhow::Result<SoundInfo>> },
    /// Play a sound by ID
    PlaySound { id: SoundId, response: Sender<anyhow::Result<()>> },
    /// Stop a sound by ID
    StopSound { id: SoundId, response: Sender<anyhow::Result<()>> },
    /// Set per-sound volume (track volume)
    SetVolume { id: SoundId, volume: f32, response: Sender<anyhow::Result<()>> },
    /// Set master volume
    SetMasterVolume { volume: f32, response: Sender<anyhow::Result<()>> },
    /// Remove a sound from the library
    RemoveSound { id: SoundId, response: Sender<anyhow::Result<()>> },
    /// Get list of loaded sounds
    ListSounds { response: Sender<Vec<SoundInfo>> },
    /// Stop all playback
    StopAll { response: Sender<anyhow::Result<()>> },
    /// Check if a sound is currently playing in the mixer
    IsPlaying { id: SoundId, response: Sender<bool> },
}

/// Sound information for GUI display
#[derive(Debug, Clone)]
pub struct SoundInfo {
    pub id: SoundId,
    pub name: String,
    pub path: String,
    pub duration: f32,
    pub is_playing: bool,
    pub volume: f32,
}

/// Commands from GUI to network server
#[derive(Debug, Clone)]
pub enum ServerCommand {
    /// Start the network server
    Start { response: Sender<anyhow::Result<String>> },
    /// Stop the network server
    Stop { response: Sender<anyhow::Result<()>> },
    /// Get current server status
    GetStatus { response: Sender<ServerStatus> },
}

/// Network server status for GUI display
#[derive(Debug, Clone)]
pub struct ServerStatus {
    pub running: bool,
    pub address: Option<String>,
    pub listener_count: usize,
}

/// Shared application state for GUI integration
#[derive(Clone)]
pub struct AppState {
    /// Channel to send commands from GUI to audio engine
    pub audio_cmd_tx: Sender<AudioCommand>,

    /// Channel to send commands from GUI to network server
    pub server_cmd_tx: Sender<ServerCommand>,

    /// Current connected listener count (for display)
    pub listener_count: std::sync::Arc<std::sync::Mutex<usize>>,

    /// Application configuration
    pub config: std::sync::Arc<std::sync::Mutex<crate::config::Config>>,
}

impl AppState {
    /// Create a new AppState with the given channels and config
    pub fn new(
        audio_cmd_tx: Sender<AudioCommand>,
        server_cmd_tx: Sender<ServerCommand>,
        listener_count: std::sync::Arc<std::sync::Mutex<usize>>,
        config: std::sync::Arc<std::sync::Mutex<crate::config::Config>>,
    ) -> Self {
        Self {
            audio_cmd_tx,
            server_cmd_tx,
            listener_count,
            config,
        }
    }
}