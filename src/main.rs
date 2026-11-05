//! SoundShared - Main Entry Point
//!
//! Integrates audio engine, network server, and GUI into a single application.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{bounded, Receiver, Sender};
use tokio::sync::{broadcast, mpsc};
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

// For Ctrl+C handling in server-only mode
use ctrlc;

use soundshared::{
    audio::{AudioEngine, MixedOutput, SoundId},
    config::{parse_bind_host, Config},
    error::{Result, SoundSharedError},
    gui::{
        types::{AppState, AudioCommand, ServerCommand, SoundInfo, ServerStatus},
        run_gui,
    },
    network::{
        broadcast::{AudioBroadcaster, AudioFrame, BroadcasterConfig, BroadcasterHandle, BroadcastError},
        server::{NetworkServer, ServerConfig, ServerHandle, ServerError, ServerState, api_status, websocket_handler, Router, get, TraceLayer},
        websocket::WebSocketConfig,
    },
};

// Audio buffer type for inter-component communication (from mixer)
type MixedOutputBuffer = MixedOutput;

/// The URL listeners should open, derived from where the socket actually bound.
///
/// * a loopback bind is only reachable from this machine, so it advertises
///   loopback;
/// * a wildcard bind (`0.0.0.0` / `::`) advertises the OS-detected LAN address of
///   the matching family, falling back to loopback when there is none;
/// * an explicit address advertises itself.
///
/// `actual` supplies the real port, so a bind to port 0 reports the port the OS
/// picked rather than the one that was asked for.
fn listen_url(
    bind_addr: std::net::IpAddr,
    actual: std::net::SocketAddr,
    detected: Option<std::net::IpAddr>,
) -> String {
    let advertise = if bind_addr.is_loopback() {
        bind_addr
    } else if bind_addr.is_unspecified() {
        match detected {
            // Never advertise an address of the other family: the socket only
            // listens on the family it was bound to.
            Some(ip) if ip.is_ipv4() == bind_addr.is_ipv4() => ip,
            _ if bind_addr.is_ipv4() => std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            _ => std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
        }
    } else {
        bind_addr
    };

    match advertise {
        std::net::IpAddr::V4(ip) => format!("http://{}:{}", ip, actual.port()),
        // IPv6 literals need brackets, otherwise the port reads as part of the address.
        std::net::IpAddr::V6(ip) => format!("http://[{}]:{}", ip, actual.port()),
    }
}

/// Audio processor that receives mixed audio from engine and forwards to broadcaster
struct AudioProcessor {
    /// Receiver for mixed audio buffers from audio engine
    audio_rx: Receiver<MixedOutputBuffer>,
    /// Broadcaster handle to send audio to listeners
    broadcaster: BroadcasterHandle,
    /// Shutdown signal (crossbeam)
    shutdown_rx: Receiver<()>,
}

impl AudioProcessor {
    fn new(audio_rx: Receiver<MixedOutputBuffer>, broadcaster: BroadcasterHandle, shutdown_rx: Receiver<()>) -> Self {
        Self {
            audio_rx,
            broadcaster,
            shutdown_rx,
        }
    }

    fn run(&mut self) {
        info!("Audio processor started");
        loop {
            crossbeam_channel::select! {
                recv(self.audio_rx) -> msg => {
                    match msg {
                        Ok(mixed) => {
                            // Convert MixedOutput to AudioFrame for broadcaster
                            let frame = AudioFrame::new(
                                mixed.buffer,
                                48000, // sample rate
                                2,     // channels
                                mixed.seq_no,
                            );
                            if let Err(e) = self.broadcaster.send_audio(frame) {
                                // Channel full - backpressure, drop frame
                                if !matches!(e, BroadcastError::ChannelFull) {
                                    warn!("Failed to broadcast audio: {}", e);
                                }
                            }
                        }
                        Err(_) => {
                            info!("Audio channel closed, stopping processor");
                            break;
                        }
                    }
                }
                recv(self.shutdown_rx) -> _ => {
                    info!("Shutdown signal received, stopping audio processor");
                    break;
                }
            }
        }
        info!("Audio processor stopped");
    }
}

