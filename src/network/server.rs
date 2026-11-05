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
pub use tower_http::trace::TraceLayer;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tracing::{error, info};

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
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            port: 8080,
            broadcaster_config: BroadcasterConfig::default(),
            websocket_config: WebSocketConfig::default(),
        }
    }
}

/// Handle to control the running server
#[derive(Clone)]
pub struct ServerHandle {
    /// Broadcast channel for shutdown signal
    pub shutdown_tx: broadcast::Sender<()>,
    /// Server URL (e.g., "http://192.168.1.100:8080"), None while stopped
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
    /// Shutdown signal for the listener sessions.
    ///
    /// Every session subscribes on connect and closes its socket when this
    /// fires. Without it a browser tab holding a WebSocket open keeps the
    /// graceful shutdown of the accept loop waiting for as long as it stays
    /// open, which used to leave the console reporting "Live" after Stop server
    /// and froze the whole server thread.
    pub shutdown_tx: broadcast::Sender<()>,
}

/// Build the router that serves the listener page, the status API and the audio
/// WebSocket.
///
/// The in-app server in main.rs and NetworkServer both build their HTTP surface
/// from this single definition, so the two can never drift apart. They used to
/// differ: the app hand-built a router with the embedded assets, while
/// NetworkServer served a ServeDir from a path that does not exist next to the
/// installed binary, so its listener page was always a 404.
pub fn build_router(state: ServerState) -> Router {
    Router::new()
        .route("/", get(serve_index_html))
        .route("/index.html", get(serve_index_html))
        .route("/app.js", get(serve_app_js))
        .route("/style.css", get(serve_style_css))
        .route("/api/status", get(api_status))
        .route("/ws/audio", get(websocket_handler))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
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
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
        let url = Arc::new(tokio::sync::Mutex::new(None::<String>));
        let local_ip = Arc::new(tokio::sync::Mutex::new(None::<IpAddr>));

        // Create broadcaster
        let (broadcaster_handle, broadcaster) =
            AudioBroadcaster::new(self.config.broadcaster_config.clone());

        // Detect local IP
        let detected_ip = detect_local_ip().await;
        *local_ip.lock().await = detected_ip;

        // Build the shared router
        let state = ServerState {
            broadcaster: broadcaster_handle.clone(),
            websocket_config: self.config.websocket_config.clone(),
            shutdown_tx: shutdown_tx.clone(),
        };
        let app = build_router(state);

        // Bind to address
        let addr = SocketAddr::new(self.config.bind_addr, self.config.port);
        let listener = TcpListener::bind(addr).await?;
        let actual_addr = listener.local_addr()?;

        // The URL follows the socket that was actually bound, so a bind to port
        // 0 advertises the port the OS picked rather than the requested one.
        let server_url = listen_url(self.config.bind_addr, actual_addr, detected_ip);
        *url.lock().await = Some(server_url.clone());

        info!("Network server listening on {}", actual_addr);
        info!("Server URL: {}", server_url);

        // Spawn broadcaster task
        let broadcaster_task = tokio::spawn(broadcaster.run());

        // Spawn server task
        let server_task = tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx;
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
            // Signal shutdown to any remaining tasks and retire the URL, so
            // is_running()/url() stop claiming a live listener.
            *handle_clone.url.lock().await = None;
            let _ = handle_clone.shutdown_tx.send(());
        });

        Ok(handle)
    }
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
    // Static header values: parsed at compile time instead of unwrapping a
    // parse that could panic while serving a request.
    headers.insert(
        header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("application/javascript; charset=utf-8"),
    );
    (headers, APP_JS)
}

pub async fn serve_style_css() -> impl IntoResponse {
    info!("📁 Serving style.css");
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/css; charset=utf-8"),
    );
    (headers, STYLE_CSS)
}

/// WebSocket handler: GET /ws/audio
pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<ServerState>,
) -> Response {
    info!("🔌 WebSocket upgrade requested at /ws/audio");
    // The configured frame limit used to be ignored here, so the library
    // default applied no matter what WebSocketConfig said.
    let ws = ws.max_message_size(state.websocket_config.max_message_size);
    // Each session owns its own subscription, so a later Stop Server reaches
    // every listener that is connected at that moment.
    let shutdown_rx = state.shutdown_tx.subscribe();
    ws.on_upgrade(move |socket| {
        handle_websocket_upgrade(
            socket,
            state.broadcaster,
            state.websocket_config,
            shutdown_rx,
        )
    })
}

/// The URL listeners should open, derived from where the socket actually bound.
///
/// * a loopback bind is only reachable from this machine, so it advertises
///   loopback;
/// * a wildcard bind (0.0.0.0 / ::) advertises the OS-detected LAN address of
///   the matching family, falling back to loopback when there is none;
/// * an explicit address advertises itself.
///
/// The actual address supplies the real port, so a bind to port 0 reports the
/// port the OS picked rather than the one that was asked for.
pub fn listen_url(bind_addr: IpAddr, actual: SocketAddr, detected: Option<IpAddr>) -> String {
    let advertise = if bind_addr.is_loopback() {
        bind_addr
    } else if bind_addr.is_unspecified() {
        match detected {
            // Never advertise an address of the other family: the socket only
            // listens on the family it was bound to.
            Some(ip) if ip.is_ipv4() == bind_addr.is_ipv4() => ip,
            _ if bind_addr.is_ipv4() => IpAddr::V4(Ipv4Addr::LOCALHOST),
            _ => IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
        }
    } else {
        bind_addr
    };

    match advertise {
        IpAddr::V4(ip) => format!("http://{}:{}", ip, actual.port()),
        // IPv6 literals need brackets, otherwise the port reads as part of the address.
        IpAddr::V6(ip) => format!("http://[{}]:{}", ip, actual.port()),
    }
}

