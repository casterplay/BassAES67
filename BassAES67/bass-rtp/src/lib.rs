//! bass-rtp: Bidirectional RTP audio plugin for BASS with Telos Z/IP ONE codec support.
//!
//! This plugin provides bidirectional unicast RTP audio streaming with support for
//! multiple codecs including PCM-16, PCM-24, MP2, OPUS, and FLAC.

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use std::ffi::c_void;
use std::net::Ipv4Addr;

pub mod ffi;
pub mod clock_bindings;
pub mod rtp;
pub mod codec;
pub mod stream;
pub mod output;
pub mod url;

use ffi::*;
use rtp::PayloadCodec;
use stream::{BidirectionalStream, BidirectionalConfig, BufferMode, input::input_stream_proc};

// ============================================================================
// Plugin Version
// ============================================================================

const PLUGIN_VERSION: DWORD = 0x01_00_00_00; // 1.0.0.0

// ============================================================================
// Configuration Constants
// ============================================================================

/// Base config option for RTP plugin (unique range to avoid conflicts)
const BASS_CONFIG_RTP_BASE: DWORD = 0x22000;

/// Jitter buffer depth in milliseconds (simple mode)
pub const BASS_CONFIG_RTP_JITTER: DWORD = BASS_CONFIG_RTP_BASE;
/// Output codec selection (0=PCM16, 1=PCM24, 2=MP2, 3=OPUS, 4=FLAC)
pub const BASS_CONFIG_RTP_OUTPUT_CODEC: DWORD = BASS_CONFIG_RTP_BASE + 1;
/// Output bitrate in kbps (for MP2/OPUS)
pub const BASS_CONFIG_RTP_OUTPUT_BITRATE: DWORD = BASS_CONFIG_RTP_BASE + 2;
/// Network interface IP address (as string pointer)
pub const BASS_CONFIG_RTP_INTERFACE: DWORD = BASS_CONFIG_RTP_BASE + 3;
/// Clock mode (0=PTP, 1=Livewire, 2=System)
pub const BASS_CONFIG_RTP_CLOCK_MODE: DWORD = BASS_CONFIG_RTP_BASE + 4;
/// PTP domain (0-127)
pub const BASS_CONFIG_RTP_PTP_DOMAIN: DWORD = BASS_CONFIG_RTP_BASE + 5;
/// Minimum buffer depth in milliseconds (min/max mode)
pub const BASS_CONFIG_RTP_MIN_BUFFER: DWORD = BASS_CONFIG_RTP_BASE + 6;
/// Maximum buffer depth in milliseconds (min/max mode)
pub const BASS_CONFIG_RTP_MAX_BUFFER: DWORD = BASS_CONFIG_RTP_BASE + 7;
/// Buffer mode: 0 = simple (use jitter_ms), 1 = min/max
pub const BASS_CONFIG_RTP_BUFFER_MODE: DWORD = BASS_CONFIG_RTP_BASE + 8;

// Read-only statistics (base + 0x10)
/// Detected input codec payload type (read-only)
pub const BASS_CONFIG_RTP_DETECTED_CODEC: DWORD = BASS_CONFIG_RTP_BASE + 0x10;
/// Input packets received (read-only)
pub const BASS_CONFIG_RTP_INPUT_PACKETS: DWORD = BASS_CONFIG_RTP_BASE + 0x11;
/// Output packets sent (read-only)
pub const BASS_CONFIG_RTP_OUTPUT_PACKETS: DWORD = BASS_CONFIG_RTP_BASE + 0x12;
/// Buffer level percentage (read-only)
pub const BASS_CONFIG_RTP_BUFFER_LEVEL: DWORD = BASS_CONFIG_RTP_BASE + 0x13;
/// Input packets dropped (read-only)
pub const BASS_CONFIG_RTP_INPUT_DROPPED: DWORD = BASS_CONFIG_RTP_BASE + 0x14;
/// Output errors (read-only)
pub const BASS_CONFIG_RTP_OUTPUT_ERRORS: DWORD = BASS_CONFIG_RTP_BASE + 0x15;

