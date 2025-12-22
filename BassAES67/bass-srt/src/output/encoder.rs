//! Audio encoder wrapper for SRT output.
//! Provides unified interface for PCM, OPUS, MP2, and FLAC encoding.

use crate::codec::{opus, twolame, flac, AudioFormat};
use crate::protocol::{FORMAT_PCM_L16, FORMAT_OPUS, FORMAT_MP2, FORMAT_FLAC};

use super::stream::{SrtOutputConfig, OutputCodec};

/// Unified audio encoder trait
pub trait AudioEncoder: Send {
    /// Encode float PCM samples to output buffer.
    ///
    /// # Arguments
    /// * `pcm` - Input float PCM samples (interleaved if stereo)
    /// * `output` - Output buffer for encoded data
    ///
    /// # Returns
    /// Tuple of (encoded_bytes, format_byte) or error.
    fn encode(&mut self, pcm: &[f32], output: &mut [u8]) -> Result<(usize, u8), String>;

    /// Get frame size in samples per channel.
    fn frame_size(&self) -> usize;

    /// Get total samples per frame (frame_size * channels).
    fn total_samples_per_frame(&self) -> usize;
}

// ============================================================================
// PCM Encoder (just converts float to i16 LE)
// ============================================================================

/// PCM encoder - converts float32 to 16-bit signed little-endian
pub struct PcmEncoder {
    channels: usize,
    frame_size: usize,
}

impl PcmEncoder {
    /// Create a new PCM encoder
    pub fn new(config: &SrtOutputConfig) -> Self {
        // 5ms frames
        let frame_size = (config.sample_rate as usize * 5) / 1000;
        Self {
            channels: config.channels as usize,
            frame_size,
        }
    }
}

impl AudioEncoder for PcmEncoder {
    fn encode(&mut self, pcm: &[f32], output: &mut [u8]) -> Result<(usize, u8), String> {
        let expected_samples = self.total_samples_per_frame();
        if pcm.len() < expected_samples {
            return Err(format!(
                "PCM encoder: expected {} samples, got {}",
                expected_samples,
                pcm.len()
            ));
        }

        let bytes_needed = expected_samples * 2; // 2 bytes per sample
        if output.len() < bytes_needed {
            return Err("PCM encoder: output buffer too small".to_string());
        }

        // Convert float to i16 LE
        for (i, &sample) in pcm.iter().take(expected_samples).enumerate() {
            let clamped = sample.clamp(-1.0, 1.0);
            let sample_i16 = (clamped * 32767.0) as i16;
            let bytes = sample_i16.to_le_bytes();
            output[i * 2] = bytes[0];
            output[i * 2 + 1] = bytes[1];
        }

        Ok((bytes_needed, FORMAT_PCM_L16))
    }

    fn frame_size(&self) -> usize {
        self.frame_size
    }

    fn total_samples_per_frame(&self) -> usize {
        self.frame_size * self.channels
    }
}

// ============================================================================
// OPUS Encoder
// ============================================================================

/// OPUS encoder wrapper
pub struct OpusEncoderWrapper {
    encoder: opus::Encoder,
    channels: usize,
}

impl OpusEncoderWrapper {
    /// Create a new OPUS encoder
    pub fn new(config: &SrtOutputConfig) -> Result<Self, String> {
        let format = AudioFormat::new(config.sample_rate, config.channels as u8);

        let mut encoder = opus::Encoder::new(format, 5.0, opus::OPUS_APPLICATION_AUDIO)
            .map_err(|e| format!("OPUS encoder init failed: {}", e))?;

        // Set bitrate
        encoder
            .set_bitrate((config.bitrate_kbps * 1000) as i32)
            .map_err(|e| format!("OPUS set bitrate failed: {}", e))?;

        Ok(Self {
            encoder,
            channels: config.channels as usize,
        })
    }
}

impl AudioEncoder for OpusEncoderWrapper {
    fn encode(&mut self, pcm: &[f32], output: &mut [u8]) -> Result<(usize, u8), String> {
        let expected_samples = self.total_samples_per_frame();
        if pcm.len() < expected_samples {
            return Err(format!(
                "OPUS encoder: expected {} samples, got {}",
                expected_samples,
                pcm.len()
            ));
        }

        let encoded_len = self
            .encoder
            .encode_float(&pcm[..expected_samples], output)
            .map_err(|e| format!("OPUS encode failed: {}", e))?;

        Ok((encoded_len, FORMAT_OPUS))
    }

    fn frame_size(&self) -> usize {
        self.encoder.frame_size()
    }

    fn total_samples_per_frame(&self) -> usize {
        self.encoder.total_samples_per_frame()
    }
}