/// Server manager that runs the network server in a Tokio runtime
/// Server does NOT auto-start - only starts when GUI sends Start command
fn run_server_and_get_handle(
    config: ServerConfig,
    server_cmd_rx: Receiver<ServerCommand>,
    shutdown_rx: Receiver<()>,
    listener_count_tx: Sender<usize>,
    server_handle_tx: Sender<ServerHandle>,
) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create Tokio runtime");

    let result = rt.block_on(async {
        let (shutdown_tx, mut shutdown_rx_tokio) = broadcast::channel(1);
        
        // Create broadcaster
        let (broadcaster_handle, broadcaster) = AudioBroadcaster::new(config.broadcaster_config.clone());

        // Detect local IP
        let detected_ip = detect_local_ip().await;

        // The bind target is live state, not a snapshot: the GUI can change the
        // CONFIGURATION port / bind address while the server is down, and the
        // next Start has to use the new values.
        let mut bind_addr = config.bind_addr;
        let mut bind_port = config.port;
        // URL of the running server; `None` while the socket is down.
        let mut server_url: Option<String> = None;

        // Build the router - all routes share ServerState
        let state = ServerState {
            broadcaster: broadcaster_handle.clone(),
            websocket_config: config.websocket_config.clone(),
        };

        let app = Router::new()
            .route("/", get(soundshared::network::server::serve_index_html))
            .route("/index.html", get(soundshared::network::server::serve_index_html))
            .route("/app.js", get(soundshared::network::server::serve_app_js))
            .route("/style.css", get(soundshared::network::server::serve_style_css))
            .route("/api/status", get(api_status))
            .route("/ws/audio", get(websocket_handler))
            .with_state(state)
            .layer(TraceLayer::new_for_http());

        // Create server handle (URL is None until Start command)
        let shutdown_tx_for_handle = shutdown_tx.clone();
        let handle = ServerHandle {
            shutdown_tx: shutdown_tx_for_handle,
            url: Arc::new(tokio::sync::Mutex::new(None)),
            broadcaster: broadcaster_handle.clone(),
            local_ip: Arc::new(tokio::sync::Mutex::new(detected_ip)),
        };

        // Send handle to audio processor
        let _ = server_handle_tx.send(handle.clone());

        // Spawn broadcaster task once — it runs continuously regardless of server state
        let mut broadcaster_task = tokio::spawn(broadcaster.run());

        // Subscribe to listener count
        let mut count_rx = broadcaster_handle.subscribe_count();

        // Convert crossbeam receiver to async-friendly polling (non-blocking)
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<ServerCommand>(32);
        let cmd_rx_crossbeam = server_cmd_rx.clone();
        tokio::spawn(async move {
            loop {
                if let Ok(cmd) = cmd_rx_crossbeam.try_recv() {
                    if cmd_tx.send(cmd).await.is_err() {
                        break;
                    }
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });

        // Server state — does NOT auto-start. Waits for GUI Start command.
        let mut server_running = false;
        let mut listener_count = 0;
        let mut server_task: Option<tokio::task::JoinHandle<()>> = None;

        info!("🌐 Server initialized, waiting for Start command");
        info!(
            "Server will bind {}:{} on the next Start command",
            bind_addr, bind_port
        );

        loop {
            tokio::select! {
                // Handle server commands from GUI
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(ServerCommand::Start { response }) => {
                            if !server_running {
                                // Bind listener and start serving ONLY on Start command
                                let addr = std::net::SocketAddr::new(bind_addr, bind_port);
                                match tokio::net::TcpListener::bind(addr).await {
                                    Ok(listener) => {
                                        let actual_addr = listener.local_addr()
                                            .map_err(|e| SoundSharedError::Other(e.to_string()))?;
                                        info!("Network server listening on {}", actual_addr);

                                        // Advertise somewhere the listener is actually
                                        // reachable, recomputed on every Start so an
                                        // edited bind address is reflected in the URL
                                        // the GUI displays and copies.
                                        let url = listen_url(bind_addr, actual_addr, detected_ip);

                                        // Spawn server task
                                        let app_clone = app.clone();
                                        let mut shutdown_rx_for_server = shutdown_tx.subscribe();
                                        let st = tokio::spawn(async move {
                                            info!("🌐 Server task started, beginning to serve on {}", actual_addr);
                                            axum::serve(listener, app_clone)
                                                .with_graceful_shutdown(async move {
                                                    let _ = shutdown_rx_for_server.recv().await;
                                                    info!("🌐 Shutdown signal received, stopping server...");
                                                })
                                                .await;
                                            info!("🌐 Server task ended");
                                        });

                                        server_running = true;
                                        listener_count = 0;
                                        server_task = Some(st);
                                        server_url = Some(url.clone());

                                        *handle.url.lock().await = Some(url.clone());
                                        let _ = response.send(Ok(url.clone()));
                                        info!("Server started at {}", url);
                                    }
                                    Err(e) => {
                                        error!("Failed to bind listener: {}", e);
                                        let _ = response.send(Err(SoundSharedError::Other(e.to_string()).into()));
                                    }
                                }
                            } else {
                                let _ = response.send(Err(anyhow::anyhow!("Server already running").into()));
                            }
                        }
                        Some(ServerCommand::Stop { response }) => {
                            if server_running {
                                shutdown_tx.send(()).ok();
                                server_running = false;
                                listener_count = 0;

                                // Wait for server task to finish
                                if let Some(st) = server_task.take() {
                                    let _ = st.await;
                                }

                                server_url = None;
                                *handle.url.lock().await = None;
                                let _ = listener_count_tx.send(0);
                                let _ = response.send(Ok(()));
                                info!("Server stopped");
                            } else {
                                let _ = response.send(Ok(()));
                            }
                        }
                        Some(ServerCommand::SetBind { host, port, response }) => {
                            if server_running {
                                // The socket is already bound; the new values
                                // only apply to the next Start.
                                let _ = response.send(Err("Stop the server first".to_string()));
                            } else {
                                match parse_bind_host(&host) {
                                    None => {
                                        let _ = response.send(Err(format!(
                                            "Not an IP address: {}",
                                            host
                                        )));
                                    }
                                    Some(_) if port == 0 => {
                                        let _ = response.send(Err(
                                            "Port must be 1-65535".to_string(),
                                        ));
                                    }
                                    Some(ip) => {
                                        bind_addr = ip;
                                        bind_port = port;
                                        info!(
                                            "Server bind address set to {}:{}",
                                            bind_addr, bind_port
                                        );
                                        let _ = response.send(Ok(()));
                                    }
                                }
                            }
                        }
                        Some(ServerCommand::GetStatus { response }) => {
                            let status = ServerStatus {
                                running: server_running,
                                address: server_url.clone(),
                                listener_count,
                            };
                            let _ = response.send(status);
                        }
                        None => break,
                    }
                }

                // Update listener count from broadcaster
                count = count_rx.recv() => {
                    if let Ok(count) = count {
                        listener_count = count;
                        let _ = listener_count_tx.send(listener_count);
                    }
                }
            }
        }

        // Cleanup on shutdown
        if let Some(st) = server_task.take() {
            let _ = st.await;
        }
        let _ = broadcaster_task.await;

        Ok::<(), SoundSharedError>(())
    });

    result
}

