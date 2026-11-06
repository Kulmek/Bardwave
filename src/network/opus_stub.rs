//! Stub Opus codec types used when the `opus-codec` feature is disabled.
//!
//! This module mirrors the public API of the `opus` crate (Encoder, Decoder,
//! Channels, Application, Signal, Bitrate) so that `opus_wire.rs` and
//! `broadcast.rs` can compile without the C-dependent `opusic-sys` crate.
//! All encoding/decoding is a no-op: the stub encoder produces empty packets,
//! the stub decoder consumes nothing. This lets `build.bat` succeed on hosts
//! where MSVC's cl.exe cannot spawn c1.dll (e.g. restricted Windows sandboxes).
//!
//! On a normal Windows machine, enable real Opus with:
//!     cargo build --features opus-codec

/// Opus encoder stub — produces no packets.
pub struct Encoder;

/// Opus decoder stub — consumes nothing, produces no PCM.
pub struct Decoder;

/// Channel count stub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channels {
    /// Stereo (2 channels).
    Stereo,
}

/// Application type stub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Application {
    /// Audio (music/ambience) mode.
    Audio,
}

/// Signal type stub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// Music signal hint.
    Music,
}

/// Bitrate stub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bitrate {
    /// Constant bitrate in bits per second.
    Bits(i32),
}

impl Encoder {
    /// Create a new stub encoder. Always succeeds.
    pub fn new(_rate: u32, _channels: Channels, _app: Application) -> Result<Self, String> {
        Ok(Self)
    }

    /// No-op: bitrate is ignored by the stub.
    pub fn set_bitrate(&mut self, _bitrate: Bitrate) {}

    /// No-op: VBR is ignored by the stub.
    pub fn set_vbr(&mut self, _vbr: bool) {}

    /// No-op: VBR constraint is ignored by the stub.
    pub fn set_vbr_constraint(&mut self, _constraint: bool) {}

    /// No-op: signal type is ignored by the stub.
    pub fn set_signal(&mut self, _signal: Signal) {}

    /// No-op: complexity is ignored by the stub.
    pub fn set_complexity(&mut self, _complexity: i32) {}

    /// Encode stub: always produces an empty packet (0 bytes).
    pub fn encode_float(&mut self, _input: &[f32], _output: &mut [u8]) -> Result<usize, String> {
        Ok(0)
    }

    /// Encode-to-vec stub: always returns an empty packet.
    pub fn encode_vec_float(
        &mut self,
        _input: &[f32],
        _max_size: usize,
    ) -> Result<Vec<u8>, String> {
        Ok(vec![])
    }
}

impl Decoder {
    /// Create a new stub decoder. Always succeeds.
    pub fn new(_rate: u32, _channels: Channels) -> Result<Self, String> {
        Ok(Self)
    }

    /// Decode stub: always consumes nothing and writes no PCM.
    pub fn decode_float(
        &mut self,
        _packet: &[u8],
        _output: &mut [f32],
        _fec: bool,
    ) -> Result<usize, String> {
        Ok(0)
    }
}