// ============================================================================
// Codec Constants
// ============================================================================

/// PCM 16-bit codec
pub const BASS_RTP_CODEC_PCM16: u8 = 0;
/// PCM 20-bit codec (packed format)
pub const BASS_RTP_CODEC_PCM20: u8 = 1;
/// PCM 24-bit codec
pub const BASS_RTP_CODEC_PCM24: u8 = 2;
/// MP2 (MPEG-1 Layer 2) codec
pub const BASS_RTP_CODEC_MP2: u8 = 3;
/// AAC codec (MP2-AAC Xstream / PT 99)
pub const BASS_RTP_CODEC_AAC: u8 = 4;
/// OPUS codec
pub const BASS_RTP_CODEC_OPUS: u8 = 5;
/// FLAC codec
pub const BASS_RTP_CODEC_FLAC: u8 = 6;

// ============================================================================
// FFI Configuration Structure
// ============================================================================

/// Buffer mode constants
pub const BASS_RTP_BUFFER_MODE_SIMPLE: u8 = 0;
pub const BASS_RTP_BUFFER_MODE_MINMAX: u8 = 1;

/// Configuration for creating an RTP stream
#[repr(C)]
pub struct RtpStreamConfigFFI {
    /// Local port to bind (Z/IP ONE sends return audio here)
    pub local_port: u16,
    /// Remote IP address (Z/IP ONE IP as 4 bytes)
    pub remote_addr: [u8; 4],
    /// Remote port (9150=receive only, 9151=G.722, 9152=same codec, 9153=current codec)
    pub remote_port: u16,
    /// Sample rate (48000 only for now)
    pub sample_rate: u32,
    /// Number of channels (1 or 2)
    pub channels: u16,
    /// Output codec (BASS_RTP_CODEC_*)
    pub output_codec: u8,
    /// Output bitrate in kbps (for MP2/OPUS, 0 = default)
    pub output_bitrate: u32,
    /// Jitter buffer depth in milliseconds (simple mode)
    pub jitter_ms: u32,
    /// Network interface IP address (4 bytes, 0.0.0.0 = default)
    pub interface_addr: [u8; 4],
    /// Minimum buffer depth in milliseconds (min/max mode target)
    pub min_buffer_ms: u32,
    /// Maximum buffer depth in milliseconds (min/max mode ceiling)
    pub max_buffer_ms: u32,
    /// Buffer mode: BASS_RTP_BUFFER_MODE_SIMPLE (0) or BASS_RTP_BUFFER_MODE_MINMAX (1)
    pub buffer_mode: u8,
}

/// Statistics for an RTP stream
#[repr(C)]
pub struct RtpStatsFFI {
    /// Input packets received
    pub input_packets: u64,
    /// Output packets sent
    pub output_packets: u64,
    /// Input packets dropped (buffer full)
    pub input_dropped: u64,
    /// Output send errors
    pub output_errors: u64,
    /// Detected input codec payload type
    pub detected_codec: u32,
    /// Buffer level percentage (relative to target/min, 0-100+)
    pub buffer_level: u32,
    /// Current buffer level in milliseconds
    pub buffer_level_ms: u32,
    /// Target buffer in milliseconds (min in min/max mode)
    pub target_buffer_ms: u32,
    /// Max buffer in milliseconds (same as target in simple mode)
    pub max_buffer_ms: u32,
    /// True (1) if using min/max buffer mode, false (0) otherwise
    pub is_minmax_mode: u8,
}

// ============================================================================
// Plugin Format Definition
// ============================================================================

/// Format name for RTP streams
static FORMAT_NAME: &[u8] = b"RTP Audio\0";

/// URL extension pattern
static FORMAT_EXTS: &[u8] = b"rtp\0";

