//! Audio codec support for bass_srt.
//!
//! Supported codecs:
//! - PCM L16: Raw 16-bit signed little-endian (no encoding needed)
//! - OPUS: Low-latency audio codec (libopus)
//! - MP2: MPEG Audio Layer 2 broadcast standard (libtwolame/libmpg123)

pub mod opus;
pub mod twolame;
pub mod mpg123;

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

    /// Samples per millisecond
    pub fn samples_per_ms(&self) -> usize {
        (self.sample_rate as usize) / 1000
    }

    /// Samples per frame (for a given duration in ms)
    pub fn samples_per_frame(&self, duration_ms: usize) -> usize {
        self.samples_per_ms() * duration_ms * self.channels as usize
    }

    /// Bytes per frame for L16 (2 bytes per sample)
    pub fn l16_bytes_per_frame(&self, duration_ms: usize) -> usize {
        self.samples_per_frame(duration_ms) * 2
    }
}

impl Default for AudioFormat {
    fn default() -> Self {
        Self::standard()
    }
}
