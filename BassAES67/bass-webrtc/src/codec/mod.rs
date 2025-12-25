//! Audio codec support for bass-webrtc.
//!
//! Supported codecs:
//! - OPUS: Low-latency audio codec (libopus) - required for WebRTC

pub mod opus;

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
    /// Decode error with message
    DecodeError(String),
    /// Encode error with message
    EncodeError(String),
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
            CodecError::DecodeError(msg) => write!(f, "Decode error: {}", msg),
            CodecError::EncodeError(msg) => write!(f, "Encode error: {}", msg),
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

    /// Standard format: 48kHz stereo (required for WebRTC)
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
