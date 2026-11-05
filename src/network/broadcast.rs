//! Audio broadcasting to WebSocket clients

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex, broadcast, oneshot};
use axum::extract::ws::Message;
use tracing::{info, warn};
use uuid::Uuid;

/// Audio format information sent to clients on connect
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AudioFormatInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub format: String, // "f32" for raw f32 PCM
    pub frame_size: usize, // samples per channel per frame
}

/// Configuration for the broadcaster
#[derive(Debug, Clone)]
pub struct BroadcasterConfig {
    /// Maximum number of audio frames to buffer per client
    pub client_buffer_size: usize,
    /// Channel capacity for incoming audio from engine
    pub engine_channel_capacity: usize,
    /// Interval for sending keepalive/ping frames
    pub ping_interval: Duration,
}

impl Default for BroadcasterConfig {
    fn default() -> Self {
        Self {
            client_buffer_size: 200,
            engine_channel_capacity: 500,
            ping_interval: Duration::from_secs(30),
        }
    }
}

/// Internal messages for broadcaster control
enum BroadcasterCommand {
    RegisterClient(mpsc::Sender<Message>, oneshot::Sender<Uuid>),
    UnregisterClient(Uuid),
}

/// Handle to control the broadcaster
#[derive(Clone)]
pub struct BroadcasterHandle {
    /// Sender for audio frames from the audio engine
    audio_tx: mpsc::Sender<AudioFrame>,
    /// Current listener count
    listener_count: Arc<Mutex<usize>>,
    /// Broadcast channel for listener count updates
    count_tx: broadcast::Sender<usize>,
    /// Channel to send commands to broadcaster
    command_tx: mpsc::Sender<BroadcasterCommand>,
}

impl BroadcasterHandle {
    /// Send mixed audio frame from audio engine
    /// Returns error if channel is full (backpressure - engine should drop frame)
    pub fn send_audio(&self, frame: AudioFrame) -> Result<(), BroadcastError> {
        self.audio_tx
            .try_send(frame)
            .map_err(|_| BroadcastError::ChannelFull)
    }

    /// Get current listener count
    pub async fn listener_count(&self) -> usize {
        *self.listener_count.lock().await
    }

    /// Subscribe to listener count changes
    pub fn subscribe_count(&self) -> broadcast::Receiver<usize> {
        self.count_tx.subscribe()
    }

    /// Register a new WebSocket client
    pub async fn register_client(&self, ws_tx: mpsc::Sender<Message>) -> Result<Uuid, BroadcastError> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.command_tx
            .send(BroadcasterCommand::RegisterClient(ws_tx, resp_tx))
            .await
            .map_err(|_| BroadcastError::NotRunning)?;
        resp_rx.await.map_err(|_| BroadcastError::NotRunning)
    }

    /// Unregister a WebSocket client
    pub async fn unregister_client(&self, id: Uuid) -> Result<(), BroadcastError> {
        self.command_tx
            .send(BroadcasterCommand::UnregisterClient(id))
            .await
            .map_err(|_| BroadcastError::NotRunning)
    }
}

/// Raw audio frame from the audio engine
#[derive(Debug, Clone)]
pub struct AudioFrame {
    /// Interleaved f32 PCM samples (L, R, L, R, ...)
    pub samples: Vec<f32>,
    /// Sample rate (e.g., 48000)
    pub sample_rate: u32,
    /// Number of channels (e.g., 2 for stereo)
    pub channels: u16,
    /// Timestamp for synchronization
    pub timestamp: std::time::Instant,
    /// Monotonically increasing sequence number for gap detection
    pub seq_no: u64,
}

impl AudioFrame {
    /// Create a new audio frame
    pub fn new(samples: Vec<f32>, sample_rate: u32, channels: u16, seq_no: u64) -> Self {
        Self {
            samples,
            sample_rate,
            channels,
            timestamp: std::time::Instant::now(),
            seq_no,
        }
    }

    /// Get number of frames (samples per channel)
    pub fn frame_count(&self) -> usize {
        self.samples.len() / self.channels as usize
    }

    /// Get duration of this frame in seconds
    pub fn duration(&self) -> f32 {
        self.frame_count() as f32 / self.sample_rate as f32
    }

    /// Convert to binary WebSocket frame.
    ///
    /// Format: [seq_no: u32 LE][pcm_samples: f32 LE ...]
    /// The sequence number allows clients to detect missing frames.
    pub fn to_binary(&self) -> Vec<u8> {
        let mut result = Vec::with_capacity(4 + self.samples.len() * 4);
        result.extend_from_slice(&self.seq_no.to_le_bytes());
        result.extend_from_slice(bytemuck::cast_slice(&self.samples));
        result
    }