/// Detect the LAN address of this machine.
///
/// Lives here only: main.rs used to carry a byte-identical copy of this probe.
pub async fn detect_local_ip() -> Option<IpAddr> {
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
    use std::net::Ipv6Addr;
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
        let handle = match server.start().await {
            Ok(handle) => handle,
            Err(_) => return, // Skip if the environment cannot bind
        };

        let url = handle.url().await.expect("a started server has a URL");
        // The URL must carry the port the OS actually bound, not the requested 0.
        assert!(!url.ends_with(":0"), "unbound port advertised: {url}");
        assert!(handle.is_running().await);

        // The server must really be listening on the advertised port.
        let addr = url.trim_start_matches("http://");
        assert!(
            tokio::net::TcpStream::connect(addr).await.is_ok(),
            "nothing is listening at {addr}"
        );

        handle.stop();
        let _ = timeout(Duration::from_secs(2), async {
            while handle.is_running().await {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
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
                // Give the session time to register and greet the client.
                let result =
                    tokio::time::timeout(std::time::Duration::from_secs(2), ws_stream.next()).await;

                match result {
                    Ok(Some(Ok(Message::Binary(data)))) => {
                        assert!(!data.is_empty(), "Received empty binary message");
                    }
                    Ok(Some(Ok(Message::Text(text)))) => {
                        // Text messages are also valid (welcome / format info)
                        assert!(!text.is_empty());
                    }
                    Ok(Some(Ok(_))) => {} // Other message types are OK
                    Ok(Some(Err(e))) => panic!("WebSocket error: {}", e),
                    Ok(None) => {} // Stream closed, that's OK for a quick test
                    Err(_) => {}   // Timeout, no audio playing yet - that's OK
                }

                let _ = ws_stream.close(None).await;
            }
            Err(e) => {
                println!(
                    "WebSocket connection failed (may be expected if server not fully started): {}",
                    e
                );
            }
        }

        handle.stop();
    }

    /// The library server must keep serving real responses after the router was
    /// unified: the listener page is embedded, not read from disk.
    #[tokio::test]
    async fn network_server_serves_the_embedded_listener_page() {
        let config = ServerConfig {
            port: 0,
            ..Default::default()
        };

        let handle = match NetworkServer::new(config).start().await {
            Ok(handle) => handle,
            Err(_) => return,
        };
        let url = handle.url().await.expect("url");
        let addr = url.trim_start_matches("http://");

        let body = http_get(addr, "/").await;
        assert!(body.contains("SoundShared"), "unexpected body: {body}");

        handle.stop();
    }

    /// Minimal HTTP GET so the test needs no extra dependency.
    async fn http_get(addr: &str, path: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut stream = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect to the running server");
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await.expect("write");
        let mut response = String::new();
        stream.read_to_string(&mut response).await.expect("read");
        response
    }

    // --- advertised URL ------------------------------------------------------

    #[test]
    fn listen_url_advertises_loopback_for_loopback_binds() {
        let v4 = SocketAddr::from((Ipv4Addr::LOCALHOST, 8080));
        assert_eq!(
            listen_url(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                v4,
                Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 245)))
            ),
            "http://127.0.0.1:8080"
        );

        let v6 = SocketAddr::from((Ipv6Addr::LOCALHOST, 9000));
        assert_eq!(
            listen_url(IpAddr::V6(Ipv6Addr::LOCALHOST), v6, None),
            "http://[::1]:9000"
        );
    }

    #[test]
    fn listen_url_prefers_the_detected_lan_address_for_wildcard_binds() {
        let actual = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 8080));
        assert_eq!(
            listen_url(
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                actual,
                Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 245)))
            ),
            "http://192.168.1.245:8080"
        );

        // Without detection a wildcard bind is only guaranteed reachable locally.
        assert_eq!(
            listen_url(IpAddr::V4(Ipv4Addr::UNSPECIFIED), actual, None),
            "http://127.0.0.1:8080"
        );
    }

    #[test]
    fn listen_url_never_advertises_the_other_address_family() {
        // IPv4 wildcard socket + IPv6 detection: advertising ::1 would point at
        // a socket this server never opened.
        let actual4 = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 8080));
        assert_eq!(
            listen_url(
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                actual4,
                Some(IpAddr::V6(Ipv6Addr::LOCALHOST))
            ),
            "http://127.0.0.1:8080"
        );

        // ...and the mirror image for an IPv6 wildcard socket.
        let actual6 = SocketAddr::from((Ipv6Addr::UNSPECIFIED, 8080));
        assert_eq!(
            listen_url(
                IpAddr::V6(Ipv6Addr::UNSPECIFIED),
                actual6,
                Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 245)))
            ),
            "http://[::1]:8080"
        );
    }

    #[test]
    fn listen_url_reports_the_port_the_os_actually_bound() {
        // The OS may pick the port (bind to 0), and the advertised URL has to
        // match the socket rather than the requested port.
        let actual = SocketAddr::from((Ipv4Addr::LOCALHOST, 41_000));
        assert_eq!(
            listen_url(IpAddr::V4(Ipv4Addr::LOCALHOST), actual, None),
            "http://127.0.0.1:41000"
        );
    }

    #[test]
    fn listen_url_advertises_an_explicit_address_verbatim() {
        let actual = SocketAddr::from((Ipv4Addr::new(10, 0, 0, 7), 8080));
        assert_eq!(
            listen_url(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7)), actual, None),
            "http://10.0.0.7:8080"
        );
    }
}
