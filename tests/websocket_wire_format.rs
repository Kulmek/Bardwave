//! End-to-end WebSocket wire-format check.
//!
//! Guards against the regression that caused browser crackle: the server must
//! send exactly one 4-byte little-endian sequence header before the f32 PCM.

use std::time::Duration;

use futures::StreamExt;
use soundshared::network::broadcast::{AudioBroadcaster, AudioFrame, BroadcasterConfig};
use soundshared::network::server::{
    api_status, get, serve_app_js, serve_index_html, serve_style_css, websocket_handler, Router,
    ServerState,
};
use soundshared::network::websocket::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn websocket_audio_frame_has_four_byte_header_and_aligned_pcm() {
    let (broadcaster, broadcaster_task) = AudioBroadcaster::new(BroadcasterConfig::default());
    tokio::spawn(broadcaster_task.run());

    let state = ServerState {
        broadcaster: broadcaster.clone(),
        websocket_config: WebSocketConfig::default(),
    };

    let app = Router::new()
        .route("/", get(serve_index_html))
        .route("/app.js", get(serve_app_js))
        .route("/style.css", get(serve_style_css))
        .route("/api/status", get(api_status))
        .route("/ws/audio", get(websocket_handler))
        .with_state(state);

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