/// Plugin format descriptor
static PLUGIN_FORMAT: BassPluginForm = BassPluginForm {
    ctype: BASS_CTYPE_STREAM_RTP,
    name: FORMAT_NAME.as_ptr() as *const i8,
    exts: FORMAT_EXTS.as_ptr() as *const i8,
};

/// Plugin info structure
static PLUGIN_INFO: BassPluginInfo = BassPluginInfo {
    version: PLUGIN_VERSION,
    formatc: 1,
    formats: &PLUGIN_FORMAT,
};

// ============================================================================
// Global Configuration Defaults
// ============================================================================

use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};

/// Default jitter buffer depth (ms) for simple mode
static DEFAULT_JITTER_MS: AtomicU32 = AtomicU32::new(100);
/// Default output codec
static DEFAULT_OUTPUT_CODEC: AtomicU8 = AtomicU8::new(BASS_RTP_CODEC_PCM16);
/// Default output bitrate (kbps)
static DEFAULT_OUTPUT_BITRATE: AtomicU32 = AtomicU32::new(192);
/// Default clock mode (0=PTP)
static DEFAULT_CLOCK_MODE: AtomicU8 = AtomicU8::new(0);
/// Default PTP domain
static DEFAULT_PTP_DOMAIN: AtomicU8 = AtomicU8::new(0);
/// Default minimum buffer (ms) for min/max mode
static DEFAULT_MIN_BUFFER_MS: AtomicU32 = AtomicU32::new(50);
/// Default maximum buffer (ms) for min/max mode
static DEFAULT_MAX_BUFFER_MS: AtomicU32 = AtomicU32::new(200);
/// Default buffer mode (0=simple)
static DEFAULT_BUFFER_MODE: AtomicU8 = AtomicU8::new(BASS_RTP_BUFFER_MODE_SIMPLE);

// ============================================================================
// Config Handler
// ============================================================================

/// Handle BASS_SetConfig/GetConfig calls for RTP options
unsafe extern "system" fn config_proc(option: DWORD, flags: DWORD, value: *mut c_void) -> BOOL {
    let is_set = (flags & BASSCONFIG_SET) != 0;

    match option {
        BASS_CONFIG_RTP_JITTER => {
            if is_set {
                let val = value as u32;
                DEFAULT_JITTER_MS.store(val.clamp(5, 500), Ordering::Relaxed);
            } else {
                *(value as *mut u32) = DEFAULT_JITTER_MS.load(Ordering::Relaxed);
            }
            TRUE
        }
        BASS_CONFIG_RTP_OUTPUT_CODEC => {
            if is_set {
                let val = (value as u8).min(BASS_RTP_CODEC_FLAC); // FLAC is highest valid codec
                DEFAULT_OUTPUT_CODEC.store(val, Ordering::Relaxed);
            } else {
                *(value as *mut u32) = DEFAULT_OUTPUT_CODEC.load(Ordering::Relaxed) as u32;
            }
            TRUE
        }
        BASS_CONFIG_RTP_OUTPUT_BITRATE => {
            if is_set {
                let val = value as u32;
                DEFAULT_OUTPUT_BITRATE.store(val.clamp(32, 384), Ordering::Relaxed);
            } else {
                *(value as *mut u32) = DEFAULT_OUTPUT_BITRATE.load(Ordering::Relaxed);
            }
            TRUE
        }
        BASS_CONFIG_RTP_CLOCK_MODE => {
            if is_set {
                let val = (value as u8).min(2);
                DEFAULT_CLOCK_MODE.store(val, Ordering::Relaxed);
            } else {
                *(value as *mut u32) = DEFAULT_CLOCK_MODE.load(Ordering::Relaxed) as u32;
            }
            TRUE
        }
        BASS_CONFIG_RTP_PTP_DOMAIN => {
            if is_set {
                let val = (value as u8).min(127);
                DEFAULT_PTP_DOMAIN.store(val, Ordering::Relaxed);
            } else {
                *(value as *mut u32) = DEFAULT_PTP_DOMAIN.load(Ordering::Relaxed) as u32;
            }
            TRUE
        }
        BASS_CONFIG_RTP_MIN_BUFFER => {
            if is_set {
                let val = value as u32;
                DEFAULT_MIN_BUFFER_MS.store(val.clamp(20, 500), Ordering::Relaxed);
            } else {
                *(value as *mut u32) = DEFAULT_MIN_BUFFER_MS.load(Ordering::Relaxed);
            }
            TRUE
        }
        BASS_CONFIG_RTP_MAX_BUFFER => {
            if is_set {
                let val = value as u32;
                DEFAULT_MAX_BUFFER_MS.store(val.clamp(50, 1000), Ordering::Relaxed);
            } else {
                *(value as *mut u32) = DEFAULT_MAX_BUFFER_MS.load(Ordering::Relaxed);
            }
            TRUE
        }
        BASS_CONFIG_RTP_BUFFER_MODE => {
            if is_set {
                let val = (value as u8).min(BASS_RTP_BUFFER_MODE_MINMAX);
                DEFAULT_BUFFER_MODE.store(val, Ordering::Relaxed);
            } else {
                *(value as *mut u32) = DEFAULT_BUFFER_MODE.load(Ordering::Relaxed) as u32;
            }
            TRUE
        }
        // Read-only statistics - return 0 for now (will be implemented with streams)
        BASS_CONFIG_RTP_DETECTED_CODEC |
        BASS_CONFIG_RTP_INPUT_PACKETS |
        BASS_CONFIG_RTP_OUTPUT_PACKETS |
        BASS_CONFIG_RTP_BUFFER_LEVEL |
        BASS_CONFIG_RTP_INPUT_DROPPED |
        BASS_CONFIG_RTP_OUTPUT_ERRORS => {
            if !is_set {
                *(value as *mut u32) = 0;
            }
            TRUE
        }
        _ => FALSE,
    }
}

