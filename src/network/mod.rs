//! Network module for SoundShared.
//! Handles HTTP server, WebSocket connections, and audio broadcasting.

pub mod broadcast;
pub mod local_ip;
pub mod server;
pub mod websocket;

pub use broadcast::{
    format_message, AudioBroadcaster, AudioFormatInfo, AudioFrame, BroadcastError,
    BroadcasterConfig, BroadcasterHandle,
};
pub use local_ip::{candidates, detect_for_bind, is_advertisable, pick_advertised};
pub use server::{
    api_status, build_router, get, listen_url, loopback_url, serve_app_js, serve_index_html,
    serve_style_css, websocket_handler, NetworkServer, Router, ServerConfig, ServerError,
    ServerHandle, ServerState, TraceLayer,
};
pub use websocket::{
    handle_websocket_upgrade, WebSocketConfig, WebSocketError, WebSocketSession,
};
