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
        let path = path.as_ref();
        let file = File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        // The extension is a strong hint for the prober: without it, MP3 files
        // with a large ID3 tag or no Xing header are often not recognised.
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let meta_opts = MetadataOptions::default();
        let fmt_opts = FormatOptions {
            enable_gapless: true,
            ..Default::default()
        };

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
            .map_err(DecoderError::Decoder)?;

        // The rate and channel count are taken from the *decoded* buffers, not
        // from the container header: MP3/AAC headers often omit the channel
        // layout (it used to default to stereo, so a mono file played twice
        // as fast with its samples split between the ears), and HE-AAC
        // declares half its real output rate.
        let mut all_samples = Vec::new();
        let mut sample_rate = codec_params.sample_rate.unwrap_or(TARGET_SAMPLE_RATE);
        let mut channels: Option<usize> = None;

        loop {
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(symphonia::core::errors::Error::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                // A chained stream (e.g. concatenated Ogg) ends the track here.
                Err(symphonia::core::errors::Error::ResetRequired) => break,
                Err(e) => return Err(DecoderError::Decoder(e)),
            };

            if packet.track_id() != track_id {
                continue;
            }

            match decoder.decode(&packet) {
                Ok(audio_buf) => {
                    let spec = *audio_buf.spec();
                    let buf_channels = spec.channels.count();
                    if buf_channels == 0 || audio_buf.frames() == 0 {
                        continue;
                    }
                    match channels {
                        None => {
                            channels = Some(buf_channels);
                            sample_rate = spec.rate;
                        }
                        // A mid-stream layout change cannot be interleaved
                        // into one buffer; skip the odd packets.
                        Some(c) if c != buf_channels => continue,
                        Some(_) => {}
                    }
                    let converted = Self::convert_audio_buffer(audio_buf, spec.rate, 0)?;
                    all_samples.extend_from_slice(&converted);
                }
                Err(symphonia::core::errors::Error::DecodeError(_)) => {
                    // Skip corrupted frames
                    continue;
                }
                Err(e) => return Err(DecoderError::Decoder(e)),
            }
        }

        let channels = channels.unwrap_or(TARGET_CHANNELS as usize);
        let channels_u32 = u32::try_from(channels).unwrap_or(TARGET_CHANNELS);

        // Downmix first: resampling two channels is cheaper than six.
        let final_samples = if channels_u32 != TARGET_CHANNELS {
            Self::convert_channels(&all_samples, channels_u32, TARGET_CHANNELS)?
        } else {
            all_samples
        };

        // Resample if needed
        let final_samples = if sample_rate != TARGET_SAMPLE_RATE && sample_rate > 0 {
            Self::resample(
                &final_samples,
                TARGET_CHANNELS,
                sample_rate,
                TARGET_SAMPLE_RATE,
            )?
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
        let frames = audio_buf.frames();
        let channels = audio_buf.spec().channels.count();

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
                AudioBufferRef::F32(ref buf) => buf.chan(ch).to_vec(),
                AudioBufferRef::F64(ref buf) => buf.chan(ch).iter().map(|&v| v as f32).collect(),
            };
            channel_data.push(samples);
        }

        // Interleave channels
        let mut interleaved = Vec::with_capacity(frames * channels);
        for frame in 0..frames {
            for channel in &channel_data {
                interleaved.push(channel[frame]);
            }
        }
        Ok(interleaved)
    }

    /// Band-limited (windowed-sinc) sample-rate conversion.
    ///
    /// Linear interpolation, used before, aliases badly: every 44.1 -> 48 kHz
    /// file got audible metallic images, and a 96 kHz source folded its
    /// ultrasonic content down into the audible band. This is a polyphase
    /// Blackman-windowed sinc, low-passed at the lower of the two Nyquist
    /// frequencies. Decoding is offline (once per file), so the cost is fine.
    fn resample(
        samples: &[f32],
        channels: u32,
        from_rate: u32,
        to_rate: u32,
    ) -> Result<Vec<f32>, DecoderError> {
        if from_rate == to_rate {
            return Ok(samples.to_vec());
        }
        let channels = channels as usize;
        if channels == 0 || from_rate == 0 || to_rate == 0 {
            return Err(DecoderError::ResampleUnsupported);
        }
        let input_frames = samples.len() / channels;
        if input_frames == 0 {
            return Ok(Vec::new());
        }

        // Source-sample step per output sample, and the low-pass cutoff as a
        // fraction of the source Nyquist (with a little transition band).
        let step = from_rate as f64 / to_rate as f64;
        let cutoff = (to_rate as f64 / from_rate as f64).min(1.0) * 0.94;
        // Zero crossings on each side of the kernel.
        const ZERO_CROSSINGS: f64 = 16.0;
        let half_width = (ZERO_CROSSINGS / cutoff).ceil() as i64;

        // Kernel table: PHASES entries per source sample, linearly interpolated.
        const PHASES: usize = 256;
        let table_len = half_width as usize * PHASES + 2;
        let table: Vec<f32> = (0..table_len)
            .map(|i| {
                let x = i as f64 / PHASES as f64; // distance in source samples
                if x >= half_width as f64 {
                    return 0.0;
                }
                let arg = std::f64::consts::PI * x * cutoff;
                let sinc = if x == 0.0 { 1.0 } else { arg.sin() / arg };
                let u = x / half_width as f64; // 0..1
                let window = 0.42
                    + 0.5 * (std::f64::consts::PI * u).cos()
                    + 0.08 * (2.0 * std::f64::consts::PI * u).cos();
                (cutoff * sinc * window) as f32
            })
            .collect();
        let kernel = |distance: f64| -> f32 {
            let pos = distance.abs() * PHASES as f64;
            let index = pos as usize;
            if index + 1 >= table_len {
                return 0.0;
            }
            let frac = (pos - index as f64) as f32;
            table[index] + (table[index + 1] - table[index]) * frac
        };

        let output_frames = ((input_frames as f64) / step).floor() as usize;
        let mut output = vec![0.0f32; output_frames * channels];
        let mut acc = vec![0.0f32; channels];
        for (out_frame, out) in output.chunks_exact_mut(channels).enumerate() {
            let center = out_frame as f64 * step;
            let first = (center.floor() as i64 - half_width + 1).max(0);
            let last = (center.floor() as i64 + half_width).min(input_frames as i64 - 1);
            acc.iter_mut().for_each(|a| *a = 0.0);
            let mut weight_sum = 0.0f32;
            for src in first..=last {
                let w = kernel(center - src as f64);
                weight_sum += w;
                let base = src as usize * channels;
                for (ch, a) in acc.iter_mut().enumerate() {
                    *a += samples[base + ch] * w;
                }
            }
            // Normalising by the summed weights keeps DC exact, including at
            // the file edges where the kernel is truncated.
            let norm = if weight_sum.abs() > 1e-6 {
                1.0 / weight_sum
            } else {
                0.0
            };
            for (o, a) in out.iter_mut().zip(acc.iter()) {
                *o = a * norm;
            }
        }

        Ok(output)
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
                for &sample in &samples[..frames] {
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
                // Multi-channel to stereo. Symphonia orders channels by the
                // WAVE/SMPTE layout: FL, FR, FC, LFE, then surrounds. Keep the
                // fronts, fold the centre and the surrounds in at -3 dB, and
                // drop the LFE, instead of throwing away everything but the
                // first two channels (which lost all dialogue in a 5.1 file).
                let n = from_channels as usize;
                let k = std::f32::consts::FRAC_1_SQRT_2;
                let norm = if n >= 3 { 1.0 / (1.0 + k + k) } else { 1.0 };
                for frame in samples.chunks_exact(n) {
                    let mut left = frame[0];
                    let mut right = frame[1];
                    if n >= 3 {
                        left += frame[2] * k;
                        right += frame[2] * k;
                    }
                    for (i, s) in frame.iter().enumerate().skip(4) {
                        if i % 2 == 0 {
                            left += s * k;
                        } else {
                            right += s * k;
                        }
                    }
                    output.push(left * norm);
                    output.push(right * norm);
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
    fn resample_keeps_a_tone_clean() {
        // 1 kHz at 44.1 kHz -> 48 kHz: same frequency, same amplitude, no images.
        let from = 44_100u32;
        let input: Vec<f32> = (0..from as usize)
            .flat_map(|i| {
                let v = 0.5 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / from as f32).sin();
                [v, v]
            })
            .collect();
        let out = AudioDecoder::resample(&input, 2, from, 48_000).unwrap();
        let frames = out.len() / 2;
        assert!((frames as i64 - 48_000).abs() <= 2, "{frames} frames");

        // Compare against the ideal 48 kHz tone away from the edges.
        let mut err = 0.0f64;
        let mut sig = 0.0f64;
        for i in 1000..frames - 1000 {
            let ideal = 0.5 * (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / 48_000.0).sin();
            let got = out[i * 2] as f64;
            err += (got - ideal).powi(2);
            sig += ideal.powi(2);
        }
        let snr_db = 10.0 * (sig / err).log10();
        assert!(snr_db > 60.0, "SNR only {snr_db:.1} dB");
    }

    #[test]
    fn surround_downmix_keeps_the_centre_channel() {
        // 6 channels: only the centre (index 2) carries signal.
        let frame = [0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
        let out = AudioDecoder::convert_channels(&frame, 6, 2).unwrap();
        assert!(out[0] > 0.2 && out[1] > 0.2);
    }

    #[test]
    fn test_slice() {
        let samples: Vec<f32> = (0..48000 * 2).map(|i| i as f32).collect();
        let audio = DecodedAudio::new(samples, 48000, 2);
        let slice = audio.slice(0, 1000);
        assert_eq!(slice.len(), 2000); // 1000 frames * 2 channels
    }
}
