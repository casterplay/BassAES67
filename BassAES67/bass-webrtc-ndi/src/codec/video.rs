//! FFmpeg video decoder bindings for bass-webrtc-ndi.
//!
//! Supports H.264, VP8, and VP9 video decoding from WebRTC streams.
//! Uses FFmpeg's libavcodec for decoding and libswscale for color conversion.
//!
//! Requires: avcodec-62.dll, avutil-60.dll, swscale-9.dll (Windows)
//!
//! If FFmpeg DLLs are not present, video decoding will be gracefully disabled.

#![allow(dead_code)]
#![allow(non_camel_case_types)]

use std::ffi::c_int;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;

use crate::frame::{VideoFrame, VideoFormat};
use super::CodecError;

// ============================================================================
// FFmpeg Availability Check
// ============================================================================

static FFMPEG_VIDEO_INIT: Once = Once::new();
static FFMPEG_VIDEO_AVAILABLE: AtomicBool = AtomicBool::new(false);

/// Check if FFmpeg video libraries are available.
pub fn is_available() -> bool {
    FFMPEG_VIDEO_INIT.call_once(|| {
        let available = check_ffmpeg_available();
        FFMPEG_VIDEO_AVAILABLE.store(available, Ordering::SeqCst);
        if available {
            println!("[FFmpeg Video] Libraries loaded successfully");
        } else {
            eprintln!("[FFmpeg Video] Libraries not found - video decoding disabled");
        }
    });
    FFMPEG_VIDEO_AVAILABLE.load(Ordering::SeqCst)
}

#[cfg(target_os = "windows")]
fn check_ffmpeg_available() -> bool {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    unsafe {
        let avcodec: Vec<u16> = OsStr::new("avcodec-62.dll")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let avutil: Vec<u16> = OsStr::new("avutil-60.dll")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let swscale: Vec<u16> = OsStr::new("swscale-9.dll")
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
            eprintln!("[FFmpeg Video] avcodec-62.dll not found");
            return false;
        }

        let h_avutil = LoadLibraryW(avutil.as_ptr());
        if h_avutil.is_null() {
            eprintln!("[FFmpeg Video] avutil-60.dll not found");
            FreeLibrary(h_avcodec);
            return false;
        }

        let h_swscale = LoadLibraryW(swscale.as_ptr());
        if h_swscale.is_null() {
            eprintln!("[FFmpeg Video] swscale-9.dll not found");
            FreeLibrary(h_avcodec);
            FreeLibrary(h_avutil);
            return false;
        }

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

        let libs = [
            ("libavcodec.so.62", "libavcodec.so"),
            ("libavutil.so.60", "libavutil.so"),
            ("libswscale.so.9", "libswscale.so"),
        ];

        for (versioned, unversioned) in libs {
            let name = CString::new(versioned).unwrap();
            let handle = dlopen(name.as_ptr(), RTLD_NOW);
            if handle.is_null() {
                let name = CString::new(unversioned).unwrap();
                let handle = dlopen(name.as_ptr(), RTLD_NOW);
                if handle.is_null() {
                    return false;
                }
            }
        }

        true
    }
}

#[cfg(target_os = "macos")]
fn check_ffmpeg_available() -> bool {
    false
}

// ============================================================================
// FFmpeg Types (opaque)
// ============================================================================

#[repr(C)]
pub struct AVCodec {
    _private: [u8; 0],
}

#[repr(C)]
pub struct AVCodecContext {
    _private: [u8; 0],
}

#[repr(C)]
pub struct AVFrame {
    _private: [u8; 0],
}

#[repr(C)]
pub struct AVPacket {
    _private: [u8; 0],
}

#[repr(C)]
pub struct SwsContext {
    _private: [u8; 0],
}

// ============================================================================
// FFmpeg Constants
// ============================================================================

/// Codec IDs
const AV_CODEC_ID_H264: c_int = 27;
const AV_CODEC_ID_VP8: c_int = 139;
const AV_CODEC_ID_VP9: c_int = 167;

/// Pixel formats
const AV_PIX_FMT_YUV420P: c_int = 0;
const AV_PIX_FMT_BGRA: c_int = 28;
const AV_PIX_FMT_RGBA: c_int = 26;

/// Scaling flags
const SWS_BILINEAR: c_int = 2;
const SWS_FAST_BILINEAR: c_int = 1;

/// Error codes
const AVERROR_EAGAIN: c_int = -11;

// ============================================================================
// FFmpeg FFI Bindings
// ============================================================================

#[cfg(target_os = "windows")]
#[link(name = "avcodec")]
extern "C" {
    fn avcodec_find_decoder(id: c_int) -> *const AVCodec;
    fn avcodec_alloc_context3(codec: *const AVCodec) -> *mut AVCodecContext;
    fn avcodec_free_context(ctx: *mut *mut AVCodecContext);
    fn avcodec_open2(ctx: *mut AVCodecContext, codec: *const AVCodec, options: *mut *mut std::ffi::c_void) -> c_int;
    fn avcodec_send_packet(ctx: *mut AVCodecContext, pkt: *const AVPacket) -> c_int;
    fn avcodec_receive_frame(ctx: *mut AVCodecContext, frame: *mut AVFrame) -> c_int;
}

