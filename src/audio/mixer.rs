//! Audio mixer for combining multiple PCM streams
//! Thread-safe, non-blocking, lock-free where possible
//!
//! The mixer exposes a pull-based [`AudioMixer::mix_into`] API. The audio
//! device callback (CPAL) is the master clock: every time the device asks for
//! N frames, the mixer advances every active track by exactly N frames and
//! fills the callback buffer. This removes the old `thread::sleep(10ms)` loop
//! whose Windows timer granularity (~15.6 ms) caused constant underruns.
//!
//! When no output device is available, [`AudioMixer::start`] runs an
//! equivalent deadline-paced loop so network streaming still works.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::RwLock;
use uuid::Uuid;

use crate::audio::decoder::{DecodedAudio, TARGET_SAMPLE_RATE};

/// Number of interleaved channels produced by the mixer.
const CHANNELS: usize = 2;

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
    /// Number of buffers to keep in the broadcast output channel
    pub channel_capacity: usize,
    /// Master volume (0.0 - 1.0)
    pub master_volume: f32,
    /// Interval between mix iterations (ms) for the headless fallback loop.
    pub mix_interval_ms: u64,
}

impl Default for MixerConfig {
    fn default() -> Self {
        Self {
            // ~10 ms at 48 kHz = 480 frames
            buffer_frames: 480,
            // ~640 ms of buffering for the network forwarder.
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

/// High-performance audio mixer.
///
/// Tracks are mixed on demand by the audio callback (or by the headless loop
/// when no device is available). There is no separate feeder thread per track
/// and no fixed-size intermediate channel for local playback, so the mixer can
/// never fall behind the device clock.
#[derive(Debug)]
pub struct AudioMixer {
    config: MixerConfig,
    master_volume: Arc<parking_lot::Mutex<f32>>,
    tracks: Arc<RwLock<HashMap<Uuid, TrackState>>>,
    /// Broadcast channel for network streaming (WebSocket clients)
    broadcast_tx: Sender<MixedOutput>,
    broadcast_rx: Receiver<MixedOutput>,
    /// Headless mixer running state
    running: Arc<AtomicBool>,
    /// Timestamp counter
    timestamp: Arc<AtomicU64>,
    /// Monotonically increasing sequence number for each mix buffer
    seq_no: Arc<AtomicU64>,
    /// Next track ID generator
    next_track_id: Arc<AtomicU64>,
    /// Handle to the headless mix loop thread (None when the device drives us)
    mix_handle: parking_lot::Mutex<Option<thread::JoinHandle<()>>>,
}

impl AudioMixer {
    /// Create a new audio mixer with default configuration
    pub fn new() -> Self {
        Self::with_config(MixerConfig::default())
    }

    /// Create a new audio mixer with custom configuration
    pub fn with_config(config: MixerConfig) -> Self {
        let (broadcast_tx, broadcast_rx) = bounded(config.channel_capacity);
        let master_volume = config.master_volume;

        Self {
            config,
            master_volume: Arc::new(parking_lot::Mutex::new(master_volume)),
            tracks: Arc::new(RwLock::new(HashMap::new())),
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
    /// [`play_track`](Self::play_track) to begin playback.
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

    /// Start the headless mix loop.
    ///
    /// This is only needed when there is no CPAL output device. When a device
    /// is present, the device callback drives [`mix_into`](Self::mix_into) and
    /// this method must not be called.
    pub fn start(&self) {
        if self.running.swap(true, Ordering::AcqRel) {
            return;
        }

        let config = self.config.clone();
        let tracks = Arc::clone(&self.tracks);
        let broadcast_tx = self.broadcast_tx.clone();
        let running = Arc::clone(&self.running);
        let timestamp = Arc::clone(&self.timestamp);
        let seq_no = Arc::clone(&self.seq_no);
        let master_volume = Arc::clone(&self.master_volume);

        let handle = thread::spawn(move || {
            Self::headless_loop(
                config,
                tracks,
                broadcast_tx,
                running,
                timestamp,
                seq_no,
                master_volume,
            );
        });

        *self.mix_handle.lock() = Some(handle);
    }

    /// Stop the headless mixer and wait for the loop to finish.
    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
        if let Some(handle) = self.mix_handle.lock().take() {
            let _ = handle.join();
        }
    }

    /// Check if the headless mixer loop is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    /// Mix all active tracks into `out`.
    ///
    /// `out` is interleaved stereo f32 and may be any length the device asks
    /// for (CPAL callbacks are not guaranteed to be 480 frames). The mixer
    /// advances every playing track by exactly `out.len() / 2` frames and
    /// applies the master volume exactly once.
    ///
    /// Returns `true` when at least one track was playing during this call.
    pub fn mix_into(&self, out: &mut [f32]) -> bool {
        Self::mix_into_parts(&self.tracks, &self.master_volume, out)
    }

    /// Publish an already-mixed buffer to the network broadcast channel.
    ///
    /// The send is non-blocking: if the forwarder is behind, the frame is
    /// dropped rather than stalling the audio callback.
    pub fn publish_broadcast(&self, buffer: &[f32]) {
        Self::publish_broadcast_parts(
            &self.broadcast_tx,
            &self.timestamp,
            &self.seq_no,
            buffer,
        );
    }

    fn mix_into_parts(
        tracks: &RwLock<HashMap<Uuid, TrackState>>,
        master_volume: &parking_lot::Mutex<f32>,
        out: &mut [f32],
    ) -> bool {
        let frames = out.len() / CHANNELS;
        out.fill(0.0);
        if frames == 0 {
            return false;
        }

        let master = *master_volume.lock();
        let mut any_playing = false;
        // (track_id, new_position, still_playing)
        let mut updates: Vec<(Uuid, usize, bool)> = Vec::new();

        {
            let tracks_read = tracks.read();
            for (track_id, track) in tracks_read.iter() {
                if !track.playing || track.audio.frames == 0 {
                    continue;
                }

                any_playing = true;
                let mut src_pos = track.position.min(track.audio.frames);
                let mut dst_frame = 0usize;
                let mut finished = false;

                while dst_frame < frames {
                    if src_pos >= track.audio.frames {
                        if track.looped {
                            src_pos = 0;
                        } else {
                            finished = true;
                            break;
                        }
                    }

                    let frames_to_mix =
                        (track.audio.frames - src_pos).min(frames - dst_frame);
                    let src_start = src_pos * CHANNELS;
                    let dst_start = dst_frame * CHANNELS;
                    let sample_count = frames_to_mix * CHANNELS;

                    for i in 0..sample_count {
                        out[dst_start + i] +=
                            track.audio.samples[src_start + i] * track.volume;
                    }

                    dst_frame += frames_to_mix;
                    src_pos += frames_to_mix;
                }

                updates.push((*track_id, src_pos, !finished));
            }
        }

        // Apply master volume once, after all tracks have been summed. The
        // CPAL callback used to multiply by master volume a second time.
        if (master - 1.0).abs() > f32::EPSILON {
            for sample in out.iter_mut() {
                *sample *= master;
            }
        }

        if !updates.is_empty() {
            let mut tracks_write = tracks.write();
            for (track_id, new_position, still_playing) in updates {
                if let Some(track) = tracks_write.get_mut(&track_id) {
                    if still_playing {
                        track.position = new_position;
                    } else {
                        track.position = track.audio.frames;
                        track.playing = false;
                    }
                }
            }
        }

        any_playing
    }

    fn publish_broadcast_parts(
        broadcast_tx: &Sender<MixedOutput>,
        timestamp: &AtomicU64,
        seq_no: &AtomicU64,
        buffer: &[f32],
    ) {
        if buffer.is_empty() {
            return;
        }

        let ts = timestamp.fetch_add(1, Ordering::Relaxed);
        let seq = seq_no.fetch_add(1, Ordering::Relaxed);
        let output = MixedOutput {
            buffer: buffer.to_vec(),
            seq_no: seq,
            timestamp: ts,
        };

        // Drop frames when the forwarder is behind. Never block the audio
        // callback on a network consumer.
        if let Err(crossbeam_channel::TrySendError::Disconnected(_)) =
            broadcast_tx.try_send(output)
        {
            // Receiver dropped; the mixer can keep running harmlessly.
        }
    }

    /// Headless mix loop — only used when no CPAL device is available.
    ///
    /// Uses a fixed deadline instead of `sleep(interval)`, so scheduler
    /// oversleep does not accumulate into a permanent underrun.
    fn headless_loop(
        config: MixerConfig,
        tracks: Arc<RwLock<HashMap<Uuid, TrackState>>>,
        broadcast_tx: Sender<MixedOutput>,
        running: Arc<AtomicBool>,
        timestamp: Arc<AtomicU64>,
        seq_no: Arc<AtomicU64>,
        master_volume: Arc<parking_lot::Mutex<f32>>,
    ) {
        let frames = config.buffer_frames.max(1);
        let mut buffer = vec![0.0f32; frames * CHANNELS];

        let sample_rate = TARGET_SAMPLE_RATE.max(1) as u64;
        let period = Duration::from_nanos((frames as u64 * 1_000_000_000) / sample_rate);
        let mut next_deadline = Instant::now() + period;

        while running.load(Ordering::Acquire) {
            let playing = Self::mix_into_parts(&tracks, &master_volume, &mut buffer);
            if playing {
                Self::publish_broadcast_parts(&broadcast_tx, &timestamp, &seq_no, &buffer);
            }

            let now = Instant::now();
            if next_deadline > now {
                thread::sleep(next_deadline - now);
            } else if now.duration_since(next_deadline) > period * 4 {
                // The process was descheduled or suspended for a long time.
                // Resync instead of trying to catch up with a burst.
                next_deadline = now;
            }
            next_deadline += period;
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
        TARGET_SAMPLE_RATE
    }

    /// Get channels
    pub fn channels(&self) -> u32 {
        CHANNELS as u32
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

    #[test]
    fn test_mix_into_applies_master_volume_once() {
        let mixer = AudioMixer::new();
        mixer.set_master_volume(0.5).unwrap();

        let audio = Arc::new(DecodedAudio::new(vec![1.0; 480 * 2], 48000, 2));
        let id = mixer.add_track(audio, false);
        mixer.play_track(id).unwrap();

        let mut out = vec![0.0f32; 480 * 2];
        assert!(mixer.mix_into(&mut out));
        assert!((out[0] - 0.5).abs() < 1e-6);
        assert!((out[out.len() - 1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_mix_into_respects_arbitrary_buffer_size() {
        let mixer = AudioMixer::new();
        let audio = Arc::new(DecodedAudio::new(vec![0.25; 48000 * 2], 48000, 2));
        let id = mixer.add_track(audio, false);
        mixer.play_track(id).unwrap();

        // A device callback may ask for more or fewer frames than the
        // nominal 10 ms mix size.
        for frames in [64usize, 480, 1024, 37] {
            let mut out = vec![0.0f32; frames * 2];
            assert!(mixer.mix_into(&mut out));
            assert!(out.iter().all(|s| (*s - 0.25).abs() < 1e-6));
        }
    }

    #[test]
    fn test_mix_into_loops_track() {
        let mixer = AudioMixer::new();
        let audio = Arc::new(DecodedAudio::new(vec![0.25; 100 * 2], 48000, 2));
        let id = mixer.add_track(audio, true);
        mixer.play_track(id).unwrap();

        let mut out = vec![0.0f32; 480 * 2];
        assert!(mixer.mix_into(&mut out));
        assert!(out.iter().all(|s| (*s - 0.25).abs() < 1e-6));
        assert!(mixer.is_track_playing(id).unwrap());
    }

    #[test]
    fn test_mix_into_marks_finished_track_stopped() {
        let mixer = AudioMixer::new();
        let audio = Arc::new(DecodedAudio::new(vec![0.5; 100 * 2], 48000, 2));
        let id = mixer.add_track(audio, false);
        mixer.play_track(id).unwrap();

        let mut out = vec![0.0f32; 480 * 2];
        assert!(mixer.mix_into(&mut out));
        assert!(out[..200].iter().all(|s| (*s - 0.5).abs() < 1e-6));
        assert!(out[200..].iter().all(|s| *s == 0.0));
        assert!(!mixer.is_track_playing(id).unwrap());
    }
}
