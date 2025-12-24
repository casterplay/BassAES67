//! PCM codec implementations for RTP.
//!
//! Supports PCM 16-bit, 20-bit, and 24-bit in big-endian (network byte order).

use super::{AudioDecoder, AudioEncoder, AudioFormat, CodecError};
use crate::rtp::PayloadCodec;

// ============================================================================
// PCM 16-bit Codec
// ============================================================================

/// PCM 16-bit encoder - converts float32 to 16-bit signed big-endian.
pub struct Pcm16Encoder {
    channels: usize,
    frame_size: usize,
    payload_type: u8,
}

impl Pcm16Encoder {
    /// Create a new PCM-16 encoder.
    ///
    /// # Arguments
    /// * `format` - Audio format (sample rate, channels)
    /// * `frame_duration_ms` - Frame duration in milliseconds (typically 1-5ms)
    pub fn new(format: AudioFormat, frame_duration_ms: usize) -> Self {
        let frame_size = format.samples_per_channel(frame_duration_ms);
        Self {
            channels: format.channels as usize,
            frame_size,
            payload_type: PayloadCodec::Pcm16.to_pt(),
        }
    }
}

impl AudioEncoder for Pcm16Encoder {
    fn encode(&mut self, pcm: &[f32], output: &mut [u8]) -> Result<usize, CodecError> {
        let expected_samples = self.total_samples_per_frame();
        if pcm.len() < expected_samples {
            return Err(CodecError::InvalidInput);
        }

        let bytes_needed = expected_samples * 2; // 2 bytes per sample
        if output.len() < bytes_needed {
            return Err(CodecError::BufferTooSmall);
        }

        // Convert float to i16 big-endian (network byte order)
        for (i, &sample) in pcm.iter().take(expected_samples).enumerate() {
            let clamped = sample.clamp(-1.0, 1.0);
            let sample_i16 = (clamped * 32767.0) as i16;
            let bytes = sample_i16.to_be_bytes(); // Big-endian for network
            output[i * 2] = bytes[0];
            output[i * 2 + 1] = bytes[1];
        }

        Ok(bytes_needed)
    }

    fn frame_size(&self) -> usize {
        self.frame_size
    }

    fn total_samples_per_frame(&self) -> usize {
        self.frame_size * self.channels
    }

    fn payload_type(&self) -> u8 {
        self.payload_type
    }
}

/// PCM 16-bit decoder - converts 16-bit signed big-endian to float32.
pub struct Pcm16Decoder {
    channels: usize,
    frame_size: usize,
}

impl Pcm16Decoder {
    /// Create a new PCM-16 decoder.
    pub fn new(format: AudioFormat, frame_duration_ms: usize) -> Self {
        let frame_size = format.samples_per_channel(frame_duration_ms);
        Self {
            channels: format.channels as usize,
            frame_size,
        }
    }

    /// Create decoder that auto-detects frame size from packet.
    pub fn new_auto(channels: u8) -> Self {
        Self {
            channels: channels as usize,
            frame_size: 0, // Will be determined by packet size
        }
    }
}

impl AudioDecoder for Pcm16Decoder {
    fn decode(&mut self, data: &[u8], output: &mut [f32]) -> Result<usize, CodecError> {
        if data.len() < 2 {
            return Err(CodecError::InvalidInput);
        }

        // Calculate sample count from data size
        let sample_count = data.len() / 2;
        if output.len() < sample_count {
            return Err(CodecError::BufferTooSmall);
        }

        // Convert i16 big-endian to float
        const SCALE: f32 = 1.0 / 32768.0;

        for i in 0..sample_count {
            let bytes = [data[i * 2], data[i * 2 + 1]];
            let sample_i16 = i16::from_be_bytes(bytes);
            output[i] = sample_i16 as f32 * SCALE;
        }

        Ok(sample_count)
    }

    fn frame_size(&self) -> usize {
        self.frame_size
    }

    fn total_samples_per_frame(&self) -> usize {
        self.frame_size * self.channels
    }
}

// ============================================================================
// PCM 20-bit Codec
// ============================================================================

