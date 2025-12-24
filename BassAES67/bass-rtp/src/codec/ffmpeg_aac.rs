//! FFmpeg AAC decoder bindings for bass-rtp.
//!
//! AAC (Advanced Audio Coding) is a lossy audio codec commonly used for
//! streaming and broadcast. This module uses FFmpeg's libavcodec for
//! decoding AAC audio received from Z/IP ONE (PT 99 / MP2-AAC Xstream).
//!
//! Note: AAC encoding is not supported due to FFmpeg API complexity.
//! For sending to Z/IP ONE, use PCM or MP2 codecs instead.
//!
//! Requires: avcodec-62.dll, avutil-60.dll (Windows)
//!           libavcodec.so.62, libavutil.so.60 (Linux)
//!
//! If FFmpeg DLLs are not present, AAC codec will be gracefully disabled
//! rather than crashing.

#![allow(dead_code)]
#![allow(non_camel_case_types)]

use std::ffi::c_int;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;

use super::CodecError;

// ============================================================================
// FFmpeg Availability Check
// ============================================================================

static FFMPEG_INIT: Once = Once::new();
static FFMPEG_AVAILABLE: AtomicBool = AtomicBool::new(false);

/// Check if FFmpeg libraries are available.
/// This is checked once on first call and cached.
pub fn is_available() -> bool {
    FFMPEG_INIT.call_once(|| {
        let available = check_ffmpeg_available();
        FFMPEG_AVAILABLE.store(available, Ordering::SeqCst);
        if !available {
            eprintln!("FFmpeg libraries not found - AAC codec disabled");
        }
    });
    FFMPEG_AVAILABLE.load(Ordering::SeqCst)
}

#[cfg(target_os = "windows")]
fn check_ffmpeg_available() -> bool {
    // Try to load the DLLs dynamically to check if they exist
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    unsafe {
        // Convert to wide string for LoadLibraryW
        let avcodec: Vec<u16> = OsStr::new("avcodec-62.dll")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let avutil: Vec<u16> = OsStr::new("avutil-60.dll")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        #[link(name = "kernel32")]
        extern "system" {
            fn LoadLibraryW(lpFileName: *const u16) -> *mut std::ffi::c_void;
            fn FreeLibrary(hModule: *mut std::ffi::c_void) -> i32;
        }

        let h_avcodec = LoadLibraryW(avcodec.as_ptr());
        if h_avcodec.is_null() {
            return false;
        }

        let h_avutil = LoadLibraryW(avutil.as_ptr());
        if h_avutil.is_null() {
            FreeLibrary(h_avcodec);
            return false;
        }

        // Libraries loaded successfully - they'll stay loaded for the process
        // (we don't FreeLibrary here since we'll use them)
        true
    }
}

#[cfg(target_os = "linux")]
fn check_ffmpeg_available() -> bool {
    use std::ffi::CString;

    unsafe {
        #[link(name = "dl")]
        extern "C" {
            fn dlopen(filename: *const i8, flag: c_int) -> *mut std::ffi::c_void;
        }

        const RTLD_NOW: c_int = 2;

        let avcodec = CString::new("libavcodec.so.62").unwrap();
        let avutil = CString::new("libavutil.so.60").unwrap();

        let h_avcodec = dlopen(avcodec.as_ptr(), RTLD_NOW);
        if h_avcodec.is_null() {
            // Try without version
            let avcodec = CString::new("libavcodec.so").unwrap();
            let h_avcodec = dlopen(avcodec.as_ptr(), RTLD_NOW);
            if h_avcodec.is_null() {
                return false;
            }
        }

        let h_avutil = dlopen(avutil.as_ptr(), RTLD_NOW);
        if h_avutil.is_null() {
            let avutil = CString::new("libavutil.so").unwrap();
            let h_avutil = dlopen(avutil.as_ptr(), RTLD_NOW);
            if h_avutil.is_null() {
                return false;
            }
        }

        true
    }
}

