//! End-to-end checks for the headless mixer -> broadcaster wire format.

use std::time::Duration;

use soundshared::audio::{AudioEngine, DecodedAudio};
use soundshared::network::broadcast::AudioFrame;

#[test]
fn headless_engine_publishes_10ms_frames_with_four_byte_header() {
    let engine = AudioEngine::new().expect("engine");
    let output_rx = engine.broadcast_output_receiver();

    // Two seconds of a constant 0.25 stereo signal.
    let audio = DecodedAudio::new(vec![0.25; 48_000 * 2 * 2], 48_000, 2);
    let sound_id = engine.load_sound_from_data(audio);

    engine.start_headless_output().expect("headless output");
    let track = engine.play(sound_id).expect("play");

    let mixed = output_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("mixer should publish a frame");

    // 10 ms at 48 kHz stereo = 480 frames = 960 interleaved samples.
    assert_eq!(mixed.buffer.len(), 960);
    assert!(mixed.buffer.iter().all(|s| (*s - 0.25).abs() < 1e-6));

    let frame = AudioFrame::new(mixed.buffer, 48_000, 2, mixed.seq_no);
    let binary = frame.to_binary();

    // Exactly one u32 sequence header, then the PCM payload.
    assert_eq!(binary.len(), 960 * 4 + 4);
    assert_eq!(
        AudioFrame::seq_no_from_binary(&binary),
        Some(mixed.seq_no as u32)
    );
    // The first PCM sample must begin at byte 4.
    let first = f32::from_le_bytes([binary[4], binary[5], binary[6], binary[7]]);
    assert!((first - 0.25).abs() < 1e-6);

    drop(track);
    engine.stop_output().expect("stop");
}

#[test]
fn master_volume_is_applied_exactly_once() {
    let engine = AudioEngine::new().expect("engine");
    let output_rx = engine.broadcast_output_receiver();

    let audio = DecodedAudio::new(vec![1.0; 48_000 * 2], 48_000, 2);
    let sound_id = engine.load_sound_from_data(audio);

    engine.set_master_volume(0.5).expect("volume");
    engine.start_headless_output().expect("headless output");
    let track = engine.play(sound_id).expect("play");

    let mixed = output_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("mixer should publish a frame");

    assert!(mixed.buffer.iter().all(|s| (*s - 0.5).abs() < 1e-6));

    drop(track);
    engine.stop_output().expect("stop");
}
