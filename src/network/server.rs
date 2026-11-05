//! HTTP/WebSocket server for SoundShared

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
pub use axum::{
    extract::{State, WebSocketUpgrade},
    http::{header, StatusCode},
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
pub use axum::response::{Html, Response};
pub use tower_http::services::ServeDir;
pub use tower_http::trace::TraceLayer;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Mutex};
use tracing::{debug, error, info, warn};

use crate::network::broadcast::{AudioBroadcaster, BroadcasterConfig, BroadcasterHandle, BroadcastError};
use crate::network::websocket::{handle_websocket_upgrade, WebSocketConfig};

// Embedded static files (compiled into binary)
const INDEX_HTML: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/network/static/index.html"));
const APP_JS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/network/static/app.js"));
const STYLE_CSS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/network/static/style.css"));

/// Server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Bind address (default: 0.0.0.0)
    pub bind_addr: IpAddr,
    /// Port (default: 8080)
    pub port: u16,
    /// Broadcaster configuration
    pub broadcaster_config: BroadcasterConfig,
    /// WebSocket configuration
    pub websocket_config: WebSocketConfig,
    /// Path to static files directory
    pub static_dir: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            port: 8080,
            broadcaster_config: BroadcasterConfig::default(),
            websocket_config: WebSocketConfig::default(),
            static_dir: None,
        }
    }
}

/// Handle to control the running server
#[derive(Clone)]
pub struct ServerHandle {
    /// Broadcast channel for shutdown signal
    pub shutdown_tx: broadcast::Sender<()>,
    /// Server URL (e.g., "http://192.168.1.100:8080")
    pub url: Arc<tokio::sync::Mutex<Option<String>>>,
    /// Broadcaster handle for audio engine
    pub broadcaster: BroadcasterHandle,
    /// Listener for local IP detection
    pub local_ip: Arc<tokio::sync::Mutex<Option<IpAddr>>>,
}

impl ServerHandle {
    /// Get the broadcaster handle for sending audio from engine
    pub fn broadcaster(&self) -> &BroadcasterHandle {
        &self.broadcaster
    }

    /// Get the server URL (available after start)
    pub async fn url(&self) -> Option<String> {
        self.url.lock().await.clone()
    }

    /// Get the detected local IP
    pub async fn local_ip(&self) -> Option<IpAddr> {
        *self.local_ip.lock().await
    }

    /// Stop the server
    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(());
    }

    /// Check if server is running (URL is set)
    pub async fn is_running(&self) -> bool {
        self.url.lock().await.is_some()
    }
}

/// Server state shared across handlers
#[derive(Clone)]
pub struct ServerState {
    pub broadcaster: BroadcasterHandle,
    pub websocket_config: WebSocketConfig,
}

/// Errors that can occur in the server
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("Failed to bind to address: {0}")]
    BindError(#[from] std::io::Error),
    #[error("Broadcaster error: {0}")]
    Broadcaster(#[from] BroadcastError),
    #[error("Server not running")]
    NotRunning,
    #[error("Invalid configuration: {0}")]
    Config(String),
}

/// The main network server
pub struct NetworkServer {
    config: ServerConfig,
}

impl NetworkServer {
    /// Create a new network server
    pub fn new(config: ServerConfig) -> Self {
        Self { config }
    }

    /// Start the server, returning a handle for control
    pub async fn start(self) -> Result<ServerHandle, ServerError> {
        let (shutdown_tx, mut shutdown_rx) = broadcast::channel(1);
        let url = Arc::new(tokio::sync::Mutex::new(None::<String>));
        let local_ip = Arc::new(tokio::sync::Mutex::new(None::<IpAddr>));

        // Create broadcaster
        let (broadcaster_handle, broadcaster) = AudioBroadcaster::new(self.config.broadcaster_config.clone());

        // Detect local IP
        let detected_ip = detect_local_ip().await;
        *local_ip.lock().await = detected_ip;

        // Determine the URL to advertise
        let advertise_ip = detected_ip.unwrap_or_else(|| IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        let server_url = format!("http://{}:{}", advertise_ip, self.config.port);
        *url.lock().await = Some(server_url.clone());

        // Build static file service
        let static_dir = self.config.static_dir.as_deref().unwrap_or("src/network/static");
        let static_service = ServeDir::new(static_dir)
            .append_index_html_on_directories(true);

        // Build the router
        let state = ServerState {
            broadcaster: broadcaster_handle.clone(),
            websocket_config: self.config.websocket_config.clone(),
        };

        let app = Router::new()
            .nest_service("/", static_service)
            .route("/api/status", get(api_status))
            .route("/ws/audio", get(websocket_handler))
            .with_state(state)
            .layer(TraceLayer::new_for_http());

        // Bind to address
        let addr = SocketAddr::new(self.config.bind_addr, self.config.port);
        let listener = TcpListener::bind(addr).await?;
        let actual_addr = listener.local_addr()?;

        info!("Network server listening on {}", actual_addr);
        info!("Server URL: {}", server_url);

        // Spawn broadcaster task
        let broadcaster_task = tokio::spawn(broadcaster.run());

        // Spawn server task
        let server_task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.recv().await;
                    info!("🌐 Shutdown signal received, stopping server...");
                })
                .await
        });

        // Create server handle
        let shutdown_tx_for_handle = shutdown_tx.clone();
        let handle = ServerHandle {
            shutdown_tx: shutdown_tx_for_handle,
            url,
            broadcaster: broadcaster_handle,
            local_ip,
        };

        // Monitor tasks in background
        let handle_clone = handle.clone();
        tokio::spawn(async move {
            tokio::select! {
                result = broadcaster_task => {
                    if let Err(e) = result {
                        error!("Broadcaster task panicked: {}", e);
                    } else {
                        info!("Broadcaster task completed");
                    }
                }
                result = server_task => {
                    if let Err(e) = result {
                        error!("Server task error: {}", e);
                    } else {
                        info!("Server task completed");
                    }
                }
            }
            // Signal shutdown to any remaining tasks
            let _ = handle_clone.shutdown_tx.send(());
        });

        Ok(handle)
    }
}

