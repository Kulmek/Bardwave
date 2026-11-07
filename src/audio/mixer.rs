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
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
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

/// Length of the gain ramp used for volume changes, stop, pause and resume:
/// 10 ms at 48 kHz. A gain that jumps between two samples is an audible click
/// (and a dragged fader is a burst of them — "zipper" noise); spreading every
/// change over a few hundred samples makes it inaudible.
const RAMP_FRAMES: f32 = 480.0;

/// Ceiling of the master bus. The peak limiter keeps the summed mix below it,
/// so several loud tracks played together are turned down smoothly instead of
/// being hard-clipped into a buzzing square wave by the device or the codec.
const LIMIT_CEILING: f32 = 0.97;

/// Release time of the limiter: how fast the gain recovers after a peak.
const LIMIT_RELEASE_SECS: f32 = 0.2;

/// What happens to a track once its fade-out reaches silence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingHalt {
    /// Keep the position: `resume_track` continues where it left off.
    Pause,
    /// Rewind to the start (a stopped track restarts from 0 when replayed).
    Stop,
}

/// Internal track state
#[derive(Debug)]
struct TrackState {
    _id: Uuid,
    /// Decoded audio data
    audio: Arc<DecodedAudio>,
    /// Current playback position in frames
    position: usize,
    /// Track volume (0.0 - 1.0): the gain the track ramps towards.
    volume: f32,
    /// Gain actually applied right now; follows `volume` (or 0 while fading
    /// out) through a short linear ramp.
    gain: f32,
    /// Whether track is playing
    playing: bool,
    /// Set while the track fades out before pausing / stopping.
    halt: Option<PendingHalt>,
    /// Paused (not stopped or finished): kept by [`AudioMixer::prune_finished`].
    paused: bool,
    /// Whether track should loop
    looped: bool,
}

impl TrackState {
    /// The gain this track is heading for.
    fn target_gain(&self) -> f32 {
        if self.halt.is_some() {
            0.0
        } else {
            self.volume
        }
    }

    /// Whether the track counts as playing for the outside world: a track that
    /// is fading out after `stop`/`pause` already reports as stopped.
    fn is_audible_playing(&self) -> bool {
        self.playing && self.halt.is_none()
    }
}

/// Smooth peak limiter on the master bus.
///
/// Instant attack (the gain drops as soon as a sample would exceed the
/// ceiling) and an exponential release. Without it the sum of a few tracks
/// at full volume goes past full scale and is hard-clipped — on bass-heavy
/// ambience that is exactly the "farting" distortion.
#[derive(Debug, Clone, Copy)]
struct Limiter {
    gain: f32,
}

impl Limiter {
    const fn new() -> Self {
        Self { gain: 1.0 }
    }

    fn process(&mut self, out: &mut [f32]) {
        let release = 1.0 - (-1.0 / (LIMIT_RELEASE_SECS * TARGET_SAMPLE_RATE as f32)).exp();
        for frame in out.as_chunks_mut::<CHANNELS>().0 {
            let peak = frame.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            self.gain += (1.0 - self.gain) * release;
            if peak * self.gain > LIMIT_CEILING {
                self.gain = LIMIT_CEILING / peak;
            }
            for sample in frame.iter_mut() {
                // The clamp is only a safety net for NaN/inf from a broken file.
                *sample = (*sample * self.gain).clamp(-1.0, 1.0);
            }
        }
    }
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
    /// Master-bus limiter state, carried across mix calls.
    limiter: Arc<parking_lot::Mutex<Limiter>>,
    /// Peak output level since the GUI last read it (f32 bits; non-negative
    /// floats order like their bit patterns, so `fetch_max` works).
    peak: Arc<AtomicU32>,
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
            limiter: Arc::new(parking_lot::Mutex::new(Limiter::new())),
            peak: Arc::new(AtomicU32::new(0)),
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
            gain: 1.0,
            playing: false,
            halt: None,
            paused: false,
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
        let limiter = Arc::clone(&self.limiter);
        let peak = Arc::clone(&self.peak);