// ============================================================================
// Plugin Entry Point
// ============================================================================

/// Plugin initialization state
static INIT_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Initialize the plugin
fn init_plugin() {
    if INIT_DONE.swap(true, Ordering::SeqCst) {
        return; // Already initialized
    }

    unsafe {
        // Get BASS functions table
        if get_bass_func().is_none() {
            return;
        }

        // Register config handler
        if let Some(func) = bassfunc() {
            if let Some(register) = func.register_plugin {
                register(config_proc as *const c_void, PLUGIN_CONFIG_ADD);
            }
        }
    }

    // Initialize clock bindings (optional - may not have clock DLLs)
    let _ = clock_bindings::init_clock_bindings();
}

/// BASSplugin entry point - called by BASS to get plugin info
#[no_mangle]
pub unsafe extern "system" fn BASSplugin(face: DWORD) -> *const c_void {
    match face {
        BASSPLUGIN_INFO => {
            init_plugin();
            &PLUGIN_INFO as *const _ as *const c_void
        }
        BASSPLUGIN_CREATEURL => {
            // URL stream creation - will be implemented later
            std::ptr::null()
        }
        _ => std::ptr::null(),
    }
}

// ============================================================================
// DLL Entry Point (Windows)
// ============================================================================

#[cfg(windows)]
#[no_mangle]
pub extern "system" fn DllMain(
    _hinst: *mut c_void,
    reason: u32,
    _reserved: *mut c_void,
) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    const DLL_PROCESS_DETACH: u32 = 0;

    match reason {
        DLL_PROCESS_ATTACH => {
            // Initialization will happen on first BASSplugin call
        }
        DLL_PROCESS_DETACH => {
            // Cleanup - stop any running clocks
            clock_bindings::clock_stop();
        }
        _ => {}
    }
    1 // TRUE
}

// ============================================================================
// Public FFI API
// ============================================================================

