//! OPUS encoder bindings for bass_opus_web.
//!
//! Opus is a low-latency audio codec ideal for real-time streaming.
//! Uses 5ms frame size at 48kHz for lowest latency.

use std::ffi::c_int;

use super::{AudioFormat, CodecError};

// Opaque encoder type
#[repr(C)]
pub struct OpusEncoder {
    _private: [u8; 0],
}

// Error codes
pub const OPUS_OK: c_int = 0;

// Application modes
pub const OPUS_APPLICATION_AUDIO: c_int = 2049;

// CTL requests
pub const OPUS_SET_BITRATE_REQUEST: c_int = 4002;
pub const OPUS_SET_COMPLEXITY_REQUEST: c_int = 4010;

#[link(name = "opus")]
extern "C" {
    fn opus_encoder_create(
        fs: i32,
        channels: c_int,
        application: c_int,
        error: *mut c_int,
    ) -> *mut OpusEncoder;
    fn opus_encoder_destroy(st: *mut OpusEncoder);
    fn opus_encode_float(
        st: *mut OpusEncoder,
        pcm: *const f32,
        frame_size: c_int,
        data: *mut u8,
        max_data_bytes: i32,
    ) -> i32;
    fn opus_encoder_ctl(st: *mut OpusEncoder, request: c_int, ...) -> c_int;
    fn opus_strerror(error: c_int) -> *const i8;
}

/// Get error message for an Opus error code.
pub fn error_string(error: c_int) -> String {
    unsafe {
        let ptr = opus_strerror(error);
        if ptr.is_null() {
            format!("Unknown OPUS error {}", error)
        } else {
            std::ffi::CStr::from_ptr(ptr)
                .to_string_lossy()
                .into_owned()
        }
    }
}

/// Opus Encoder wrapper.
pub struct Encoder {
    encoder: *mut OpusEncoder,
    format: AudioFormat,
    frame_size: usize,
}

// SAFETY: OpusEncoder is internally synchronized
unsafe impl Send for Encoder {}

impl Encoder {
    /// Create a new Opus encoder.
    ///
    /// # Arguments
    /// * `format` - Audio format (sample rate and channels)
    /// * `frame_duration_ms` - Frame duration in milliseconds (2.5, 5, 10, 20, 40, or 60)
    /// * `application` - Application type (OPUS_APPLICATION_AUDIO recommended)
    pub fn new(
        format: AudioFormat,
        frame_duration_ms: f32,
        application: c_int,
    ) -> Result<Self, CodecError> {
        // Validate sample rate (Opus only supports specific rates)
        let valid_rates = [8000, 12000, 16000, 24000, 48000];
        if !valid_rates.contains(&format.sample_rate) {
            return Err(CodecError::Other(format!(
                "OPUS requires sample rate of {:?}, got {}",
                valid_rates, format.sample_rate
            )));
        }

        // Validate channels (1 or 2)
        if format.channels < 1 || format.channels > 2 {
            return Err(CodecError::Other(format!(
                "OPUS requires 1 or 2 channels, got {}",
                format.channels
            )));
        }

        // Calculate frame size in samples per channel
        let frame_size = ((format.sample_rate as f32 * frame_duration_ms) / 1000.0) as usize;

        // Valid frame sizes for Opus
        let valid_frame_sizes = [
            format.sample_rate / 400,      // 2.5ms
            format.sample_rate / 200,      // 5ms
            format.sample_rate / 100,      // 10ms
            format.sample_rate / 50,       // 20ms
            format.sample_rate / 25,       // 40ms
            (format.sample_rate * 3) / 50, // 60ms
        ];

        // Snap to nearest valid frame size
        let frame_size = valid_frame_sizes
            .iter()
            .copied()
            .min_by_key(|&s| ((s as i32) - (frame_size as i32)).unsigned_abs())
            .unwrap() as usize;

        unsafe {
            let mut error: c_int = 0;
            let encoder = opus_encoder_create(
                format.sample_rate as i32,
                format.channels as c_int,
                application,
                &mut error,
            );

            if error != OPUS_OK || encoder.is_null() {
                return Err(CodecError::LibraryError(error));
            }

            Ok(Self {
                encoder,
                format,
                frame_size,
            })
        }
    }

    /// Create an encoder for audio at 48kHz stereo with 5ms frames.
    pub fn new_audio_48k_stereo_5ms() -> Result<Self, CodecError> {
        Self::new(AudioFormat::standard(), 5.0, OPUS_APPLICATION_AUDIO)
    }

    /// Set the target bitrate in bits per second.
    pub fn set_bitrate(&mut self, bitrate: i32) -> Result<(), CodecError> {
        unsafe {
            let result = opus_encoder_ctl(self.encoder, OPUS_SET_BITRATE_REQUEST, bitrate);
            if result != OPUS_OK {
                Err(CodecError::LibraryError(result))
            } else {
                Ok(())
            }
        }
    }

    /// Set encoder complexity (0-10, higher = better quality, more CPU).
    pub fn set_complexity(&mut self, complexity: c_int) -> Result<(), CodecError> {
        unsafe {
            let result = opus_encoder_ctl(self.encoder, OPUS_SET_COMPLEXITY_REQUEST, complexity);
            if result != OPUS_OK {
                Err(CodecError::LibraryError(result))
            } else {
                Ok(())
            }
        }
    }

    /// Get the frame size in samples per channel.
    pub fn frame_size(&self) -> usize {
        self.frame_size
    }

    /// Get total samples per frame (frame_size * channels).
    pub fn total_samples_per_frame(&self) -> usize {
        self.frame_size * self.format.channels as usize
    }

    /// Encode float PCM samples to Opus.
    ///
    /// # Arguments
    /// * `pcm` - Input PCM samples (interleaved if stereo). Must be exactly frame_size * channels samples.
    /// * `output` - Output buffer for encoded data. Should be at least 4000 bytes.
    ///
    /// # Returns
    /// Number of bytes written to output, or error.
    pub fn encode_float(&mut self, pcm: &[f32], output: &mut [u8]) -> Result<usize, CodecError> {
        let expected_samples = self.total_samples_per_frame();
        if pcm.len() != expected_samples {
            return Err(CodecError::Other(format!(
                "Expected {} samples, got {}",
                expected_samples,
                pcm.len()
            )));
        }

        unsafe {
            let result = opus_encode_float(
                self.encoder,
                pcm.as_ptr(),
                self.frame_size as c_int,
                output.as_mut_ptr(),
                output.len() as i32,
            );

            if result < 0 {
                Err(CodecError::LibraryError(result))
            } else {
                Ok(result as usize)
            }
        }
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        unsafe {
            opus_encoder_destroy(self.encoder);
        }
    }
}
