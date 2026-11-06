//! Stub Opus codec types used when the `opus-codec` feature is disabled.
//!
//! This module mirrors the public API of the [`rusty-opus`](https://crates.io/crates/rusty-opus)
//! crate (Encoder, Decoder, Application, SignalType) so that `opus_wire.rs` and
//! `broadcast.rs` can compile without any C-dependent crate.
//! All encoding/decoding is a no-op: the stub encoder produces empty packets,
//! the stub decoder consumes nothing. This lets `build.bat` succeed on hosts
//! where MSVC's cl.exe cannot spawn c1.dll (e.g. restricted Windows sandboxes).
//!
//! On a normal Windows machine, enable real Opus with:
//!     cargo build --features opus-codec
//!
//! rusty-opus is a pure-Rust implementation of the Opus codec (RFC 6716):
//! no C code, no FFI, no build-time toolchain.

/// Signal type hint for the encoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalType {
    /// Music signal hint (biases toward music-oriented coding mode).
    Music,
    /// Speech signal hint.
    Speech,
}

/// Application type for the encoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Application {
    /// Audio (music/ambience) mode.
    Audio,
    /// Voice mode.
    Voice,
}

/// Opus encoder stub — produces no packets.
pub struct Encoder {
    /// Target bitrate in bits per second.
    pub bitrate_bps: i32,
    /// Encoder complexity (0–10).
    pub complexity: i32,
    /// Use constant bitrate when true; false = VBR.
    pub use_cbr: bool,
    /// Signal type hint.
    pub signal_type: Option<SignalType>,
}

/// Opus decoder stub — consumes nothing, produces no PCM.
pub struct Decoder;

impl Encoder {
    /// Create a new stub encoder. Always succeeds.
    pub fn new(_rate: i32, _channels: usize, _app: Application) -> Result<Self, String> {
        Ok(Self {
            bitrate_bps: 0,
            complexity: 0,
            use_cbr: true,
            signal_type: None,
        })
    }

    /// Encode stub: always produces an empty packet (0 bytes).
    pub fn encode(
        &mut self,
        _input: &[f32],
        _max_size: usize,
        _output: &mut [u8],
    ) -> Result<usize, String> {
        Ok(0)
    }
}

impl Decoder {
    /// Create a new stub decoder. Always succeeds.
    pub fn new(_rate: i32, _channels: i32) -> Result<Self, String> {
        Ok(Self)
    }

    /// Decode stub: always consumes nothing and writes no PCM.
    pub fn decode(
        &mut self,
        _packet: &[u8],
        _output: &mut [f32],
        _fec: bool,
    ) -> Result<usize, String> {
        Ok(0)
    }
}