/// Convert FFI config to internal config
fn convert_ffi_config(config: &RtpStreamConfigFFI) -> BidirectionalConfig {
    let codec = match config.output_codec {
        BASS_RTP_CODEC_PCM16 => PayloadCodec::Pcm16,
        BASS_RTP_CODEC_PCM20 => PayloadCodec::Pcm20,
        BASS_RTP_CODEC_PCM24 => PayloadCodec::Pcm24,
        BASS_RTP_CODEC_MP2 => PayloadCodec::Mp2,
        BASS_RTP_CODEC_AAC => PayloadCodec::Aac,
        BASS_RTP_CODEC_OPUS => PayloadCodec::Opus,
        BASS_RTP_CODEC_FLAC => PayloadCodec::Flac,
        _ => PayloadCodec::Pcm16,
    };

    // Determine buffer mode from config
    let buffer_mode = if config.buffer_mode == BASS_RTP_BUFFER_MODE_MINMAX {
        // Min/Max mode
        let min_ms = if config.min_buffer_ms > 0 {
            config.min_buffer_ms.clamp(20, 500)
        } else {
            DEFAULT_MIN_BUFFER_MS.load(Ordering::Relaxed)
        };
        let max_ms = if config.max_buffer_ms > 0 {
            config.max_buffer_ms.clamp(min_ms, 1000)
        } else {
            DEFAULT_MAX_BUFFER_MS.load(Ordering::Relaxed).max(min_ms)
        };
        BufferMode::MinMax { min_ms, max_ms }
    } else {
        // Simple mode (default)
        let buffer_ms = if config.jitter_ms > 0 {
            config.jitter_ms.clamp(20, 500)
        } else {
            DEFAULT_JITTER_MS.load(Ordering::Relaxed)
        };
        BufferMode::Simple { buffer_ms }
    };

    BidirectionalConfig {
        local_port: config.local_port,
        remote_addr: Ipv4Addr::new(
            config.remote_addr[0],
            config.remote_addr[1],
            config.remote_addr[2],
            config.remote_addr[3],
        ),
        remote_port: config.remote_port,
        sample_rate: config.sample_rate,
        channels: config.channels,
        output_codec: codec,
        output_bitrate: if config.output_bitrate > 0 {
            config.output_bitrate
        } else {
            DEFAULT_OUTPUT_BITRATE.load(Ordering::Relaxed)
        },
        buffer_mode,
        interface_addr: Ipv4Addr::new(
            config.interface_addr[0],
            config.interface_addr[1],
            config.interface_addr[2],
            config.interface_addr[3],
        ),
    }
}

/// Create a bidirectional RTP stream
///
/// # Arguments
/// * `bass_channel` - BASS channel to read audio from for output (use 0 for input-only)
/// * `config` - Stream configuration
///
/// # Returns
/// Opaque handle to the RTP stream, or null on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_Create(
    bass_channel: DWORD,
    config: *const RtpStreamConfigFFI,
) -> *mut c_void {
    if config.is_null() {
        set_error(BASS_ERROR_MEM);
        return std::ptr::null_mut();
    }

    let config_ref = &*config;
    let internal_config = convert_ffi_config(config_ref);

    // Create bidirectional stream
    let stream = match BidirectionalStream::new(bass_channel, internal_config) {
        Ok(s) => s,
        Err(_) => {
            set_error(BASS_ERROR_CREATE);
            return std::ptr::null_mut();
        }
    };

    // Box the stream and return as opaque pointer
    let boxed = Box::new(stream);
    Box::into_raw(boxed) as *mut c_void
}