// Helper function for local IP detection
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

fn main() -> Result<()> {
    // Initialize panic hook for clean error reporting
    std::panic::set_hook(Box::new(|panic_info| {
        let msg = match panic_info.payload().downcast_ref::<&str>() {
            Some(s) => *s,
            None => match panic_info.payload().downcast_ref::<String>() {
                Some(s) => s.as_str(),
                None => "Unknown panic",
            },
        };
        let location = panic_info.location().map(|l| format!(" at {}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
        error!("PANIC: {}{}", msg, location);
        
        if let Ok(bt) = std::panic::catch_unwind(|| {
            backtrace::Backtrace::new()
        }) {
            error!("Backtrace:\n{:?}", bt);
        }
    }));

    // Load configuration
    let config = Config::load()?;
    info!("Configuration loaded: server={}, audio_buffer={}, log_level={}", 
        config.server_addr(), config.audio.buffer_size, config.logging.level);

    // Initialize logging
    init_logging(&config)?;

    info!("Starting SoundShared v{}", soundshared::VERSION);

    // Check for --server-only flag to run without GUI
    let args: Vec<String> = std::env::args().collect();
    let server_only = args.iter().any(|a| a == "--server-only");

    // Create channels for inter-component communication
    let (audio_cmd_tx, audio_cmd_rx) = bounded::<AudioCommand>(32);
    let (server_cmd_tx, server_cmd_rx) = bounded::<ServerCommand>(32);
    let (shutdown_tx, shutdown_rx) = bounded::<()>(1);
    let (listener_count_tx, listener_count_rx) = bounded::<usize>(1);
    let (server_handle_tx, server_handle_rx) = bounded::<ServerHandle>(1);
    let server_cmd_tx_for_start = server_cmd_tx.clone();

    // Create audio engine
    let audio_engine = AudioEngine::new()
        .map_err(|e| SoundSharedError::Audio(soundshared::error::AudioError::Engine(e.to_string())))?;
    
    // Set master volume from config
    audio_engine.set_master_volume(config.audio.master_volume)
        .map_err(|e| SoundSharedError::Audio(soundshared::error::AudioError::Mixer(e.to_string())))?;

    // Get the broadcast output receiver for WebSocket streaming
    let broadcast_output_rx = audio_engine.broadcast_output_receiver();

    // Wrap audio engine in Arc<Mutex> for sharing with GUI (main thread)
    let audio_engine_shared = Arc::new(Mutex::new(Some(audio_engine)));

    // Create network server config
    let server_config = ServerConfig {
        port: config.server.port,
        bind_addr: parse_bind_host(&config.server.host)
            .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0))),
        broadcaster_config: BroadcasterConfig::default(),
        websocket_config: WebSocketConfig::default(),
        static_dir: Some("src/network/static".to_string()),
    };

    // Create shared app state (for GUI)
    let app_state = AppState::new(
        audio_cmd_tx,
        server_cmd_tx,
        Arc::new(Mutex::new(0)),
        Arc::new(Mutex::new(config.clone())),
    );

    // Spawn server thread (does NOT auto-start - waits for Start command from GUI)
    let server_shutdown_rx = shutdown_rx.clone();
    let listener_count_tx_clone = listener_count_tx.clone();
    let server_cmd_rx_clone = server_cmd_rx.clone();
    let server_config_clone = server_config.clone();
    let server_handle_tx_clone = server_handle_tx.clone();
    thread::spawn(move || {
        if let Err(e) = run_server_and_get_handle(
            server_config_clone,
            server_cmd_rx_clone,
            server_shutdown_rx,
            listener_count_tx_clone,
            server_handle_tx_clone,
        ) {
            error!("Server error: {}", e);
        }
    });

    // Spawn listener count updater for GUI
    let listener_count_state = app_state.listener_count.clone();
    thread::spawn(move || {
        for count in listener_count_rx {
            *listener_count_state.lock().unwrap() = count;
        }
    });

    // Wait for server handle to be available, then spawn audio processor
    let broadcast_output_rx_for_processor = broadcast_output_rx;
    let processor_shutdown_rx = shutdown_rx.clone();
    thread::spawn(move || {
        // Wait for server handle
        let server_handle = match server_handle_rx.recv() {
            Ok(handle) => handle,
            Err(_) => {
                info!("Server handle channel closed, audio processor not started");
                return;
            }
        };

        let broadcaster = server_handle.broadcaster().clone();
        let mut processor = AudioProcessor::new(broadcast_output_rx_for_processor, broadcaster, processor_shutdown_rx);
        processor.run();
    });

    // Start the audio engine output stream (on main thread). If no local
    // output device is available, keep streaming to network listeners.
    {
        let engine_guard = audio_engine_shared.lock().unwrap();
        if let Some(engine) = engine_guard.as_ref() {
            if let Err(e) = engine.start_output() {
                warn!(
                    "Failed to start local audio output: {}. Falling back to headless streaming.",
                    e
                );
                // Carry the reason the device could not be opened, so the status
                // strip can say *why* the desktop is silent instead of just
                // reporting "headless".
                let reason = format!("local output unavailable: {e}");
                if let Err(headless_err) = engine.start_headless_output_with_reason(reason) {
                    error!("Failed to start headless audio engine: {}", headless_err);
                }
            }
        }
    }

    if server_only {
        info!("Running in server-only mode (no GUI). Press Ctrl+C to stop.");

        // There is no GUI to press "Start", so start the listener server here.
        let (response_tx, response_rx) = bounded::<anyhow::Result<String>>(1);
        if let Err(e) = server_cmd_tx_for_start.send(ServerCommand::Start { response: response_tx }) {
            error!("Failed to request server start: {}", e);
        } else {
            match response_rx.recv_timeout(Duration::from_secs(5)) {
                Ok(Ok(url)) => info!("Listener server available at {}", url),
                Ok(Err(e)) => error!("Failed to start listener server: {}", e),
                Err(e) => error!("Timed out waiting for listener server to start: {}", e),
            }
        }

        // Wait for Ctrl+C. The process exits after this, which tears down the
        // server and audio threads.
        let (ctrlc_tx, ctrlc_rx) = bounded::<()>(1);
        ctrlc::set_handler(move || {
            let _ = ctrlc_tx.send(());
        }).expect("Error setting Ctrl+C handler");

        let _ = ctrlc_rx.recv();

        info!("Shutting down...");
        let _ = shutdown_tx.send(());
        thread::sleep(Duration::from_millis(300));

        if let Some(mut engine) = audio_engine_shared.lock().unwrap().take() {
            if let Err(e) = engine.stop_output() {
                warn!("Error stopping audio engine: {}", e);
            }
        }

        info!("SoundShared stopped");
        Ok(())
    } else {
        // Run the GUI (blocks until window closes)
        // Pass audio_engine_shared and audio_cmd_rx so GUI can process commands on main thread
        info!("Starting GUI...");
        let gui_result = run_gui(app_state.clone(), audio_engine_shared.clone(), audio_cmd_rx);

        // Shutdown sequence
        info!("Shutting down...");
        let _ = shutdown_tx.send(());

        // Wait a bit for threads to clean up
        thread::sleep(Duration::from_millis(500));

        // Stop audio engine (on main thread)
        if let Some(mut engine) = audio_engine_shared.lock().unwrap().take() {
            if let Err(e) = engine.stop_output() {
                warn!("Error stopping audio engine: {}", e);
            }
        }

        info!("SoundShared stopped");
        gui_result.map_err(|e| SoundSharedError::Other(e.to_string()))
    }
}

/// Initialize logging based on configuration
fn init_logging(config: &Config) -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.logging.level));

    let subscriber = FmtSubscriber::builder()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .map_err(|e| SoundSharedError::Other(format!("Failed to set global logger: {}", e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

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
        // IPv4 wildcard socket + IPv6 detection: advertising `::1` would point
        // at a socket this server never opened.
        let actual4 = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 8080));
        assert_eq!(
            listen_url(
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                actual4,
                Some(IpAddr::V6(Ipv6Addr::LOCALHOST))
            ),
            "http://127.0.0.1:8080"
        );

        // …and the mirror image for an IPv6 wildcard socket.
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