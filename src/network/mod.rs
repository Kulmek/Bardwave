//! Network module for SoundShared.
//! Handles HTTP server, WebSocket connections, and audio broadcasting.

pub mod broadcast;
pub mod server;
pub mod websocket;

pub use broadcast::{AudioBroadcaster, BroadcasterHandle, BroadcasterConfig, AudioFrame, AudioFormatInfo, BroadcastError};
pub use server::{NetworkServer, ServerConfig, ServerHandle, ServerError, ServerState, static_files_service, api_status, websocket_handler, Router, get, TraceLayer};
pub use websocket::{WebSocketSession, WebSocketConfig, WebSocketError, handle_websocket_upgrade};