/// Start the RTP stream (both input and output)
///
/// # Arguments
/// * `handle` - Handle from BASS_RTP_Create
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_Start(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let stream = &mut *(handle as *mut BidirectionalStream);

    // Create the BASS input stream first
    // Note: NOT using BASS_STREAM_DECODE so it can be played directly
    // For mixer use, the caller can add BASS_STREAM_DECODE when needed
    let bass_stream = BASS_StreamCreate(
        48000,
        stream.input_mut().config.channels as u32,
        BASS_SAMPLE_FLOAT,
        Some(input_stream_proc),
        stream.input_mut() as *mut _ as *mut c_void,
    );

    if bass_stream == 0 {
        set_error(BASS_ErrorGetCode());
        return 0;
    }

    stream.set_input_handle(bass_stream);

    // Start the bidirectional stream
    match stream.start() {
        Ok(_) => 1,
        Err(_) => {
            set_error(BASS_ERROR_START);
            0
        }
    }
}

/// Stop the RTP stream
///
/// # Arguments
/// * `handle` - Handle from BASS_RTP_Create
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_Stop(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let stream = &mut *(handle as *mut BidirectionalStream);
    stream.stop();
    1
}

/// Get the input stream handle for playing received audio
///
/// # Arguments
/// * `handle` - Handle from BASS_RTP_Create
///
/// # Returns
/// BASS stream handle for the input audio, or 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_GetInputStream(handle: *mut c_void) -> HSTREAM {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let stream = &*(handle as *const BidirectionalStream);
    stream.input_handle()
}

/// Get statistics for the RTP stream
///
/// # Arguments
/// * `handle` - Handle from BASS_RTP_Create
/// * `stats` - Pointer to RtpStatsFFI structure to fill
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_GetStats(
    handle: *mut c_void,
    stats: *mut RtpStatsFFI,
) -> i32 {
    if stats.is_null() {
        set_error(BASS_ERROR_MEM);
        return 0;
    }

    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let stream = &*(handle as *const BidirectionalStream);
    let bidir_stats = stream.stats();

    (*stats) = RtpStatsFFI {
        input_packets: bidir_stats.rx_packets,
        output_packets: bidir_stats.tx_packets,
        input_dropped: bidir_stats.rx_underruns,
        output_errors: bidir_stats.tx_encode_errors,
        detected_codec: bidir_stats.detected_input_pt as u32,
        buffer_level: bidir_stats.buffer_fill_percent,
        buffer_level_ms: bidir_stats.buffer_level_ms,
        target_buffer_ms: bidir_stats.target_buffer_ms,
        max_buffer_ms: bidir_stats.max_buffer_ms,
        is_minmax_mode: if bidir_stats.is_minmax_mode { 1 } else { 0 },
    };

    1
}

/// Check if the RTP stream is running
///
/// # Arguments
/// * `handle` - Handle from BASS_RTP_Create
///
/// # Returns
/// 1 if running, 0 if not running or invalid handle
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_IsRunning(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        return 0;
    }

    let stream = &*(handle as *const BidirectionalStream);
    if stream.is_running() { 1 } else { 0 }
}

/// Free resources associated with an RTP stream
///
/// # Arguments
/// * `handle` - Handle from BASS_RTP_Create
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_Free(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    // Stop the stream first
    let stream = &mut *(handle as *mut BidirectionalStream);
    stream.stop();

    // Free the BASS input stream
    let input_handle = stream.input_handle();
    if input_handle != 0 {
        BASS_StreamFree(input_handle);
    }

    // Drop the boxed stream
    let _ = Box::from_raw(handle as *mut BidirectionalStream);
    1
}

// ============================================================================
// Output Module FFI API (We connect TO Z/IP ONE)
// ============================================================================

use output::{RtpOutputBidirectional, RtpOutputConfig};
use clock_bindings::ClockMode;

