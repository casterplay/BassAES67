//! FLAC (Free Lossless Audio Codec) bindings for bass_srt.
//!
//! FLAC provides lossless audio compression, typically achieving 50-70%
//! compression ratio while preserving perfect audio quality.
//!
//! - Encoder: Uses native libFLAC for encoding (required for streaming output)
//! - Decoder: Uses Symphonia (pure Rust) for cross-platform decoding

use std::ffi::c_int;
use std::io::Cursor;

use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_FLAC};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use super::{AudioFormat, CodecError};

/// Default samples per frame for FLAC encoding (matches MP2 for consistency)
pub const DEFAULT_FRAME_SIZE: usize = 1152;

// FLAC encoder init status codes
pub const FLAC__STREAM_ENCODER_INIT_STATUS_OK: c_int = 0;

// FLAC stream encoder write status
pub const FLAC__STREAM_ENCODER_WRITE_STATUS_OK: c_int = 0;

/// Opaque encoder structure
#[repr(C)]
pub struct FLAC__StreamEncoder {
    _private: [u8; 0],
}

/// Stream metadata (simplified)
#[repr(C)]
pub struct FLAC__StreamMetadata {
    pub metadata_type: c_int,
    pub is_last: c_int,
    pub length: u32,
    // data union follows but we don't need it
}

// Callback types for encoder
type FlacEncoderWriteCallback = unsafe extern "C" fn(
    encoder: *const FLAC__StreamEncoder,
    buffer: *const u8,
    bytes: usize,
    samples: u32,
    current_frame: u32,
    client_data: *mut std::ffi::c_void,
) -> c_int;

type FlacEncoderSeekCallback = unsafe extern "C" fn(
    encoder: *const FLAC__StreamEncoder,
    absolute_byte_offset: u64,
    client_data: *mut std::ffi::c_void,
) -> c_int;

type FlacEncoderTellCallback = unsafe extern "C" fn(
    encoder: *const FLAC__StreamEncoder,
    absolute_byte_offset: *mut u64,
    client_data: *mut std::ffi::c_void,
) -> c_int;

type FlacEncoderMetadataCallback = unsafe extern "C" fn(
    encoder: *const FLAC__StreamEncoder,
    metadata: *const FLAC__StreamMetadata,
    client_data: *mut std::ffi::c_void,
);

#[link(name = "FLAC")]
extern "C" {
    // Encoder functions
    fn FLAC__stream_encoder_new() -> *mut FLAC__StreamEncoder;
    fn FLAC__stream_encoder_delete(encoder: *mut FLAC__StreamEncoder);
    fn FLAC__stream_encoder_set_channels(encoder: *mut FLAC__StreamEncoder, value: u32) -> c_int;
    fn FLAC__stream_encoder_set_bits_per_sample(encoder: *mut FLAC__StreamEncoder, value: u32) -> c_int;
    fn FLAC__stream_encoder_set_sample_rate(encoder: *mut FLAC__StreamEncoder, value: u32) -> c_int;
    fn FLAC__stream_encoder_set_compression_level(encoder: *mut FLAC__StreamEncoder, value: u32) -> c_int;
    fn FLAC__stream_encoder_set_blocksize(encoder: *mut FLAC__StreamEncoder, value: u32) -> c_int;
    fn FLAC__stream_encoder_set_streamable_subset(encoder: *mut FLAC__StreamEncoder, value: c_int) -> c_int;

    fn FLAC__stream_encoder_init_stream(
        encoder: *mut FLAC__StreamEncoder,
        write_callback: FlacEncoderWriteCallback,
        seek_callback: Option<FlacEncoderSeekCallback>,
        tell_callback: Option<FlacEncoderTellCallback>,
        metadata_callback: Option<FlacEncoderMetadataCallback>,
        client_data: *mut std::ffi::c_void,
    ) -> c_int;

    fn FLAC__stream_encoder_process_interleaved(
        encoder: *mut FLAC__StreamEncoder,
        buffer: *const i32,
        samples: u32,
    ) -> c_int;

    fn FLAC__stream_encoder_finish(encoder: *mut FLAC__StreamEncoder) -> c_int;
    fn FLAC__stream_encoder_get_state(encoder: *const FLAC__StreamEncoder) -> c_int;
}

/// Client data for encoder callbacks
struct EncoderClientData {
    output_buffer: Vec<u8>,
    total_written: usize,
}