    /// Parse the sequence number from a binary frame (first 4 bytes).
    pub fn seq_no_from_binary(data: &[u8]) -> Option<u32> {
        if data.len() >= 4 {
            Some(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
        } else {
            None
        }
    }
}

/// Errors that can occur in the broadcaster
#[derive(Debug, thiserror::Error)]
pub enum BroadcastError {
    #[error("Audio channel full - backpressure, frame dropped")]
    ChannelFull,
    #[error("Broadcaster not running")]
    NotRunning,
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Internal client session for broadcasting
struct ClientSession {
    id: Uuid,
    ws_tx: mpsc::Sender<Message>,
    connected_at: std::time::Instant,
}

/// The audio broadcaster - receives from engine, sends to all WebSocket clients
pub struct AudioBroadcaster {
    config: BroadcasterConfig,
    /// Receiver for audio frames from engine
    audio_rx: mpsc::Receiver<AudioFrame>,
    /// Receiver for control commands
    command_rx: mpsc::Receiver<BroadcasterCommand>,
    /// Connected clients
    clients: Arc<Mutex<Vec<ClientSession>>>,
    /// Listener count for external tracking
    listener_count: Arc<Mutex<usize>>,
    /// Broadcast channel for listener count updates
    count_tx: broadcast::Sender<usize>,
    /// Audio format info (set on first frame)
    format_info: Arc<Mutex<Option<AudioFormatInfo>>>,
    /// Monotonically increasing sequence number for audio frames
    seq_no: AtomicU64,
}

impl AudioBroadcaster {
    /// Create a new broadcaster, returning handle for engine and the broadcaster to run
    pub fn new(config: BroadcasterConfig) -> (BroadcasterHandle, Self) {
        let (audio_tx, audio_rx) = mpsc::channel(config.engine_channel_capacity);
        let (command_tx, command_rx) = mpsc::channel(32);
        let (count_tx, _) = broadcast::channel(16);
        let listener_count = Arc::new(Mutex::new(0));
        let clients = Arc::new(Mutex::new(Vec::new()));
        let format_info = Arc::new(Mutex::new(None));

        let handle = BroadcasterHandle {
            audio_tx,
            listener_count: listener_count.clone(),
            count_tx: count_tx.clone(),
            command_tx,
        };

        let broadcaster = Self {
            config,
            audio_rx,
            command_rx,
            clients,
            listener_count,
            count_tx,
            format_info,
            seq_no: AtomicU64::new(0),
        };

        (handle, broadcaster)
    }

    /// Register a new WebSocket client
    async fn register_client(&self, ws_tx: mpsc::Sender<Message>) -> Uuid {
        let id = Uuid::new_v4();
        let session = ClientSession {
            id,
            ws_tx,
            connected_at: std::time::Instant::now(),
        };

        let mut clients = self.clients.lock().await;
        clients.push(session);

        // Update listener count
        let count = clients.len();
        let mut listener_count = self.listener_count.lock().await;
        *listener_count = count;
        let _ = self.count_tx.send(count);

        info!("Client connected: {} (total listeners: {})", id, count);
        id
    }

    /// Unregister a WebSocket client
    async fn unregister_client(&self, id: Uuid) {
        let mut clients = self.clients.lock().await;
        clients.retain(|c| c.id != id);

        let count = clients.len();
        let mut listener_count = self.listener_count.lock().await;
        *listener_count = count;
        let _ = self.count_tx.send(count);

        info!("Client disconnected: {} (total listeners: {})", id, count);
    }

    /// Send audio format info to a newly connected client
    async fn send_format_info(&self, ws_tx: &mpsc::Sender<Message>) {
        let format_info = self.format_info.lock().await;
        if let Some(info) = format_info.as_ref() {
            let msg = serde_json::json!({
                "type": "format",
                "data": info
            });
            if let Ok(text) = serde_json::to_string(&msg) {
                let _ = ws_tx.try_send(Message::Text(text.into()));
            }
        }
    }

