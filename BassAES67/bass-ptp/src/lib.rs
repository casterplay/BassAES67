//! bass_ptp - PTP IEEE 1588v2 client library for AES67 audio
//!
//! Provides a shared PTP client that can be used by multiple BASS plugins
//! (bass_aes67 input and future bass_aes67_send output).

pub mod client;
pub mod messages;
pub mod platform;
pub mod servo;
pub mod stats;
pub mod timer;

use std::ffi::{c_char, c_void, CStr};
use std::net::Ipv4Addr;

// Re-export key types
pub use client::{
    force_stop_ptp_client, get_frequency_ppm, get_offset_ns, get_ptp_stats, is_ptp_running,
    start_ptp_client, stop_ptp_client,
};
pub use stats::{PtpState, PtpStats};

// ============================================================================
// C API Error Codes
// ============================================================================

pub const BASS_PTP_OK: i32 = 0;
pub const BASS_PTP_ERROR_ALREADY: i32 = 1;
pub const BASS_PTP_ERROR_NOT_INIT: i32 = 2;
pub const BASS_PTP_ERROR_SOCKET: i32 = 3;
pub const BASS_PTP_ERROR_INVALID: i32 = 4;

// ============================================================================
// C API Version
// ============================================================================

/// Library version (major.minor format: 0x0100 = 1.0)
pub const BASS_PTP_VERSION: u32 = 0x0100;

// ============================================================================
// C API Functions
// ============================================================================

/// Start the PTP client.
///
/// Multiple calls are reference-counted - only first call actually starts.
/// Each Start must be matched with a Stop.
///
/// # Arguments
/// * `interface_ip` - Network interface IP as null-terminated C string (e.g., "192.168.1.100")
/// * `domain` - PTP domain number (0-127)
///
/// # Returns
/// * BASS_PTP_OK on success
/// * BASS_PTP_ERROR_INVALID if interface_ip is null or invalid
/// * BASS_PTP_ERROR_SOCKET if socket creation fails
#[no_mangle]
pub unsafe extern "C" fn BASS_PTP_Start(interface_ip: *const c_char, domain: u8) -> i32 {
    if interface_ip.is_null() {
        return BASS_PTP_ERROR_INVALID;
    }

    let ip_str = match CStr::from_ptr(interface_ip).to_str() {
        Ok(s) => s,
        Err(_) => return BASS_PTP_ERROR_INVALID,
    };

    let ip_addr: Ipv4Addr = match ip_str.parse() {
        Ok(ip) => ip,
        Err(_) => return BASS_PTP_ERROR_INVALID,
    };

    match start_ptp_client(ip_addr, domain) {
        Ok(()) => BASS_PTP_OK,
        Err(_) => BASS_PTP_ERROR_SOCKET,
    }
}

/// Stop the PTP client.
///
/// Decrements reference count. Only actually stops when count reaches 0.
///
/// # Returns
/// * BASS_PTP_OK always
#[no_mangle]
pub unsafe extern "C" fn BASS_PTP_Stop() -> i32 {
    stop_ptp_client();
    BASS_PTP_OK
}

/// Force stop the PTP client regardless of reference count.
///
/// # Returns
/// * BASS_PTP_OK always
#[no_mangle]
pub unsafe extern "C" fn BASS_PTP_ForceStop() -> i32 {
    force_stop_ptp_client();
    BASS_PTP_OK
}

/// Check if PTP client is running.
///
/// # Returns
/// * 1 if running, 0 if not
#[no_mangle]
pub unsafe extern "C" fn BASS_PTP_IsRunning() -> i32 {
    if is_ptp_running() { 1 } else { 0 }
}

/// Get current clock offset in nanoseconds.
///
/// # Returns
/// * Offset in nanoseconds, or 0 if not synchronized
#[no_mangle]
pub unsafe extern "C" fn BASS_PTP_GetOffset() -> i64 {
    get_offset_ns()
}

/// Get current frequency adjustment in PPM (parts per million).
///
/// # Returns
/// * Frequency adjustment in ppm, or 0.0 if not synchronized
#[no_mangle]
pub unsafe extern "C" fn BASS_PTP_GetFrequencyPPM() -> f64 {
    get_frequency_ppm()
}

/// Get formatted stats string.
///
/// # Arguments
/// * `buffer` - Output buffer for the string
/// * `buffer_size` - Size of the buffer in bytes
///
/// # Returns
/// * Length of string written (excluding null terminator), or 0 on error
#[no_mangle]
pub unsafe extern "C" fn BASS_PTP_GetStatsString(buffer: *mut c_char, buffer_size: i32) -> i32 {
    if buffer.is_null() || buffer_size <= 0 {
        return 0;
    }

    let stats_str = stats::get_stats_string();
    let bytes = stats_str.as_bytes();
    let max_len = (buffer_size - 1) as usize; // Leave room for null terminator
    let copy_len = bytes.len().min(max_len);

    std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer as *mut u8, copy_len);
    *buffer.add(copy_len) = 0; // Null terminator

    copy_len as i32
}

/// Get library version.
///
/// # Returns
/// * Version in format 0xMMNN (major, minor)
#[no_mangle]
pub unsafe extern "C" fn BASS_PTP_GetVersion() -> u32 {
    BASS_PTP_VERSION
}

