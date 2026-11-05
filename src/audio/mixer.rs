//! Audio mixer for combining multiple PCM streams
//! Thread-safe, non-blocking, lock-free where possible
//!
//! The mixer runs a single central mix loop that combines all active playing
//! tracks into one buffer per interval (480 frames = 10 ms at 48 kHz) and
//! sends the mixed result to both the local CPAL output and the network
//! broadcast channel. This ensures all tracks are properly sample-aligned
//! before being streamed to WebSocket clients.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::RwLock;
use uuid::Uuid;

use crate::audio::decoder::DecodedAudio;

/// Errors that can occur in the mixer
#[derive(Debug, thiserror::Error)]
pub enum MixerError {
    #[error("Track not found: {0}")]
    TrackNotFound(Uuid),
    #[error("Channel send error: {0}")]
    ChannelSend(#[from] crossbeam_channel::SendError<AudioBuffer>),
    #[error("Channel receive error: {0}")]
    ChannelRecv(#[from] crossbeam_channel::RecvError),
    #[error("Track already exists: {0}")]
    TrackExists(Uuid),
    #[error("Invalid volume value: {0} (must be 0.0-1.0)")]
    InvalidVolume(f32),
}

/// Audio buffer for mixing (interleaved stereo f32 at 48kHz)
pub type AudioBuffer = Vec<f32>;

/// Output from the mixer
#[derive(Debug, Clone)]
pub struct MixedOutput {
    /// Mixed audio samples (interleaved stereo f32)
    pub buffer: AudioBuffer,
    /// Monotonically increasing sequence number for this mix buffer
    pub seq_no: u64,
    /// Timestamp of when this buffer was mixed
    pub timestamp: u64,
}

/// Configuration for the mixer
#[derive(Debug, Clone)]
pub struct MixerConfig {
    /// Buffer size in frames (stereo pairs). 480 = 10 ms at 48 kHz.
    pub buffer_frames: usize,
    /// Number of buffers to keep in each output channel
    pub channel_capacity: usize,
    /// Master volume (0.0 - 1.0)
    pub master_volume: f32,
    /// Interval between mix iterations (ms). Default 10 ms.
    pub mix_interval_ms: u64,
}

impl Default for MixerConfig {
    fn default() -> Self {
        Self {
            // ~10 ms at 48 kHz = 480 frames
            buffer_frames: 480,
            // Increased from 8 to 64 to prevent drops under load (~640 ms buffer)
            channel_capacity: 64,
            master_volume: 1.0,
            mix_interval_ms: 10,
        }
    }
}

/// Internal track state
#[derive(Debug)]
struct TrackState {
    _id: Uuid,
    /// Decoded audio data
    audio: Arc<DecodedAudio>,
    /// Current playback position in frames
    position: usize,
    /// Track volume (0.0 - 1.0)
    volume: f32,
    /// Whether track is playing
    playing: bool,
    /// Whether track should loop
    looped: bool,
}

/// High-performance audio mixer
///
/// A single central mix loop (started via `start()`) reads from all active
/// tracks, mixes them into one buffer, and pushes to both output channels.
/// This replaces the old per-track feeder threads which sent un-mixed audio
/// independently, causing crackling and buffer overflows.
#[derive(Debug)]
pub struct AudioMixer {
    config: MixerConfig,
    master_volume: Arc<parking_lot::Mutex<f32>>,
    tracks: Arc<RwLock<HashMap<Uuid, TrackState>>>,
    /// Output channel for mixed audio (CPAL local playback)
    output_tx: Sender<MixedOutput>,
    output_rx: Receiver<MixedOutput>,
    /// Broadcast channel for network streaming (WebSocket clients)
    broadcast_tx: Sender<MixedOutput>,
    broadcast_rx: Receiver<MixedOutput>,
    /// Mixer running state
    running: Arc<AtomicBool>,
    /// Timestamp counter
    timestamp: Arc<AtomicU64>,
    /// Monotonically increasing sequence number for each mix buffer
    seq_no: Arc<AtomicU64>,
    /// Next track ID generator
    next_track_id: Arc<AtomicU64>,
    /// Handle to the central mix loop thread
    mix_handle: parking_lot::Mutex<Option<thread::JoinHandle<()>>>,
}

impl AudioMixer {
    /// Create a new audio mixer with default configuration
    pub fn new() -> Self {
        let config = MixerConfig::default();
        let (output_tx, output_rx) = bounded(config.channel_capacity);
        let (broadcast_tx, broadcast_rx) = bounded(config.channel_capacity);

        Self {
            config,
            master_volume: Arc::new(parking_lot::Mutex::new(1.0)),
            tracks: Arc::new(RwLock::new(HashMap::new())),
            output_tx,
            output_rx,
            broadcast_tx,
            broadcast_rx,
            running: Arc::new(AtomicBool::new(false)),
            timestamp: Arc::new(AtomicU64::new(0)),
            seq_no: Arc::new(AtomicU64::new(0)),
            next_track_id: Arc::new(AtomicU64::new(0)),
            mix_handle: parking_lot::Mutex::new(None),
        }
    }

    /// Create a new audio mixer with custom configuration
    pub fn with_config(config: MixerConfig) -> Self {
        let (output_tx, output_rx) = bounded(config.channel_capacity);
        let (broadcast_tx, broadcast_rx) = bounded(config.channel_capacity);

        Self {
            config: config.clone(),
            master_volume: Arc::new(parking_lot::Mutex::new(config.master_volume)),
            tracks: Arc::new(RwLock::new(HashMap::new())),
            output_tx,
            output_rx,
            broadcast_tx,
            broadcast_rx,
            running: Arc::new(AtomicBool::new(false)),
            timestamp: Arc::new(AtomicU64::new(0)),
            seq_no: Arc::new(AtomicU64::new(0)),
            next_track_id: Arc::new(AtomicU64::new(0)),
            mix_handle: parking_lot::Mutex::new(None),
        }
    }

    /// Add a new track to the mixer
    ///
    /// Returns the track ID. The track is created in a stopped state; call
    /// [`play_track`] to begin playback. The mix loop reads audio directly
    /// from the track's decoded samples, so no input sender is needed.
    pub fn add_track(&self, audio: Arc<DecodedAudio>, looped: bool) -> Uuid {
        let id = Uuid::from_u64_pair(0, self.next_track_id.fetch_add(1, Ordering::Relaxed));

        let track = TrackState {
            _id: id,
            audio,
            position: 0,
            volume: 1.0,
            playing: false,
            looped,
        };

        self.tracks.write().insert(id, track);
        id
    }

    /// Start the central mix loop.
    ///
    /// Spawns a thread that runs at `mix_interval_ms` intervals, mixes all
    /// active playing tracks into one buffer, and sends to both output
    /// channels. Must be called before tracks will produce any output.
    pub fn start(&self) {
        self.running.store(true, Ordering::Release);

        let config = self.config.clone();
        let tracks = self.tracks.clone();
        let output_tx = self.output_tx.clone();
        let broadcast_tx = self.broadcast_tx.clone();
        let running = self.running.clone();
        let timestamp = self.timestamp.clone();
        let seq_no = self.seq_no.clone();
        let master_volume = self.master_volume.clone();

        let handle = thread::spawn(move || {
            Self::mix_loop(
                config,
                tracks,
                output_tx,
                broadcast_tx,
                running,
                timestamp,
                seq_no,
                master_volume,
            );
        });

        *self.mix_handle.lock() = Some(handle);
    }

    /// Stop the mixer and wait for the mix loop to finish.
    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
        if let Some(handle) = self.mix_handle.lock().take() {
            let _ = handle.join();
        }
    }

    /// Check if mixer is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    /// Central mix loop — runs on its own thread.
    ///
    /// Every `mix_interval_ms` it:
    /// 1. Allocates a zeroed mix buffer
    /// 2. For each active playing track, reads the next chunk of PCM from
    ///    the track's decoded samples and adds it into the mix buffer
    /// 3. Applies master volume
    /// 4. Sends the single mixed buffer to both output channels
    ///
    /// This ensures all tracks are sample-aligned and mixed together before
    /// being sent to local output or WebSocket clients.
    fn mix_loop(
        config: MixerConfig,
        tracks: Arc<RwLock<HashMap<Uuid, TrackState>>>,
        output_tx: Sender<MixedOutput>,
        broadcast_tx: Sender<MixedOutput>,
        running: Arc<AtomicBool>,
        timestamp: Arc<AtomicU64>,
        seq_no: Arc<AtomicU64>,
        master_volume: Arc<parking_lot::Mutex<f32>>,
    ) {
        let buffer_frames = config.buffer_frames;
        let channels = 2; // stereo
        let buffer_size = buffer_frames * channels;
        let interval = Duration::from_millis(config.mix_interval_ms);

        let mut mix_buffer = vec![0.0f32; buffer_size];

        while running.load(Ordering::Acquire) {
            // Start from silence
            mix_buffer.fill(0.0);
            let master_vol = *master_volume.lock();

            // Collect tracks that need position updates while holding read lock,
            // then apply updates under brief write locks to avoid long lock holds.
            let mut tracks_to_advance: Vec<(Uuid, usize)> = Vec::new();

            {
                let tracks_read = tracks.read();
                for (track_id, track) in tracks_read.iter() {
                    if !track.playing {
                        continue;
                    }

                    let position = track.position;
                    let frames_to_mix = buffer_frames.min(track.audio.frames.saturating_sub(position));
                    if frames_to_mix == 0 {
                        // Track finished
                        if track.looped {
                            tracks_to_advance.push((*track_id, 0));
                        }
                        // Non-looping tracks: position will be updated to end
                        // and playing set to false in the apply section below.
                        // Track stays in mixer so stop/remove can still find it.
                        continue;
                    }

                    let samples_to_mix = frames_to_mix * channels;
                    let src_start = position * channels;

                    // Mix this track's samples into the shared buffer
                    for i in 0..samples_to_mix {
                        if i < buffer_size {
                            mix_buffer[i] += track.audio.samples[src_start + i] * track.volume;
                        }
                    }

                    tracks_to_advance.push((*track_id, position + frames_to_mix));
                }
            }

            // Apply position updates and track stops
            {
                let mut tracks_write = tracks.write();
                for (track_id, new_pos) in tracks_to_advance {
                    if let Some(track) = tracks_write.get_mut(&track_id) {
                        track.position = new_pos;
                        if track.position >= track.audio.frames && !track.looped {
                            // Track finished — mark as stopped but do NOT remove it from the mixer.
                            // Removing it would make subsequent stop/remove operations fail with
                            // TrackNotFound, leaving the GUI stuck in a "playing" state.
                            track.playing = false;
                            // Do not push to tracks_to_stop — keep the track in the map so
                            // stop_track/remove_sound can still find and operate on it.
                        }
                    }
                }
                // No track removal here — tracks stay in the mixer until explicitly removed
                // via remove_track/unload_sound.
            }

            // Apply master volume
            for sample in &mut mix_buffer {
                *sample *= master_vol;
            }

            let ts = timestamp.fetch_add(1, Ordering::Relaxed);
            let seq = seq_no.fetch_add(1, Ordering::Relaxed);
            let output = MixedOutput {
                buffer: mix_buffer.clone(),
                seq_no: seq,
                timestamp: ts,
            };

            // Send to CPAL local playback channel (non-blocking — drop if full)
            match output_tx.try_send(output.clone()) {
                Ok(()) => {}
                Err(crossbeam_channel::TrySendError::Disconnected(_)) => break,
                Err(crossbeam_channel::TrySendError::Full(_)) => {
                    // Channel full — drop frame, CPAL will output silence
                }
            }

            // Send to broadcast channel for WebSocket clients only when at least
            // one track is playing — otherwise we'd stream silence and fill
            // client buffers with useless data (causing buffer overflow before
            // any music has even been started).
            let any_playing = {
                let tracks_read = tracks.read();
                tracks_read.values().any(|t| t.playing)
            };
            if any_playing {
                let _ = broadcast_tx.try_send(output);
            }

            // Sleep until next mix interval
            thread::sleep(interval);
        }
    }

    /// Set master volume (0.0 - 1.0)
    pub fn set_master_volume(&self, volume: f32) -> Result<(), MixerError> {
        if !(0.0..=1.0).contains(&volume) {
            return Err(MixerError::InvalidVolume(volume));
        }
        *self.master_volume.lock() = volume;
        Ok(())
    }

    /// Get current master volume
    pub fn master_volume(&self) -> f32 {
        *self.master_volume.lock()
    }

    /// Set volume for a specific track
    pub fn set_track_volume(&self, track_id: Uuid, volume: f32) -> Result<(), MixerError> {
        if !(0.0..=1.0).contains(&volume) {
            return Err(MixerError::InvalidVolume(volume));
        }
        let mut tracks = self.tracks.write();
        if let Some(track) = tracks.get_mut(&track_id) {
            track.volume = volume;
            Ok(())
        } else {
            Err(MixerError::TrackNotFound(track_id))
        }
    }

    /// Get volume for a specific track
    pub fn track_volume(&self, track_id: Uuid) -> Result<f32, MixerError> {
        let tracks = self.tracks.read();
        tracks
            .get(&track_id)
            .map(|t| t.volume)
            .ok_or(MixerError::TrackNotFound(track_id))
    }

    /// Start playback for a track (resets position to 0)
    pub fn play_track(&self, track_id: Uuid) -> Result<(), MixerError> {
        let mut tracks = self.tracks.write();
        if let Some(track) = tracks.get_mut(&track_id) {
            track.playing = true;
            track.position = 0;
            Ok(())
        } else {
            Err(MixerError::TrackNotFound(track_id))
        }
    }

    /// Stop playback for a track (does not remove it)
    pub fn stop_track(&self, track_id: Uuid) -> Result<(), MixerError> {
        let mut tracks = self.tracks.write();
        if let Some(track) = tracks.get_mut(&track_id) {
            track.playing = false;
            Ok(())
        } else {
            Err(MixerError::TrackNotFound(track_id))
        }
    }

    /// Pause playback for a track
    pub fn pause_track(&self, track_id: Uuid) -> Result<(), MixerError> {
        let mut tracks = self.tracks.write();
        if let Some(track) = tracks.get_mut(&track_id) {
            track.playing = false;
            Ok(())
        } else {
            Err(MixerError::TrackNotFound(track_id))
        }
    }

    /// Check if a track is playing
    pub fn is_track_playing(&self, track_id: Uuid) -> Result<bool, MixerError> {
        let tracks = self.tracks.read();
        tracks
            .get(&track_id)
            .map(|t| t.playing)
            .ok_or(MixerError::TrackNotFound(track_id))
    }

    /// Remove a track from the mixer
    pub fn remove_track(&self, track_id: Uuid) -> Result<(), MixerError> {
        let mut tracks = self.tracks.write();
        if tracks.remove(&track_id).is_some() {
            Ok(())
        } else {
            Err(MixerError::TrackNotFound(track_id))
        }
    }

    /// Get the mixed output receiver for CPAL local playback
    pub fn output_receiver(&self) -> &Receiver<MixedOutput> {
        &self.output_rx
    }

    /// Get the broadcast output receiver for network streaming (WebSocket clients)
    pub fn broadcast_output_receiver(&self) -> Receiver<MixedOutput> {
        self.broadcast_rx.clone()
    }

    /// Get number of active tracks
    pub fn track_count(&self) -> usize {
        self.tracks.read().len()
    }

    /// Get buffer size in frames
    pub fn buffer_frames(&self) -> usize {
        self.config.buffer_frames
    }

    /// Get sample rate
    pub fn sample_rate(&self) -> u32 {
        crate::audio::decoder::TARGET_SAMPLE_RATE
    }

    /// Get channels
    pub fn channels(&self) -> u32 {
        crate::audio::decoder::TARGET_CHANNELS
    }
}

impl Default for AudioMixer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::decoder::DecodedAudio;

