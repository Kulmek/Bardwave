//! SoundShared - Local Network Audio Streaming
//!
//! A Rust application for streaming audio from a desktop to browsers
//! on the local network via WebSocket.

pub mod audio;
pub mod config;
pub mod error;
pub mod gui;
pub mod network;

/// SoundShared version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Re-export commonly used types
pub use config::Config;
pub use error::{SoundSharedError, Result};
pub use gui::app::SoundSharedApp;
pub use gui::types::{AppState, AudioCommand, ServerCommand, SoundInfo};
pub use gui::app::ServerStatus as GuiServerStatus;
pub use network::broadcast::{AudioBroadcaster, AudioFrame, BroadcasterConfig, BroadcasterHandle, BroadcastError};
pub use network::server::{NetworkServer, ServerConfig, ServerHandle, ServerError};
pub use network::websocket::WebSocketConfig;