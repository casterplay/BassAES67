//! OPUS codec bindings for bass-webrtc.
//!
//! OPUS is the mandatory audio codec for WebRTC.
//! Supports 2.5ms to 60ms frame sizes at 48kHz.

#![allow(dead_code)]

use std::ffi::c_int;
use std::ptr;

use super::{AudioFormat, CodecError};

// Opaque encoder/decoder types
#[repr(C)]
pub struct OpusEncoder {
    _private: [u8; 0],
}

#[repr(C)]
pub struct OpusDecoder {
    _private: [u8; 0],
}

// Error codes
pub const OPUS_OK: c_int = 0;
pub const OPUS_BAD_ARG: c_int = -1;
pub const OPUS_BUFFER_TOO_SMALL: c_int = -2;
pub const OPUS_INTERNAL_ERROR: c_int = -3;
pub const OPUS_INVALID_PACKET: c_int = -4;
pub const OPUS_UNIMPLEMENTED: c_int = -5;
pub const OPUS_INVALID_STATE: c_int = -6;
pub const OPUS_ALLOC_FAIL: c_int = -7;

// Application modes
pub const OPUS_APPLICATION_VOIP: c_int = 2048;
pub const OPUS_APPLICATION_AUDIO: c_int = 2049;
pub const OPUS_APPLICATION_RESTRICTED_LOWDELAY: c_int = 2051;

// CTL requests
pub const OPUS_SET_BITRATE_REQUEST: c_int = 4002;
pub const OPUS_GET_BITRATE_REQUEST: c_int = 4003;
pub const OPUS_SET_COMPLEXITY_REQUEST: c_int = 4010;
pub const OPUS_SET_SIGNAL_REQUEST: c_int = 4024;

// Signal types
pub const OPUS_SIGNAL_VOICE: c_int = 3001;
pub const OPUS_SIGNAL_MUSIC: c_int = 3002;

// Special bitrate values
pub const OPUS_AUTO: c_int = -1000;
pub const OPUS_BITRATE_MAX: c_int = -1;

#[link(name = "opus")]
extern "C" {
    // Encoder
    fn opus_encoder_get_size(channels: c_int) -> c_int;
    fn opus_encoder_create(
        fs: i32,
        channels: c_int,
        application: c_int,
        error: *mut c_int,
    ) -> *mut OpusEncoder;
    fn opus_encoder_destroy(st: *mut OpusEncoder);
    fn opus_encode(
        st: *mut OpusEncoder,
        pcm: *const i16,
        frame_size: c_int,
        data: *mut u8,
        max_data_bytes: i32,
    ) -> i32;
    fn opus_encode_float(
        st: *mut OpusEncoder,
        pcm: *const f32,
        frame_size: c_int,
        data: *mut u8,
        max_data_bytes: i32,
    ) -> i32;
    fn opus_encoder_ctl(st: *mut OpusEncoder, request: c_int, ...) -> c_int;

    // Decoder
    fn opus_decoder_get_size(channels: c_int) -> c_int;
    fn opus_decoder_create(fs: i32, channels: c_int, error: *mut c_int) -> *mut OpusDecoder;
    fn opus_decoder_destroy(st: *mut OpusDecoder);
    fn opus_decode(
        st: *mut OpusDecoder,
        data: *const u8,
        len: i32,
        pcm: *mut i16,
        frame_size: c_int,
        decode_fec: c_int,
    ) -> c_int;
    fn opus_decode_float(
        st: *mut OpusDecoder,
        data: *const u8,
        len: i32,
        pcm: *mut f32,
        frame_size: c_int,
        decode_fec: c_int,
    ) -> c_int;

    // Error string
    fn opus_strerror(error: c_int) -> *const i8;
}

/// Get error message for an OPUS error code
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

/// OPUS Encoder wrapper
pub struct Encoder {
    encoder: *mut OpusEncoder,
    format: AudioFormat,
    frame_size: usize, // samples per channel per frame
}