#[cfg(target_os = "windows")]
#[link(name = "avutil")]
extern "C" {
    fn av_frame_alloc() -> *mut AVFrame;
    fn av_frame_free(frame: *mut *mut AVFrame);
    fn av_frame_unref(frame: *mut AVFrame);
    fn av_packet_alloc() -> *mut AVPacket;
    fn av_packet_free(pkt: *mut *mut AVPacket);
    fn av_packet_unref(pkt: *mut AVPacket);
    fn av_strerror(errnum: c_int, errbuf: *mut i8, errbuf_size: usize) -> c_int;
    fn av_image_get_buffer_size(pix_fmt: c_int, width: c_int, height: c_int, align: c_int) -> c_int;
    fn av_image_fill_arrays(
        dst_data: *mut *mut u8,
        dst_linesize: *mut c_int,
        src: *const u8,
        pix_fmt: c_int,
        width: c_int,
        height: c_int,
        align: c_int,
    ) -> c_int;
}

#[cfg(target_os = "windows")]
#[link(name = "swscale")]
extern "C" {
    fn sws_getContext(
        srcW: c_int,
        srcH: c_int,
        srcFormat: c_int,
        dstW: c_int,
        dstH: c_int,
        dstFormat: c_int,
        flags: c_int,
        srcFilter: *mut std::ffi::c_void,
        dstFilter: *mut std::ffi::c_void,
        param: *const f64,
    ) -> *mut SwsContext;
    fn sws_scale(
        c: *mut SwsContext,
        srcSlice: *const *const u8,
        srcStride: *const c_int,
        srcSliceY: c_int,
        srcSliceH: c_int,
        dst: *const *mut u8,
        dstStride: *const c_int,
    ) -> c_int;
    fn sws_freeContext(swsContext: *mut SwsContext);
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
    fn av_image_get_buffer_size(pix_fmt: c_int, width: c_int, height: c_int, align: c_int) -> c_int;
    fn av_image_fill_arrays(
        dst_data: *mut *mut u8,
        dst_linesize: *mut c_int,
        src: *const u8,
        pix_fmt: c_int,
        width: c_int,
        height: c_int,
        align: c_int,
    ) -> c_int;
}

