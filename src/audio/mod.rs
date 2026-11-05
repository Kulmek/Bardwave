//! Audio engine module for SoundShared
//! Provides decoding, mixing, and playback functionality

pub mod decoder;
pub mod engine;
pub mod mixer;

pub use decoder::{AudioDecoder, DecodedAudio, DecoderError, TARGET_CHANNELS, TARGET_SAMPLE_RATE};
pub use engine::{
    AudioEngine, EngineError, EngineStats, LocalOutputState, LocalOutputStatus, SoundId,
    TrackHandle,
};
pub use mixer::{AudioMixer, MixedOutput, MixerConfig, MixerError};