// SAFETY: OpusEncoder is internally synchronized
unsafe impl Send for Encoder {}

impl Encoder {
    /// Create a new OPUS encoder.
    ///
    /// # Arguments
    /// * `format` - Audio format (sample rate and channels)
    /// * `frame_duration_ms` - Frame duration in milliseconds (2.5, 5, 10, 20, 40, or 60)
    /// * `application` - Application type (VOIP, AUDIO, or RESTRICTED_LOWDELAY)
    pub fn new(
        format: AudioFormat,
        frame_duration_ms: f32,
        application: c_int,
    ) -> Result<Self, CodecError> {
        // Validate sample rate (OPUS only supports specific rates)
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

        // Valid frame sizes for OPUS
        let valid_frame_sizes = [
            format.sample_rate / 400,  // 2.5ms
            format.sample_rate / 200,  // 5ms
            format.sample_rate / 100,  // 10ms
            format.sample_rate / 50,   // 20ms
            format.sample_rate / 25,   // 40ms
            (format.sample_rate * 3) / 50, // 60ms
        ];

        // Check if frame_size is close to a valid size
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

    /// Create an encoder for audio (music/mixed content) at 48kHz stereo with 20ms frames (WebRTC standard)
    pub fn new_audio_48k_stereo_20ms() -> Result<Self, CodecError> {
        Self::new(AudioFormat::standard(), 20.0, OPUS_APPLICATION_AUDIO)
    }

    /// Create an encoder for VOIP at 48kHz stereo with 20ms frames
    pub fn new_voip_48k_stereo_20ms() -> Result<Self, CodecError> {
        Self::new(AudioFormat::standard(), 20.0, OPUS_APPLICATION_VOIP)
    }

    /// Set the target bitrate in bits per second
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

    /// Set encoder complexity (0-10, higher = better quality, more CPU)
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

    /// Get the frame size in samples per channel
    pub fn frame_size(&self) -> usize {
        self.frame_size
    }

    /// Get total samples per frame (frame_size * channels)
    pub fn total_samples_per_frame(&self) -> usize {
        self.frame_size * self.format.channels as usize
    }

    /// Encode PCM samples to OPUS.
    ///
    /// # Arguments
    /// * `pcm` - Input PCM samples (interleaved if stereo). Must be exactly frame_size * channels samples.
    /// * `output` - Output buffer for encoded data. Should be at least 4000 bytes.
    ///
    /// # Returns
    /// Number of bytes written to output, or error.
    pub fn encode(&mut self, pcm: &[i16], output: &mut [u8]) -> Result<usize, CodecError> {
        let expected_samples = self.total_samples_per_frame();
        if pcm.len() != expected_samples {
            return Err(CodecError::Other(format!(
                "Expected {} samples, got {}",
                expected_samples,
                pcm.len()
            )));
        }

        unsafe {
            let result = opus_encode(
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

    /// Encode float PCM samples to OPUS.
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

/// OPUS Decoder wrapper
pub struct Decoder {
    decoder: *mut OpusDecoder,
    format: AudioFormat,
    frame_size: usize,
}

// SAFETY: OpusDecoder is internally synchronized
unsafe impl Send for Decoder {}

impl Decoder {
    /// Create a new OPUS decoder.
    ///
    /// # Arguments
    /// * `format` - Audio format (sample rate and channels)
    /// * `frame_duration_ms` - Expected frame duration in milliseconds
    pub fn new(format: AudioFormat, frame_duration_ms: f32) -> Result<Self, CodecError> {
        // Validate sample rate
        let valid_rates = [8000, 12000, 16000, 24000, 48000];
        if !valid_rates.contains(&format.sample_rate) {
            return Err(CodecError::Other(format!(
                "OPUS requires sample rate of {:?}, got {}",
                valid_rates, format.sample_rate
            )));
        }

        // Validate channels
        if format.channels < 1 || format.channels > 2 {
            return Err(CodecError::Other(format!(
                "OPUS requires 1 or 2 channels, got {}",
                format.channels
            )));
        }

        let frame_size = ((format.sample_rate as f32 * frame_duration_ms) / 1000.0) as usize;

        unsafe {
            let mut error: c_int = 0;
            let decoder = opus_decoder_create(
                format.sample_rate as i32,
                format.channels as c_int,
                &mut error,
            );

            if error != OPUS_OK || decoder.is_null() {
                return Err(CodecError::LibraryError(error));
            }

            Ok(Self {
                decoder,
                format,
                frame_size,
            })
        }
    }

    /// Create a decoder for 48kHz stereo with 20ms frames (WebRTC standard)
    pub fn new_48k_stereo_20ms() -> Result<Self, CodecError> {
        Self::new(AudioFormat::standard(), 20.0)
    }

    /// Get the frame size in samples per channel
    pub fn frame_size(&self) -> usize {
        self.frame_size
    }

    /// Get total samples per frame (frame_size * channels)
    pub fn total_samples_per_frame(&self) -> usize {
        self.frame_size * self.format.channels as usize
    }

    /// Decode OPUS data to PCM samples.
    ///
    /// # Arguments
    /// * `data` - Encoded OPUS data
    /// * `output` - Output buffer for decoded PCM samples. Must be large enough for frame_size * channels samples.
    /// * `fec` - Set to true to enable forward error correction (for lost packet recovery)
    ///
    /// # Returns
    /// Number of samples per channel decoded, or error.
    pub fn decode(&mut self, data: &[u8], output: &mut [i16], fec: bool) -> Result<usize, CodecError> {
        let max_frame_size = output.len() / self.format.channels as usize;

        unsafe {
            let result = opus_decode(
                self.decoder,
                data.as_ptr(),
                data.len() as i32,
                output.as_mut_ptr(),
                max_frame_size as c_int,
                if fec { 1 } else { 0 },
            );

            if result < 0 {
                Err(CodecError::LibraryError(result))
            } else {
                Ok(result as usize)
            }
        }
    }

    /// Decode OPUS data to float PCM samples.
    pub fn decode_float(&mut self, data: &[u8], output: &mut [f32], fec: bool) -> Result<usize, CodecError> {
        let max_frame_size = output.len() / self.format.channels as usize;

        unsafe {
            let result = opus_decode_float(
                self.decoder,
                data.as_ptr(),
                data.len() as i32,
                output.as_mut_ptr(),
                max_frame_size as c_int,
                if fec { 1 } else { 0 },
            );

            if result < 0 {
                Err(CodecError::LibraryError(result))
            } else {
                Ok(result as usize)
            }
        }
    }

    /// Decode a lost packet using packet loss concealment.
    ///
    /// Call this when a packet is lost to generate audio that smoothly conceals the gap.
    pub fn decode_lost_packet(&mut self, output: &mut [i16]) -> Result<usize, CodecError> {
        let max_frame_size = output.len() / self.format.channels as usize;

        unsafe {
            let result = opus_decode(
                self.decoder,
                ptr::null(),  // NULL pointer indicates lost packet
                0,
                output.as_mut_ptr(),
                max_frame_size as c_int,
                0,  // fec = 0 for PLC
            );

            if result < 0 {
                Err(CodecError::LibraryError(result))
            } else {
                Ok(result as usize)
            }
        }
    }

    /// Decode a lost packet using packet loss concealment (float output).
    pub fn decode_lost_packet_float(&mut self, output: &mut [f32]) -> Result<usize, CodecError> {
        let max_frame_size = output.len() / self.format.channels as usize;

        unsafe {
            let result = opus_decode_float(
                self.decoder,
                ptr::null(),  // NULL pointer indicates lost packet
                0,
                output.as_mut_ptr(),
                max_frame_size as c_int,
                0,  // fec = 0 for PLC
            );

            if result < 0 {
                Err(CodecError::LibraryError(result))
            } else {
                Ok(result as usize)
            }
        }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            opus_decoder_destroy(self.decoder);
        }
    }
}