    #[test]
    fn test_mixer_creation() {
        let mixer = AudioMixer::new();
        assert!(!mixer.is_running());
        assert_eq!(mixer.master_volume(), 1.0);
        assert_eq!(mixer.track_count(), 0);
    }

    #[test]
    fn test_master_volume() {
        let mixer = AudioMixer::new();
        mixer.set_master_volume(0.5).unwrap();
        assert_eq!(mixer.master_volume(), 0.5);

        assert!(mixer.set_master_volume(1.5).is_err());
        assert!(mixer.set_master_volume(-0.1).is_err());
    }

    #[test]
    fn test_add_remove_track() {
        let mixer = AudioMixer::new();
        let audio = Arc::new(DecodedAudio::new(vec![0.0; 48000 * 2], 48000, 2));
        let id = mixer.add_track(audio, false);
        assert_eq!(mixer.track_count(), 1);

        mixer.remove_track(id).unwrap();
        assert_eq!(mixer.track_count(), 0);
        assert!(mixer.remove_track(id).is_err());
    }

    #[test]
    fn test_track_volume() {
        let mixer = AudioMixer::new();
        let audio = Arc::new(DecodedAudio::new(vec![0.0; 48000 * 2], 48000, 2));
        let id = mixer.add_track(audio, false);

        mixer.set_track_volume(id, 0.5).unwrap();
        assert_eq!(mixer.track_volume(id).unwrap(), 0.5);

        assert!(mixer.set_track_volume(id, 1.5).is_err());
    }

    #[test]
    fn test_play_stop_track() {
        let mixer = AudioMixer::new();
        let audio = Arc::new(DecodedAudio::new(vec![0.5; 4800], 48000, 2));
        let id = mixer.add_track(audio, false);

        assert!(!mixer.is_track_playing(id).unwrap());
        mixer.play_track(id).unwrap();
        assert!(mixer.is_track_playing(id).unwrap());
        mixer.stop_track(id).unwrap();
        assert!(!mixer.is_track_playing(id).unwrap());
    }
}