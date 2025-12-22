//! MPEG audio decoder using Symphonia (pure Rust).
//!
//! This module decodes MP1, MP2, and MP3 audio using the Symphonia library.
//! It replaces the native mpg123 library for cross-platform compatibility.

use std::io::Cursor;

use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_MP1, CODEC_TYPE_MP2, CODEC_TYPE_MP3};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use super::{AudioFormat, CodecError};

/// MP2/MP3 Decoder using Symphonia library (pure Rust)
pub struct Decoder {
    format: Option<AudioFormat>,
    /// Buffer for accumulating input data across calls
    input_buffer: Vec<u8>,
}

// SAFETY: No native pointers, pure Rust
unsafe impl Send for Decoder {}

impl Decoder {
    /// Create a new MP2/MP3 decoder.
    ///
    /// The decoder auto-detects format from the input stream.
    pub fn new() -> Result<Self, CodecError> {
        Ok(Self {
            format: None,
            input_buffer: Vec::with_capacity(8192),
        })
    }

    /// Get the detected audio format (available after first decode)
    pub fn format(&self) -> Option<AudioFormat> {
        self.format
    }

    /// Decode MP2/MP3 data to PCM samples.
    ///
    /// # Arguments
    /// * `data` - Compressed MP2/MP3 data
    /// * `output` - Output buffer for decoded PCM samples (i16)
    ///
    /// # Returns
    /// Number of samples written to output, or error.
    /// May return 0 if more input data is needed.
    pub fn decode(&mut self, data: &[u8], output: &mut [i16]) -> Result<usize, CodecError> {
        let output_bytes = unsafe {
            std::slice::from_raw_parts_mut(output.as_mut_ptr() as *mut u8, output.len() * 2)
        };

        self.decode_bytes(data, output_bytes).map(|bytes| bytes / 2)
    }

    /// Decode MP2/MP3 data to raw bytes.
    ///
    /// Returns number of bytes written to output.
    pub fn decode_bytes(&mut self, data: &[u8], output: &mut [u8]) -> Result<usize, CodecError> {
        // Accumulate input data
        self.input_buffer.extend_from_slice(data);

        // Need enough data for at least a frame header (4 bytes) plus some frame data
        if self.input_buffer.len() < 128 {
            return Ok(0);
        }

        // Create a media source from the accumulated buffer
        let cursor = Cursor::new(self.input_buffer.clone());
        let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

        // Probe the format
        let mut hint = Hint::new();
        hint.with_extension("mp2");

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

        // Find the audio track
        let track = match format_reader.tracks().iter().find(|t| {
            matches!(
                t.codec_params.codec,
                CODEC_TYPE_MP1 | CODEC_TYPE_MP2 | CODEC_TYPE_MP3
            )
        }) {
            Some(t) => t,
            None => return Err(CodecError::Other("No MPEG audio track found".to_string())),
        };

        let track_id = track.id;

        // Update format info
        if let (Some(sample_rate), Some(channels)) = (
            track.codec_params.sample_rate,
            track.codec_params.channels,
        ) {
            self.format = Some(AudioFormat::new(sample_rate, channels.count() as u8));
        }

        // Create decoder
        let decoder_opts = DecoderOptions::default();
        let mut decoder = match symphonia::default::get_codecs()
            .make(&track.codec_params, &decoder_opts)
        {
            Ok(d) => d,
            Err(e) => {
                return Err(CodecError::Other(format!("Failed to create decoder: {}", e)));
            }
        };

        let mut total_bytes = 0;

        // Decode packets
        loop {
            let packet = match format_reader.next_packet() {
                Ok(p) => p,
                Err(symphonia::core::errors::Error::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    // End of current buffer, keep accumulated data for next call
                    break;
                }
                Err(_) => break,
            };

            if packet.track_id() != track_id {
                continue;
            }

            match decoder.decode(&packet) {
                Ok(decoded) => {
                    let bytes_written = copy_audio_to_i16_bytes(&decoded, &mut output[total_bytes..]);
                    total_bytes += bytes_written;

                    if total_bytes + 8192 > output.len() {
                        // Output buffer getting full
                        break;
                    }
                }
                Err(symphonia::core::errors::Error::DecodeError(_)) => {
                    // Skip corrupted frames
                    continue;
                }
                Err(_) => break,
            }
        }

        // Clear consumed data (keep any remaining for next call)
        // For streaming, we clear all since we process what we can
        if total_bytes > 0 {
            self.input_buffer.clear();
        }

        Ok(total_bytes)
    }

