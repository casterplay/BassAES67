//! Opus codec support for bass_opus_web.

pub mod opus;

/// Common codec error type
#[derive(Debug)]
pub enum CodecError {
    /// Encoder not initialized
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
    /// Sample rate in Hz (48000 for Opus)
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
}

impl Default for AudioFormat {
    fn default() -> Self {
        Self::standard()
    }
}