// ============================================================================
// MP2 Encoder
// ============================================================================

/// MP2 encoder wrapper
pub struct Mp2EncoderWrapper {
    encoder: twolame::Encoder,
    channels: usize,
    /// Accumulation buffer for float samples (MP2 needs 1152 samples)
    float_buffer: Vec<f32>,
}

impl Mp2EncoderWrapper {
    /// Create a new MP2 encoder
    pub fn new(config: &SrtOutputConfig) -> Result<Self, String> {
        let format = AudioFormat::new(config.sample_rate, config.channels as u8);

        let encoder = twolame::Encoder::new(format, config.bitrate_kbps)
            .map_err(|e| format!("MP2 encoder init failed: {}", e))?;

        Ok(Self {
            encoder,
            channels: config.channels as usize,
            float_buffer: Vec::with_capacity(twolame::SAMPLES_PER_FRAME * config.channels as usize),
        })
    }
}

impl AudioEncoder for Mp2EncoderWrapper {
    fn encode(&mut self, pcm: &[f32], output: &mut [u8]) -> Result<(usize, u8), String> {
        // Accumulate samples
        self.float_buffer.extend_from_slice(pcm);

        let samples_per_frame = self.total_samples_per_frame();
        if self.float_buffer.len() < samples_per_frame {
            // Not enough samples yet
            return Ok((0, FORMAT_MP2));
        }

        // Extract one frame worth of samples
        let frame_samples: Vec<f32> = self.float_buffer.drain(..samples_per_frame).collect();

        // Encode
        let encoded_len = self
            .encoder
            .encode_float(&frame_samples, output)
            .map_err(|e| format!("MP2 encode failed: {}", e))?;

        Ok((encoded_len, FORMAT_MP2))
    }

    fn frame_size(&self) -> usize {
        twolame::SAMPLES_PER_FRAME
    }

    fn total_samples_per_frame(&self) -> usize {
        twolame::SAMPLES_PER_FRAME * self.channels
    }
}

// ============================================================================
// FLAC Encoder
// ============================================================================

/// FLAC encoder wrapper
pub struct FlacEncoderWrapper {
    encoder: flac::Encoder,
    channels: usize,
    /// Accumulation buffer for samples
    sample_buffer: Vec<i16>,
}

impl FlacEncoderWrapper {
    /// Create a new FLAC encoder
    pub fn new(config: &SrtOutputConfig) -> Result<Self, String> {
        let format = AudioFormat::new(config.sample_rate, config.channels as u8);

        let encoder = flac::Encoder::new(format, config.flac_level)
            .map_err(|e| format!("FLAC encoder init failed: {}", e))?;

        Ok(Self {
            encoder,
            channels: config.channels as usize,
            sample_buffer: Vec::with_capacity(flac::DEFAULT_FRAME_SIZE * config.channels as usize),
        })
    }
}

impl AudioEncoder for FlacEncoderWrapper {
    fn encode(&mut self, pcm: &[f32], output: &mut [u8]) -> Result<(usize, u8), String> {
        // Convert float to i16 and accumulate
        for &sample in pcm {
            let clamped = sample.clamp(-1.0, 1.0);
            let sample_i16 = (clamped * 32767.0) as i16;
            self.sample_buffer.push(sample_i16);
        }

        let samples_per_frame = self.total_samples_per_frame();
        if self.sample_buffer.len() < samples_per_frame {
            // Not enough samples yet
            return Ok((0, FORMAT_FLAC));
        }

        // Extract one frame worth of samples
        let frame_samples: Vec<i16> = self.sample_buffer.drain(..samples_per_frame).collect();

        // Encode
        let encoded_len = self
            .encoder
            .encode(&frame_samples, output)
            .map_err(|e| format!("FLAC encode failed: {}", e))?;

        Ok((encoded_len, FORMAT_FLAC))
    }

    fn frame_size(&self) -> usize {
        flac::DEFAULT_FRAME_SIZE
    }

    fn total_samples_per_frame(&self) -> usize {
        flac::DEFAULT_FRAME_SIZE * self.channels
    }
}

// ============================================================================
// Factory function
// ============================================================================

/// Create an encoder based on configuration
pub fn create_encoder(config: &SrtOutputConfig) -> Result<Box<dyn AudioEncoder>, String> {
    match config.codec {
        OutputCodec::Pcm => Ok(Box::new(PcmEncoder::new(config))),
        OutputCodec::Opus => Ok(Box::new(OpusEncoderWrapper::new(config)?)),
        OutputCodec::Mp2 => Ok(Box::new(Mp2EncoderWrapper::new(config)?)),
        OutputCodec::Flac => Ok(Box::new(FlacEncoderWrapper::new(config)?)),
    }
}
