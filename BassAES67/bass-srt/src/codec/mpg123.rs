//! mpg123 MPEG audio decoder bindings for bass_srt.
//!
//! mpg123 decodes MP1, MP2, and MP3 audio. We use it specifically for
//! MP2 (Layer 2) decoding which is the broadcast standard.

use std::ffi::c_int;
use std::ptr;

use super::{AudioFormat, CodecError};

/// Opaque decoder handle
#[repr(C)]
pub struct Mpg123Handle {
    _private: [u8; 0],
}

// Error codes
pub const MPG123_OK: c_int = 0;
pub const MPG123_DONE: c_int = -12;
pub const MPG123_NEW_FORMAT: c_int = -11;
pub const MPG123_NEED_MORE: c_int = -10;
pub const MPG123_ERR: c_int = -1;

// Encoding flags
pub const MPG123_ENC_SIGNED_16: c_int = 0x0d0;   // Signed 16-bit
pub const MPG123_ENC_FLOAT_32: c_int = 0x200;     // 32-bit float

// Channel count flags
pub const MPG123_MONO: c_int = 1;
pub const MPG123_STEREO: c_int = 2;

#[link(name = "mpg123")]
extern "C" {
    // Library initialization
    fn mpg123_init() -> c_int;
    fn mpg123_exit();

    // Handle creation/destruction
    fn mpg123_new(decoder: *const i8, error: *mut c_int) -> *mut Mpg123Handle;
    fn mpg123_delete(mh: *mut Mpg123Handle);

    // Format configuration
    fn mpg123_format_none(mh: *mut Mpg123Handle) -> c_int;
    fn mpg123_format(
        mh: *mut Mpg123Handle,
        rate: i64,
        channels: c_int,
        encodings: c_int,
    ) -> c_int;
    fn mpg123_getformat(
        mh: *mut Mpg123Handle,
        rate: *mut i64,
        channels: *mut c_int,
        encoding: *mut c_int,
    ) -> c_int;

    // Feed mode
    fn mpg123_open_feed(mh: *mut Mpg123Handle) -> c_int;

    // Decoding
    fn mpg123_feed(mh: *mut Mpg123Handle, data: *const u8, size: usize) -> c_int;
    fn mpg123_decode(
        mh: *mut Mpg123Handle,
        inmemory: *const u8,
        inmemsize: usize,
        outmemory: *mut u8,
        outmemsize: usize,
        done: *mut usize,
    ) -> c_int;
    fn mpg123_read(
        mh: *mut Mpg123Handle,
        outmemory: *mut u8,
        outmemsize: usize,
        done: *mut usize,
    ) -> c_int;

    // Error handling
    fn mpg123_plain_strerror(errcode: c_int) -> *const i8;
    fn mpg123_strerror(mh: *mut Mpg123Handle) -> *const i8;
}

// Thread-safe initialization tracking
use std::sync::Once;
static INIT: Once = Once::new();

/// Initialize the mpg123 library (called once automatically)
fn ensure_init() {
    INIT.call_once(|| {
        unsafe {
            let result = mpg123_init();
            if result != MPG123_OK {
                panic!("Failed to initialize mpg123: {}", error_string(result));
            }
        }
    });
}

/// Get error message for an mpg123 error code
pub fn error_string(error: c_int) -> String {
    unsafe {
        let ptr = mpg123_plain_strerror(error);
        if ptr.is_null() {
            format!("Unknown mpg123 error {}", error)
        } else {
            std::ffi::CStr::from_ptr(ptr)
                .to_string_lossy()
                .into_owned()
        }
    }
}

/// MP2/MP3 Decoder using mpg123 library
pub struct Decoder {
    handle: *mut Mpg123Handle,
    format: Option<AudioFormat>,
    /// Buffer for feeding compressed data
    input_buffer: Vec<u8>,
}

// SAFETY: Mpg123Handle is internally managed
unsafe impl Send for Decoder {}

impl Decoder {
    /// Create a new MP2/MP3 decoder.
    ///
    /// The decoder auto-detects format from the input stream.
    pub fn new() -> Result<Self, CodecError> {
        ensure_init();

        unsafe {
            let mut error: c_int = 0;
            let handle = mpg123_new(ptr::null(), &mut error);

            if handle.is_null() || error != MPG123_OK {
                return Err(CodecError::LibraryError(error));
            }

            // Clear all format support, then set what we want
            if mpg123_format_none(handle) != MPG123_OK {
                mpg123_delete(handle);
                return Err(CodecError::Other("Failed to clear formats".to_string()));
            }

            // Support common sample rates with signed 16-bit output
            for rate in [44100i64, 48000] {
                // Allow mono and stereo
                mpg123_format(handle, rate, MPG123_MONO | MPG123_STEREO, MPG123_ENC_SIGNED_16);
            }

            // Open for feeding (no file, we feed data directly)
            let result = mpg123_open_feed(handle);
            if result != MPG123_OK {
                mpg123_delete(handle);
                return Err(CodecError::LibraryError(result));
            }

            Ok(Self {
                handle,
                format: None,
                input_buffer: Vec::with_capacity(8192),
            })
        }
    }