/// Encoder write callback - captures encoded data
unsafe extern "C" fn encoder_write_callback(
    _encoder: *const FLAC__StreamEncoder,
    buffer: *const u8,
    bytes: usize,
    _samples: u32,
    _current_frame: u32,
    client_data: *mut std::ffi::c_void,
) -> c_int {
    let data = &mut *(client_data as *mut EncoderClientData);
    let slice = std::slice::from_raw_parts(buffer, bytes);
    data.output_buffer.extend_from_slice(slice);
    data.total_written += bytes;
    FLAC__STREAM_ENCODER_WRITE_STATUS_OK
}

/// FLAC Encoder wrapper (uses native libFLAC)
pub struct Encoder {
    encoder: *mut FLAC__StreamEncoder,
    format: AudioFormat,
    frame_size: usize,
    compression_level: u32,
    /// Buffer to accumulate samples until we have a complete frame
    sample_buffer: Vec<i32>,
    /// Client data for callbacks (Box to ensure stable address)
    client_data: Box<EncoderClientData>,
    /// Whether encoder has been initialized
    initialized: bool,
}

// SAFETY: FLAC encoder is internally managed
unsafe impl Send for Encoder {}

impl Encoder {
    /// Create a new FLAC encoder.
    ///
    /// # Arguments
    /// * `format` - Audio format (sample rate and channels)
    /// * `compression_level` - Compression level 0-8 (higher = better compression, more CPU)
    pub fn new(format: AudioFormat, compression_level: u32) -> Result<Self, CodecError> {
        // Validate channels (FLAC supports 1-8, but we limit to stereo)
        if format.channels < 1 || format.channels > 2 {
            return Err(CodecError::Other(format!(
                "FLAC encoder requires 1 or 2 channels, got {}",
                format.channels
            )));
        }

        // Validate compression level
        if compression_level > 8 {
            return Err(CodecError::Other(format!(
                "FLAC compression level must be 0-8, got {}",
                compression_level
            )));
        }

        unsafe {
            let encoder = FLAC__stream_encoder_new();
            if encoder.is_null() {
                return Err(CodecError::Other("Failed to create FLAC encoder".to_string()));
            }

            // Configure encoder
            FLAC__stream_encoder_set_channels(encoder, format.channels as u32);
            FLAC__stream_encoder_set_bits_per_sample(encoder, 16);
            FLAC__stream_encoder_set_sample_rate(encoder, format.sample_rate);
            FLAC__stream_encoder_set_compression_level(encoder, compression_level);
            FLAC__stream_encoder_set_blocksize(encoder, DEFAULT_FRAME_SIZE as u32);
            FLAC__stream_encoder_set_streamable_subset(encoder, 1);

            let client_data = Box::new(EncoderClientData {
                output_buffer: Vec::with_capacity(DEFAULT_FRAME_SIZE * 4),
                total_written: 0,
            });

            Ok(Self {
                encoder,
                format,
                frame_size: DEFAULT_FRAME_SIZE,
                compression_level,
                sample_buffer: Vec::with_capacity(DEFAULT_FRAME_SIZE * format.channels as usize),
                client_data,
                initialized: false,
            })
        }
    }

    /// Initialize the encoder stream (called on first encode)
    fn ensure_initialized(&mut self) -> Result<(), CodecError> {
        if self.initialized {
            return Ok(());
        }

        unsafe {
            let status = FLAC__stream_encoder_init_stream(
                self.encoder,
                encoder_write_callback,
                None,
                None,
                None,
                self.client_data.as_mut() as *mut EncoderClientData as *mut std::ffi::c_void,
            );

            if status != FLAC__STREAM_ENCODER_INIT_STATUS_OK {
                return Err(CodecError::LibraryError(status));
            }

            self.initialized = true;
        }
        Ok(())
    }

    /// Create an encoder for 48kHz stereo with default compression (level 5)
    pub fn new_48k_stereo() -> Result<Self, CodecError> {
        Self::new(AudioFormat::standard(), 5)
    }

    /// Get the frame size in samples per channel
    pub fn frame_size(&self) -> usize {
        self.frame_size
    }

    /// Get total samples per frame (frame_size * channels)
    pub fn total_samples_per_frame(&self) -> usize {
        self.frame_size * self.format.channels as usize
    }

