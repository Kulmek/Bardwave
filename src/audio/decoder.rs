//! Audio decoding using Symphonia
//! Supports MP3, WAV, OGG, FLAC, OPUS formats

use std::fs::File;
use std::path::Path;

use anyhow::Result;
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use thiserror::Error;

/// Target sample rate for all decoded audio
pub const TARGET_SAMPLE_RATE: u32 = 48000;
/// Target channels for all decoded audio (stereo)
pub const TARGET_CHANNELS: u32 = 2;

/// Errors that can occur during audio decoding
#[derive(Error, Debug)]
pub enum DecoderError {
    #[error("Failed to open file: {0}")]
    FileOpen(#[from] std::io::Error),
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("No audio tracks found in file")]
    NoAudioTracks,
    #[error("Decoder error: {0}")]
    Decoder(#[from] symphonia::core::errors::Error),
    #[error("Resampling not implemented for this configuration")]
    ResampleUnsupported,
    #[error("Channel conversion not supported: {0}")]
    ChannelConversion(String),
}

/// Decoded audio data with metadata
#[derive(Debug, Clone)]
pub struct DecodedAudio {
    /// PCM samples as f32, interleaved stereo (L, R, L, R, ...)
    pub samples: Vec<f32>,
    /// Sample rate (always TARGET_SAMPLE_RATE after decoding)
    pub sample_rate: u32,
    /// Number of channels (always TARGET_CHANNELS after decoding)
    pub channels: u32,
    /// Duration in seconds
    pub duration: f32,
    /// Number of frames (samples per channel)
    pub frames: usize,
}

impl DecodedAudio {
    /// Create a new DecodedAudio from raw samples
    pub fn new(samples: Vec<f32>, sample_rate: u32, channels: u32) -> Self {
        let frames = samples.len() / channels as usize;
        let duration = frames as f32 / sample_rate as f32;
        Self {
            samples,
            sample_rate,
            channels,
            duration,
            frames,
        }
    }

    /// Get the total number of samples (interleaved)
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Check if the audio is empty
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Get a slice of samples for a specific time range
    pub fn slice(&self, start_frame: usize, end_frame: usize) -> Vec<f32> {
        let start = start_frame * self.channels as usize;
        let end = end_frame * self.channels as usize;
        self.samples[start..end.min(self.samples.len())].to_vec()
    }
}

/// High-level audio decoder using Symphonia
pub struct AudioDecoder;

impl AudioDecoder {
    /// Decode an audio file to PCM samples at target sample rate and channels
    pub fn decode_file<P: AsRef<Path>>(path: P) -> Result<DecodedAudio, DecoderError> {
        let file = File::open(path.as_ref())?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        // Create a probe hint from the file extension
        let hint = Hint::new();
        let meta_opts = MetadataOptions::default();
        let fmt_opts = FormatOptions::default();

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &fmt_opts, &meta_opts)
            .map_err(|e| DecoderError::UnsupportedFormat(e.to_string()))?;

        let mut format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or(DecoderError::NoAudioTracks)?;

        let track_id = track.id;
        let codec_params = track.codec_params.clone();

        // Create decoder
        let dec_opts = DecoderOptions::default();
        let mut decoder = symphonia::default::get_codecs()
            .make(&codec_params, &dec_opts)
            .map_err(|e| DecoderError::Decoder(e))?;

        let mut all_samples = Vec::new();
        let sample_rate = codec_params.sample_rate.unwrap_or(TARGET_SAMPLE_RATE);
        let channels = codec_params
            .channels
            .map(|c| c.count() as usize)
            .unwrap_or(TARGET_CHANNELS as usize);
        let channels_u32: u32 = channels.try_into().unwrap();

        // Decode all packets
        loop {
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(symphonia::core::errors::Error::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(e) => return Err(DecoderError::Decoder(e)),
            };

            if packet.track_id() != track_id {
                continue;
            }

            match decoder.decode(&packet) {
                Ok(audio_buf) => {
                    let converted =
                        Self::convert_audio_buffer(audio_buf, sample_rate, channels_u32)?;
                    all_samples.extend_from_slice(&converted);
                }
                Err(symphonia::core::errors::Error::DecodeError(_)) => {
                    // Skip corrupted frames
                    continue;
                }
                Err(e) => return Err(DecoderError::Decoder(e)),
            }
        }

        // Resample if needed
        let final_samples = if sample_rate != TARGET_SAMPLE_RATE {
            Self::resample(
                &all_samples,
                channels as u32,
                sample_rate,
                TARGET_SAMPLE_RATE,
            )?
        } else {
            all_samples
        };

        // Convert channels if needed
        let final_samples = if channels as u32 != TARGET_CHANNELS {
            Self::convert_channels(&final_samples, channels as u32, TARGET_CHANNELS)?
        } else {
            final_samples
        };

        let frames = final_samples.len() / TARGET_CHANNELS as usize;
        let duration = frames as f32 / TARGET_SAMPLE_RATE as f32;

        Ok(DecodedAudio {
            samples: final_samples,
            sample_rate: TARGET_SAMPLE_RATE,
            channels: TARGET_CHANNELS,
            duration,
            frames,
        })
    }