/// PCM 20-bit encoder - converts float32 to 20-bit signed big-endian (packed in 3 bytes).
///
/// Format: 20-bit audio packed in 3 bytes, with the 4 LSBs of the last byte unused (zero).
/// Used by Z/IP ONE with PT 116.
pub struct Pcm20Encoder {
    channels: usize,
    frame_size: usize,
    payload_type: u8,
}

impl Pcm20Encoder {
    /// Create a new PCM-20 encoder.
    pub fn new(format: AudioFormat, frame_duration_ms: usize) -> Self {
        let frame_size = format.samples_per_channel(frame_duration_ms);
        Self {
            channels: format.channels as usize,
            frame_size,
            payload_type: PayloadCodec::Pcm20.to_pt(),
        }
    }
}

impl AudioEncoder for Pcm20Encoder {
    fn encode(&mut self, pcm: &[f32], output: &mut [u8]) -> Result<usize, CodecError> {
        let expected_samples = self.total_samples_per_frame();
        if pcm.len() < expected_samples {
            return Err(CodecError::InvalidInput);
        }

        // Z/IP ONE uses packed 20-bit format: 2 samples in 5 bytes (40 bits)
        let sample_pairs = expected_samples / 2;
        let bytes_needed = sample_pairs * 5;
        if output.len() < bytes_needed {
            return Err(CodecError::BufferTooSmall);
        }

        // Scale factor: 2^19 - 1 = 524287
        const SCALE: f32 = 524287.0;

        for i in 0..sample_pairs {
            // Convert two samples to 20-bit integers
            let s1 = (pcm[i * 2].clamp(-1.0, 1.0) * SCALE) as i32;
            let s2 = (pcm[i * 2 + 1].clamp(-1.0, 1.0) * SCALE) as i32;

            let s1_clamped = (s1.clamp(-524288, 524287) as u32) & 0xFFFFF;
            let s2_clamped = (s2.clamp(-524288, 524287) as u32) & 0xFFFFF;

            // Pack 2 samples into 5 bytes:
            // Byte 0: S1[19:12]
            // Byte 1: S1[11:4]
            // Byte 2: S1[3:0] | S2[19:16]
            // Byte 3: S2[15:8]
            // Byte 4: S2[7:0]
            output[i * 5] = ((s1_clamped >> 12) & 0xFF) as u8;
            output[i * 5 + 1] = ((s1_clamped >> 4) & 0xFF) as u8;
            output[i * 5 + 2] = (((s1_clamped & 0x0F) << 4) | ((s2_clamped >> 16) & 0x0F)) as u8;
            output[i * 5 + 3] = ((s2_clamped >> 8) & 0xFF) as u8;
            output[i * 5 + 4] = (s2_clamped & 0xFF) as u8;
        }

        Ok(bytes_needed)
    }

    fn frame_size(&self) -> usize {
        self.frame_size
    }

    fn total_samples_per_frame(&self) -> usize {
        self.frame_size * self.channels
    }

    fn payload_type(&self) -> u8 {
        self.payload_type
    }
}

/// PCM 20-bit decoder - converts 20-bit signed big-endian to float32.
///
/// Format: 20-bit audio packed in 3 bytes, with the 4 LSBs unused.
pub struct Pcm20Decoder {
    channels: usize,
    frame_size: usize,
}

impl Pcm20Decoder {
    /// Create a new PCM-20 decoder.
    pub fn new(format: AudioFormat, frame_duration_ms: usize) -> Self {
        let frame_size = format.samples_per_channel(frame_duration_ms);
        Self {
            channels: format.channels as usize,
            frame_size,
        }
    }

    /// Create decoder that auto-detects frame size from packet.
    pub fn new_auto(channels: u8) -> Self {
        Self {
            channels: channels as usize,
            frame_size: 0, // Will be determined by packet size
        }
    }
}

