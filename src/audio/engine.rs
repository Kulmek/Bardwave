//! Audio engine with CPAL output stream
//! High-level API for sound management and playback

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, SampleRate, StreamConfig};
use crossbeam_channel::{Receiver, Sender};
use parking_lot::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::audio::decoder::{AudioDecoder, DecodedAudio, DecoderError, TARGET_CHANNELS, TARGET_SAMPLE_RATE};
use crate::audio::mixer::{AudioMixer, MixerConfig, MixedOutput};

/// Unique identifier for a loaded sound
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SoundId(pub Uuid);

impl SoundId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SoundId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SoundId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Handle for controlling playback of a sound
#[derive(Debug, Clone)]
pub struct TrackHandle {
    pub sound_id: SoundId,
    pub track_id: Uuid,
    pub(crate) engine: Arc<AudioEngineInner>,
}

/// Errors that can occur in the audio engine
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("Decoder error: {0}")]
    Decoder(#[from] DecoderError),
    #[error("Mixer error: {0}")]
    Mixer(#[from] crate::audio::mixer::MixerError),
    #[error("CPAL error: {0}")]
    Cpal(#[from] cpal::BuildStreamError),
    #[error("CPAL play error: {0}")]
    CpalPlay(#[from] cpal::PlayStreamError),
    #[error("No audio device available")]
    NoDevice,
    #[error("Unsupported sample format: {0}")]
    UnsupportedFormat(String),
    #[error("Sound not found: {0}")]
    SoundNotFound(SoundId),
    #[error("Track not found: {0}")]
    TrackNotFound(Uuid),
    #[error("Channel error: {0}")]
    Channel(#[from] crossbeam_channel::SendError<MixedOutput>),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Internal engine state shared across handles
struct AudioEngineInner {
    mixer: Arc<AudioMixer>,
    /// Loaded sounds: SoundId -> Arc<DecodedAudio>
    sounds: RwLock<HashMap<SoundId, Arc<DecodedAudio>>>,
    /// Active tracks: track_id -> SoundId
    active_tracks: RwLock<HashMap<Uuid, SoundId>>,
    /// CPAL output stream
    stream: Mutex<Option<cpal::Stream>>,
    /// Engine running state
    running: Arc<AtomicBool>,
    /// Master volume (synced with mixer)
    master_volume: Arc<parking_lot::Mutex<f32>>,
    /// Output sender for network broadcast
    broadcast_tx: Option<Sender<MixedOutput>>,
    /// Next track ID
    next_track_id: AtomicU64,
}

// Manual Debug implementation since cpal::Stream doesn't implement Debug
impl std::fmt::Debug for AudioEngineInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioEngineInner")
            .field("mixer", &self.mixer)
            .field("sounds", &self.sounds)
            .field("active_tracks", &self.active_tracks)
            .field("stream", &"<cpal::Stream>")
            .field("running", &self.running)
            .field("master_volume", &self.master_volume)
            .field("broadcast_tx", &self.broadcast_tx)
            .field("next_track_id", &self.next_track_id)
            .finish()
    }
}

impl AudioEngineInner {
    /// Stop a specific track by its ID
    fn stop_track(&self, track_id: Uuid) -> Result<(), EngineError> {
        self.mixer.stop_track(track_id)?;
        self.active_tracks.write().remove(&track_id);
        Ok(())
    }

    /// Pause a specific track
    fn pause_track(&self, track_id: Uuid) -> Result<(), EngineError> {
        self.mixer.pause_track(track_id)?;
        Ok(())
    }

    /// Resume a paused track
    fn resume_track(&self, track_id: Uuid) -> Result<(), EngineError> {
        self.mixer.play_track(track_id)?;
        Ok(())
    }

    /// Set volume for a specific track
    fn set_track_volume(&self, track_id: Uuid, volume: f32) -> Result<(), EngineError> {
        self.mixer.set_track_volume(track_id, volume)?;
        Ok(())
    }

    /// Get volume for a specific track
    fn track_volume(&self, track_id: Uuid) -> Result<f32, EngineError> {
        Ok(self.mixer.track_volume(track_id)?)
    }

    /// Check if a track is playing
    fn is_track_playing(&self, track_id: Uuid) -> Result<bool, EngineError> {
        Ok(self.mixer.is_track_playing(track_id)?)
    }
}

/// High-level audio engine
#[derive(Clone)]
pub struct AudioEngine {
    inner: Arc<AudioEngineInner>,
}

impl AudioEngine {
    /// Create a new audio engine with default settings
    pub fn new() -> Result<Self, EngineError> {
        let config = MixerConfig::default();
        let mixer = Arc::new(AudioMixer::with_config(config));

        let inner = Arc::new(AudioEngineInner {
            mixer,
            sounds: RwLock::new(HashMap::new()),
            active_tracks: RwLock::new(HashMap::new()),
            stream: Mutex::new(None),
            running: Arc::new(AtomicBool::new(false)),
            master_volume: Arc::new(parking_lot::Mutex::new(1.0)),
            broadcast_tx: None,
            next_track_id: AtomicU64::new(0),
        });

        Ok(Self { inner })
    }

    /// Create a new audio engine with custom mixer config and broadcast channel
    pub fn with_config(config: MixerConfig) -> Result<Self, EngineError> {
        let mixer = Arc::new(AudioMixer::with_config(config));

        let inner = Arc::new(AudioEngineInner {
            mixer,
            sounds: RwLock::new(HashMap::new()),
            active_tracks: RwLock::new(HashMap::new()),
            stream: Mutex::new(None),
            running: Arc::new(AtomicBool::new(false)),
            master_volume: Arc::new(parking_lot::Mutex::new(1.0)),
            broadcast_tx: None,
            next_track_id: AtomicU64::new(0),
        });

        Ok(Self { inner })
    }

    /// Set the broadcast sender for network streaming
    pub fn set_broadcast_sender(&self, _tx: Sender<MixedOutput>) {
        // Note: This requires interior mutability, for now we document the pattern
        // In practice, you'd use a RwLock<Option<Sender>> or similar
    }

    /// Initialize and start the CPAL output stream
    pub fn start_output(&self) -> Result<(), EngineError> {
        if self.inner.running.load(Ordering::Acquire) {
            warn!("Audio engine already running");
            return Ok(());
        }

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(EngineError::NoDevice)?;

        info!("Using audio device: {}", device.name().unwrap_or_default());

        // Get supported config
        let supported_configs = device.supported_output_configs()
            .map_err(|e| EngineError::UnsupportedFormat(e.to_string()))?
            .collect::<Vec<_>>();

        // Find a config that matches our target (48kHz, stereo, f32)
        let config = supported_configs
            .iter()
            .find(|c| {
                c.channels() == TARGET_CHANNELS as u16
                    && c.min_sample_rate().0 <= TARGET_SAMPLE_RATE
                    && c.max_sample_rate().0 >= TARGET_SAMPLE_RATE
                    && c.sample_format() == SampleFormat::F32
            })
            .map(|c| c.with_sample_rate(SampleRate(TARGET_SAMPLE_RATE)))
            .ok_or_else(|| {
                EngineError::UnsupportedFormat(
                    "No supported config for 48kHz stereo f32".to_string(),
                )
            })?;

        let stream_config: StreamConfig = config.into();
        info!(
            "Audio config: {}Hz, {}ch, buffer: {:?}",
            stream_config.sample_rate.0,
            stream_config.channels,
            stream_config.buffer_size
        );

        // Ask for a 10 ms buffer. Not every backend accepts a fixed size, so
        // we retry with the device default below instead of failing outright.
        let stream_config = StreamConfig {
            buffer_size: BufferSize::Fixed(480), // 480 frames = 10ms at 48kHz
            ..stream_config
        };

        // Use Arc::clone since these are already wrapped in Arcs
        let mixer = Arc::clone(&self.inner.mixer);
        let running = Arc::clone(&self.inner.running);

        // The device callback is the master clock: it pulls exactly the
        // number of frames the hardware needs and immediately publishes the
        // same buffer to the network forwarder. No intermediate 10 ms channel
        // and no `thread::sleep` timing, both of which caused underruns.
        let build_stream = |config: &StreamConfig| -> Result<cpal::Stream, cpal::BuildStreamError> {
            let mixer = Arc::clone(&mixer);
            let running = Arc::clone(&running);
            device.build_output_stream(
                config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if !running.load(Ordering::Acquire) {
                        data.fill(0.0);
                        return;
                    }

                    // Master volume is applied inside `mix_into`, exactly once.
                    let playing = mixer.mix_into(data);
                    if playing {
                        mixer.publish_broadcast(data);
                    }
                },
                move |err| error!("Audio stream error: {}", err),
                None,
            )
        };

        let stream = match build_stream(&stream_config) {
            Ok(stream) => stream,
            Err(first_err) => {
                warn!(
                    "Failed to build 10 ms output stream ({}); retrying with the device default buffer size",
                    first_err
                );
                let fallback_config = StreamConfig {
                    buffer_size: BufferSize::Default,
                    ..stream_config.clone()
                };
                build_stream(&fallback_config)?
            }
        };

        // Mark the engine running before starting the stream so the first
        // callback already mixes instead of emitting one silent buffer.
        self.inner.running.store(true, Ordering::Release);
        if let Err(e) = stream.play() {
            self.inner.running.store(false, Ordering::Release);
            return Err(e.into());
        }

        *self.inner.stream.lock().unwrap() = Some(stream);

        info!("Audio engine started successfully (device-driven mixer)");
        Ok(())
    }

    /// Start the audio engine without a local output device.
    ///
    /// Used when CPAL cannot open an output stream (headless server, no sound
    /// card, exclusive-mode conflict). The mixer then runs its own deadline-
    /// paced loop and still publishes frames to the network broadcaster.
    pub fn start_headless_output(&self) -> Result<(), EngineError> {
        if self.inner.running.load(Ordering::Acquire) {
            warn!("Audio engine already running");
            return Ok(());
        }

        self.inner.running.store(true, Ordering::Release);
        self.inner.mixer.start();

        info!("Audio engine started in headless mode (network streaming only)");
        Ok(())
    }

    /// Stop the audio engine and output stream
    pub fn stop_output(&self) -> Result<(), EngineError> {
        if !self.inner.running.load(Ordering::Acquire) {
            return Ok(());
        }

        // Stop the clock first so the callback emits silence, then join the
        // headless loop if it was the one driving the mixer.
        self.inner.running.store(false, Ordering::Release);
        self.inner.mixer.stop();

        if let Some(stream) = self.inner.stream.lock().unwrap().take() {
            // Drop the stream to stop it (pause() returns PauseStreamError not in EngineError)
            drop(stream);
        }

        info!("Audio engine stopped");
        Ok(())
    }

    /// Check if the engine is running
    pub fn is_running(&self) -> bool {
        self.inner.running.load(Ordering::Acquire)
    }

    /// Load a sound from a file path
    pub fn load_sound<P: AsRef<Path>>(&self, path: P) -> Result<SoundId, EngineError> {
        let decoded = AudioDecoder::decode_file(path.as_ref())?;
        let sound_id = SoundId::new();
        let audio = Arc::new(decoded);

        info!(
            "Loaded sound {}: {} frames, {:.2}s, {}Hz, {}ch",
            sound_id.0,
            audio.frames,
            audio.duration,
            audio.sample_rate,
            audio.channels
        );

        self.inner.sounds.write().insert(sound_id, audio);
        Ok(sound_id)
    }

    /// Load a sound from raw decoded audio data
    pub fn load_sound_from_data(&self, audio: DecodedAudio) -> SoundId {
        let sound_id = SoundId::new();
        self.inner.sounds.write().insert(sound_id, Arc::new(audio));
        sound_id
    }

    /// Play a loaded sound
    /// Returns a TrackHandle for controlling playback
    pub fn play(&self, sound_id: SoundId) -> Result<TrackHandle, EngineError> {
        let audio = {
            let sounds = self.inner.sounds.read();
            sounds
                .get(&sound_id)
                .cloned()
                .ok_or(EngineError::SoundNotFound(sound_id))?
        };

        let looped = false; // Default to non-looping
        let track_id = self.inner.mixer.add_track(audio, looped);

        // Start playback — the central mix loop will read audio directly from
        // the track's decoded samples, no feeder thread needed.
        self.inner.mixer.play_track(track_id)?;

        // Register active track
        self.inner.active_tracks.write().insert(track_id, sound_id);

        let handle = TrackHandle {
            sound_id,
            track_id,
            engine: self.inner.clone(),
        };

        debug!("Started playback of sound {} on track {}", sound_id.0, track_id);
        Ok(handle)
    }

    /// Play a sound with looping enabled
    pub fn play_loop(&self, sound_id: SoundId) -> Result<TrackHandle, EngineError> {
        let audio = {
            let sounds = self.inner.sounds.read();
            sounds
                .get(&sound_id)
                .cloned()
                .ok_or(EngineError::SoundNotFound(sound_id))?
        };

        let track_id = self.inner.mixer.add_track(audio, true);
        self.inner.mixer.play_track(track_id)?;
        self.inner.active_tracks.write().insert(track_id, sound_id);

        let handle = TrackHandle {
            sound_id,
            track_id,
            engine: self.inner.clone(),
        };

        debug!("Started looped playback of sound {} on track {}", sound_id.0, track_id);
        Ok(handle)
    }

    /// Stop a specific track by its handle
    pub fn stop_track(&self, track_id: Uuid) -> Result<(), EngineError> {
        self.inner.mixer.stop_track(track_id)?;
        self.inner.active_tracks.write().remove(&track_id);
        Ok(())
    }

    /// Stop all tracks playing a specific sound
    pub fn stop_sound(&self, sound_id: SoundId) -> Result<(), EngineError> {
        let track_ids: Vec<Uuid> = {
            let tracks = self.inner.active_tracks.read();
            tracks
                .iter()
                .filter_map(|(tid, sid)| if *sid == sound_id { Some(*tid) } else { None })
                .collect()
        };

        for track_id in track_ids {
            self.stop_track(track_id)?;
        }

        Ok(())
    }

    /// Stop all playback
    pub fn stop_all(&self) -> Result<(), EngineError> {
        let track_ids: Vec<Uuid> = {
            let tracks = self.inner.active_tracks.read();
            tracks.keys().copied().collect()
        };

        for track_id in track_ids {
            self.stop_track(track_id)?;
        }

        Ok(())
    }

    /// Set master volume (0.0 - 1.0)
    pub fn set_master_volume(&self, volume: f32) -> Result<(), EngineError> {
        if !(0.0..=1.0).contains(&volume) {
            return Err(EngineError::Mixer(crate::audio::mixer::MixerError::InvalidVolume(volume)));
        }
        *self.inner.master_volume.lock() = volume;
        self.inner.mixer.set_master_volume(volume)?;
        Ok(())
    }

    /// Get current master volume
    pub fn master_volume(&self) -> f32 {
        *self.inner.master_volume.lock()
    }

    /// Set volume for a specific track
    pub fn set_track_volume(&self, track_id: Uuid, volume: f32) -> Result<(), EngineError> {
        self.inner.mixer.set_track_volume(track_id, volume)?;
        Ok(())
    }

    /// Get volume for a specific track
    pub fn track_volume(&self, track_id: Uuid) -> Result<f32, EngineError> {
        Ok(self.inner.mixer.track_volume(track_id)?)
    }

    /// Pause a specific track
    pub fn pause_track(&self, track_id: Uuid) -> Result<(), EngineError> {
        self.inner.mixer.pause_track(track_id)?;
        Ok(())
    }

    /// Resume a paused track
    pub fn resume_track(&self, track_id: Uuid) -> Result<(), EngineError> {
        self.inner.mixer.play_track(track_id)?;
        Ok(())
    }

    /// Check if a track is playing
    pub fn is_track_playing(&self, track_id: Uuid) -> Result<bool, EngineError> {
        Ok(self.inner.mixer.is_track_playing(track_id)?)
    }

    /// Check if any track for a given sound is currently playing
    pub fn is_sound_playing(&self, sound_id: SoundId) -> Result<bool, EngineError> {
        let track_ids: Vec<Uuid> = {
            let tracks = self.inner.active_tracks.read();
            tracks
                .iter()
                .filter_map(|(tid, sid)| if *sid == sound_id { Some(*tid) } else { None })
                .collect()
        };
        for track_id in track_ids {
            if self.inner.mixer.is_track_playing(track_id)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Get a loaded sound's metadata
    pub fn sound_info(&self, sound_id: SoundId) -> Result<(f32, u32, u32), EngineError> {
        let sounds = self.inner.sounds.read();
        let audio = sounds
            .get(&sound_id)
            .ok_or(EngineError::SoundNotFound(sound_id))?;
        Ok((audio.duration, audio.sample_rate, audio.channels))
    }

    /// List all loaded sound IDs
    pub fn list_sounds(&self) -> Vec<SoundId> {
        self.inner.sounds.read().keys().copied().collect()
    }

    /// Remove a sound from memory (stops any playing tracks first)
    pub fn unload_sound(&self, sound_id: SoundId) -> Result<(), EngineError> {
        self.stop_sound(sound_id)?;
        self.inner.sounds.write().remove(&sound_id);
        Ok(())
    }

    /// Get the mixer's broadcast output receiver for network streaming (WebSocket clients)
    pub fn broadcast_output_receiver(&self) -> Receiver<MixedOutput> {
        self.inner.mixer.broadcast_output_receiver()
    }

    /// Get mixer statistics
    pub fn stats(&self) -> EngineStats {
        EngineStats {
            loaded_sounds: self.inner.sounds.read().len(),
            active_tracks: self.inner.active_tracks.read().len(),
            mixer_tracks: self.inner.mixer.track_count(),
            running: self.inner.running.load(Ordering::Acquire),
            master_volume: self.master_volume(),
            sample_rate: TARGET_SAMPLE_RATE,
            channels: TARGET_CHANNELS,
            buffer_frames: self.inner.mixer.buffer_frames(),
        }
    }
}

/// Engine statistics
#[derive(Debug, Clone, serde::Serialize)]
pub struct EngineStats {
    pub loaded_sounds: usize,
    pub active_tracks: usize,
    pub mixer_tracks: usize,
    pub running: bool,
    pub master_volume: f32,
    pub sample_rate: u32,
    pub channels: u32,
    pub buffer_frames: usize,
}

impl TrackHandle {
    /// Stop this track
    pub fn stop(&self) -> Result<(), EngineError> {
        self.engine.stop_track(self.track_id)
    }

    /// Pause this track
    pub fn pause(&self) -> Result<(), EngineError> {
        self.engine.pause_track(self.track_id)
    }

    /// Resume this track
    pub fn resume(&self) -> Result<(), EngineError> {
        self.engine.resume_track(self.track_id)
    }

    /// Set volume for this track (0.0 - 1.0)
    pub fn set_volume(&self, volume: f32) -> Result<(), EngineError> {
        self.engine.set_track_volume(self.track_id, volume)
    }

    /// Get current volume for this track
    pub fn volume(&self) -> Result<f32, EngineError> {
        self.engine.track_volume(self.track_id)
    }

    /// Check if this track is playing
    pub fn is_playing(&self) -> Result<bool, EngineError> {
        self.engine.is_track_playing(self.track_id)
    }

    /// Get the sound ID for this track
    pub fn sound_id(&self) -> SoundId {
        self.sound_id
    }

    /// Get the track ID
    pub fn track_id(&self) -> Uuid {
        self.track_id
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        if self.inner.running.load(Ordering::Acquire) {
            let _ = self.stop_output();
        }
    }
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new().expect("Failed to create default audio engine")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_creation() {
        let engine = AudioEngine::new().unwrap();
        assert!(!engine.is_running());
        assert_eq!(engine.master_volume(), 1.0);
        assert_eq!(engine.list_sounds().len(), 0);
    }

    #[test]
    fn test_master_volume() {
        let engine = AudioEngine::new().unwrap();
        engine.set_master_volume(0.5).unwrap();
        assert_eq!(engine.master_volume(), 0.5);

        assert!(engine.set_master_volume(1.5).is_err());
    }

    #[test]
    fn test_sound_id_generation() {
        let id1 = SoundId::new();
        let id2 = SoundId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_load_sound_from_data() {
        let engine = AudioEngine::new().unwrap();
        let audio = DecodedAudio::new(vec![0.0; 48000 * 2], 48000, 2);
        let sound_id = engine.load_sound_from_data(audio);
        assert_eq!(engine.list_sounds().len(), 1);
        assert!(engine.list_sounds().contains(&sound_id));
    }
}