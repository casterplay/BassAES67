//! TwoLAME MP2 encoder bindings for bass_srt.
//!
//! MP2 (MPEG Audio Layer 2) is the broadcast standard for DAB, DVB, and many
//! professional audio systems. TwoLAME is an optimized MP2 encoder.

use std::ffi::c_int;
use std::ptr;

use super::{AudioFormat, CodecError};

/// Number of samples per frame for MP2 (fixed by MPEG-1 Layer 2 spec)
pub const SAMPLES_PER_FRAME: usize = 1152;

/// MPEG modes
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpegMode {
    Auto = -1,
    Stereo = 0,
    JointStereo = 1,
    DualChannel = 2,
    Mono = 3,
}

/// Opaque encoder options structure
#[repr(C)]
pub struct TwolameOptions {
    _private: [u8; 0],
}

// On Windows, the library is named "libtwolame_dll", on other platforms it's "twolame"
#[cfg_attr(target_os = "windows", link(name = "libtwolame_dll"))]
#[cfg_attr(not(target_os = "windows"), link(name = "twolame"))]
extern "C" {
    // Initialization
    fn twolame_init() -> *mut TwolameOptions;
    fn twolame_init_params(glopts: *mut TwolameOptions) -> c_int;
    fn twolame_close(glopts: *mut *mut TwolameOptions);

    // Configuration
    fn twolame_set_verbosity(glopts: *mut TwolameOptions, verbosity: c_int) -> c_int;
    fn twolame_set_mode(glopts: *mut TwolameOptions, mode: c_int) -> c_int;
    fn twolame_set_num_channels(glopts: *mut TwolameOptions, num_channels: c_int) -> c_int;
    fn twolame_set_in_samplerate(glopts: *mut TwolameOptions, samplerate: c_int) -> c_int;
    fn twolame_set_out_samplerate(glopts: *mut TwolameOptions, samplerate: c_int) -> c_int;
    fn twolame_set_bitrate(glopts: *mut TwolameOptions, bitrate: c_int) -> c_int;

    // Encoding
    fn twolame_encode_buffer_interleaved(
        glopts: *mut TwolameOptions,
        pcm: *const i16,
        num_samples: c_int,
        mp2buffer: *mut u8,
        mp2buffer_size: c_int,
    ) -> c_int;

    fn twolame_encode_buffer_float32_interleaved(
        glopts: *mut TwolameOptions,
        pcm: *const f32,
        num_samples: c_int,
        mp2buffer: *mut u8,
        mp2buffer_size: c_int,
    ) -> c_int;

    fn twolame_encode_flush(
        glopts: *mut TwolameOptions,
        mp2buffer: *mut u8,
        mp2buffer_size: c_int,
    ) -> c_int;

    // Version info
    fn get_twolame_version() -> *const i8;
}

/// Get the TwoLAME library version
pub fn version() -> String {
    unsafe {
        let ptr = get_twolame_version();
        if ptr.is_null() {
            "unknown".to_string()
        } else {
            std::ffi::CStr::from_ptr(ptr)
                .to_string_lossy()
                .into_owned()
        }
    }
}

/// MP2 Encoder using TwoLAME library
pub struct Encoder {
    options: *mut TwolameOptions,
    format: AudioFormat,
    bitrate: u32,
    /// Internal buffer to accumulate samples until we have SAMPLES_PER_FRAME
    sample_buffer: Vec<i16>,
}

// SAFETY: TwolameOptions is internally managed
unsafe impl Send for Encoder {}

impl Encoder {
    /// Create a new MP2 encoder.
    ///
    /// # Arguments
    /// * `format` - Audio format (sample rate and channels)
    /// * `bitrate` - Target bitrate in kbps (e.g., 192, 256, 320)
    pub fn new(format: AudioFormat, bitrate: u32) -> Result<Self, CodecError> {
        // Validate channels
        if format.channels < 1 || format.channels > 2 {
            return Err(CodecError::Other(format!(
                "MP2 requires 1 or 2 channels, got {}",
                format.channels
            )));
        }

        // Valid MP2 sample rates (MPEG-1 and MPEG-2)
        let valid_rates = [16000, 22050, 24000, 32000, 44100, 48000];
        if !valid_rates.contains(&format.sample_rate) {
            return Err(CodecError::Other(format!(
                "MP2 requires sample rate of {:?}, got {}",
                valid_rates, format.sample_rate
            )));
        }

        // Valid bitrates for MPEG-1 Layer 2 (kbps)
        let valid_bitrates = [32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384];
        if !valid_bitrates.contains(&bitrate) {
            return Err(CodecError::Other(format!(
                "MP2 requires bitrate of {:?} kbps, got {}",
                valid_bitrates, bitrate
            )));
        }

        unsafe {
            let options = twolame_init();
            if options.is_null() {
                return Err(CodecError::Other("Failed to initialize TwoLAME".to_string()));
            }

            // Configure encoder
            twolame_set_verbosity(options, 0); // Silent

            let mode = if format.channels == 1 {
                MpegMode::Mono
            } else {
                MpegMode::Stereo
            };
            twolame_set_mode(options, mode as c_int);
            twolame_set_num_channels(options, format.channels as c_int);
            twolame_set_in_samplerate(options, format.sample_rate as c_int);
            twolame_set_out_samplerate(options, format.sample_rate as c_int);
            twolame_set_bitrate(options, bitrate as c_int);

            // Initialize parameters
            let result = twolame_init_params(options);
            if result != 0 {
                let mut opts = options;
                twolame_close(&mut opts);
                return Err(CodecError::LibraryError(result));
            }

            Ok(Self {
                options,
                format,
                bitrate,
                sample_buffer: Vec::with_capacity(SAMPLES_PER_FRAME * format.channels as usize),
            })
        }
    }