impl AudioDecoder for Pcm20Decoder {
    fn decode(&mut self, data: &[u8], output: &mut [f32]) -> Result<usize, CodecError> {
        if data.len() < 5 {
            return Err(CodecError::InvalidInput);
        }

        // Z/IP ONE uses packed 20-bit format: 2 samples in 5 bytes (40 bits)
        // Layout: [S1: 20 bits][S2: 20 bits] = 5 bytes
        // Byte 0: S1[19:12]
        // Byte 1: S1[11:4]
        // Byte 2: S1[3:0] | S2[19:16]
        // Byte 3: S2[15:8]
        // Byte 4: S2[7:0]

        let sample_pairs = data.len() / 5;
        let sample_count = sample_pairs * 2;

        if output.len() < sample_count {
            return Err(CodecError::BufferTooSmall);
        }

        // Normalization: 1.0 / 2^19 (20-bit range)
        const SCALE: f32 = 1.0 / 524288.0;

        for i in 0..sample_pairs {
            let b0 = data[i * 5] as u32;
            let b1 = data[i * 5 + 1] as u32;
            let b2 = data[i * 5 + 2] as u32;
            let b3 = data[i * 5 + 3] as u32;
            let b4 = data[i * 5 + 4] as u32;

            // Sample 1: bits from b0, b1, and upper nibble of b2
            let s1_raw = (b0 << 12) | (b1 << 4) | (b2 >> 4);

            // Sample 2: lower nibble of b2, b3, b4
            let s2_raw = ((b2 & 0x0F) << 16) | (b3 << 8) | b4;

            // Sign extend from 20 bits to i32
            let s1 = if s1_raw & 0x80000 != 0 {
                (s1_raw | 0xFFF00000) as i32
            } else {
                s1_raw as i32
            };

            let s2 = if s2_raw & 0x80000 != 0 {
                (s2_raw | 0xFFF00000) as i32
            } else {
                s2_raw as i32
            };

            output[i * 2] = s1 as f32 * SCALE;
            output[i * 2 + 1] = s2 as f32 * SCALE;
        }

        Ok(sample_count)
    }

    fn frame_size(&self) -> usize {
        self.frame_size
    }

    fn total_samples_per_frame(&self) -> usize {
        self.frame_size * self.channels
    }
}

// ============================================================================
// PCM 24-bit Codec
// ============================================================================

/// PCM 24-bit encoder - converts float32 to 24-bit signed big-endian.
pub struct Pcm24Encoder {
    channels: usize,
    frame_size: usize,
    payload_type: u8,
}

impl Pcm24Encoder {
    /// Create a new PCM-24 encoder.
    pub fn new(format: AudioFormat, frame_duration_ms: usize) -> Self {
        let frame_size = format.samples_per_channel(frame_duration_ms);
        Self {
            channels: format.channels as usize,
            frame_size,
            payload_type: PayloadCodec::Pcm24.to_pt(),
        }
    }
}

impl AudioEncoder for Pcm24Encoder {
    fn encode(&mut self, pcm: &[f32], output: &mut [u8]) -> Result<usize, CodecError> {
        let expected_samples = self.total_samples_per_frame();
        if pcm.len() < expected_samples {
            return Err(CodecError::InvalidInput);
        }

        let bytes_needed = expected_samples * 3; // 3 bytes per sample
        if output.len() < bytes_needed {
            return Err(CodecError::BufferTooSmall);
        }

        // Convert float to i24 big-endian (network byte order)
        // Scale factor: 2^23 - 1 = 8388607
        const SCALE: f32 = 8388607.0;

        for (i, &sample) in pcm.iter().take(expected_samples).enumerate() {
            let clamped = sample.clamp(-1.0, 1.0);
            let sample_i32 = (clamped * SCALE) as i32;
            let sample_clamped = sample_i32.clamp(-8388608, 8388607);

            // Extract 24-bit value as big-endian (3 bytes, MSB first)
            output[i * 3] = ((sample_clamped >> 16) & 0xFF) as u8;
            output[i * 3 + 1] = ((sample_clamped >> 8) & 0xFF) as u8;
            output[i * 3 + 2] = (sample_clamped & 0xFF) as u8;
        }

        Ok(bytes_needed)
    }

    fn frame_size(&self) -> usize {
        self.frame_size
    }

    fn total_samples_per_frame(&self) -> usize {
        self.frame_size * self.channels
    }