#[cfg(target_os = "linux")]
#[link(name = "swscale")]
extern "C" {
    fn sws_getContext(
        srcW: c_int,
        srcH: c_int,
        srcFormat: c_int,
        dstW: c_int,
        dstH: c_int,
        dstFormat: c_int,
        flags: c_int,
        srcFilter: *mut std::ffi::c_void,
        dstFilter: *mut std::ffi::c_void,
        param: *const f64,
    ) -> *mut SwsContext;
    fn sws_scale(
        c: *mut SwsContext,
        srcSlice: *const *const u8,
        srcStride: *const c_int,
        srcSliceY: c_int,
        srcSliceH: c_int,
        dst: *const *mut u8,
        dstStride: *const c_int,
    ) -> c_int;
    fn sws_freeContext(swsContext: *mut SwsContext);
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
// Video Codec Type
// ============================================================================

/// Supported video codecs
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VideoCodec {
    H264,
    VP8,
    VP9,
}

impl VideoCodec {
    /// Get FFmpeg codec ID
    fn to_av_codec_id(self) -> c_int {
        match self {
            VideoCodec::H264 => AV_CODEC_ID_H264,
            VideoCodec::VP8 => AV_CODEC_ID_VP8,
            VideoCodec::VP9 => AV_CODEC_ID_VP9,
        }
    }

    /// Detect codec from MIME type string
    pub fn from_mime_type(mime: &str) -> Option<Self> {
        let mime_lower = mime.to_lowercase();
        if mime_lower.contains("h264") || mime_lower.contains("avc") {
            Some(VideoCodec::H264)
        } else if mime_lower.contains("vp9") {
            Some(VideoCodec::VP9)
        } else if mime_lower.contains("vp8") {
            Some(VideoCodec::VP8)
        } else {
            None
        }
    }
}

// ============================================================================
// Video Decoder
// ============================================================================

/// FFmpeg video decoder with swscale color conversion
pub struct VideoDecoder {
    ctx: *mut AVCodecContext,
    frame: *mut AVFrame,
    packet: *mut AVPacket,
    sws_ctx: *mut SwsContext,
    codec: VideoCodec,
    width: u32,
    height: u32,
    output_format: VideoFormat,
    /// Buffer for converted BGRA output
    output_buffer: Vec<u8>,
    /// Flag indicating if we've received first frame (to setup swscale)
    initialized: bool,
}

unsafe impl Send for VideoDecoder {}

impl VideoDecoder {
    /// Create a new video decoder for the specified codec.
    pub fn new(codec: VideoCodec) -> Result<Self, CodecError> {
        if !is_available() {
            return Err(CodecError::Other("FFmpeg video libraries not available".to_string()));
        }

        unsafe {
            let av_codec = avcodec_find_decoder(codec.to_av_codec_id());
            if av_codec.is_null() {
                return Err(CodecError::Other(format!("Decoder for {:?} not found", codec)));
            }

            let ctx = avcodec_alloc_context3(av_codec);
            if ctx.is_null() {
                return Err(CodecError::NotInitialized);
            }

            let ret = avcodec_open2(ctx, av_codec, ptr::null_mut());
            if ret < 0 {
                avcodec_free_context(&mut (ctx as *mut _));
                return Err(CodecError::LibraryError(ret));
            }

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
                sws_ctx: ptr::null_mut(),
                codec,
                width: 0,
                height: 0,
                output_format: VideoFormat::BGRA,
                output_buffer: Vec::new(),
                initialized: false,
            })
        }
    }

    /// Decode video data and return a VideoFrame if available.
    ///
    /// Returns Ok(None) if more data is needed, Ok(Some(frame)) on success.
    pub fn decode(&mut self, data: &[u8]) -> Result<Option<VideoFrame>, CodecError> {
        if data.is_empty() {
            return Ok(None);
        }

        unsafe {
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
            (*pkt_header).data = data.as_ptr() as *mut u8;
            (*pkt_header).size = data.len() as c_int;

            // Send packet to decoder
            let ret = avcodec_send_packet(self.ctx, self.packet);
            if ret < 0 && ret != AVERROR_EAGAIN {
                return Err(CodecError::DecodeError(error_string(ret)));
            }

            // Try to receive decoded frame
            av_frame_unref(self.frame);
            let ret = avcodec_receive_frame(self.ctx, self.frame);
            if ret < 0 {
                if ret == AVERROR_EAGAIN {
                    return Ok(None); // Need more input
                }
                return Err(CodecError::DecodeError(error_string(ret)));
            }

            // Frame decoded - convert to BGRA
            self.convert_frame()
        }
    }

    /// Convert decoded frame to BGRA VideoFrame
    unsafe fn convert_frame(&mut self) -> Result<Option<VideoFrame>, CodecError> {
        // Access frame header to get dimensions and format
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
        let width = (*frame_header).width as u32;
        let height = (*frame_header).height as u32;
        let src_format = (*frame_header).format;

        if width == 0 || height == 0 {
            return Ok(None);
        }

        // Update dimensions if changed
        if self.width != width || self.height != height || !self.initialized {
            self.width = width;
            self.height = height;

            // Free old swscale context
            if !self.sws_ctx.is_null() {
                sws_freeContext(self.sws_ctx);
            }

            // Create new swscale context for YUV420P -> BGRA conversion
            self.sws_ctx = sws_getContext(
                width as c_int,
                height as c_int,
                src_format, // Source format (usually YUV420P)
                width as c_int,
                height as c_int,
                AV_PIX_FMT_BGRA, // Output format for NDI
                SWS_FAST_BILINEAR,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            );

            if self.sws_ctx.is_null() {
                return Err(CodecError::Other("Failed to create swscale context".to_string()));
            }

            // Resize output buffer
            let buffer_size = (width * height * 4) as usize; // BGRA = 4 bytes per pixel
            self.output_buffer.resize(buffer_size, 0);
            self.initialized = true;

            println!("[VideoDecoder] Initialized {}x{} (format {})", width, height, src_format);
        }

        // Setup destination pointers
        let dst_stride = (width * 4) as c_int;
        let dst_data = self.output_buffer.as_mut_ptr();
        let dst_slice: [*mut u8; 1] = [dst_data];
        let dst_stride_arr: [c_int; 1] = [dst_stride];

        // Convert
        let ret = sws_scale(
            self.sws_ctx,
            (*frame_header).data.as_ptr() as *const *const u8,
            (*frame_header).linesize.as_ptr(),
            0,
            height as c_int,
            dst_slice.as_ptr(),
            dst_stride_arr.as_ptr(),
        );

        if ret <= 0 {
            return Err(CodecError::Other("swscale conversion failed".to_string()));
        }

        // Create VideoFrame
        let mut frame = VideoFrame::new(width, height, VideoFormat::BGRA);
        frame.data = self.output_buffer.clone();
        frame.stride = width * 4;

        Ok(Some(frame))
    }

    /// Get detected video dimensions
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Get the codec type
    pub fn codec(&self) -> VideoCodec {
        self.codec
    }

    /// Check if decoder has been initialized with first frame
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}

impl Drop for VideoDecoder {
    fn drop(&mut self) {
        unsafe {
            if !self.sws_ctx.is_null() {
                sws_freeContext(self.sws_ctx);
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffmpeg_availability() {
        let available = is_available();
        println!("FFmpeg video available: {}", available);
    }

    #[test]
    fn test_codec_from_mime() {
        assert_eq!(VideoCodec::from_mime_type("video/H264"), Some(VideoCodec::H264));
        assert_eq!(VideoCodec::from_mime_type("video/VP8"), Some(VideoCodec::VP8));
        assert_eq!(VideoCodec::from_mime_type("video/VP9"), Some(VideoCodec::VP9));
        assert_eq!(VideoCodec::from_mime_type("video/unknown"), None);
    }
}
