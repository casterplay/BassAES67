//! bass-rtp: Bidirectional RTP audio module for BASS with Telos Z/IP ONE codec support.
//!
//! This module provides bidirectional unicast RTP audio streaming with support for
//! multiple codecs including PCM-16, PCM-20, PCM-24, MP2, G.711, G.722, and AAC (decode only).
//!
//! ## Modules
//!
//! - **Input module**: WE connect TO Z/IP ONE, send audio, receive return audio
//! - **Output module**: Z/IP ONE connects TO us, we receive audio, send backfeed

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use std::ffi::c_void;
use std::net::Ipv4Addr;

pub mod ffi;
pub mod clock_bindings;
pub mod rtp;
pub mod codec;
pub mod input;
#[path = "output_new/mod.rs"]
pub mod output;

use ffi::*;
use rtp::PayloadCodec;
use clock_bindings::ClockMode;
use input::{RtpInput, RtpInputConfig, RtpInputStats, BufferMode, input_return_stream_proc};
use output::{RtpOutput, RtpOutputConfig, RtpOutputStats, output_incoming_stream_proc};

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
/// G.711 u-Law codec
pub const BASS_RTP_CODEC_G711: u8 = 4;
/// G.722 codec
pub const BASS_RTP_CODEC_G722: u8 = 5;

// ============================================================================
// Buffer Mode Constants
// ============================================================================

/// Simple buffer mode
pub const BASS_RTP_BUFFER_MODE_SIMPLE: u8 = 0;
/// Min/Max buffer mode
pub const BASS_RTP_BUFFER_MODE_MINMAX: u8 = 1;

// ============================================================================
// Clock Mode Constants
// ============================================================================

/// PTP clock mode
pub const BASS_RTP_CLOCK_PTP: u8 = 0;
/// Livewire clock mode
pub const BASS_RTP_CLOCK_LIVEWIRE: u8 = 1;
/// System clock mode
pub const BASS_RTP_CLOCK_SYSTEM: u8 = 2;

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
            // Initialize clock bindings
            let _ = clock_bindings::init_clock_bindings();
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
// INPUT MODULE FFI API (WE connect TO Z/IP ONE)
// ============================================================================

/// Configuration for RTP Input stream (we connect TO Z/IP ONE)
#[repr(C)]
pub struct RtpInputConfigFFI {
    /// Remote IP address (Z/IP ONE) as 4 bytes - we connect TO this
    pub remote_addr: [u8; 4],
    /// Remote port (9150-9153 for Z/IP ONE, or custom)
    pub remote_port: u16,
    /// Local port to bind (0 = auto-assign)
    pub local_port: u16,
    /// Network interface IP address (4 bytes, 0.0.0.0 = any)
    pub interface_addr: [u8; 4],
    /// Sample rate (48000)
    pub sample_rate: u32,
    /// Number of channels (1 or 2)
    pub channels: u16,
    /// Send codec (BASS_RTP_CODEC_*)
    pub send_codec: u8,
    /// Send bitrate in kbps (for MP2, 0 = default 256)
    pub send_bitrate: u32,
    /// Frame duration in milliseconds (1-5, 0 = default 1)
    pub frame_duration_ms: u32,
    /// Clock mode (BASS_RTP_CLOCK_*)
    pub clock_mode: u8,
    /// PTP domain (0-127)
    pub ptp_domain: u8,
    /// Return audio buffer mode (BASS_RTP_BUFFER_MODE_*)
    pub return_buffer_mode: u8,
    /// Return audio buffer in milliseconds (simple mode, or min in min/max mode)
    pub return_buffer_ms: u32,
    /// Return audio max buffer in milliseconds (min/max mode only)
    pub return_max_buffer_ms: u32,
    /// Create return stream with BASS_STREAM_DECODE flag (for mixer compatibility)
    pub decode_stream: u8,
}