/// Configuration for output RTP stream (we connect to remote)
#[repr(C)]
pub struct RtpOutputConfigFFI {
    /// Remote IP address (Z/IP ONE) as 4 bytes
    pub remote_addr: [u8; 4],
    /// Remote port (9150-9153 for Z/IP ONE, or custom)
    pub remote_port: u16,
    /// Local port to bind (0 = auto-assign)
    pub local_port: u16,
    /// Network interface IP address (4 bytes, 0.0.0.0 = default)
    pub interface_addr: [u8; 4],
    /// Sample rate (48000)
    pub sample_rate: u32,
    /// Number of channels (1 or 2)
    pub channels: u16,
    /// Send codec (BASS_RTP_CODEC_*)
    pub send_codec: u8,
    /// Send bitrate in kbps (for MP2/OPUS, 0 = default)
    pub send_bitrate: u32,
    /// Frame duration in milliseconds
    pub frame_duration_ms: u32,
    /// Clock mode (0=PTP, 1=Livewire, 2=System)
    pub clock_mode: u8,
    /// PTP domain (0-127)
    pub ptp_domain: u8,
    /// Return audio buffer mode (0=simple, 1=min/max)
    pub return_buffer_mode: u8,
    /// Return audio jitter/min buffer in milliseconds
    pub return_buffer_ms: u32,
    /// Return audio max buffer in milliseconds (min/max mode)
    pub return_max_buffer_ms: u32,
}

/// Statistics for output RTP stream
#[repr(C)]
pub struct RtpOutputStatsFFI {
    /// TX packets sent
    pub tx_packets: u64,
    /// TX bytes sent
    pub tx_bytes: u64,
    /// TX encode errors
    pub tx_encode_errors: u64,
    /// TX buffer underruns
    pub tx_underruns: u64,
    /// RX packets received (return audio)
    pub rx_packets: u64,
    /// RX bytes received
    pub rx_bytes: u64,
    /// RX decode errors
    pub rx_decode_errors: u64,
    /// RX packets dropped (buffer full)
    pub rx_dropped: u64,
    /// Current return buffer level (samples)
    pub buffer_level: u32,
    /// Detected return audio payload type
    pub detected_return_pt: u8,
    /// Current PPM adjustment (scaled by 1000)
    pub current_ppm_x1000: i32,
}

/// Convert output FFI config to internal config
fn convert_output_ffi_config(config: &RtpOutputConfigFFI) -> RtpOutputConfig {
    let send_codec = match config.send_codec {
        BASS_RTP_CODEC_PCM16 => PayloadCodec::Pcm16,
        BASS_RTP_CODEC_PCM20 => PayloadCodec::Pcm20,
        BASS_RTP_CODEC_PCM24 => PayloadCodec::Pcm24,
        BASS_RTP_CODEC_MP2 => PayloadCodec::Mp2,
        4 => PayloadCodec::G711Ulaw, // G.711
        5 => PayloadCodec::G722,     // G.722
        _ => PayloadCodec::Pcm16,
    };

    let clock_mode = match config.clock_mode {
        0 => ClockMode::Ptp,
        1 => ClockMode::Livewire,
        _ => ClockMode::System,
    };

    let return_buffer_mode = if config.return_buffer_mode == BASS_RTP_BUFFER_MODE_MINMAX {
        BufferMode::MinMax {
            min_ms: config.return_buffer_ms.max(20),
            max_ms: config.return_max_buffer_ms.max(config.return_buffer_ms),
        }
    } else {
        BufferMode::Simple {
            buffer_ms: if config.return_buffer_ms > 0 {
                config.return_buffer_ms
            } else {
                100
            },
        }
    };

    RtpOutputConfig {
        remote_addr: Ipv4Addr::new(
            config.remote_addr[0],
            config.remote_addr[1],
            config.remote_addr[2],
            config.remote_addr[3],
        ),
        remote_port: config.remote_port,
        local_port: config.local_port,
        interface_addr: Ipv4Addr::new(
            config.interface_addr[0],
            config.interface_addr[1],
            config.interface_addr[2],
            config.interface_addr[3],
        ),
        sample_rate: config.sample_rate,
        channels: config.channels,
        send_codec,
        send_bitrate: if config.send_bitrate > 0 {
            config.send_bitrate
        } else {
            256
        },
        frame_duration_ms: if config.frame_duration_ms > 0 {
            config.frame_duration_ms
        } else {
            1
        },
        clock_mode,
        ptp_domain: config.ptp_domain,
        return_buffer_mode,
    }
}

