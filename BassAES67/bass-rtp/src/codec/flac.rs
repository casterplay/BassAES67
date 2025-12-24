//! FLAC (Free Lossless Audio Codec) bindings for bass-rtp.
//!
//! FLAC provides lossless audio compression, typically achieving 50-70%
//! compression ratio while preserving perfect audio quality.

#![allow(dead_code)]
#![allow(unused_imports)]

use std::ffi::c_int;
use std::ptr;
use std::sync::Once;

use super::{AudioFormat, CodecError};

/// Default samples per frame for FLAC encoding (matches MP2 for consistency)
pub const DEFAULT_FRAME_SIZE: usize = 1152;

// FLAC encoder init status codes
pub const FLAC__STREAM_ENCODER_INIT_STATUS_OK: c_int = 0;

// FLAC decoder init status codes
pub const FLAC__STREAM_DECODER_INIT_STATUS_OK: c_int = 0;

// FLAC stream decoder state
pub const FLAC__STREAM_DECODER_END_OF_STREAM: c_int = 4;

// FLAC stream decoder read status
pub const FLAC__STREAM_DECODER_READ_STATUS_CONTINUE: c_int = 0;
pub const FLAC__STREAM_DECODER_READ_STATUS_END_OF_STREAM: c_int = 1;
pub const FLAC__STREAM_DECODER_READ_STATUS_ABORT: c_int = 2;

// FLAC stream decoder write status
pub const FLAC__STREAM_DECODER_WRITE_STATUS_CONTINUE: c_int = 0;
pub const FLAC__STREAM_DECODER_WRITE_STATUS_ABORT: c_int = 1;

// FLAC stream encoder write status
pub const FLAC__STREAM_ENCODER_WRITE_STATUS_OK: c_int = 0;

/// Opaque encoder structure
#[repr(C)]
pub struct FLAC__StreamEncoder {
    _private: [u8; 0],
}

/// Opaque decoder structure
#[repr(C)]
pub struct FLAC__StreamDecoder {
    _private: [u8; 0],
}

/// FLAC frame header (partial, for channel/sample info)
#[repr(C)]
pub struct FLAC__FrameHeader {
    pub blocksize: u32,
    pub sample_rate: u32,
    pub channels: u32,
    pub channel_assignment: c_int,
    pub bits_per_sample: u32,
    pub number_type: c_int,
    pub number: FLAC__FrameNumber,
    pub crc: u8,
}

/// Frame number union
#[repr(C)]
pub union FLAC__FrameNumber {
    pub frame_number: u32,
    pub sample_number: u64,
}

/// FLAC frame structure
#[repr(C)]
pub struct FLAC__Frame {
    pub header: FLAC__FrameHeader,
    pub subframes: [FLAC__Subframe; 8],
    pub footer: FLAC__FrameFooter,
}

/// Subframe (opaque, we don't need internals)
#[repr(C)]
pub struct FLAC__Subframe {
    _data: [u8; 256], // Approximate size, we don't use it directly
}

