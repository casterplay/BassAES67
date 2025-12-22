//! bass_opus_web - Opus encoder for web streaming via SignalR.
//!
//! Reads PCM from a BASS channel, encodes to Opus 5ms frames,
//! delivers via callback to C# for SignalR broadcast.

mod ffi;
mod codec;
mod encoder;

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

pub use encoder::{OpusWebEncoder, EncoderConfig, EncoderStats};
use ffi::{DWORD, TRUE, FALSE, BOOL};

// ============================================================================
// CALLBACK REGISTRATION
// ============================================================================

/// Callback signature: called for each Opus frame.
/// - data: pointer to Opus encoded bytes
/// - len: number of bytes
/// - timestamp_ms: monotonic timestamp in milliseconds
/// - user: user data pointer
pub type OpusFrameCallback = extern "C" fn(
    data: *const u8,
    len: u32,
    timestamp_ms: u64,
    user: *mut c_void,
);

// Global callback storage (atomic for lock-free access)
pub(crate) static FRAME_CALLBACK: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
pub(crate) static FRAME_CALLBACK_USER: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

// ============================================================================
// FFI STRUCTURES
// ============================================================================

/// FFI-compatible encoder configuration.
#[repr(C)]
pub struct EncoderConfigFFI {
    /// Sample rate (must be 48000)
    pub sample_rate: u32,
    /// Number of channels (1 or 2)
    pub channels: u16,
    /// Opus bitrate in kbps (64-256 typical)
    pub bitrate_kbps: u32,
    /// Reserved for future use (clock mode, etc.)
    pub reserved: u8,
}

/// FFI-compatible statistics.
#[repr(C)]
pub struct EncoderStatsFFI {
    pub frames_encoded: u64,
    pub samples_processed: u64,
    pub underruns: u64,
    pub callback_errors: u64,
}

// ============================================================================
// FFI EXPORTS
// ============================================================================

/// Set the callback for receiving Opus frames.
#[no_mangle]
pub unsafe extern "C" fn BASS_OPUS_WEB_SetCallback(
    callback: OpusFrameCallback,
    user: *mut c_void,
) {
    FRAME_CALLBACK.store(callback as *mut c_void, Ordering::Release);
    FRAME_CALLBACK_USER.store(user, Ordering::Release);
}

/// Clear the callback.
#[no_mangle]
pub unsafe extern "C" fn BASS_OPUS_WEB_ClearCallback() {
    FRAME_CALLBACK.store(ptr::null_mut(), Ordering::Release);
    FRAME_CALLBACK_USER.store(ptr::null_mut(), Ordering::Release);
}

/// Create an Opus web encoder.
/// Returns opaque handle or null on error.
#[no_mangle]
pub unsafe extern "C" fn BASS_OPUS_WEB_Create(
    bass_channel: DWORD,
    config: *const EncoderConfigFFI,
) -> *mut c_void {
    if config.is_null() {
        return ptr::null_mut();
    }

    let cfg = &*config;
    let encoder_config = EncoderConfig {
        sample_rate: cfg.sample_rate,
        channels: cfg.channels,
        bitrate_kbps: cfg.bitrate_kbps,
    };

    match OpusWebEncoder::new(bass_channel, encoder_config) {
        Ok(encoder) => Box::into_raw(Box::new(encoder)) as *mut c_void,
        Err(e) => {
            eprintln!("[BASS_OPUS_WEB] Create failed: {}", e);
            ptr::null_mut()
        }
    }
}

/// Start the encoder (begins pulling from BASS and encoding).
/// Returns 1 on success, 0 on failure.
#[no_mangle]
pub unsafe extern "C" fn BASS_OPUS_WEB_Start(handle: *mut c_void) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let encoder = &mut *(handle as *mut OpusWebEncoder);
    match encoder.start() {
        Ok(()) => TRUE,
        Err(e) => {
            eprintln!("[BASS_OPUS_WEB] Start failed: {}", e);
            FALSE
        }
    }
}

/// Stop the encoder.
/// Returns 1 on success, 0 on failure.
#[no_mangle]
pub unsafe extern "C" fn BASS_OPUS_WEB_Stop(handle: *mut c_void) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let encoder = &mut *(handle as *mut OpusWebEncoder);
    encoder.stop();
    TRUE
}

/// Check if encoder is running.
/// Returns 1 if running, 0 if not.
#[no_mangle]
pub unsafe extern "C" fn BASS_OPUS_WEB_IsRunning(handle: *mut c_void) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let encoder = &*(handle as *mut OpusWebEncoder);
    if encoder.is_running() { TRUE } else { FALSE }
}

/// Get encoder statistics.
/// Returns 1 on success, 0 on failure.
#[no_mangle]
pub unsafe extern "C" fn BASS_OPUS_WEB_GetStats(
    handle: *mut c_void,
    stats: *mut EncoderStatsFFI,
) -> BOOL {
    if handle.is_null() || stats.is_null() {
        return FALSE;
    }

    let encoder = &*(handle as *mut OpusWebEncoder);
    let s = encoder.stats();

    (*stats).frames_encoded = s.frames_encoded;
    (*stats).samples_processed = s.samples_processed;
    (*stats).underruns = s.underruns;
    (*stats).callback_errors = s.callback_errors;

    TRUE
}

/// Free the encoder.
/// Returns 1 on success, 0 on failure.
#[no_mangle]
pub unsafe extern "C" fn BASS_OPUS_WEB_Free(handle: *mut c_void) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let encoder = Box::from_raw(handle as *mut OpusWebEncoder);
    drop(encoder);
    TRUE
}

// ============================================================================
// DLL ENTRY POINTS
// ============================================================================

#[cfg(windows)]
#[no_mangle]
pub extern "system" fn DllMain(
    _dll_module: *mut c_void,
    call_reason: u32,
    _reserved: *mut c_void,
) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    const DLL_PROCESS_DETACH: u32 = 0;

    match call_reason {
        DLL_PROCESS_ATTACH => {
            // Library loaded
        }
        DLL_PROCESS_DETACH => {
            // Library unloaded - clear callback
            FRAME_CALLBACK.store(ptr::null_mut(), Ordering::Release);
            FRAME_CALLBACK_USER.store(ptr::null_mut(), Ordering::Release);
        }
        _ => {}
    }
    1 // TRUE
}

// Note: On Linux, _init/_fini are handled by the C runtime.
// Cleanup happens via Drop implementations.