    /// Broadcast audio frame to all connected clients
    async fn broadcast_frame(&self, frame: AudioFrame) {
        // Update format info on first frame
        {
            let mut format_info = self.format_info.lock().await;
            if format_info.is_none() {
                *format_info = Some(AudioFormatInfo {
                    sample_rate: frame.sample_rate,
                    channels: frame.channels,
                    format: "f32".to_string(),
                    frame_size: frame.frame_count(),
                });
            }
        }

        let seq = self.seq_no.fetch_add(1, Ordering::Relaxed);
        let frame_with_seq = AudioFrame {
            samples: frame.samples,
            sample_rate: frame.sample_rate,
            channels: frame.channels,
            timestamp: frame.timestamp,
            seq_no: seq,
        };

        let binary_data = frame_with_seq.to_binary();
        let message = Message::Binary(binary_data.into());

        let mut clients = self.clients.lock().await;
        let mut disconnected = Vec::new();

        for client in clients.iter_mut() {
            // Try to send; if full, skip this frame for this client but keep
            // the connection open — dropping clients causes reconnect storms.
            if client.ws_tx.try_send(message.clone()).is_err() {
                // Mark for removal only if the channel is closed (not just full)
                disconnected.push(client.id);
            }
        }

        // Remove disconnected clients
        if !disconnected.is_empty() {
            for id in disconnected {
                clients.retain(|c| c.id != id);
                warn!("Dropped disconnected client: {}", id);
            }

            let count = clients.len();
            let mut listener_count = self.listener_count.lock().await;
            *listener_count = count;
            let _ = self.count_tx.send(count);
        }
    }

    /// Run the broadcaster main loop
    pub async fn run(mut self) {
        info!("Audio broadcaster started");

        loop {
            tokio::select! {
                // Handle audio frames from engine
                Some(frame) = self.audio_rx.recv() => {
                    self.broadcast_frame(frame).await;
                }
                // Handle control commands
                Some(cmd) = self.command_rx.recv() => {
                    match cmd {
                        BroadcasterCommand::RegisterClient(ws_tx, resp_tx) => {
                            let id = self.register_client(ws_tx).await;
                            let _ = resp_tx.send(id);
                        }
                        BroadcasterCommand::UnregisterClient(id) => {
                            self.unregister_client(id).await;
                        }
                    }
                }
                else => break, // Both channels closed
            }
        }

        info!("Audio broadcaster stopped (channels closed)");
    }
}

/// Helper: create a test audio frame for testing
#[cfg(test)]
pub fn create_test_frame() -> AudioFrame {
    // 48000 Hz, stereo, 10ms frame = 480 frames = 960 samples
    let samples: Vec<f32> = (0..960).map(|i| (i as f32 * 0.01).sin() * 0.1).collect();
    AudioFrame::new(samples, 48000, 2, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn test_broadcaster_basic() {
        let config = BroadcasterConfig::default();
        let (handle, broadcaster) = AudioBroadcaster::new(config);

        // Spawn broadcaster
        let broadcaster_task = tokio::spawn(broadcaster.run());

        // Send a test frame
        let frame = create_test_frame();
        assert!(handle.send_audio(frame).is_ok());

        // Give it time to process
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Shutdown
        drop(handle);
        let _ = timeout(Duration::from_secs(1), broadcaster_task).await;
    }

    #[tokio::test]
    async fn test_audio_frame_properties() {
        let frame = create_test_frame();
        assert_eq!(frame.sample_rate, 48000);
        assert_eq!(frame.channels, 2);
        assert_eq!(frame.frame_count(), 480);
        assert!((frame.duration() - 0.01).abs() < 0.001);
        
        let binary = frame.to_binary();
        assert_eq!(binary.len(), frame.samples.len() * 4 + 4); // f32 = 4 bytes + 4 byte seq_no header
    }

    #[tokio::test]
    async fn test_backpressure() {
        let config = BroadcasterConfig {
            engine_channel_capacity: 2,
            ..Default::default()
        };
        let (handle, broadcaster) = AudioBroadcaster::new(config);
        let _task = tokio::spawn(broadcaster.run());

        // Fill the channel
        assert!(handle.send_audio(create_test_frame()).is_ok());
        assert!(handle.send_audio(create_test_frame()).is_ok());
        // Third should fail (backpressure)
        assert!(matches!(handle.send_audio(create_test_frame()), Err(BroadcastError::ChannelFull)));

        drop(handle);
    }

    #[tokio::test]
    async fn test_register_unregister_client() {
        let config = BroadcasterConfig::default();
        let (handle, broadcaster) = AudioBroadcaster::new(config);
        let _task = tokio::spawn(broadcaster.run());

        // Create a dummy message channel
        let (tx, _rx) = mpsc::channel(10);
        
        // Register client
        let id = handle.register_client(tx).await.unwrap();
        assert_eq!(handle.listener_count().await, 1);
        
        // Unregister client
        handle.unregister_client(id).await.unwrap();
        assert_eq!(handle.listener_count().await, 0);

        drop(handle);
    }
}