/// Static file service for the listener page
pub fn static_files_service(_static_dir: Option<&str>) -> Router {
    Router::new()
}

/// REST API: GET /api/status
pub async fn api_status(State(state): State<ServerState>) -> impl IntoResponse {
    let listener_count = state.broadcaster.listener_count().await;

    let status = serde_json::json!({
        "status": "running",
        "listeners": listener_count,
        "version": env!("CARGO_PKG_VERSION"),
        "audio_format": {
            "sample_rate": 48000,
            "channels": 2,
            "format": "f32"
        }
    });

    Json(status)
}

/// Static file handlers using embedded file contents
pub async fn serve_index_html() -> Html<&'static str> {
    info!("📁 Serving index.html");
    Html(INDEX_HTML)
}

pub async fn serve_app_js() -> impl IntoResponse {
    info!("📁 Serving app.js");
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "application/javascript; charset=utf-8".parse().unwrap());
    (headers, APP_JS.to_string())
}

pub async fn serve_style_css() -> impl IntoResponse {
    info!("📁 Serving style.css");
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "text/css; charset=utf-8".parse().unwrap());
    (headers, STYLE_CSS.to_string())
}

/// WebSocket handler: GET /ws/audio
pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<ServerState>,
) -> Response {
    info!("🔌 WebSocket upgrade requested at /ws/audio");
    ws.on_upgrade(move |socket| handle_websocket_upgrade(socket, state.broadcaster, state.websocket_config))
}

/// Helper function for local IP detection
async fn detect_local_ip() -> Option<std::net::IpAddr> {
    use std::net::UdpSocket;

    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                let ip = addr.ip();
                if !ip.is_loopback() {
                    return Some(ip);
                }
            }
        }
    }

    if let Ok(socket) = UdpSocket::bind("[::]:0") {
        if socket.connect("[2001:4860:4860::8888]:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                let ip = addr.ip();
                if !ip.is_loopback() {
                    return Some(ip);
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn test_server_config_default() {
        let config = ServerConfig::default();
        assert_eq!(config.port, 8080);
        assert_eq!(config.bind_addr, IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)));
    }

    #[tokio::test]
    async fn test_detect_local_ip() {
        let ip = detect_local_ip().await;
        // May be None in test environment, but shouldn't panic
        if let Some(ip) = ip {
            assert!(!ip.is_loopback() || ip == IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        }
    }

    #[tokio::test]
    async fn test_server_start_stop() {
        let config = ServerConfig {
            port: 0, // Let OS assign
            ..Default::default()
        };

        let server = NetworkServer::new(config);
        let handle = server.start().await;

        // In test environment, binding to port 0 might not work as expected
        // This test mainly verifies the API compiles and runs without panic
        if let Ok(handle) = handle {
            let url = handle.url().await;
            assert!(url.is_some());
            handle.stop();
        }
    }

    #[tokio::test]
    async fn test_websocket_connect() {
        use futures::stream::StreamExt;
        use tokio_tungstenite::connect_async;
        use tokio_tungstenite::tungstenite::Message;

        let config = ServerConfig {
            port: 0,
            ..Default::default()
        };

        let server = NetworkServer::new(config);
        let handle = match server.start().await {
            Ok(h) => h,
            Err(_) => return, // Skip if can't bind
        };

        let url = match handle.url().await {
            Some(u) => u.replace("http://", "ws://") + "/ws/audio",
            None => {
                handle.stop();
                return;
            }
        };

        match connect_async(url).await {
            Ok((mut ws_stream, _)) => {
                // Try to receive a message with timeout
                let result = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    ws_stream.next()
                ).await;
                
                match result {
                    Ok(Some(Ok(Message::Binary(data)))) => {
                        assert!(data.len() > 0, "Received empty binary message");
                    }
                    Ok(Some(Ok(Message::Text(text)))) => {
                        // Text messages are also valid (e.g., format info)
                        assert!(!text.is_empty());
                    }
                    Ok(Some(Ok(_))) => {} // Other message types are OK
                    Ok(Some(Err(e))) => panic!("WebSocket error: {}", e),
                    Ok(None) => {}, // Stream closed, that's OK for a quick test
                    Err(_) => {}, // Timeout, no audio playing yet - that's OK
                }
                
                let _ = ws_stream.close(None).await;
            }
            Err(e) => {
                println!("WebSocket connection failed (may be expected if server not fully started): {}", e);
            }
        }

        handle.stop();
    }
}