/// Get PTP state.
///
/// # Returns
/// * 0 = Disabled, 1 = Listening, 2 = Uncalibrated, 3 = Slave
#[no_mangle]
pub unsafe extern "C" fn BASS_PTP_GetState() -> u8 {
    get_ptp_stats()
        .map(|s| s.state as u8)
        .unwrap_or(PtpState::Disabled as u8)
}

/// Check if PTP is locked (stable synchronization).
///
/// # Returns
/// * 1 if locked, 0 if not
#[no_mangle]
pub unsafe extern "C" fn BASS_PTP_IsLocked() -> i32 {
    get_ptp_stats()
        .map(|s| if s.locked { 1 } else { 0 })
        .unwrap_or(0)
}

// ============================================================================
// Timer C API Functions
// ============================================================================

/// Timer callback function type
#[allow(non_camel_case_types)]
pub type BASS_PTP_TimerProc = unsafe extern "C" fn(*mut c_void);

/// Start the precision timer.
///
/// # Arguments
/// * `interval_ms` - Timer period in milliseconds (1-1000)
/// * `callback` - Function to call on each tick (can be NULL)
/// * `user` - User data passed to callback
///
/// # Returns
/// * BASS_PTP_OK on success
/// * BASS_PTP_ERROR_INVALID if interval is out of range
/// * BASS_PTP_ERROR_ALREADY if timer is already running
#[no_mangle]
pub unsafe extern "C" fn BASS_PTP_TimerStart(
    interval_ms: u32,
    callback: Option<BASS_PTP_TimerProc>,
    user: *mut c_void,
) -> i32 {
    match timer::start_timer(interval_ms, callback, user) {
        0 => BASS_PTP_OK,
        -1 => BASS_PTP_ERROR_INVALID,
        -3 => BASS_PTP_ERROR_ALREADY,
        _ => BASS_PTP_ERROR_SOCKET,
    }
}

/// Stop the precision timer.
///
/// # Returns
/// * BASS_PTP_OK always
#[no_mangle]
pub unsafe extern "C" fn BASS_PTP_TimerStop() -> i32 {
    timer::stop_timer();
    BASS_PTP_OK
}

/// Check if timer is running.
///
/// # Returns
/// * 1 if running, 0 if not
#[no_mangle]
pub unsafe extern "C" fn BASS_PTP_TimerIsRunning() -> i32 {
    if timer::is_timer_running() { 1 } else { 0 }
}

/// Set timer interval (can change while running).
///
/// # Arguments
/// * `interval_ms` - New interval in milliseconds (1-1000)
///
/// # Returns
/// * BASS_PTP_OK on success
/// * BASS_PTP_ERROR_INVALID if interval is out of range
#[no_mangle]
pub unsafe extern "C" fn BASS_PTP_TimerSetInterval(interval_ms: u32) -> i32 {
    match timer::set_interval(interval_ms) {
        0 => BASS_PTP_OK,
        _ => BASS_PTP_ERROR_INVALID,
    }
}

/// Get current timer interval.
///
/// # Returns
/// * Current interval in milliseconds
#[no_mangle]
pub unsafe extern "C" fn BASS_PTP_TimerGetInterval() -> u32 {
    timer::get_interval()
}

/// Enable or disable PLL frequency adjustment.
///
/// When enabled (default), the timer period is adjusted based on PTP servo
/// frequency correction to track the network clock rate.
///
/// # Arguments
/// * `enabled` - 1 to enable, 0 to disable
///
/// # Returns
/// * BASS_PTP_OK always
#[no_mangle]
pub unsafe extern "C" fn BASS_PTP_TimerSetPLL(enabled: i32) -> i32 {
    timer::set_pll_enabled(enabled != 0);
    BASS_PTP_OK
}

/// Check if PLL adjustment is enabled.
///
/// # Returns
/// * 1 if enabled, 0 if disabled
#[no_mangle]
pub unsafe extern "C" fn BASS_PTP_TimerIsPLLEnabled() -> i32 {
    if timer::is_pll_enabled() { 1 } else { 0 }
}

// ============================================================================
// Windows DLL Entry Point
// ============================================================================

#[cfg(windows)]
const DLL_PROCESS_ATTACH: u32 = 1;
#[cfg(windows)]
const DLL_PROCESS_DETACH: u32 = 0;

#[cfg(windows)]
type BOOL = i32;
#[cfg(windows)]
type DWORD = u32;

#[cfg(windows)]
#[no_mangle]
pub unsafe extern "system" fn DllMain(
    _hinst: *mut c_void,
    reason: DWORD,
    _reserved: *mut c_void,
) -> BOOL {
    match reason {
        DLL_PROCESS_ATTACH => {
            // Initialize - nothing special needed
        }
        DLL_PROCESS_DETACH => {
            // Cleanup - stop timer and PTP client
            timer::stop_timer();
            force_stop_ptp_client();
        }
        _ => {}
    }
    1 // TRUE
}

// ============================================================================
// Linux/macOS Cleanup
// ============================================================================

#[cfg(not(windows))]
#[used]
#[link_section = ".fini_array"]
static FINI: extern "C" fn() = {
    extern "C" fn fini() {
        // Cleanup - stop timer and PTP client
        timer::stop_timer();
        force_stop_ptp_client();
    }
    fini
};