        let handle = thread::spawn(move || {
            Self::headless_loop(
                config,
                tracks,
                broadcast_tx,
                running,
                timestamp,
                seq_no,
                master_volume,
                limiter,
                peak,
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
        Self::mix_into_parts(
            &self.tracks,
            &self.master_volume,
            &self.limiter,
            &self.peak,
            out,
        )
    }

    /// Peak output level (0.0..=1.0) since the previous call, for the meter.
    pub fn take_peak(&self) -> f32 {
        f32::from_bits(self.peak.swap(0, Ordering::Relaxed))
    }

    /// Publish an already-mixed buffer to the network broadcast channel.
    ///
    /// The send is non-blocking: if the forwarder is behind, the frame is
    /// dropped rather than stalling the audio callback.
    pub fn publish_broadcast(&self, buffer: &[f32]) {
        Self::publish_broadcast_parts(&self.broadcast_tx, &self.timestamp, &self.seq_no, buffer);
    }

    fn mix_into_parts(
        tracks: &RwLock<HashMap<Uuid, TrackState>>,
        master_volume: &parking_lot::Mutex<f32>,
        limiter: &parking_lot::Mutex<Limiter>,
        peak: &AtomicU32,
        out: &mut [f32],
    ) -> bool {
        let frames = out.len() / CHANNELS;
        out.fill(0.0);
        if frames == 0 {
            return false;
        }

        let master = *master_volume.lock();
        let mut any_playing = false;
        let ramp_step = 1.0 / RAMP_FRAMES;

        {
            // One write lock for the whole pass: positions, gains and fade
            // states are updated in place, so nothing is allocated on the
            // audio thread and the GUI never sees a half-updated track.
            let mut tracks_write = tracks.write();
            for track in tracks_write.values_mut() {
                if !track.playing || track.audio.frames == 0 {
                    continue;
                }

                any_playing = true;
                let total = track.audio.frames;
                let samples = &track.audio.samples;
                let target = track.target_gain();
                let mut gain = track.gain;
                let mut src_pos = track.position.min(total);
                let mut finished = false;

                for frame in 0..frames {
                    if src_pos >= total {
                        if track.looped {
                            src_pos = 0;
                        } else {
                            finished = true;
                            break;
                        }
                    }

                    if gain != target {
                        let delta = (target - gain).clamp(-ramp_step, ramp_step);
                        gain += delta;
                        if (target - gain).abs() < 1e-6 {
                            gain = target;
                        }
                    }

                    let src = src_pos * CHANNELS;
                    let dst = frame * CHANNELS;
                    out[dst] += samples[src] * gain;
                    out[dst + 1] += samples[src + 1] * gain;
                    src_pos += 1;

                    // The fade-out of a pause/stop has reached silence.
                    if track.halt.is_some() && gain <= 0.0 {
                        break;
                    }
                }

                track.gain = gain;
                track.position = src_pos;
                if finished {
                    track.position = total;
                    track.playing = false;
                    track.halt = None;
                } else if let Some(halt) = track.halt {
                    if gain <= 0.0 {
                        track.playing = false;
                        track.halt = None;
                        match halt {
                            PendingHalt::Stop => track.position = 0,
                            PendingHalt::Pause => track.paused = true,
                        }
                    }
                }
            }
        }

        // Apply master volume once, after all tracks have been summed. The
        // CPAL callback used to multiply by master volume a second time.
        if (master - 1.0).abs() > f32::EPSILON {
            for sample in out.iter_mut() {
                *sample *= master;
            }
        }

        // Keep the bus below full scale. Hard clipping of an overloaded sum is
        // the harsh, buzzing distortion; the limiter turns it down instead.
        limiter.lock().process(out);

        if any_playing {
            let block_peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            peak.fetch_max(block_peak.to_bits(), Ordering::Relaxed);
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
        if let Err(crossbeam_channel::TrySendError::Disconnected(_)) = broadcast_tx.try_send(output)
        {
            // Receiver dropped; the mixer can keep running harmlessly.
        }
    }

    /// Headless mix loop — only used when no CPAL device is available.
    ///
    /// Uses a fixed deadline instead of `sleep(interval)`, so scheduler
    /// oversleep does not accumulate into a permanent underrun.
    #[allow(clippy::too_many_arguments)]
    fn headless_loop(
        config: MixerConfig,
        tracks: Arc<RwLock<HashMap<Uuid, TrackState>>>,
        broadcast_tx: Sender<MixedOutput>,
        running: Arc<AtomicBool>,
        timestamp: Arc<AtomicU64>,
        seq_no: Arc<AtomicU64>,
        master_volume: Arc<parking_lot::Mutex<f32>>,
        limiter: Arc<parking_lot::Mutex<Limiter>>,
        peak: Arc<AtomicU32>,
    ) {
        let frames = config.buffer_frames.max(1);
        let mut buffer = vec![0.0f32; frames * CHANNELS];

        let sample_rate = TARGET_SAMPLE_RATE.max(1) as u64;
        let period = Duration::from_nanos((frames as u64 * 1_000_000_000) / sample_rate);
        let mut next_deadline = Instant::now() + period;

        while running.load(Ordering::Acquire) {
            let playing =
                Self::mix_into_parts(&tracks, &master_volume, &limiter, &peak, &mut buffer);
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

    /// Enable or disable looping for a specific track.
    ///
    /// Takes effect immediately: the mix loop checks the flag when it reaches
    /// the end of the buffer, so toggling it mid-playback either wraps around
    /// from the start or lets the track finish and stop.
    pub fn set_track_looped(&self, track_id: Uuid, looped: bool) -> Result<(), MixerError> {
        let mut tracks = self.tracks.write();
        if let Some(track) = tracks.get_mut(&track_id) {
            track.looped = looped;
            Ok(())
        } else {
            Err(MixerError::TrackNotFound(track_id))
        }
    }

    /// Whether a specific track is set to loop.
    pub fn is_track_looped(&self, track_id: Uuid) -> Result<bool, MixerError> {
        let tracks = self.tracks.read();
        tracks
            .get(&track_id)
            .map(|t| t.looped)
            .ok_or(MixerError::TrackNotFound(track_id))
    }

    /// Start playback for a track (resets position to 0)
    ///
    /// Starts at full gain on purpose: a one-shot stinger (door slam, sword
    /// hit) must keep its attack.
    pub fn play_track(&self, track_id: Uuid) -> Result<(), MixerError> {
        let mut tracks = self.tracks.write();
        if let Some(track) = tracks.get_mut(&track_id) {
            track.playing = true;
            track.position = 0;
            track.halt = None;
            track.paused = false;
            track.gain = track.volume;
            Ok(())
        } else {
            Err(MixerError::TrackNotFound(track_id))
        }
    }

    /// Continue a paused track from where it was, fading back in.
    ///
    /// Unlike [`play_track`](Self::play_track) this keeps the position; the
    /// engine's `resume` used to call `play_track` and restart from the top.
    pub fn resume_track(&self, track_id: Uuid) -> Result<(), MixerError> {
        let mut tracks = self.tracks.write();
        if let Some(track) = tracks.get_mut(&track_id) {
            if !track.playing {
                track.gain = 0.0;
            }
            track.playing = track.position < track.audio.frames || track.looped;
            track.halt = None;
            track.paused = false;
            Ok(())
        } else {
            Err(MixerError::TrackNotFound(track_id))
        }
    }

    /// Stop playback for a track (does not remove it).
    ///
    /// The track fades out over a few milliseconds instead of being cut
    /// mid-waveform, which is an audible click; it reports as stopped at once.
    pub fn stop_track(&self, track_id: Uuid) -> Result<(), MixerError> {
        self.halt_track(track_id, PendingHalt::Stop)
    }

    /// Pause playback for a track (fades out, keeps the position).
    pub fn pause_track(&self, track_id: Uuid) -> Result<(), MixerError> {
        self.halt_track(track_id, PendingHalt::Pause)
    }

    fn halt_track(&self, track_id: Uuid, halt: PendingHalt) -> Result<(), MixerError> {
        let mut tracks = self.tracks.write();
        if let Some(track) = tracks.get_mut(&track_id) {
            if track.playing {
                track.halt = Some(halt);
            } else if halt == PendingHalt::Stop {
                track.position = 0;
                track.paused = false;
            }
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
            .map(TrackState::is_audible_playing)
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

    /// Drop every track that is neither playing nor fading out.
    ///
    /// Each `play` adds a track; without this a long session accumulates
    /// thousands of dead entries that the audio callback walks on every buffer.
    pub fn prune_finished(&self) -> usize {
        let mut tracks = self.tracks.write();
        let before = tracks.len();
        tracks.retain(|_, t| t.playing || t.paused);
        before - tracks.len()
    }

    /// Whether the mixer still holds this track.
    pub fn has_track(&self, track_id: Uuid) -> bool {
        self.tracks.read().contains_key(&track_id)
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

    /// Turning "repeat" on for a track that is already running keeps it going
    /// past the end of its buffer instead of letting it stop.
    #[test]
    fn test_set_track_looped_takes_effect_mid_playback() {
        let mixer = AudioMixer::new();
        let audio = Arc::new(DecodedAudio::new(vec![0.25; 100 * 2], 48000, 2));
        let id = mixer.add_track(audio, false);
        mixer.play_track(id).unwrap();

        assert!(!mixer.is_track_looped(id).unwrap());
        mixer.set_track_looped(id, true).unwrap();
        assert!(mixer.is_track_looped(id).unwrap());

        // One buffer longer than the sound: with repeat on it wraps and keeps
        // producing audio instead of falling silent.
        let mut out = vec![0.0f32; 480 * 2];
        assert!(mixer.mix_into(&mut out));
        assert!(out.iter().all(|s| (*s - 0.25).abs() < 1e-6));
        assert!(mixer.is_track_playing(id).unwrap());
    }

    /// Turning "repeat" off mid-playback lets the current pass finish and stop.
    #[test]
    fn test_clearing_track_loop_stops_at_end_of_buffer() {
        let mixer = AudioMixer::new();
        let audio = Arc::new(DecodedAudio::new(vec![0.5; 100 * 2], 48000, 2));
        let id = mixer.add_track(audio, true);
        mixer.play_track(id).unwrap();

        mixer.set_track_looped(id, false).unwrap();
        assert!(!mixer.is_track_looped(id).unwrap());

        let mut out = vec![0.0f32; 480 * 2];
        assert!(mixer.mix_into(&mut out));
        assert!(!mixer.is_track_playing(id).unwrap());
    }

    /// Stopping fades the track out over the ramp instead of cutting it
    /// mid-waveform (an audible click), and it reports as stopped at once.
    #[test]
    fn test_stop_fades_out_instead_of_cutting() {
        let mixer = AudioMixer::new();
        let audio = Arc::new(DecodedAudio::new(vec![0.5; 48_000 * 2], 48_000, 2));
        let id = mixer.add_track(audio, true);
        mixer.play_track(id).unwrap();

        let mut out = vec![0.0f32; 480 * 2];
        mixer.mix_into(&mut out);
        mixer.stop_track(id).unwrap();
        assert!(!mixer.is_track_playing(id).unwrap());

        let mut tail = vec![0.0f32; 960 * 2];
        mixer.mix_into(&mut tail);
        // Starts near the old level, decreases monotonically, ends silent.
        assert!(tail[0] > 0.49);
        assert!(tail
            .as_chunks::<2>()
            .0
            .iter()
            .map(|f| f[0])
            .collect::<Vec<_>>()
            .windows(2)
            .all(|w| w[1] <= w[0] + 1e-6));
        assert_eq!(tail[tail.len() - 1], 0.0);
        // After the fade the track rewinds, so a new play starts from 0.
        assert_eq!(mixer.prune_finished(), 1);
    }

    /// A volume change ramps instead of stepping (no zipper noise).
    #[test]
    fn test_volume_change_is_ramped() {
        let mixer = AudioMixer::new();
        let audio = Arc::new(DecodedAudio::new(vec![0.5; 48_000 * 2], 48_000, 2));
        let id = mixer.add_track(audio, true);
        mixer.play_track(id).unwrap();
        mixer.set_track_volume(id, 0.0).unwrap();

        let mut out = vec![0.0f32; 480 * 2];
        mixer.mix_into(&mut out);
        let max_step = out
            .as_chunks::<2>()
            .0
            .iter()
            .map(|f| f[0])
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(max_step < 0.01, "volume jumped by {max_step}");
    }

    /// Pause keeps the position; resume continues instead of restarting.
    #[test]
    fn test_pause_resume_keeps_position() {
        let mixer = AudioMixer::new();
        let ramp: Vec<f32> = (0..48_000).flat_map(|i| [i as f32 / 48_000.0; 2]).collect();
        let id = mixer.add_track(Arc::new(DecodedAudio::new(ramp, 48_000, 2)), false);
        mixer.play_track(id).unwrap();

        let mut out = vec![0.0f32; 4_800 * 2];
        mixer.mix_into(&mut out);
        mixer.pause_track(id).unwrap();
        mixer.mix_into(&mut out); // fade-out completes
        assert_eq!(mixer.prune_finished(), 0, "a paused track must be kept");

        mixer.resume_track(id).unwrap();
        mixer.mix_into(&mut out);
        // Well past the first 0.1 s, so it did not restart from the top.
        assert!(out[out.len() - 2] > 0.15);
    }

    /// Several loud tracks are limited below full scale instead of clipping.
    #[test]
    fn test_overloaded_mix_is_limited() {
        let mixer = AudioMixer::new();
        for _ in 0..4 {
            let audio = Arc::new(DecodedAudio::new(vec![0.8; 4_800 * 2], 48_000, 2));
            let id = mixer.add_track(audio, true);
            mixer.play_track(id).unwrap();
        }
        let mut out = vec![0.0f32; 4_800 * 2];
        mixer.mix_into(&mut out);
        assert!(out.iter().all(|s| s.abs() <= LIMIT_CEILING + 1e-6));
        assert!(out[out.len() - 1] > 0.9);
    }

    #[test]
    fn test_set_track_looped_rejects_unknown_track() {
        let mixer = AudioMixer::new();
        let unknown = uuid::Uuid::new_v4();
        assert!(mixer.set_track_looped(unknown, true).is_err());
        assert!(mixer.is_track_looped(unknown).is_err());
    }
}