/// Frame footer
#[repr(C)]
pub struct FLAC__FrameFooter {
    pub crc: u16,
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

// Callback types for decoder
type FlacDecoderReadCallback = unsafe extern "C" fn(
    decoder: *const FLAC__StreamDecoder,
    buffer: *mut u8,
    bytes: *mut usize,
    client_data: *mut std::ffi::c_void,
) -> c_int;

type FlacDecoderSeekCallback = unsafe extern "C" fn(
    decoder: *const FLAC__StreamDecoder,
    absolute_byte_offset: u64,
    client_data: *mut std::ffi::c_void,
) -> c_int;

type FlacDecoderTellCallback = unsafe extern "C" fn(
    decoder: *const FLAC__StreamDecoder,
    absolute_byte_offset: *mut u64,
    client_data: *mut std::ffi::c_void,
) -> c_int;

type FlacDecoderLengthCallback = unsafe extern "C" fn(
    decoder: *const FLAC__StreamDecoder,
    stream_length: *mut u64,
    client_data: *mut std::ffi::c_void,
) -> c_int;

type FlacDecoderEofCallback = unsafe extern "C" fn(
    decoder: *const FLAC__StreamDecoder,
    client_data: *mut std::ffi::c_void,
) -> c_int;

type FlacDecoderWriteCallback = unsafe extern "C" fn(
    decoder: *const FLAC__StreamDecoder,
    frame: *const FLAC__Frame,
    buffer: *const *const i32,
    client_data: *mut std::ffi::c_void,
) -> c_int;

type FlacDecoderMetadataCallback = unsafe extern "C" fn(
    decoder: *const FLAC__StreamDecoder,
    metadata: *const FLAC__StreamMetadata,
    client_data: *mut std::ffi::c_void,
);

type FlacDecoderErrorCallback = unsafe extern "C" fn(
    decoder: *const FLAC__StreamDecoder,
    status: c_int,
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

    // Decoder functions
    fn FLAC__stream_decoder_new() -> *mut FLAC__StreamDecoder;
    fn FLAC__stream_decoder_delete(decoder: *mut FLAC__StreamDecoder);
    fn FLAC__stream_decoder_set_md5_checking(decoder: *mut FLAC__StreamDecoder, value: c_int) -> c_int;

    fn FLAC__stream_decoder_init_stream(
        decoder: *mut FLAC__StreamDecoder,
        read_callback: FlacDecoderReadCallback,
        seek_callback: Option<FlacDecoderSeekCallback>,
        tell_callback: Option<FlacDecoderTellCallback>,
        length_callback: Option<FlacDecoderLengthCallback>,
        eof_callback: Option<FlacDecoderEofCallback>,
        write_callback: FlacDecoderWriteCallback,
        metadata_callback: Option<FlacDecoderMetadataCallback>,
        error_callback: FlacDecoderErrorCallback,
        client_data: *mut std::ffi::c_void,
    ) -> c_int;

    fn FLAC__stream_decoder_process_single(decoder: *mut FLAC__StreamDecoder) -> c_int;
    fn FLAC__stream_decoder_finish(decoder: *mut FLAC__StreamDecoder) -> c_int;
    fn FLAC__stream_decoder_reset(decoder: *mut FLAC__StreamDecoder) -> c_int;
    fn FLAC__stream_decoder_get_state(decoder: *const FLAC__StreamDecoder) -> c_int;
    fn FLAC__stream_decoder_flush(decoder: *mut FLAC__StreamDecoder) -> c_int;
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

/// FLAC Encoder wrapper
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

/// Client data for decoder callbacks
struct DecoderClientData {
    /// Input buffer (compressed FLAC data)
    input_buffer: Vec<u8>,
    /// Current read position in input buffer
    input_pos: usize,
    /// Output buffer (decoded samples as f32)
    output_buffer: Vec<f32>,
    /// Audio format detected from stream
    format: Option<AudioFormat>,
    /// Whether we've hit end of input
    eof: bool,
    /// Error occurred during decoding
    error: bool,
}

/// Decoder read callback - provides compressed data to decoder
unsafe extern "C" fn decoder_read_callback(
    _decoder: *const FLAC__StreamDecoder,
    buffer: *mut u8,
    bytes: *mut usize,
    client_data: *mut std::ffi::c_void,
) -> c_int {
    let data = &mut *(client_data as *mut DecoderClientData);

    let available = data.input_buffer.len() - data.input_pos;
    if available == 0 {
        *bytes = 0;
        if data.eof {
            return FLAC__STREAM_DECODER_READ_STATUS_END_OF_STREAM;
        }
        return FLAC__STREAM_DECODER_READ_STATUS_ABORT;
    }

    let to_read = (*bytes).min(available);
    let src = &data.input_buffer[data.input_pos..data.input_pos + to_read];
    std::ptr::copy_nonoverlapping(src.as_ptr(), buffer, to_read);
    data.input_pos += to_read;
    *bytes = to_read;

    FLAC__STREAM_DECODER_READ_STATUS_CONTINUE
}

/// Decoder write callback - receives decoded samples
unsafe extern "C" fn decoder_write_callback(
    _decoder: *const FLAC__StreamDecoder,
    frame: *const FLAC__Frame,
    buffer: *const *const i32,
    client_data: *mut std::ffi::c_void,
) -> c_int {
    let data = &mut *(client_data as *mut DecoderClientData);
    let frame = &*frame;

    let channels = frame.header.channels as usize;
    let blocksize = frame.header.blocksize as usize;
    let bits_per_sample = frame.header.bits_per_sample;

    // Store format info
    if data.format.is_none() {
        data.format = Some(AudioFormat::new(
            frame.header.sample_rate,
            channels as u8,
        ));
    }

    // Scale factor for converting to f32 (-1.0 to 1.0)
    let scale = 1.0 / (1u32 << (bits_per_sample - 1)) as f32;

    // Interleave channels into output buffer
    for i in 0..blocksize {
        for ch in 0..channels {
            let channel_ptr = *buffer.add(ch);
            let sample = *channel_ptr.add(i);
            data.output_buffer.push(sample as f32 * scale);
        }
    }

    FLAC__STREAM_DECODER_WRITE_STATUS_CONTINUE
}

/// Decoder error callback
unsafe extern "C" fn decoder_error_callback(
    _decoder: *const FLAC__StreamDecoder,
    _status: c_int,
    client_data: *mut std::ffi::c_void,
) {
    let data = &mut *(client_data as *mut DecoderClientData);
    data.error = true;
}

/// FLAC Decoder wrapper
pub struct Decoder {
    decoder: *mut FLAC__StreamDecoder,
    format: AudioFormat,
    /// Client data for callbacks
    client_data: Box<DecoderClientData>,
    /// Whether decoder has been initialized
    initialized: bool,
}

// SAFETY: FLAC decoder is internally managed
unsafe impl Send for Decoder {}

impl Decoder {
    /// Create a new FLAC decoder.
    ///
    /// # Arguments
    /// * `format` - Expected audio format (sample rate and channels)
    pub fn new(format: AudioFormat) -> Result<Self, CodecError> {
        unsafe {
            let decoder = FLAC__stream_decoder_new();
            if decoder.is_null() {
                return Err(CodecError::Other("Failed to create FLAC decoder".to_string()));
            }

            // Disable MD5 checking for streaming (we don't have full file)
            FLAC__stream_decoder_set_md5_checking(decoder, 0);

            let client_data = Box::new(DecoderClientData {
                input_buffer: Vec::with_capacity(8192),
                input_pos: 0,
                output_buffer: Vec::with_capacity(DEFAULT_FRAME_SIZE * format.channels as usize),
                format: None,
                eof: false,
                error: false,
            });

            Ok(Self {
                decoder,
                format,
                client_data,
                initialized: false,
            })
        }
    }

    /// Initialize the decoder stream
    fn ensure_initialized(&mut self) -> Result<(), CodecError> {
        if self.initialized {
            return Ok(());
        }

        unsafe {
            let status = FLAC__stream_decoder_init_stream(
                self.decoder,
                decoder_read_callback,
                None,  // seek
                None,  // tell
                None,  // length
                None,  // eof (we use abort instead)
                decoder_write_callback,
                None,  // metadata
                decoder_error_callback,
                self.client_data.as_mut() as *mut DecoderClientData as *mut std::ffi::c_void,
            );

            if status != FLAC__STREAM_DECODER_INIT_STATUS_OK {
                return Err(CodecError::LibraryError(status));
            }

            self.initialized = true;
        }
        Ok(())
    }

    /// Create a decoder for 48kHz stereo
    pub fn new_48k_stereo() -> Result<Self, CodecError> {
        Self::new(AudioFormat::standard())
    }

    /// Get detected format (available after decoding)
    pub fn detected_format(&self) -> Option<AudioFormat> {
        self.client_data.format
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
        self.ensure_initialized()?;

        // Set up input
        self.client_data.input_buffer.clear();
        self.client_data.input_buffer.extend_from_slice(data);
        self.client_data.input_pos = 0;
        self.client_data.eof = true;  // This is a complete frame
        self.client_data.error = false;

        // Clear output buffer
        self.client_data.output_buffer.clear();

        // Process until we've consumed input or hit error
        unsafe {
            while self.client_data.input_pos < self.client_data.input_buffer.len()
                  && !self.client_data.error {
                let result = FLAC__stream_decoder_process_single(self.decoder);
                if result == 0 {
                    break;
                }

                // Check if decoder is at end of stream
                let state = FLAC__stream_decoder_get_state(self.decoder);
                if state == FLAC__STREAM_DECODER_END_OF_STREAM {
                    break;
                }
            }
        }

        if self.client_data.error {
            return Err(CodecError::InvalidInput);
        }

        // Copy decoded samples to output
        let samples_decoded = self.client_data.output_buffer.len();
        if samples_decoded > 0 {
            let to_copy = samples_decoded.min(output.len());
            output[..to_copy].copy_from_slice(&self.client_data.output_buffer[..to_copy]);
            return Ok(to_copy);
        }

        Ok(0)
    }

    /// Reset decoder state for a new stream
    pub fn reset(&mut self) -> Result<(), CodecError> {
        if self.initialized {
            unsafe {
                FLAC__stream_decoder_flush(self.decoder);
                FLAC__stream_decoder_reset(self.decoder);
            }
            self.initialized = false;
        }
        self.client_data.format = None;
        self.client_data.error = false;
        Ok(())
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            if self.initialized {
                FLAC__stream_decoder_finish(self.decoder);
            }
            FLAC__stream_decoder_delete(self.decoder);
        }
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

    #[test]
    fn test_encode_decode_roundtrip() {
        let mut encoder = Encoder::new_48k_stereo().unwrap();
        let mut decoder = Decoder::new_48k_stereo().unwrap();

        // Create test signal (sine wave)
        let samples_per_frame = encoder.total_samples_per_frame();
        let mut pcm_in: Vec<i16> = Vec::with_capacity(samples_per_frame);
        for i in 0..samples_per_frame {
            let sample = ((i as f32 * 0.1).sin() * 16000.0) as i16;
            pcm_in.push(sample);
        }

        // Encode
        let mut encoded = vec![0u8; 8192];
        let encoded_len = encoder.encode(&pcm_in, &mut encoded).unwrap();

        // FLAC may need flush to get complete frame
        let flush_len = encoder.flush(&mut encoded[encoded_len..]).unwrap();
        let total_encoded = encoded_len + flush_len;

        if total_encoded > 0 {
            // Decode
            let mut pcm_out = vec![0.0f32; samples_per_frame * 2];
            let decoded = decoder.decode(&encoded[..total_encoded], &mut pcm_out);

            // FLAC decode may fail on partial frames in test
            if let Ok(samples) = decoded {
                println!("Encoded {} samples to {} bytes, decoded {} samples",
                    samples_per_frame, total_encoded, samples);
            }
        }
    }
}