#[cfg(target_os = "macos")]
fn check_ffmpeg_available() -> bool {
    // Similar to Linux but with .dylib
    false // TODO: Implement for macOS if needed
}

// ============================================================================
// FFmpeg Types (opaque)
// ============================================================================

/// Opaque codec structure
#[repr(C)]
pub struct AVCodec {
    _private: [u8; 0],
}

/// Opaque codec context structure
#[repr(C)]
pub struct AVCodecContext {
    _private: [u8; 0],
}

/// Opaque frame structure
#[repr(C)]
pub struct AVFrame {
    _private: [u8; 0],
}

/// Opaque packet structure
#[repr(C)]
pub struct AVPacket {
    _private: [u8; 0],
}


// ============================================================================
// FFmpeg Constants
// ============================================================================

/// AV_CODEC_ID_AAC = 0x15000 + 2 = 86018
const AV_CODEC_ID_AAC: c_int = 0x15002;

/// Sample format: signed 16-bit
const AV_SAMPLE_FMT_S16: c_int = 1;

/// Sample format: float
const AV_SAMPLE_FMT_FLT: c_int = 3;

/// Sample format: signed 16-bit planar
const AV_SAMPLE_FMT_S16P: c_int = 6;

/// Sample format: float planar
const AV_SAMPLE_FMT_FLTP: c_int = 8;

/// AVERROR_EAGAIN - need more input
const AVERROR_EAGAIN: c_int = -11; // -EAGAIN on most systems

/// AVERROR_EOF - end of file
const AVERROR_EOF: c_int = -(('E' as c_int) | (('O' as c_int) << 8) | (('F' as c_int) << 16) | ((' ' as c_int) << 24));

// ============================================================================
// FFmpeg FFI Bindings
// ============================================================================

#[cfg(target_os = "windows")]
#[link(name = "avcodec")]
extern "C" {
    // Codec lookup
    fn avcodec_find_decoder(id: c_int) -> *const AVCodec;

    // Context management
    fn avcodec_alloc_context3(codec: *const AVCodec) -> *mut AVCodecContext;
    fn avcodec_free_context(ctx: *mut *mut AVCodecContext);
    fn avcodec_open2(ctx: *mut AVCodecContext, codec: *const AVCodec, options: *mut *mut std::ffi::c_void) -> c_int;

    // Decode API
    fn avcodec_send_packet(ctx: *mut AVCodecContext, pkt: *const AVPacket) -> c_int;
    fn avcodec_receive_frame(ctx: *mut AVCodecContext, frame: *mut AVFrame) -> c_int;

    // Context properties
    fn av_opt_set_int(obj: *mut std::ffi::c_void, name: *const i8, val: i64, search_flags: c_int) -> c_int;
}

#[cfg(target_os = "windows")]
#[link(name = "avutil")]
extern "C" {
    // Frame management
    fn av_frame_alloc() -> *mut AVFrame;
    fn av_frame_free(frame: *mut *mut AVFrame);
    fn av_frame_unref(frame: *mut AVFrame);

    // Packet management
    fn av_packet_alloc() -> *mut AVPacket;
    fn av_packet_free(pkt: *mut *mut AVPacket);
    fn av_packet_unref(pkt: *mut AVPacket);

    // Error handling
    fn av_strerror(errnum: c_int, errbuf: *mut i8, errbuf_size: usize) -> c_int;
}

#[cfg(target_os = "linux")]
#[link(name = "avcodec")]
extern "C" {
    fn avcodec_find_decoder(id: c_int) -> *const AVCodec;
    fn avcodec_alloc_context3(codec: *const AVCodec) -> *mut AVCodecContext;
    fn avcodec_free_context(ctx: *mut *mut AVCodecContext);
    fn avcodec_open2(ctx: *mut AVCodecContext, codec: *const AVCodec, options: *mut *mut std::ffi::c_void) -> c_int;
    fn avcodec_send_packet(ctx: *mut AVCodecContext, pkt: *const AVPacket) -> c_int;
    fn avcodec_receive_frame(ctx: *mut AVCodecContext, frame: *mut AVFrame) -> c_int;
    fn av_opt_set_int(obj: *mut std::ffi::c_void, name: *const i8, val: i64, search_flags: c_int) -> c_int;
}

