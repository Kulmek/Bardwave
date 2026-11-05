//! End-to-end WebSocket wire-format check.
//!
//! Guards against the regression that caused browser crackle: the server must
//! send exactly one 4-byte little-endian sequence header before the f32 PCM.

use std::time::Duration;

use futures::StreamExt;
use soundshared::network::broadcast::{AudioBroadcaster, AudioFrame, BroadcasterConfig};
use soundshared::network::server::{build_router, ServerState};
use soundshared::network::websocket::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn websocket_audio_frame_has_four_byte_header_and_aligned_pcm() {
    let (broadcaster, broadcaster_task) = AudioBroadcaster::new(BroadcasterConfig::default());
    tokio::spawn(broadcaster_task.run());

    let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
    let state = ServerState {
        broadcaster: broadcaster.clone(),
        websocket_config: WebSocketConfig::default(),
        shutdown_tx,
    };

    // The router the app actually serves, so this test covers the real routes.
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let url = format!("ws://{}/ws/audio", addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(url)
        .await
        .expect("websocket connect");

    // Give the session task time to register with the broadcaster.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut samples = vec![0.0f32; 960];
    samples[0] = 0.5;
    samples[1] = -0.5;
    broadcaster
        .send_audio(AudioFrame::new(samples, 48_000, 2, 0))
        .expect("queue frame");

    let mut binary = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(500), ws.next()).await {
            Ok(Some(Ok(Message::Binary(data)))) => {
                binary = Some(data);
                break;
            }
            Ok(Some(Ok(_))) => continue, // welcome/format text frames
            Ok(Some(Err(e))) => panic!("websocket error: {e}"),
            Ok(None) => break,
            Err(_) => continue,
        }
    }

    let data = binary.expect("expected an audio frame");
    assert_eq!(data.len(), 4 + 960 * 4, "4-byte header + 960 f32 samples");

    let seq = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    assert_eq!(seq, 0);

    let first = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let second = f32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    assert!((first - 0.5).abs() < 1e-6, "left sample shifted");
    assert!((second + 0.5).abs() < 1e-6, "right sample shifted");

    let _ = ws.close(None).await;
}

/// A listener that connects *after* audio started must still be told the stream
/// format: the announcement is replayed from the broadcaster's remembered
/// format instead of being prepared and quietly dropped.
#[tokio::test]
async fn late_listener_receives_the_stream_format() {
    let (broadcaster, broadcaster_task) = AudioBroadcaster::new(BroadcasterConfig::default());
    tokio::spawn(broadcaster_task.run());

    // Establish the format the way the engine does: one mixed frame.
    broadcaster
        .send_audio(AudioFrame::new(vec![0.0f32; 960], 48_000, 2, 0))
        .expect("queue frame");
    tokio::time::sleep(Duration::from_millis(100)).await;

    let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
    let state = ServerState {
        broadcaster: broadcaster.clone(),
        websocket_config: WebSocketConfig::default(),
        shutdown_tx,
    };
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws/audio", addr))
        .await
        .expect("websocket connect");

    let mut format = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline && format.is_none() {
        match tokio::time::timeout(Duration::from_millis(500), ws.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let value: serde_json::Value = serde_json::from_str(&text).expect("json");
                if value.get("type").and_then(|t| t.as_str()) == Some("format") {
                    format = Some(value);
                }
            }
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => panic!("websocket error: {e}"),
            Ok(None) => break,
            Err(_) => continue,
        }
    }

    let format = format.expect("a late listener must be sent the stream format");
    assert_eq!(format["data"]["sample_rate"], 48_000);
    assert_eq!(format["data"]["channels"], 2);
    assert_eq!(format["data"]["format"], "f32");

    let _ = ws.close(None).await;
}

/// Stopping the server closes the listener socket. Before this, every session
/// stayed open and the graceful shutdown never completed, which left the
/// server thread blocked forever on "Stop server".
#[tokio::test]
async fn server_shutdown_closes_listener_sessions() {
    let (broadcaster, broadcaster_task) = AudioBroadcaster::new(BroadcasterConfig::default());
    tokio::spawn(broadcaster_task.run());

    let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
    let state = ServerState {
        broadcaster: broadcaster.clone(),
        websocket_config: WebSocketConfig::default(),
        shutdown_tx: shutdown_tx.clone(),
    };
    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws/audio", addr))
        .await
        .expect("websocket connect");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // What main.rs's Stop Server sends.
    shutdown_tx.send(()).expect("shutdown signal");

    let closed = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(message) = ws.next().await {
            match message {
                Ok(Message::Close(_)) | Err(_) => return true,
                Ok(_) => continue,
            }
        }
        true
    })
    .await
    .unwrap_or(false);

    assert!(closed, "the session must close when the server stops");
    let _ = ws.close(None).await;
    // The plain test server keeps accepting, so it is detached, not awaited.
    server.abort();
}