/// Statistics for RTP Input stream
#[repr(C)]
pub struct RtpInputStatsFFI {
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

/// Convert FFI config to internal config for Input module
fn convert_input_ffi_config(config: &RtpInputConfigFFI) -> RtpInputConfig {
    let send_codec = match config.send_codec {
        BASS_RTP_CODEC_PCM16 => PayloadCodec::Pcm16,
        BASS_RTP_CODEC_PCM20 => PayloadCodec::Pcm20,
        BASS_RTP_CODEC_PCM24 => PayloadCodec::Pcm24,
        BASS_RTP_CODEC_MP2 => PayloadCodec::Mp2,
        BASS_RTP_CODEC_G711 => PayloadCodec::G711Ulaw,
        BASS_RTP_CODEC_G722 => PayloadCodec::G722,
        _ => PayloadCodec::Pcm16,
    };

    let clock_mode = match config.clock_mode {
        BASS_RTP_CLOCK_PTP => ClockMode::Ptp,
        BASS_RTP_CLOCK_LIVEWIRE => ClockMode::Livewire,
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

    RtpInputConfig {
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
        send_bitrate: if config.send_bitrate > 0 { config.send_bitrate } else { 256 },
        frame_duration_ms: if config.frame_duration_ms > 0 { config.frame_duration_ms } else { 1 },
        clock_mode,
        ptp_domain: config.ptp_domain,
        return_buffer_mode,
        decode_stream: config.decode_stream != 0,
    }
}

/// Create an RTP Input stream (WE connect TO Z/IP ONE)
///
/// # Arguments
/// * `source_channel` - BASS channel to read audio FROM to send to Z/IP ONE
/// * `config` - Stream configuration
///
/// # Returns
/// Opaque handle to the RTP Input stream, or null on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_InputCreate(
    source_channel: HSTREAM,
    config: *const RtpInputConfigFFI,
) -> *mut c_void {
    if config.is_null() {
        set_error(BASS_ERROR_MEM);
        return std::ptr::null_mut();
    }

    let config_ref = &*config;
    let internal_config = convert_input_ffi_config(config_ref);

    match RtpInput::new(source_channel, internal_config) {
        Ok(stream) => Box::into_raw(Box::new(stream)) as *mut c_void,
        Err(_) => {
            set_error(BASS_ERROR_CREATE);
            std::ptr::null_mut()
        }
    }
}

/// Start the RTP Input stream
///
/// # Arguments
/// * `handle` - Handle from BASS_RTP_InputCreate
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_InputStart(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let stream = &mut *(handle as *mut RtpInput);

    // Create BASS stream for return audio (what we receive from Z/IP ONE)
    // Use BASS_STREAM_DECODE if decode_stream is set (for mixer compatibility)
    let flags = if stream.config.decode_stream {
        BASS_SAMPLE_FLOAT | BASS_STREAM_DECODE
    } else {
        BASS_SAMPLE_FLOAT
    };

    let bass_stream = BASS_StreamCreate(
        48000,
        stream.config.channels as u32,
        flags,
        Some(input_return_stream_proc),
        stream as *mut _ as *mut c_void,
    );

    if bass_stream == 0 {
        set_error(BASS_ErrorGetCode());
        return 0;
    }

    stream.return_handle = bass_stream;

    match stream.start() {
        Ok(_) => 1,
        Err(_) => {
            set_error(BASS_ERROR_START);
            0
        }
    }
}

/// Stop the RTP Input stream
///
/// # Arguments
/// * `handle` - Handle from BASS_RTP_InputCreate
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_InputStop(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let stream = &mut *(handle as *mut RtpInput);
    stream.stop();
    1
}

/// Get the return audio stream handle (audio received FROM Z/IP ONE)
///
/// # Arguments
/// * `handle` - Handle from BASS_RTP_InputCreate
///
/// # Returns
/// BASS stream handle for return audio, or 0 if not available
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_InputGetReturnStream(handle: *mut c_void) -> HSTREAM {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let stream = &*(handle as *const RtpInput);
    stream.return_handle
}

/// Get statistics for the RTP Input stream
///
/// # Arguments
/// * `handle` - Handle from BASS_RTP_InputCreate
/// * `stats` - Pointer to RtpInputStatsFFI structure to fill
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_InputGetStats(
    handle: *mut c_void,
    stats: *mut RtpInputStatsFFI,
) -> i32 {
    if stats.is_null() {
        set_error(BASS_ERROR_MEM);
        return 0;
    }

    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let stream = &*(handle as *const RtpInput);
    let s = stream.stats();

    (*stats) = RtpInputStatsFFI {
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

/// Check if the RTP Input stream is running
///
/// # Arguments
/// * `handle` - Handle from BASS_RTP_InputCreate
///
/// # Returns
/// 1 if running, 0 if not running or invalid handle
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_InputIsRunning(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        return 0;
    }

    let stream = &*(handle as *const RtpInput);
    if stream.is_running() { 1 } else { 0 }
}

/// Free resources associated with an RTP Input stream
///
/// # Arguments
/// * `handle` - Handle from BASS_RTP_InputCreate
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_InputFree(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    // Stop the stream
    let stream = &mut *(handle as *mut RtpInput);
    stream.stop();

    // Free the BASS return stream
    let return_handle = stream.return_handle;
    if return_handle != 0 {
        BASS_StreamFree(return_handle);
    }

    // Drop the boxed stream
    let _ = Box::from_raw(handle as *mut RtpInput);
    1
}

// ============================================================================
// OUTPUT MODULE FFI API (Z/IP ONE connects TO us)
// ============================================================================

/// Configuration for RTP Output stream (Z/IP ONE connects TO us)
#[repr(C)]
pub struct RtpOutputConfigFFI {
    /// Local port to listen on (Z/IP ONE connects here)
    pub local_port: u16,
    /// Network interface IP address (4 bytes, 0.0.0.0 = any)
    pub interface_addr: [u8; 4],
    /// Sample rate (48000)
    pub sample_rate: u32,
    /// Number of channels (1 or 2)
    pub channels: u16,
    /// Backfeed codec (BASS_RTP_CODEC_*)
    pub backfeed_codec: u8,
    /// Backfeed bitrate in kbps (for MP2, 0 = default 256)
    pub backfeed_bitrate: u32,
    /// Frame duration in milliseconds (1-5, 0 = default 1)
    pub frame_duration_ms: u32,
    /// Clock mode (BASS_RTP_CLOCK_*)
    pub clock_mode: u8,
    /// PTP domain (0-127)
    pub ptp_domain: u8,
    /// Incoming audio buffer mode (BASS_RTP_BUFFER_MODE_*)
    pub buffer_mode: u8,
    /// Incoming audio buffer in milliseconds (simple mode, or min in min/max mode)
    pub buffer_ms: u32,
    /// Incoming audio max buffer in milliseconds (min/max mode only)
    pub max_buffer_ms: u32,
    /// Create incoming stream with BASS_STREAM_DECODE flag (for mixer compatibility)
    pub decode_stream: u8,
    /// Connection state callback (optional, can be null)
    pub connection_callback: Option<output::ConnectionCallback>,
    /// User data for callback
    pub callback_user_data: *mut std::ffi::c_void,
}

/// Statistics for RTP Output stream
#[repr(C)]
pub struct RtpOutputStatsFFI {
    /// RX packets received (incoming audio)
    pub rx_packets: u64,
    /// RX bytes received
    pub rx_bytes: u64,
    /// RX decode errors
    pub rx_decode_errors: u64,
    /// RX packets dropped (buffer full)
    pub rx_dropped: u64,
    /// TX packets sent (backfeed)
    pub tx_packets: u64,
    /// TX bytes sent
    pub tx_bytes: u64,
    /// TX encode errors
    pub tx_encode_errors: u64,
    /// TX buffer underruns
    pub tx_underruns: u64,
    /// Current incoming buffer level (samples)
    pub buffer_level: u32,
    /// Detected incoming audio payload type
    pub detected_incoming_pt: u8,
    /// Current PPM adjustment (scaled by 1000)
    pub current_ppm_x1000: i32,
}

/// Convert FFI config to internal config for Output module
fn convert_output_ffi_config(config: &RtpOutputConfigFFI) -> RtpOutputConfig {
    let backfeed_codec = match config.backfeed_codec {
        BASS_RTP_CODEC_PCM16 => PayloadCodec::Pcm16,
        BASS_RTP_CODEC_PCM20 => PayloadCodec::Pcm20,
        BASS_RTP_CODEC_PCM24 => PayloadCodec::Pcm24,
        BASS_RTP_CODEC_MP2 => PayloadCodec::Mp2,
        BASS_RTP_CODEC_G711 => PayloadCodec::G711Ulaw,
        BASS_RTP_CODEC_G722 => PayloadCodec::G722,
        _ => PayloadCodec::Pcm16,
    };

    let clock_mode = match config.clock_mode {
        BASS_RTP_CLOCK_PTP => ClockMode::Ptp,
        BASS_RTP_CLOCK_LIVEWIRE => ClockMode::Livewire,
        _ => ClockMode::System,
    };

    let buffer_mode = if config.buffer_mode == BASS_RTP_BUFFER_MODE_MINMAX {
        BufferMode::MinMax {
            min_ms: config.buffer_ms.max(20),
            max_ms: config.max_buffer_ms.max(config.buffer_ms),
        }
    } else {
        BufferMode::Simple {
            buffer_ms: if config.buffer_ms > 0 {
                config.buffer_ms
            } else {
                100
            },
        }
    };

    RtpOutputConfig {
        local_port: config.local_port,
        interface_addr: Ipv4Addr::new(
            config.interface_addr[0],
            config.interface_addr[1],
            config.interface_addr[2],
            config.interface_addr[3],
        ),
        sample_rate: config.sample_rate,
        channels: config.channels,
        backfeed_codec,
        backfeed_bitrate: if config.backfeed_bitrate > 0 { config.backfeed_bitrate } else { 256 },
        frame_duration_ms: if config.frame_duration_ms > 0 { config.frame_duration_ms } else { 1 },
        clock_mode,
        ptp_domain: config.ptp_domain,
        buffer_mode,
        decode_stream: config.decode_stream != 0,
        connection_callback: config.connection_callback,
        callback_user_data: config.callback_user_data,
    }
}

/// Create an RTP Output stream (Z/IP ONE connects TO us)
///
/// # Arguments
/// * `backfeed_channel` - BASS channel to read audio FROM to send as backfeed
/// * `config` - Stream configuration
///
/// # Returns
/// Opaque handle to the RTP Output stream, or null on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_OutputCreate(
    backfeed_channel: HSTREAM,
    config: *const RtpOutputConfigFFI,
) -> *mut c_void {
    if config.is_null() {
        set_error(BASS_ERROR_MEM);
        return std::ptr::null_mut();
    }

    let config_ref = &*config;
    let internal_config = convert_output_ffi_config(config_ref);

    match RtpOutput::new(backfeed_channel, internal_config) {
        Ok(stream) => Box::into_raw(Box::new(stream)) as *mut c_void,
        Err(_) => {
            set_error(BASS_ERROR_CREATE);
            std::ptr::null_mut()
        }
    }
}

/// Start the RTP Output stream (listening for connections)
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