    /// Get the detected audio format (available after first decode)
    pub fn format(&self) -> Option<AudioFormat> {
        self.format
    }

    /// Update format from the decoder
    fn update_format(&mut self) -> Result<(), CodecError> {
        unsafe {
            let mut rate: i64 = 0;
            let mut channels: c_int = 0;
            let mut encoding: c_int = 0;

            let result = mpg123_getformat(self.handle, &mut rate, &mut channels, &mut encoding);
            if result != MPG123_OK {
                return Err(CodecError::LibraryError(result));
            }

            self.format = Some(AudioFormat::new(rate as u32, channels as u8));
        }
        Ok(())
    }

    /// Decode MP2/MP3 data to PCM samples.
    ///
    /// # Arguments
    /// * `data` - Compressed MP2/MP3 data
    /// * `output` - Output buffer for decoded PCM samples (i16)
    ///
    /// # Returns
    /// Number of bytes written to output, or error.
    /// May return 0 if more input data is needed.
    pub fn decode(&mut self, data: &[u8], output: &mut [i16]) -> Result<usize, CodecError> {
        let output_bytes = unsafe {
            std::slice::from_raw_parts_mut(
                output.as_mut_ptr() as *mut u8,
                output.len() * 2,
            )
        };

        self.decode_bytes(data, output_bytes).map(|bytes| bytes / 2)
    }

    /// Decode MP2/MP3 data to raw bytes.
    ///
    /// Returns number of bytes written to output.
    pub fn decode_bytes(&mut self, data: &[u8], output: &mut [u8]) -> Result<usize, CodecError> {
        unsafe {
            let mut done: usize = 0;

            let result = mpg123_decode(
                self.handle,
                data.as_ptr(),
                data.len(),
                output.as_mut_ptr(),
                output.len(),
                &mut done,
            );

            match result {
                MPG123_OK | MPG123_DONE => Ok(done),
                MPG123_NEED_MORE => Ok(done), // Return what we have, caller should feed more
                MPG123_NEW_FORMAT => {
                    self.update_format()?;
                    // After format change, try to get more output
                    if done == 0 {
                        let mut more_done: usize = 0;
                        let result2 = mpg123_read(
                            self.handle,
                            output.as_mut_ptr(),
                            output.len(),
                            &mut more_done,
                        );
                        if result2 == MPG123_OK || result2 == MPG123_NEED_MORE {
                            Ok(more_done)
                        } else {
                            Ok(0)
                        }
                    } else {
                        Ok(done)
                    }
                }
                _ => Err(CodecError::LibraryError(result)),
            }
        }
    }

    /// Feed compressed data without immediate decoding.
    ///
    /// Use this to buffer data, then call read() to get decoded output.
    pub fn feed(&mut self, data: &[u8]) -> Result<(), CodecError> {
        unsafe {
            let result = mpg123_feed(self.handle, data.as_ptr(), data.len());
            if result != MPG123_OK {
                Err(CodecError::LibraryError(result))
            } else {
                Ok(())
            }
        }
    }

    /// Read decoded output from previously fed data.
    ///
    /// Returns number of bytes written to output.
    pub fn read(&mut self, output: &mut [u8]) -> Result<usize, CodecError> {
        unsafe {
            let mut done: usize = 0;
            let result = mpg123_read(self.handle, output.as_mut_ptr(), output.len(), &mut done);

            match result {
                MPG123_OK | MPG123_DONE => Ok(done),
                MPG123_NEED_MORE => Ok(done),
                MPG123_NEW_FORMAT => {
                    self.update_format()?;
                    Ok(done)
                }
                _ => Err(CodecError::LibraryError(result)),
            }
        }
    }

    /// Read decoded output as i16 samples.
    pub fn read_samples(&mut self, output: &mut [i16]) -> Result<usize, CodecError> {
        let output_bytes = unsafe {
            std::slice::from_raw_parts_mut(
                output.as_mut_ptr() as *mut u8,
                output.len() * 2,
            )
        };

        self.read(output_bytes).map(|bytes| bytes / 2)
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            mpg123_delete(self.handle);
        }
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new().expect("Failed to create mpg123 decoder")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decoder_create() {
        let decoder = Decoder::new();
        assert!(decoder.is_ok());
    }

    #[test]
    fn test_error_string() {
        let msg = error_string(MPG123_ERR);
        assert!(!msg.is_empty());
        println!("MPG123_ERR message: {}", msg);
    }

    // Note: Full encode/decode test requires a valid MP2 stream.
    // The twolame encoder can be used to create test data.
}