/// Create an output bidirectional RTP stream (we connect TO Z/IP ONE)
///
/// # Arguments
/// * `source_channel` - BASS channel to read audio from for sending
/// * `config` - Stream configuration
///
/// # Returns
/// Opaque handle to the output stream, or null on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_OutputCreate(
    source_channel: HSTREAM,
    config: *const RtpOutputConfigFFI,
) -> *mut c_void {
    if config.is_null() {
        set_error(BASS_ERROR_MEM);
        return std::ptr::null_mut();
    }

    let config_ref = &*config;
    let internal_config = convert_output_ffi_config(config_ref);

    match RtpOutputBidirectional::new(source_channel, internal_config) {
        Ok(stream) => Box::into_raw(Box::new(stream)) as *mut c_void,
        Err(_) => {
            set_error(BASS_ERROR_CREATE);
            std::ptr::null_mut()
        }
    }
}

/// Start the output stream
///
/// # Arguments
/// * `handle` - Handle from BASS_RTP_OutputCreate
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_OutputStart(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let stream = &mut *(handle as *mut RtpOutputBidirectional);

    match stream.start() {
        Ok(_) => 1,
        Err(_) => {
            set_error(BASS_ERROR_START);
            0
        }
    }
}

/// Stop the output stream
///
/// # Arguments
/// * `handle` - Handle from BASS_RTP_OutputCreate
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_OutputStop(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let stream = &mut *(handle as *mut RtpOutputBidirectional);
    stream.stop();
    1
}

/// Get the return audio stream handle
///
/// # Arguments
/// * `handle` - Handle from BASS_RTP_OutputCreate
///
/// # Returns
/// BASS stream handle for return audio, or 0 if not available
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_OutputGetReturnStream(handle: *mut c_void) -> HSTREAM {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let stream = &*(handle as *const RtpOutputBidirectional);
    stream.return_handle
}

/// Get statistics for the output stream
///
/// # Arguments
/// * `handle` - Handle from BASS_RTP_OutputCreate
/// * `stats` - Pointer to RtpOutputStatsFFI structure to fill
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_OutputGetStats(
    handle: *mut c_void,
    stats: *mut RtpOutputStatsFFI,
) -> i32 {
    if stats.is_null() {
        set_error(BASS_ERROR_MEM);
        return 0;
    }

    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let stream = &*(handle as *const RtpOutputBidirectional);
    let s = stream.stats();

    (*stats) = RtpOutputStatsFFI {
        tx_packets: s.tx_packets,
        tx_bytes: s.tx_bytes,
        tx_encode_errors: s.tx_encode_errors,
        tx_underruns: s.tx_underruns,
        rx_packets: s.rx_packets,
        rx_bytes: s.rx_bytes,
        rx_decode_errors: s.rx_decode_errors,
        rx_dropped: s.rx_dropped,
        buffer_level: s.buffer_level,
        detected_return_pt: s.detected_return_pt,
        current_ppm_x1000: (s.current_ppm * 1000.0) as i32,
    };

    1
}

/// Check if the output stream is running
///
/// # Arguments
/// * `handle` - Handle from BASS_RTP_OutputCreate
///
/// # Returns
/// 1 if running, 0 if not running or invalid handle
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_OutputIsRunning(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        return 0;
    }

    let stream = &*(handle as *const RtpOutputBidirectional);
    if stream.is_running() { 1 } else { 0 }
}

/// Free resources associated with an output stream
///
/// # Arguments
/// * `handle` - Handle from BASS_RTP_OutputCreate
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_OutputFree(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    // Stop and drop the stream
    let stream = Box::from_raw(handle as *mut RtpOutputBidirectional);
    drop(stream);
    1
}