    /// Convert an audio buffer to f32 stereo at the given sample rate
    fn convert_audio_buffer(
        audio_buf: AudioBufferRef,
        _src_sample_rate: u32,
        _src_channels: u32,
    ) -> Result<Vec<f32>, DecoderError> {
        let frames = audio_buf.frames() as usize;
        let channels = audio_buf.spec().channels.count() as usize;

        // Convert to f32 - get all channels
        let mut channel_data: Vec<Vec<f32>> = Vec::with_capacity(channels);
        for ch in 0..channels {
            let samples: Vec<f32> = match &audio_buf {
                AudioBufferRef::U8(ref buf) => buf
                    .chan(ch)
                    .iter()
                    .map(|&v| v as f32 / 255.0 * 2.0 - 1.0)
                    .collect(),
                AudioBufferRef::U16(ref buf) => buf
                    .chan(ch)
                    .iter()
                    .map(|&v| v as f32 / 65535.0 * 2.0 - 1.0)
                    .collect(),
                AudioBufferRef::U24(ref buf) => buf
                    .chan(ch)
                    .iter()
                    .map(|&v| v.0 as f32 / 16777215.0 * 2.0 - 1.0)
                    .collect(),
                AudioBufferRef::U32(ref buf) => buf
                    .chan(ch)
                    .iter()
                    .map(|&v| v as f32 / 4294967295.0 * 2.0 - 1.0)
                    .collect(),
                AudioBufferRef::S8(ref buf) => {
                    buf.chan(ch).iter().map(|&v| v as f32 / 128.0).collect()
                }
                AudioBufferRef::S16(ref buf) => {
                    buf.chan(ch).iter().map(|&v| v as f32 / 32768.0).collect()
                }
                AudioBufferRef::S24(ref buf) => buf
                    .chan(ch)
                    .iter()
                    .map(|&v| v.0 as f32 / 8388608.0)
                    .collect(),
                AudioBufferRef::S32(ref buf) => buf
                    .chan(ch)
                    .iter()
                    .map(|&v| v as f32 / 2147483648.0)
                    .collect(),
                AudioBufferRef::F32(ref buf) => buf.chan(ch).iter().copied().collect(),
                AudioBufferRef::F64(ref buf) => buf.chan(ch).iter().map(|&v| v as f32).collect(),
            };
            channel_data.push(samples);
        }

        // Interleave channels
        let mut interleaved = Vec::with_capacity(frames * channels);
        for frame in 0..frames {
            for ch in 0..channels {
                interleaved.push(channel_data[ch][frame]);
            }
        }
        Ok(interleaved)
    }

    /// Simple linear resampling (for production, consider using rubato or samplerrate crate)
    fn resample(
        samples: &[f32],
        channels: u32,
        from_rate: u32,
        to_rate: u32,
    ) -> Result<Vec<f32>, DecoderError> {
        if from_rate == to_rate {
            return Ok(samples.to_vec());
        }

        let ratio = from_rate as f64 / to_rate as f64;
        let input_frames = samples.len() / channels as usize;
        let output_frames = (input_frames as f64 / ratio).ceil() as usize;
        let mut output = Vec::with_capacity(output_frames * channels as usize);

        for ch in 0..channels as usize {
            for out_frame in 0..output_frames {
                let src_pos = out_frame as f64 * ratio;
                let src_frame = src_pos.floor() as usize;
                let frac = src_pos - src_frame as f64;

                let idx1 = src_frame * channels as usize + ch;
                let idx2 = ((src_frame + 1).min(input_frames - 1)) * channels as usize + ch;

                let sample1 = samples.get(idx1).copied().unwrap_or(0.0);
                let sample2 = samples.get(idx2).copied().unwrap_or(0.0);

                let interpolated = sample1 + (sample2 - sample1) * frac as f32;
                output.push(interpolated);
            }
        }

        // Re-interleave
        let mut interleaved = Vec::with_capacity(output.len());
        let out_frames = output.len() / channels as usize;
        for frame in 0..out_frames {
            for ch in 0..channels as usize {
                interleaved.push(output[frame + ch * out_frames]);
            }
        }

        Ok(interleaved)
    }

    /// Convert between channel configurations
    fn convert_channels(
        samples: &[f32],
        from_channels: u32,
        to_channels: u32,
    ) -> Result<Vec<f32>, DecoderError> {
        if from_channels == to_channels {
            return Ok(samples.to_vec());
        }

        let frames = samples.len() / from_channels as usize;
        let mut output = Vec::with_capacity(frames * to_channels as usize);

        match (from_channels, to_channels) {
            (1, 2) => {
                // Mono to stereo: duplicate
                for frame in 0..frames {
                    let sample = samples[frame];
                    output.push(sample); // Left
                    output.push(sample); // Right
                }
            }
            (2, 1) => {
                // Stereo to mono: average
                for frame in 0..frames {
                    let left = samples[frame * 2];
                    let right = samples[frame * 2 + 1];
                    output.push((left + right) * 0.5);
                }
            }
            (_, 2) => {
                // Multi-channel to stereo: take first two or downmix
                for frame in 0..frames {
                    let left = samples[frame * from_channels as usize];
                    let right = if from_channels >= 2 {
                        samples[frame * from_channels as usize + 1]
                    } else {
                        left
                    };
                    output.push(left);
                    output.push(right);
                }
            }
            _ => {
                return Err(DecoderError::ChannelConversion(format!(
                    "{} -> {} not implemented",
                    from_channels, to_channels
                )));
            }
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decoded_audio_creation() {
        let samples = vec![0.0; 48000 * 2]; // 1 second of silence at 48kHz stereo
        let audio = DecodedAudio::new(samples, 48000, 2);
        assert_eq!(audio.frames, 48000);
        assert_eq!(audio.duration, 1.0);
        assert_eq!(audio.sample_rate, 48000);
        assert_eq!(audio.channels, 2);
    }

    #[test]
    fn test_slice() {
        let samples: Vec<f32> = (0..48000 * 2).map(|i| i as f32).collect();
        let audio = DecodedAudio::new(samples, 48000, 2);
        let slice = audio.slice(0, 1000);
        assert_eq!(slice.len(), 2000); // 1000 frames * 2 channels
    }
}
