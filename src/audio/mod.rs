//! Audio engine module for SoundShared
//! Provides decoding, mixing, and playback functionality

pub mod decoder;
pub mod engine;
pub mod mixer;

pub use decoder::{AudioDecoder, DecodedAudio, DecoderError, TARGET_CHANNELS, TARGET_SAMPLE_RATE};
pub use engine::{AudioEngine, EngineError, SoundId, TrackHandle, EngineStats};
pub use mixer::{AudioMixer, MixerConfig, MixedOutput, MixerError};