    let stream = &mut *(handle as *mut RtpOutput);

    // Create BASS stream for incoming audio (what we receive from Z/IP ONE)
    // Use BASS_STREAM_DECODE if decode_stream is set (for mixer compatibility)
    let flags = if stream.config.decode_stream {
        BASS_SAMPLE_FLOAT | BASS_STREAM_DECODE
    } else {
        BASS_SAMPLE_FLOAT
    };

    let bass_stream = BASS_StreamCreate(
        48000,
        stream.config.channels as u32,
        flags,
        Some(output_incoming_stream_proc),
        stream as *mut _ as *mut c_void,
    );

    if bass_stream == 0 {
        set_error(BASS_ErrorGetCode());
        return 0;
    }

    stream.incoming_handle = bass_stream;

    match stream.start() {
        Ok(_) => 1,
        Err(_) => {
            set_error(BASS_ERROR_START);
            0
        }
    }
}

/// Stop the RTP Output stream
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

    let stream = &mut *(handle as *mut RtpOutput);
    stream.stop();
    1
}

/// Get the incoming audio stream handle (audio received FROM Z/IP ONE)
///
/// # Arguments
/// * `handle` - Handle from BASS_RTP_OutputCreate
///
/// # Returns
/// BASS stream handle for incoming audio, or 0 if not available
#[no_mangle]
pub unsafe extern "system" fn BASS_RTP_OutputGetInputStream(handle: *mut c_void) -> HSTREAM {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let stream = &*(handle as *const RtpOutput);
    stream.incoming_handle
}

