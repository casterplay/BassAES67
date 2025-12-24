//! Audio codec support for bass-rtp.
//!
//! Supported codecs:
//! - PCM 16-bit: 16-bit signed big-endian (network byte order)
//! - PCM 24-bit: 24-bit signed big-endian (network byte order)
//! - MP2: MPEG Audio Layer 2 broadcast standard (libtwolame/libmpg123)
//! - OPUS: Low-latency audio codec (libopus)
//! - FLAC: Free Lossless Audio Codec (libFLAC)

pub mod pcm;
pub mod opus;
pub mod twolame;
pub mod mpg123;
pub mod flac;

pub use pcm::*;

/// Common codec error type
#[derive(Debug)]
pub enum CodecError {
    /// Encoder/decoder not initialized
    NotInitialized,
    /// Invalid input data
    InvalidInput,
    /// Buffer too small
    BufferTooSmall,
    /// Codec library error with error code
    LibraryError(i32),
    /// Other error with message
    Other(String),
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodecError::NotInitialized => write!(f, "Codec not initialized"),
            CodecError::InvalidInput => write!(f, "Invalid input data"),
            CodecError::BufferTooSmall => write!(f, "Output buffer too small"),
            CodecError::LibraryError(code) => write!(f, "Codec library error: {}", code),
            CodecError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for CodecError {}

/// Audio format parameters
#[derive(Debug, Clone, Copy)]
pub struct AudioFormat {
    /// Sample rate in Hz (e.g., 48000)
    pub sample_rate: u32,
    /// Number of channels (1 = mono, 2 = stereo)
    pub channels: u8,
}

impl AudioFormat {
    pub fn new(sample_rate: u32, channels: u8) -> Self {
        Self { sample_rate, channels }
    }

    /// Standard format: 48kHz stereo
    pub fn standard() -> Self {
        Self::new(48000, 2)
    }

    /// Samples per millisecond (per channel)
    pub fn samples_per_ms(&self) -> usize {
        (self.sample_rate as usize) / 1000
    }

    /// Total samples per frame for given duration (samples * channels)
    pub fn total_samples_per_frame(&self, duration_ms: usize) -> usize {
        self.samples_per_ms() * duration_ms * self.channels as usize
    }

    /// Samples per channel per frame for given duration
    pub fn samples_per_channel(&self, duration_ms: usize) -> usize {
        self.samples_per_ms() * duration_ms
    }
}

impl Default for AudioFormat {
    fn default() -> Self {
        Self::standard()
    }
}

/// Audio encoder trait
pub trait AudioEncoder: Send {
    /// Encode float PCM samples to output buffer.
    ///
    /// # Arguments
    /// * `pcm` - Input float PCM samples (interleaved if stereo)
    /// * `output` - Output buffer for encoded data
    ///
    /// # Returns
    /// Number of bytes written, or error.
    fn encode(&mut self, pcm: &[f32], output: &mut [u8]) -> Result<usize, CodecError>;

    /// Get frame size in samples per channel.
    fn frame_size(&self) -> usize;

    /// Get total samples per frame (frame_size * channels).
    fn total_samples_per_frame(&self) -> usize;

    /// Get the RTP payload type for this encoder.
    fn payload_type(&self) -> u8;
}

/// Audio decoder trait
pub trait AudioDecoder: Send {
    /// Decode encoded data to float PCM samples.
    ///
    /// # Arguments
    /// * `data` - Encoded input data
    /// * `output` - Output buffer for decoded float samples
    ///
    /// # Returns
    /// Number of samples written (total, including all channels), or error.
    fn decode(&mut self, data: &[u8], output: &mut [f32]) -> Result<usize, CodecError>;

    /// Get expected frame size in samples per channel.
    fn frame_size(&self) -> usize;

    /// Get total samples per frame (frame_size * channels).
    fn total_samples_per_frame(&self) -> usize;
}