    /// Encode PCM samples to FLAC.
    ///
    /// Note: FLAC uses fixed block sizes. This function buffers input samples
    /// until a complete frame is available.
    ///
    /// # Arguments
    /// * `pcm` - Input PCM samples (interleaved if stereo), 16-bit
    /// * `output` - Output buffer for encoded data
    ///
    /// # Returns
    /// Number of bytes written to output (may be 0 if buffering).
    pub fn encode(&mut self, pcm: &[i16], output: &mut [u8]) -> Result<usize, CodecError> {
        self.ensure_initialized()?;

        // Convert i16 to i32 and add to buffer
        for &sample in pcm {
            self.sample_buffer.push(sample as i32);
        }

        let samples_per_frame = self.total_samples_per_frame();
        let mut total_written = 0;

        // Reset client data buffer
        self.client_data.output_buffer.clear();
        self.client_data.total_written = 0;

        // Encode complete frames
        while self.sample_buffer.len() >= samples_per_frame {
            unsafe {
                let result = FLAC__stream_encoder_process_interleaved(
                    self.encoder,
                    self.sample_buffer.as_ptr(),
                    self.frame_size as u32,
                );

                if result == 0 {
                    return Err(CodecError::Other("FLAC encode failed".to_string()));
                }
            }

            // Remove encoded samples from buffer
            self.sample_buffer.drain(..samples_per_frame);
        }

        // Copy encoded data to output
        let encoded_len = self.client_data.output_buffer.len();
        if encoded_len > 0 {
            if output.len() < encoded_len {
                return Err(CodecError::BufferTooSmall);
            }
            output[..encoded_len].copy_from_slice(&self.client_data.output_buffer);
            total_written = encoded_len;
        }

        Ok(total_written)
    }

    /// Flush any remaining buffered samples and finish the stream.
    pub fn flush(&mut self, output: &mut [u8]) -> Result<usize, CodecError> {
        if !self.initialized {
            return Ok(0);
        }

        // Reset output buffer
        self.client_data.output_buffer.clear();
        self.client_data.total_written = 0;

        unsafe {
            FLAC__stream_encoder_finish(self.encoder);
        }

        let encoded_len = self.client_data.output_buffer.len();
        if encoded_len > 0 && output.len() >= encoded_len {
            output[..encoded_len].copy_from_slice(&self.client_data.output_buffer);
        }

        self.sample_buffer.clear();
        self.initialized = false;

        Ok(encoded_len.min(output.len()))
    }

    /// Get number of samples currently buffered
    pub fn buffered_samples(&self) -> usize {
        self.sample_buffer.len()
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        unsafe {
            if self.initialized {
                FLAC__stream_encoder_finish(self.encoder);
            }
            FLAC__stream_encoder_delete(self.encoder);
        }
    }
}

/// FLAC Decoder using Symphonia (pure Rust)
pub struct Decoder {
    format: AudioFormat,
    /// Detected format from stream
    detected_format: Option<AudioFormat>,
    /// Buffer for accumulating input data
    input_buffer: Vec<u8>,
}

// SAFETY: No native pointers, pure Rust
unsafe impl Send for Decoder {}

impl Decoder {
    /// Create a new FLAC decoder.
    ///
    /// # Arguments
    /// * `format` - Expected audio format (sample rate and channels)
    pub fn new(format: AudioFormat) -> Result<Self, CodecError> {
        Ok(Self {
            format,
            detected_format: None,
            input_buffer: Vec::with_capacity(8192),
        })
    }

    /// Create a decoder for 48kHz stereo
    pub fn new_48k_stereo() -> Result<Self, CodecError> {
        Self::new(AudioFormat::standard())
    }

    /// Get detected format (available after decoding)
    pub fn detected_format(&self) -> Option<AudioFormat> {
        self.detected_format
    }