    /// Feed compressed data without immediate decoding.
    ///
    /// Use this to buffer data, then call read() to get decoded output.
    pub fn feed(&mut self, data: &[u8]) -> Result<(), CodecError> {
        self.input_buffer.extend_from_slice(data);
        Ok(())
    }

    /// Read decoded output from previously fed data.
    ///
    /// Returns number of bytes written to output.
    pub fn read(&mut self, output: &mut [u8]) -> Result<usize, CodecError> {
        if self.input_buffer.is_empty() {
            return Ok(0);
        }

        // Decode the buffered data
        let data = std::mem::take(&mut self.input_buffer);
        self.decode_bytes(&data, output)
    }

    /// Read decoded output as i16 samples.
    pub fn read_samples(&mut self, output: &mut [i16]) -> Result<usize, CodecError> {
        let output_bytes =
            unsafe { std::slice::from_raw_parts_mut(output.as_mut_ptr() as *mut u8, output.len() * 2) };

        self.read(output_bytes).map(|bytes| bytes / 2)
    }
}

/// Copy decoded audio to i16 bytes buffer
fn copy_audio_to_i16_bytes(decoded: &AudioBufferRef, output: &mut [u8]) -> usize {
    match decoded {
        AudioBufferRef::S16(buf) => {
            let samples = buf.chan(0).len() * buf.spec().channels.count();
            let bytes_needed = samples * 2;

            if bytes_needed > output.len() {
                return 0;
            }

            let channels = buf.spec().channels.count();
            let frames = buf.frames();
            let mut offset = 0;

            // Interleave channels
            for frame in 0..frames {
                for ch in 0..channels {
                    let sample = buf.chan(ch)[frame];
                    let bytes = sample.to_le_bytes();
                    if offset + 2 <= output.len() {
                        output[offset] = bytes[0];
                        output[offset + 1] = bytes[1];
                        offset += 2;
                    }
                }
            }

            offset
        }
        AudioBufferRef::S32(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();
            let bytes_needed = frames * channels * 2;

            if bytes_needed > output.len() {
                return 0;
            }

            let mut offset = 0;

            // Convert S32 to S16 and interleave
            for frame in 0..frames {
                for ch in 0..channels {
                    let sample = buf.chan(ch)[frame];
                    // Convert 32-bit to 16-bit by taking upper 16 bits
                    let sample_16 = (sample >> 16) as i16;
                    let bytes = sample_16.to_le_bytes();
                    if offset + 2 <= output.len() {
                        output[offset] = bytes[0];
                        output[offset + 1] = bytes[1];
                        offset += 2;
                    }
                }
            }

            offset
        }
        AudioBufferRef::F32(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();
            let bytes_needed = frames * channels * 2;

            if bytes_needed > output.len() {
                return 0;
            }

            let mut offset = 0;

            // Convert F32 to S16 and interleave
            for frame in 0..frames {
                for ch in 0..channels {
                    let sample = buf.chan(ch)[frame];
                    let sample_16 = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
                    let bytes = sample_16.to_le_bytes();
                    if offset + 2 <= output.len() {
                        output[offset] = bytes[0];
                        output[offset + 1] = bytes[1];
                        offset += 2;
                    }
                }
            }

            offset
        }
        _ => 0,
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new().expect("Failed to create Symphonia decoder")
    }
}

/// Get error message (compatibility function, Symphonia uses Rust errors)
pub fn error_string(error: i32) -> String {
    format!("Symphonia decoder error code: {}", error)
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
        let msg = error_string(-1);
        assert!(!msg.is_empty());
        println!("Error message: {}", msg);
    }
}