    /// Create an encoder for 48kHz stereo at 192kbps (broadcast quality)
    pub fn new_48k_stereo_192k() -> Result<Self, CodecError> {
        Self::new(AudioFormat::standard(), 192)
    }

    /// Create an encoder for 48kHz stereo at 256kbps (high quality)
    pub fn new_48k_stereo_256k() -> Result<Self, CodecError> {
        Self::new(AudioFormat::standard(), 256)
    }

    /// Get the frame size in samples per channel (always 1152 for MP2)
    pub fn frame_size(&self) -> usize {
        SAMPLES_PER_FRAME
    }

    /// Get total samples per frame (frame_size * channels)
    pub fn total_samples_per_frame(&self) -> usize {
        SAMPLES_PER_FRAME * self.format.channels as usize
    }

    /// Get the bitrate in kbps
    pub fn bitrate(&self) -> u32 {
        self.bitrate
    }

    /// Get approximate output size for one frame in bytes
    /// Formula: (bitrate_kbps * 1000 * frame_duration_seconds) / 8
    /// MP2 frame duration at 48kHz: 1152 / 48000 = 24ms
    pub fn frame_bytes(&self) -> usize {
        // MP2 frame size calculation
        let frame_duration_ms = (SAMPLES_PER_FRAME * 1000) / self.format.sample_rate as usize;
        (self.bitrate as usize * frame_duration_ms) / 8
    }

    /// Encode PCM samples to MP2.
    ///
    /// Note: MP2 frames are always 1152 samples. This function will buffer
    /// input samples until a complete frame is available.
    ///
    /// # Arguments
    /// * `pcm` - Input PCM samples (interleaved if stereo)
    /// * `output` - Output buffer for encoded data. Should be at least 4608 bytes.
    ///
    /// # Returns
    /// Number of bytes written to output (may be 0 if buffering).
    pub fn encode(&mut self, pcm: &[i16], output: &mut [u8]) -> Result<usize, CodecError> {
        // Add samples to buffer
        self.sample_buffer.extend_from_slice(pcm);

        let samples_per_frame = self.total_samples_per_frame();
        let mut total_written = 0;

        // Encode complete frames
        while self.sample_buffer.len() >= samples_per_frame {
            unsafe {
                let result = twolame_encode_buffer_interleaved(
                    self.options,
                    self.sample_buffer.as_ptr(),
                    SAMPLES_PER_FRAME as c_int, // samples per channel
                    output[total_written..].as_mut_ptr(),
                    (output.len() - total_written) as c_int,
                );

                if result < 0 {
                    return Err(CodecError::LibraryError(result));
                }

                total_written += result as usize;
            }

            // Remove encoded samples from buffer
            self.sample_buffer.drain(..samples_per_frame);
        }

        Ok(total_written)
    }

    /// Encode float PCM samples to MP2.
    pub fn encode_float(&mut self, pcm: &[f32], output: &mut [u8]) -> Result<usize, CodecError> {
        // Convert float to i16 and use regular encode
        let pcm_i16: Vec<i16> = pcm
            .iter()
            .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect();

        self.encode(&pcm_i16, output)
    }

    /// Flush any remaining buffered samples.
    ///
    /// Call this when you're done encoding to get any remaining data.
    pub fn flush(&mut self, output: &mut [u8]) -> Result<usize, CodecError> {
        unsafe {
            let result = twolame_encode_flush(
                self.options,
                output.as_mut_ptr(),
                output.len() as c_int,
            );

            if result < 0 {
                Err(CodecError::LibraryError(result))
            } else {
                self.sample_buffer.clear();
                Ok(result as usize)
            }
        }
    }

    /// Get number of samples currently buffered
    pub fn buffered_samples(&self) -> usize {
        self.sample_buffer.len()
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        unsafe {
            let mut opts = self.options;
            twolame_close(&mut opts);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        let ver = version();
        assert!(!ver.is_empty());
        println!("TwoLAME version: {}", ver);
    }

    #[test]
    fn test_encoder_create() {
        let encoder = Encoder::new_48k_stereo_192k();
        assert!(encoder.is_ok());

        let encoder = encoder.unwrap();
        assert_eq!(encoder.frame_size(), 1152);
        assert_eq!(encoder.total_samples_per_frame(), 2304); // 1152 * 2
        assert_eq!(encoder.bitrate(), 192);
    }

    #[test]
    fn test_encode() {
        let mut encoder = Encoder::new_48k_stereo_192k().unwrap();

        // Create test samples (1152 stereo samples = 2304 total)
        let samples: Vec<i16> = (0..2304)
            .map(|i| ((i as f32 * 0.1).sin() * 16000.0) as i16)
            .collect();

        let mut output = vec![0u8; 4608];
        let result = encoder.encode(&samples, &mut output);
        assert!(result.is_ok());

        let bytes_written = result.unwrap();
        // Should have encoded one complete frame
        assert!(bytes_written > 0, "Expected encoded output, got {} bytes", bytes_written);
        println!("Encoded {} samples to {} bytes", samples.len(), bytes_written);
    }
}