    /// Decode FLAC data to float PCM samples.
    ///
    /// # Arguments
    /// * `data` - Compressed FLAC data (one frame)
    /// * `output` - Output buffer for decoded samples (f32, interleaved)
    ///
    /// # Returns
    /// Number of samples written to output (total, including all channels).
    pub fn decode(&mut self, data: &[u8], output: &mut [f32]) -> Result<usize, CodecError> {
        // Accumulate input data
        self.input_buffer.extend_from_slice(data);

        // Need enough data for FLAC frame
        if self.input_buffer.len() < 64 {
            return Ok(0);
        }

        // Create a media source from the accumulated buffer
        let cursor = Cursor::new(self.input_buffer.clone());
        let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

        // Probe the format
        let mut hint = Hint::new();
        hint.with_extension("flac");

        let format_opts = FormatOptions {
            enable_gapless: false,
            ..Default::default()
        };

        let metadata_opts = MetadataOptions::default();

        let probed = match symphonia::default::get_probe().format(&hint, mss, &format_opts, &metadata_opts) {
            Ok(p) => p,
            Err(_) => {
                // Not enough data yet or invalid format
                return Ok(0);
            }
        };

        let mut format_reader = probed.format;

        // Find the FLAC track
        let track = match format_reader
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec == CODEC_TYPE_FLAC)
        {
            Some(t) => t,
            None => return Err(CodecError::Other("No FLAC track found".to_string())),
        };

        let track_id = track.id;

        // Update format info
        if let (Some(sample_rate), Some(channels)) = (
            track.codec_params.sample_rate,
            track.codec_params.channels,
        ) {
            self.detected_format = Some(AudioFormat::new(sample_rate, channels.count() as u8));
        }

        // Create decoder
        let decoder_opts = DecoderOptions::default();
        let mut decoder = match symphonia::default::get_codecs()
            .make(&track.codec_params, &decoder_opts)
        {
            Ok(d) => d,
            Err(e) => {
                return Err(CodecError::Other(format!("Failed to create FLAC decoder: {}", e)));
            }
        };

        let mut total_samples = 0;

        // Decode packets
        loop {
            let packet = match format_reader.next_packet() {
                Ok(p) => p,
                Err(symphonia::core::errors::Error::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(_) => break,
            };

            if packet.track_id() != track_id {
                continue;
            }

            match decoder.decode(&packet) {
                Ok(decoded) => {
                    let samples_written = copy_audio_to_f32(&decoded, &mut output[total_samples..]);
                    total_samples += samples_written;

                    if total_samples + 8192 > output.len() {
                        break;
                    }
                }
                Err(symphonia::core::errors::Error::DecodeError(_)) => {
                    continue;
                }
                Err(_) => break,
            }
        }

        // Clear consumed data
        if total_samples > 0 {
            self.input_buffer.clear();
        }

        Ok(total_samples)
    }

    /// Reset decoder state for a new stream
    pub fn reset(&mut self) -> Result<(), CodecError> {
        self.input_buffer.clear();
        self.detected_format = None;
        Ok(())
    }
}

/// Copy decoded audio to f32 buffer
fn copy_audio_to_f32(decoded: &AudioBufferRef, output: &mut [f32]) -> usize {
    match decoded {
        AudioBufferRef::S16(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();
            let samples_needed = frames * channels;

            if samples_needed > output.len() {
                return 0;
            }

            let mut offset = 0;
            let scale = 1.0 / 32768.0;

            for frame in 0..frames {
                for ch in 0..channels {
                    let sample = buf.chan(ch)[frame];
                    if offset < output.len() {
                        output[offset] = sample as f32 * scale;
                        offset += 1;
                    }
                }
            }

            offset
        }
        AudioBufferRef::S32(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();
            let samples_needed = frames * channels;

            if samples_needed > output.len() {
                return 0;
            }

            let mut offset = 0;
            let scale = 1.0 / 2147483648.0;

            for frame in 0..frames {
                for ch in 0..channels {
                    let sample = buf.chan(ch)[frame];
                    if offset < output.len() {
                        output[offset] = sample as f32 * scale;
                        offset += 1;
                    }
                }
            }

            offset
        }
        AudioBufferRef::F32(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();
            let samples_needed = frames * channels;

            if samples_needed > output.len() {
                return 0;
            }

            let mut offset = 0;

            for frame in 0..frames {
                for ch in 0..channels {
                    let sample = buf.chan(ch)[frame];
                    if offset < output.len() {
                        output[offset] = sample;
                        offset += 1;
                    }
                }
            }

            offset
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encoder_create() {
        let encoder = Encoder::new_48k_stereo();
        assert!(encoder.is_ok());

        let encoder = encoder.unwrap();
        assert_eq!(encoder.frame_size(), DEFAULT_FRAME_SIZE);
        assert_eq!(encoder.total_samples_per_frame(), DEFAULT_FRAME_SIZE * 2);
    }

    #[test]
    fn test_decoder_create() {
        let decoder = Decoder::new_48k_stereo();
        assert!(decoder.is_ok());
    }
}