    fn payload_type(&self) -> u8 {
        self.payload_type
    }
}

/// PCM 24-bit decoder - converts 24-bit signed big-endian to float32.
pub struct Pcm24Decoder {
    channels: usize,
    frame_size: usize,
}

impl Pcm24Decoder {
    /// Create a new PCM-24 decoder.
    pub fn new(format: AudioFormat, frame_duration_ms: usize) -> Self {
        let frame_size = format.samples_per_channel(frame_duration_ms);
        Self {
            channels: format.channels as usize,
            frame_size,
        }
    }

    /// Create decoder that auto-detects frame size from packet.
    pub fn new_auto(channels: u8) -> Self {
        Self {
            channels: channels as usize,
            frame_size: 0, // Will be determined by packet size
        }
    }
}

impl AudioDecoder for Pcm24Decoder {
    fn decode(&mut self, data: &[u8], output: &mut [f32]) -> Result<usize, CodecError> {
        if data.len() < 3 {
            return Err(CodecError::InvalidInput);
        }

        // Calculate sample count from data size
        let sample_count = data.len() / 3;
        if output.len() < sample_count {
            return Err(CodecError::BufferTooSmall);
        }

        // Convert i24 big-endian to float
        // Normalization: 1.0 / 2^23
        const SCALE: f32 = 1.0 / 8388608.0;

        for i in 0..sample_count {
            let b0 = data[i * 3] as i32;     // MSB
            let b1 = data[i * 3 + 1] as i32;
            let b2 = data[i * 3 + 2] as i32; // LSB

            // Reconstruct 24-bit signed value with sign extension
            let mut sample_i32 = (b0 << 16) | (b1 << 8) | b2;

            // Sign extend from 24 bits to 32 bits
            if sample_i32 & 0x800000 != 0 {
                sample_i32 |= 0xFF000000u32 as i32;
            }

            output[i] = sample_i32 as f32 * SCALE;
        }

        Ok(sample_count)
    }

    fn frame_size(&self) -> usize {
        self.frame_size
    }

    fn total_samples_per_frame(&self) -> usize {
        self.frame_size * self.channels
    }
}

// ============================================================================
// Conversion Functions (for direct use without encoder/decoder structs)
// ============================================================================

/// Convert float32 samples to 16-bit big-endian PCM.
pub fn convert_float_to_16bit_be(input: &[f32], output: &mut [u8]) {
    for (i, &sample) in input.iter().enumerate() {
        if i * 2 + 1 >= output.len() {
            break;
        }
        let clamped = sample.clamp(-1.0, 1.0);
        let sample_i16 = (clamped * 32767.0) as i16;
        let bytes = sample_i16.to_be_bytes();
        output[i * 2] = bytes[0];
        output[i * 2 + 1] = bytes[1];
    }
}

/// Convert 16-bit big-endian PCM to float32 samples.
pub fn convert_16bit_be_to_float(input: &[u8], output: &mut [f32]) {
    const SCALE: f32 = 1.0 / 32768.0;
    let sample_count = input.len() / 2;

    for i in 0..sample_count.min(output.len()) {
        let bytes = [input[i * 2], input[i * 2 + 1]];
        let sample_i16 = i16::from_be_bytes(bytes);
        output[i] = sample_i16 as f32 * SCALE;
    }
}

/// Convert float32 samples to 24-bit big-endian PCM.
pub fn convert_float_to_24bit_be(input: &[f32], output: &mut [u8]) {
    const SCALE: f32 = 8388607.0;

    for (i, &sample) in input.iter().enumerate() {
        if i * 3 + 2 >= output.len() {
            break;
        }
        let clamped = sample.clamp(-1.0, 1.0);
        let sample_i32 = (clamped * SCALE) as i32;
        let sample_clamped = sample_i32.clamp(-8388608, 8388607);

        output[i * 3] = ((sample_clamped >> 16) & 0xFF) as u8;
        output[i * 3 + 1] = ((sample_clamped >> 8) & 0xFF) as u8;
        output[i * 3 + 2] = (sample_clamped & 0xFF) as u8;
    }
}