#[cfg(target_os = "linux")]
#[link(name = "avutil")]
extern "C" {
    fn av_frame_alloc() -> *mut AVFrame;
    fn av_frame_free(frame: *mut *mut AVFrame);
    fn av_frame_unref(frame: *mut AVFrame);
    fn av_packet_alloc() -> *mut AVPacket;
    fn av_packet_free(pkt: *mut *mut AVPacket);
    fn av_packet_unref(pkt: *mut AVPacket);
    fn av_strerror(errnum: c_int, errbuf: *mut i8, errbuf_size: usize) -> c_int;
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Get error message for an FFmpeg error code
pub fn error_string(errnum: c_int) -> String {
    let mut buf = [0i8; 256];
    unsafe {
        av_strerror(errnum, buf.as_mut_ptr(), buf.len());
        std::ffi::CStr::from_ptr(buf.as_ptr())
            .to_string_lossy()
            .into_owned()
    }
}

// ============================================================================
// AAC Decoder
// ============================================================================

/// AAC Decoder using FFmpeg libavcodec
///
/// Supports AAC with ADTS format (Z/IP ONE PT 99 / MP2-AAC Xstream).
/// Note: PT 122 (AAC-LATM) is NOT supported - requires native LATM decoder.
pub struct Decoder {
    /// AAC decoder context
    ctx: *mut AVCodecContext,
    frame: *mut AVFrame,
    packet: *mut AVPacket,
    sample_rate: u32,
    channels: u8,
}

// SAFETY: FFmpeg contexts are internally managed
unsafe impl Send for Decoder {}

impl Decoder {
    /// Create a new AAC decoder.
    ///
    /// Configures for 48kHz stereo output to match our stream format.
    /// Only supports ADTS format (PT 99 / MP2-AAC Xstream).
    ///
    /// Returns error if FFmpeg libraries are not available.
    pub fn new() -> Result<Self, CodecError> {
        // Check FFmpeg availability first (safe check before calling FFmpeg functions)
        if !is_available() {
            return Err(CodecError::Other("FFmpeg not available - AAC codec disabled".to_string()));
        }

        unsafe {
            // Find the AAC decoder
            let codec = avcodec_find_decoder(AV_CODEC_ID_AAC);
            if codec.is_null() {
                return Err(CodecError::Other("AAC decoder not found".to_string()));
            }

            // Allocate context
            let ctx = avcodec_alloc_context3(codec);
            if ctx.is_null() {
                return Err(CodecError::NotInitialized);
            }

            // Configure decoder
            av_opt_set_int(ctx as *mut _, b"sample_rate\0".as_ptr() as *const i8, 48000, 0);

            // Open the decoder
            let ret = avcodec_open2(ctx, codec, ptr::null_mut());
            if ret < 0 {
                avcodec_free_context(&mut (ctx as *mut _));
                return Err(CodecError::LibraryError(ret));
            }

            // Allocate frame and packet
            let frame = av_frame_alloc();
            let packet = av_packet_alloc();
            if frame.is_null() || packet.is_null() {
                if !frame.is_null() {
                    av_frame_free(&mut (frame as *mut _));
                }
                if !packet.is_null() {
                    av_packet_free(&mut (packet as *mut _));
                }
                avcodec_free_context(&mut (ctx as *mut _));
                return Err(CodecError::NotInitialized);
            }

            Ok(Self {
                ctx,
                frame,
                packet,
                sample_rate: 48000,
                channels: 2,
            })
        }
    }

    /// Decode AAC data to PCM samples.
    ///
    /// Only supports ADTS format (PT 99 / MP2-AAC Xstream).
    /// PT 122 (LATM) is not supported.
    ///
    /// # Arguments
    /// * `data` - AAC compressed data (ADTS format or RFC 3640 with AU headers)
    /// * `output` - Output buffer for f32 interleaved stereo samples
    ///
    /// # Returns
    /// Number of samples written (total, including all channels), or error.
    pub fn decode(&mut self, data: &[u8], output: &mut [f32]) -> Result<usize, CodecError> {
        if data.len() < 4 {
            return Ok(0); // Too small
        }

        // Prepare data - strip AU headers if present
        let aac_data = self.prepare_data(data);

        unsafe {
            // Unref any previous packet data
            av_packet_unref(self.packet);

            // Set up packet with data
            #[repr(C)]
            struct PacketHeader {
                buf: *mut std::ffi::c_void,
                pts: i64,
                dts: i64,
                data: *mut u8,
                size: c_int,
            }

            let pkt_header = self.packet as *mut PacketHeader;
            (*pkt_header).data = aac_data.as_ptr() as *mut u8;
            (*pkt_header).size = aac_data.len() as c_int;

            // Send packet to decoder
            let ret = avcodec_send_packet(self.ctx, self.packet);
            if ret < 0 && ret != AVERROR_EAGAIN {
                return Err(CodecError::DecodeError(error_string(ret)));
            }

            // Receive decoded frame
            av_frame_unref(self.frame);
            let ret = avcodec_receive_frame(self.ctx, self.frame);
            if ret < 0 {
                if ret == AVERROR_EAGAIN {
                    return Ok(0); // Need more input
                }
                return Err(CodecError::DecodeError(error_string(ret)));
            }

            self.extract_samples(output)
        }
    }

    /// Prepare AAC data for decoding - strip AU headers if present.
    fn prepare_data<'a>(&self, data: &'a [u8]) -> &'a [u8] {
        // ADTS sync word: 0xFFF (first 12 bits) - pass through directly
        if data[0] == 0xFF && (data[1] & 0xF0) == 0xF0 {
            return data;
        }

        // RFC 3640 AU header check (for MP2-AAC Xstream / PT 99)
        // AU-headers-length is typically 0x0010 (16 bits = 2 bytes of AU headers)
        let au_hdr_len = ((data[0] as u16) << 8) | (data[1] as u16);
        if au_hdr_len == 0x0010 && data.len() > 4 {
            // Skip 2 bytes length + 2 bytes AU header = 4 bytes
            return &data[4..];
        }

        // Pass through as-is
        data
    }

    /// Extract samples from decoded frame to output buffer.
    fn extract_samples(&self, output: &mut [f32]) -> Result<usize, CodecError> {
        unsafe {
            #[repr(C)]
            struct FrameHeader {
                data: [*mut u8; 8],
                linesize: [c_int; 8],
                extended_data: *mut *mut u8,
                width: c_int,
                height: c_int,
                nb_samples: c_int,
                format: c_int,
            }

            let frame_header = self.frame as *const FrameHeader;
            let nb_samples = (*frame_header).nb_samples as usize;
            let format = (*frame_header).format;

            // Calculate output samples (stereo interleaved)
            let output_samples = nb_samples * 2; // Always output stereo

            if output.len() < output_samples {
                return Err(CodecError::BufferTooSmall);
            }

            // Convert based on sample format
            match format {
                f if f == AV_SAMPLE_FMT_FLTP => {
                    // Float planar - channels in separate planes
                    let left = (*frame_header).data[0] as *const f32;
                    let right = if (*frame_header).data[1].is_null() {
                        left // Mono source - duplicate
                    } else {
                        (*frame_header).data[1] as *const f32
                    };

                    for i in 0..nb_samples {
                        output[i * 2] = *left.add(i);
                        output[i * 2 + 1] = *right.add(i);
                    }
                }
                f if f == AV_SAMPLE_FMT_S16P => {
                    // Signed 16-bit planar
                    let left = (*frame_header).data[0] as *const i16;
                    let right = if (*frame_header).data[1].is_null() {
                        left
                    } else {
                        (*frame_header).data[1] as *const i16
                    };

                    for i in 0..nb_samples {
                        output[i * 2] = *left.add(i) as f32 / 32768.0;
                        output[i * 2 + 1] = *right.add(i) as f32 / 32768.0;
                    }
                }
                f if f == AV_SAMPLE_FMT_FLT => {
                    // Float interleaved
                    let samples = (*frame_header).data[0] as *const f32;
                    for i in 0..output_samples {
                        output[i] = *samples.add(i);
                    }
                }
                f if f == AV_SAMPLE_FMT_S16 => {
                    // Signed 16-bit interleaved
                    let samples = (*frame_header).data[0] as *const i16;
                    for i in 0..output_samples {
                        output[i] = *samples.add(i) as f32 / 32768.0;
                    }
                }
                _ => {
                    return Err(CodecError::Other(format!("Unsupported sample format: {}", format)));
                }
            }

            Ok(output_samples)
        }
    }

    /// Get detected sample rate
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Get detected channel count
    pub fn channels(&self) -> u8 {
        self.channels
    }

    /// Strip RFC 3640 AU headers from AAC RTP payload.
    ///
    /// RFC 3640 format:
    /// - 2 bytes: AU-headers-length (in bits)
    /// - N bytes: AU headers (typically 2 bytes each: 13 bits size + 3 bits index)
    /// - Remaining: Raw AAC frame data
    ///
    /// Also handles:
    /// - Raw AAC frames (no headers)
    /// - ADTS-wrapped AAC (sync word 0xFFF)
    ///
    /// Returns the raw AAC data portion.
    fn strip_au_headers<'a>(&self, data: &'a [u8]) -> &'a [u8] {
        if data.len() < 4 {
            return data;
        }

        // Check for ADTS sync word (0xFFF in first 12 bits)
        // ADTS header: 0xFF 0xFx where x has upper nibble = 0xF
        if data[0] == 0xFF && (data[1] & 0xF0) == 0xF0 {
            // This is ADTS-wrapped AAC, pass through as-is
            // FFmpeg can decode ADTS directly
            return data;
        }

        // Check if this looks like RFC 3640 format
        // AU-headers-length is in bits, typically 16 bits (2 bytes) for single AU
        let au_headers_length_bits = ((data[0] as u16) << 8) | (data[1] as u16);

        // Sanity check: AU-headers-length should be reasonable (16-128 bits typically)
        // Common value is 16 bits (0x0010) for single AU with 13-bit size + 3-bit index
        if au_headers_length_bits == 0 || au_headers_length_bits > 256 {
            // Doesn't look like RFC 3640, assume raw AAC
            return data;
        }

        // Calculate AU-headers size in bytes (round up)
        let au_headers_bytes = ((au_headers_length_bits + 7) / 8) as usize;

        // Total header overhead: 2 bytes for length + AU headers
        let header_size = 2 + au_headers_bytes;

        if header_size >= data.len() {
            // Header would consume all data, not valid
            return data;
        }

        // Return data after the AU headers
        &data[header_size..]
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            if !self.frame.is_null() {
                av_frame_free(&mut self.frame);
            }
            if !self.packet.is_null() {
                av_packet_free(&mut self.packet);
            }
            if !self.ctx.is_null() {
                avcodec_free_context(&mut self.ctx);
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decoder_create() {
        // This test will fail if FFmpeg DLLs are not available
        // In that case, it's expected behavior
        match Decoder::new() {
            Ok(decoder) => {
                assert_eq!(decoder.sample_rate(), 48000);
                assert_eq!(decoder.channels(), 2);
            }
            Err(e) => {
                eprintln!("FFmpeg not available: {:?}", e);
            }
        }
    }
}
