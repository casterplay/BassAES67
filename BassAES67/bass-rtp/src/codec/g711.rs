//! G.711 mu-law (PCMU) audio codec encoder and decoder.
//!
//! G.711 is a narrowband audio codec operating at 8 kHz sample rate.
//! Each 8-bit encoded byte represents one 16-bit PCM sample.
//!
//! Payload type: PT 0 (mu-law)
//!
//! Algorithm sourced from ezk-media (MIT license):
//! https://github.com/kbalt/ezk-media

use super::{AudioDecoder, AudioEncoder, CodecError};

/// G.711 mu-law decoder.
///
/// Decodes 8-bit mu-law encoded audio to f32 PCM samples.
/// Stateless decoder - each byte is independently decoded.
pub struct G711UlawDecoder {
    channels: u8,
}

impl G711UlawDecoder {
    /// Create a new G.711 mu-law decoder.
    pub fn new() -> Self {
        Self { channels: 1 }
    }

    /// Create a new G.711 mu-law decoder with specified channel count.
    pub fn with_channels(channels: u8) -> Self {
        Self { channels }
    }
}

impl Default for G711UlawDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioDecoder for G711UlawDecoder {
    /// Decode mu-law encoded data to f32 samples.
    ///
    /// G.711 decodes to 8 kHz mono, but we upsample to 48 kHz stereo for compatibility
    /// with the 48 kHz stereo stream. Each input byte produces 1 sample at 8 kHz,
    /// which becomes 12 samples at 48 kHz stereo (6x upsample, 2x for stereo).
    fn decode(&mut self, data: &[u8], output: &mut [f32]) -> Result<usize, CodecError> {
        // We need 6x upsampling * 2 channels = 12 output samples per input byte
        let output_samples = data.len() * 12;

        if output.len() < output_samples {
            return Err(CodecError::BufferTooSmall);
        }

        // Decode, upsample 8kHz -> 48kHz (6x), and duplicate to stereo
        let mut out_idx = 0;
        for &byte in data.iter() {
            let sample_i16 = ulaw_decode(byte);
            let f32_sample = sample_i16 as f32 / 32768.0;
            // Replicate each sample 6 times for 8kHz -> 48kHz, with L+R stereo pairs
            for _ in 0..6 {
                output[out_idx] = f32_sample;     // Left
                output[out_idx + 1] = f32_sample; // Right
                out_idx += 2;
            }
        }

        Ok(output_samples)
    }

    /// Frame size in samples per channel (20ms at 48kHz after upsampling = 960 samples).
    fn frame_size(&self) -> usize {
        960 // 160 samples at 8kHz * 6 = 960 at 48kHz
    }

    /// Total samples per frame including all channels (stereo output).
    fn total_samples_per_frame(&self) -> usize {
        960 * 2 // Always stereo output
    }
}

/// Decode a single mu-law byte to a 16-bit signed sample.
///
/// Standard ITU-T G.711 mu-law decoding algorithm.
#[inline]
fn ulaw_decode(y: u8) -> i16 {
    let y = y as i16;
    let sign: i16 = if y < 0x0080 { -1 } else { 1 };

    let mantissa = !y;
    let exponent = (mantissa >> 4) & 0x7;
    let segment = exponent + 1;
    let mantissa = mantissa & 0xF;

    let step = 4 << segment;

    sign * ((0x0080 << exponent) + step * mantissa + step / 2 - 4 * 33)
}

/// Encode a single 16-bit signed sample to mu-law byte.
///
/// Standard ITU-T G.711 mu-law encoding algorithm.
#[inline]
fn ulaw_encode(sample: i16) -> u8 {
    const BIAS: i32 = 0x84;
    const CLIP: i32 = 32635;

    // Get the sign and the magnitude of the sample
    let sign = if sample < 0 { 0x80 } else { 0x00 };
    let mut sample = if sample < 0 { -sample } else { sample } as i32;

    // Clip the magnitude
    if sample > CLIP {
        sample = CLIP;
    }

    // Add bias for rounding and quantization
    sample += BIAS;

    // Find the segment (exponent) and quantization (mantissa)
    let exponent = if sample >= 0x4000 {
        7
    } else if sample >= 0x2000 {
        6
    } else if sample >= 0x1000 {
        5
    } else if sample >= 0x0800 {
        4
    } else if sample >= 0x0400 {
        3
    } else if sample >= 0x0200 {
        2
    } else if sample >= 0x0100 {
        1
    } else {
        0
    };

    let mantissa = (sample >> (exponent + 3)) & 0x0F;

    // Combine sign, exponent, and mantissa, then complement
    let ulaw_byte = !(sign | (exponent << 4) | mantissa as i32) as u8;
    ulaw_byte
}

// ============================================================================
// G.711 mu-law Encoder
// ============================================================================

/// G.711 mu-law encoder.
///
/// Encodes f32 PCM samples to 8-bit mu-law encoded audio.
/// Input is 48kHz stereo, output is 8kHz mono mu-law.
pub struct G711UlawEncoder {
    /// Accumulator for downsampling (6:1 ratio from 48kHz to 8kHz)
    downsample_accum: f32,
    downsample_count: usize,
}

impl G711UlawEncoder {
    /// Create a new G.711 mu-law encoder.
    pub fn new() -> Self {
        Self {
            downsample_accum: 0.0,
            downsample_count: 0,
        }
    }
}

