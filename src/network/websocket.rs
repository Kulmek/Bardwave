//! WebSocket session handling for audio streaming

use std::time::Duration;
use tokio::sync::mpsc;
use axum::extract::ws::{Message, WebSocket};
use tracing::{info, warn, error, debug};
use uuid::Uuid;

use crate::network::broadcast::BroadcasterHandle;

/// Configuration for WebSocket sessions
#[derive(Debug, Clone)]
pub struct WebSocketConfig {
    /// Maximum message size (bytes)
    pub max_message_size: usize,
    /// Ping interval for keepalive
    pub ping_interval: Duration,
    /// Client send buffer size
    pub send_buffer_size: usize,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            max_message_size: 16 * 1024 * 1024, // 16 MB
            ping_interval: Duration::from_secs(30),
            send_buffer_size: 100,
        }
    }
}

/// Errors that can occur in WebSocket handling
#[derive(Debug, thiserror::Error)]
pub enum WebSocketError {
    #[error("WebSocket protocol error: {0}")]
    Protocol(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Client disconnected")]
    Disconnected,
    #[error("Invalid message format: {0}")]
    InvalidMessage(String),
    #[error("Send channel closed")]
    SendClosed,
}

/// A WebSocket session representing a connected listener
pub struct WebSocketSession {
    id: Uuid,
    ws: WebSocket,
    broadcaster: BroadcasterHandle,
    config: WebSocketConfig,
    /// Channel to send messages to this client
    msg_tx: mpsc::Sender<Message>,
    /// Receiver for messages to send
    msg_rx: mpsc::Receiver<Message>,
}

impl WebSocketSession {
    /// Create a new WebSocket session
    pub fn new(
        ws: WebSocket,
        broadcaster: BroadcasterHandle,
        config: WebSocketConfig,
    ) -> (Self, mpsc::Sender<Message>) {
        let (msg_tx, msg_rx) = mpsc::channel(config.send_buffer_size);
        let id = Uuid::new_v4();

        let session = Self {
            id,
            ws,
            broadcaster,
            config,
            msg_tx: msg_tx.clone(),
            msg_rx,
        };

        (session, msg_tx)
    }

    /// Get session ID
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Get the message sender for external use (e.g., broadcaster)
    pub fn message_sender(&self) -> mpsc::Sender<Message> {
        self.msg_tx.clone()
    }

    /// Run the session - handles incoming/outgoing messages
    pub async fn run(mut self) -> Result<(), WebSocketError> {
        info!("🔌 WebSocket session started: {}", self.id);

        // Register with broadcaster
        let msg_tx = self.msg_tx.clone();
        let client_id = self.broadcaster.register_client(msg_tx).await.map_err(|e| WebSocketError::Protocol(e.to_string()))?;
        info!("🔌 Client registered with ID: {}", client_id);

        // Send welcome message
        let welcome = serde_json::json!({
            "type": "welcome",
            "data": {
                "client_id": client_id.to_string(),
                "message": "Connected to SoundShared audio stream"
            }
        });
        let _ = self.msg_tx.try_send(Message::Text(welcome.to_string().into()));
        info!("🔌 Welcome message sent to client");

        // Split into incoming and outgoing tasks
        // Note: axum's WebSocket doesn't have split() so we handle both in one loop
        let mut interval = tokio::time::interval(self.config.ping_interval);

        loop {
            tokio::select! {
                // Outgoing: messages from broadcaster -> WebSocket
                Some(msg) = self.msg_rx.recv() => {
                    debug!("🔌 Sending message to client: {:?}", msg);
                    if let Err(e) = self.ws.send(msg).await {
                        warn!("🔌 Failed to send WebSocket message to {}: {}", self.id, e);
                        break;
                    }
                }
                // Incoming: WebSocket messages
                Some(msg_result) = self.ws.recv() => {
                    match msg_result {
                        Ok(Message::Text(text)) => {
                            debug!("🔌 Received text from {}: {}", self.id, text);
                            if let Err(e) = self.handle_text_message(&text).await {
                                warn!("🔌 Error handling text message from {}: {}", self.id, e);
                            }
                        }
                        Ok(Message::Binary(data)) => {
                            debug!("🔌 Received binary from {}: {} bytes", self.id, data.len());
                            // Ignore binary from client (we only send audio)
                        }
                        Ok(Message::Ping(data)) => {
                            debug!("🔌 Ping from {}, sending pong", self.id);
                            let _ = self.msg_tx.try_send(Message::Pong(data));
                        }
                        Ok(Message::Pong(_)) => {
                            debug!("🔌 Pong from {}", self.id);
                            // Keepalive response
                        }
                        Ok(Message::Close(reason)) => {
                            info!("🔌 Client {} requested close: {:?}", self.id, reason);
                            break;
                        }
                        Err(e) => {
                            error!("🔌 WebSocket error for {}: {}", self.id, e);
                            return Err(WebSocketError::Protocol(e.to_string()));
                        }
                    }
                }
                // Periodic ping
                _ = interval.tick() => {
                    debug!("🔌 Sending ping to {}", self.id);
                    let _ = self.msg_tx.try_send(Message::Ping(vec![].into()));
                }
            }
        }

        // Unregister on disconnect
        self.broadcaster.unregister_client(client_id).await.map_err(|e| WebSocketError::Protocol(e.to_string()))?;
        info!("🔌 WebSocket session ended: {}", self.id);

        Ok(())
    }

    /// Handle incoming text messages (control messages from client)
    async fn handle_text_message(&mut self, text: &str) -> Result<(), WebSocketError> {
        debug!("Received text from {}: {}", self.id, text);

        // Parse as JSON for control messages
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
            if let Some(msg_type) = value.get("type").and_then(|v| v.as_str()) {
                match msg_type {
                    "ping" => {
                        let pong = serde_json::json!({"type": "pong"});
                        let _ = self.msg_tx.try_send(Message::Text(pong.to_string().into()));
                    }
                    "get_format" => {
                        // Client requests current audio format
                        // The broadcaster will send format info when available
                    }
                    _ => {
                        debug!("Unknown message type from {}: {}", self.id, msg_type);
                    }
                }
            }
        }

        Ok(())
    }
}

/// Upgrade HTTP request to WebSocket and spawn session
pub async fn handle_websocket_upgrade(
    ws: WebSocket,
    broadcaster: BroadcasterHandle,
    config: WebSocketConfig,
) {
    info!("🔌 New WebSocket connection from client");
    let (session, _msg_tx) = WebSocketSession::new(ws, broadcaster, config);

    if let Err(e) = session.run().await {
        warn!("🔌 WebSocket session error: {}", e);
    } else {
        info!("🔌 WebSocket session ended cleanly");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn test_websocket_config_default() {
        let config = WebSocketConfig::default();
        assert_eq!(config.max_message_size, 16 * 1024 * 1024);
        assert_eq!(config.ping_interval, Duration::from_secs(30));
        assert_eq!(config.send_buffer_size, 100);
    }
}