/// Convert 24-bit big-endian PCM to float32 samples.
pub fn convert_24bit_be_to_float(input: &[u8], output: &mut [f32]) {
    const SCALE: f32 = 1.0 / 8388608.0;
    let sample_count = input.len() / 3;

    for i in 0..sample_count.min(output.len()) {
        let b0 = input[i * 3] as i32;
        let b1 = input[i * 3 + 1] as i32;
        let b2 = input[i * 3 + 2] as i32;

        let mut sample_i32 = (b0 << 16) | (b1 << 8) | b2;

        if sample_i32 & 0x800000 != 0 {
            sample_i32 |= 0xFF000000u32 as i32;
        }

        output[i] = sample_i32 as f32 * SCALE;
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pcm16_roundtrip() {
        // Use 8kHz mono to get 8 samples per 1ms frame (matches our test input size)
        let format = AudioFormat::new(8000, 1);
        let mut encoder = Pcm16Encoder::new(format, 1); // 1ms = 8 samples * 1 channel = 8 samples
        let mut decoder = Pcm16Decoder::new(format, 1);

        // Test samples (8 samples for 1ms at 8kHz mono)
        let input = vec![0.0f32, 0.5, -0.5, 1.0, -1.0, 0.25, -0.25, 0.0];
        let mut encoded = vec![0u8; input.len() * 2];
        let mut decoded = vec![0.0f32; input.len()];

        // Encode (only first frame)
        let total = encoder.total_samples_per_frame();
        assert_eq!(total, 8, "Expected 8 samples per frame");
        let bytes = encoder.encode(&input[..total], &mut encoded).unwrap();
        assert_eq!(bytes, total * 2);

        // Decode
        let samples = decoder.decode(&encoded[..bytes], &mut decoded).unwrap();
        assert_eq!(samples, total);

        // Check roundtrip accuracy (within 16-bit quantization error)
        for i in 0..total {
            let diff = (input[i] - decoded[i]).abs();
            assert!(diff < 0.0001, "Sample {} mismatch: {} vs {}", i, input[i], decoded[i]);
        }
    }

    #[test]
    fn test_pcm24_roundtrip() {
        // Use 8kHz mono to get 8 samples per 1ms frame
        let format = AudioFormat::new(8000, 1);
        let mut encoder = Pcm24Encoder::new(format, 1);
        let mut decoder = Pcm24Decoder::new(format, 1);

        let input = vec![0.0f32, 0.5, -0.5, 1.0, -1.0, 0.25, -0.25, 0.0];
        let mut encoded = vec![0u8; input.len() * 3];
        let mut decoded = vec![0.0f32; input.len()];

        let total = encoder.total_samples_per_frame();
        assert_eq!(total, 8, "Expected 8 samples per frame");
        let bytes = encoder.encode(&input[..total], &mut encoded).unwrap();
        assert_eq!(bytes, total * 3);

        let samples = decoder.decode(&encoded[..bytes], &mut decoded).unwrap();
        assert_eq!(samples, total);

        // 24-bit should be more accurate
        for i in 0..total {
            let diff = (input[i] - decoded[i]).abs();
            assert!(diff < 0.000001, "Sample {} mismatch: {} vs {}", i, input[i], decoded[i]);
        }
    }

    #[test]
    fn test_conversion_functions() {
        // Test 16-bit
        let input = [0.5f32, -0.5];
        let mut encoded16 = [0u8; 4];
        let mut decoded16 = [0.0f32; 2];

        convert_float_to_16bit_be(&input, &mut encoded16);
        convert_16bit_be_to_float(&encoded16, &mut decoded16);

        for i in 0..2 {
            assert!((input[i] - decoded16[i]).abs() < 0.0001);
        }

        // Test 24-bit
        let mut encoded24 = [0u8; 6];
        let mut decoded24 = [0.0f32; 2];

        convert_float_to_24bit_be(&input, &mut encoded24);
        convert_24bit_be_to_float(&encoded24, &mut decoded24);

        for i in 0..2 {
            assert!((input[i] - decoded24[i]).abs() < 0.000001);
        }
    }
}