impl Default for G711UlawEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEncoder for G711UlawEncoder {
    /// Encode f32 samples to mu-law.
    ///
    /// Input: 48kHz stereo f32 samples (interleaved L,R,L,R,...)
    /// Output: 8kHz mono mu-law bytes
    ///
    /// Downsamples 6:1 (48kHz -> 8kHz) and mixes stereo to mono.
    fn encode(&mut self, pcm: &[f32], output: &mut [u8]) -> Result<usize, CodecError> {
        // We process stereo pairs, downsample 6:1 (48kHz -> 8kHz)
        // Input: 48kHz stereo = 48 samples/ms * 2 channels = 96 values/ms
        // Output: 8kHz mono = 8 samples/ms = 8 bytes/ms
        // Ratio: 96 input values -> 8 output bytes (12:1 for stereo->mono+downsample)

        let stereo_pairs = pcm.len() / 2;
        let output_bytes = stereo_pairs / 6; // 6:1 downsample

        if output.len() < output_bytes {
            return Err(CodecError::BufferTooSmall);
        }

        let mut out_idx = 0;

        for i in 0..stereo_pairs {
            let left = pcm[i * 2];
            let right = pcm[i * 2 + 1];
            let mono = (left + right) * 0.5; // Mix to mono

            self.downsample_accum += mono;
            self.downsample_count += 1;

            if self.downsample_count >= 6 {
                // Average the 6 samples for downsampling
                let sample = self.downsample_accum / 6.0;
                self.downsample_accum = 0.0;
                self.downsample_count = 0;

                // Convert to i16 and encode
                let sample_i16 = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
                output[out_idx] = ulaw_encode(sample_i16);
                out_idx += 1;
            }
        }

        Ok(out_idx)
    }

    /// Frame size in samples per channel.
    /// 20ms at 48kHz = 960 samples per channel.
    fn frame_size(&self) -> usize {
        960
    }

    /// Total samples per frame (stereo input).
    /// 20ms at 48kHz stereo = 1920 samples.
    fn total_samples_per_frame(&self) -> usize {
        1920
    }

    /// RTP payload type for G.711 mu-law.
    fn payload_type(&self) -> u8 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ulaw_decode_silence() {
        // mu-law silence is typically 0xFF or 0x7F
        let sample = ulaw_decode(0xFF);
        assert!(sample.abs() < 10, "Silence should decode to near-zero");
    }

    #[test]
    fn test_decoder_basic() {
        let mut decoder = G711UlawDecoder::new();
        let input = [0xFF, 0x7F, 0x00, 0x80]; // Various test values
        let mut output = [0.0f32; 48]; // 4 input bytes * 12 (6x upsample * 2 stereo) = 48 samples

        let samples = decoder.decode(&input, &mut output).unwrap();
        assert_eq!(samples, 48); // 4 input * 6x upsampling * 2 channels

        // Check outputs are in valid range
        for sample in &output {
            assert!(*sample >= -1.0 && *sample <= 1.0);
        }
    }

    #[test]
    fn test_decoder_frame_size() {
        let decoder = G711UlawDecoder::new();
        assert_eq!(decoder.frame_size(), 960); // 160 * 6x upsampling
        assert_eq!(decoder.total_samples_per_frame(), 1920); // 960 * 2 stereo channels
    }

    #[test]
    fn test_ulaw_encode_decode_roundtrip() {
        // Test that encode/decode roundtrip is reasonably accurate
        let test_values: [i16; 8] = [0, 100, 1000, 10000, -100, -1000, -10000, 32000];
        for &original in &test_values {
            let encoded = ulaw_encode(original);
            let decoded = ulaw_decode(encoded);
            // G.711 is lossy, but should be within ~3% for most values
            let diff = (original - decoded).abs();
            let tolerance = (original.abs() / 20).max(100); // 5% or at least 100
            assert!(
                diff <= tolerance,
                "Roundtrip error too large: {} -> {} -> {}, diff={}",
                original,
                encoded,
                decoded,
                diff
            );
        }
    }

    #[test]
    fn test_encoder_basic() {
        let mut encoder = G711UlawEncoder::new();

        // 20ms at 48kHz stereo = 1920 samples (960 stereo pairs)
        // After 6:1 downsample = 160 output bytes
        let input = vec![0.0f32; 1920];
        let mut output = [0u8; 256];

        let bytes = encoder.encode(&input, &mut output).unwrap();
        assert_eq!(bytes, 160); // 960 pairs / 6 = 160 bytes
    }

    #[test]
    fn test_encoder_frame_size() {
        let encoder = G711UlawEncoder::new();
        assert_eq!(encoder.frame_size(), 960);
        assert_eq!(encoder.total_samples_per_frame(), 1920);
        assert_eq!(encoder.payload_type(), 0);
    }

    #[test]
    fn test_encoder_output_range() {
        let mut encoder = G711UlawEncoder::new();

        // Create a sine wave input
        let mut input = vec![0.0f32; 1920];
        for i in 0..960 {
            let sample = (i as f32 * 0.1).sin() * 0.5;
            input[i * 2] = sample;     // Left
            input[i * 2 + 1] = sample; // Right
        }

        let mut output = [0u8; 256];
        let bytes = encoder.encode(&input, &mut output).unwrap();

        // All output bytes should be valid (0-255)
        assert!(bytes > 0);
        // Check we got reasonable output (not all zeros or all same value)
        let unique_values: std::collections::HashSet<u8> = output[..bytes].iter().copied().collect();
        assert!(unique_values.len() > 1, "Output should have variation");
    }
}