/// Get statistics for the RTP Output stream
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

    let stream = &*(handle as *const RtpOutput);
    let s = stream.stats();

    (*stats) = RtpOutputStatsFFI {
        rx_packets: s.rx_packets,
        rx_bytes: s.rx_bytes,
        rx_decode_errors: s.rx_decode_errors,
        rx_dropped: s.rx_dropped,
        tx_packets: s.tx_packets,
        tx_bytes: s.tx_bytes,
        tx_encode_errors: s.tx_encode_errors,
        tx_underruns: s.tx_underruns,
        buffer_level: s.buffer_level,
        detected_incoming_pt: s.detected_incoming_pt,
        current_ppm_x1000: (s.current_ppm * 1000.0) as i32,
    };

    1
}

/// Check if the RTP Output stream is running
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

    let stream = &*(handle as *const RtpOutput);
    if stream.is_running() { 1 } else { 0 }
}

/// Free resources associated with an RTP Output stream
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

    // Stop the stream
    let stream = &mut *(handle as *mut RtpOutput);
    stream.stop();

    // Free the BASS incoming stream
    let incoming_handle = stream.incoming_handle;
    if incoming_handle != 0 {
        BASS_StreamFree(incoming_handle);
    }

    // Drop the boxed stream
    let _ = Box::from_raw(handle as *mut RtpOutput);
    1
}
