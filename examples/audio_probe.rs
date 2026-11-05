//! Temporary diagnostic: report the CPAL device/config situation and whether
//! the engine's local output stream actually produces audio callbacks.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use soundshared::audio::engine::device_adapter::{convert_stereo_into, DeviceAdapter};
use soundshared::audio::AudioEngine;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn main() {
    let host = cpal::default_host();
    println!("host: {:?}", host.id());

    match host.default_output_device() {
        Some(device) => {
            println!("default output device: {:?}", device.name());
            match device.supported_output_configs() {
                Ok(configs) => {
                    let list: Vec<_> = configs.collect();
                    println!("supported_output_configs: {}", list.len());
                    for c in &list {
                        println!(
                            "  {}ch {:?} {}..{}",
                            c.channels(),
                            c.sample_format(),
                            c.min_sample_rate().0,
                            c.max_sample_rate().0
                        );
                    }
                }
                Err(e) => println!("supported_output_configs error: {}", e),
            }
            match device.default_output_config() {
                Ok(c) => println!(
                    "default_output_config: {}ch {} {:?}",
                    c.channels(),
                    c.sample_rate().0,
                    c.sample_format()
                ),
                Err(e) => println!("default_output_config error: {}", e),
            }
        }
        None => println!("no default output device"),
    }

    // Raw stream probe: does the device callback fire at all?
    if let Some(device) = host.default_output_device() {
        if let Ok(supported) = device.default_output_config() {
            let cfg: cpal::StreamConfig = supported.config();
            let calls = Arc::new(AtomicU64::new(0));
            let calls_cb = Arc::clone(&calls);
            let fmt = supported.sample_format();
            let stream = match fmt {
                cpal::SampleFormat::F32 => device.build_output_stream(
                    &cfg,
                    move |data: &mut [f32], _| {
                        calls_cb.fetch_add(1, Ordering::Relaxed);
                        for s in data.iter_mut() {
                            *s = 0.0;
                        }
                    },
                    |e| eprintln!("stream err: {}", e),
                    None,
                ),
                cpal::SampleFormat::I16 => device.build_output_stream(
                    &cfg,
                    move |data: &mut [i16], _| {
                        calls_cb.fetch_add(1, Ordering::Relaxed);
                        for s in data.iter_mut() {
                            *s = 0;
                        }
                    },
                    |e| eprintln!("stream err: {}", e),
                    None,
                ),
                cpal::SampleFormat::U16 => device.build_output_stream(
                    &cfg,
                    move |data: &mut [u16], _| {
                        calls_cb.fetch_add(1, Ordering::Relaxed);
                        for s in data.iter_mut() {
                            *s = 0;
                        }
                    },
                    |e| eprintln!("stream err: {}", e),
                    None,
                ),
                _ => {
                    println!("raw probe: unsupported default sample format {:?}", fmt);
                    return;
                }
            };
            match stream {
                Ok(s) => {
                    let _ = s.play();
                    std::thread::sleep(Duration::from_millis(400));
                    println!(
                        "raw probe callbacks in 400ms: {}",
                        calls.load(Ordering::Relaxed)
                    );
                }
                Err(e) => println!("raw probe build error: {}", e),
            }
        }
    }

    // Engine probe: exactly the path main.rs takes.
    let engine = AudioEngine::new().expect("engine");
    engine.set_master_volume(0.8).unwrap();
    println!(
        "local output status before start: {:?}",
        engine.local_output_status()
    );
    match engine.start_output() {
        Ok(()) => println!("engine.start_output: OK"),
        Err(e) => println!("engine.start_output: ERR {}", e),
    }
    let status = engine.local_output_status();
    println!("local output status after start: {:?}", status);
    println!("local output summary: {}", status.summary());
    println!("local audio running: {}", status.is_local_audio_running());
    match engine.start_headless_output() {
        Ok(()) => println!("engine.start_headless_output: OK (already running is fine)"),
        Err(e) => println!("engine.start_headless_output: ERR {}", e),
    }
    println!("engine.is_running: {}", engine.is_running());
    println!(
        "broadcast receiver present: {}",
        engine.broadcast_output_receiver().is_empty()
    );

    // Simulated device-config fallback: what the engine does when the device
    // cannot run the 48 kHz stereo f32 mix. No device is switched here — this
    // exercises the same conversion the device callback would use.
    demonstrate_device_fallback(44_100, 2);
    demonstrate_device_fallback(48_000, 1);
}

/// Show the mix → device conversion for a device layout we cannot force CPAL to
/// open on this machine.
fn demonstrate_device_fallback(sample_rate: u32, channels: u16) {
    let adapter = DeviceAdapter::new(sample_rate, channels);
    println!(
        "--- simulated device: {} Hz, {} ch (identity: {}) ---",
        adapter.sample_rate,
        adapter.channels,
        adapter.is_identity()
    );

    // A 10 ms block of the mix: 48 kHz, stereo, f32.
    let mut staging: Vec<f32> = Vec::new();
    let mut carry = 0.0f64;
    let device_frames = 480;
    let staging_frames = adapter.frames(device_frames, &mut carry);
    staging.resize(staging_frames * 2, 0.25);
    println!(
        "  {} device frames need {} frames of the 48 kHz mix (carry left: {:.3})",
        device_frames, staging_frames, carry
    );

    match channels {
        1 => {
            let mut out = vec![0.0f32; device_frames];
            convert_stereo_into(&staging, &mut out, adapter);
            println!("  mono device buffer: {:?} ...", &out[..4]);
        }
        2 => {
            let mut out = vec![0u16; device_frames * 2];
            convert_stereo_into(&staging, &mut out, adapter);
            println!("  u16 stereo device buffer: {:?} ...", &out[..4]);
        }
        _ => {
            let mut out = vec![0.0f32; device_frames * channels as usize];
            convert_stereo_into(&staging, &mut out, adapter);
            println!("  {}-channel device buffer: {:?} ...", channels, &out[..4]);
        }
    }
}
