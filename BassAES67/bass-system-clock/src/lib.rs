//! bass_system_clock - System clock (free-running) for AES67 audio
//!
//! Provides a fallback clock source when PTP or Livewire clocks are unavailable.
//! Always reports "locked" with 0 ppm correction (nominal rate).
//! Cross-platform: Windows and Linux.

use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::OnceLock;
use parking_lot::Mutex;

// Timer module for high-precision timing
mod timer;
use timer::{Timer, TimerCallback};

// ============================================================================
// C API Error Codes
// ============================================================================

pub const BASS_SYS_OK: i32 = 0;
pub const BASS_SYS_ERROR_ALREADY: i32 = 1;
pub const BASS_SYS_ERROR_NOT_INIT: i32 = 2;
pub const BASS_SYS_ERROR_INVALID: i32 = 4;

// ============================================================================
// C API Version
// ============================================================================

/// Library version (major.minor format: 0x0100 = 1.0)
pub const BASS_SYS_VERSION: u32 = 0x0100;

// ============================================================================
// Global State
// ============================================================================

/// Running state
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Reference count for start/stop
static REF_COUNT: AtomicU32 = AtomicU32::new(0);

/// Stats string buffer
static STATS_STRING: OnceLock<Mutex<String>> = OnceLock::new();

/// Timer instance
static TIMER: OnceLock<Mutex<Option<Timer>>> = OnceLock::new();

// ============================================================================
// C API Functions
// ============================================================================

/// Start the system clock.
///
/// Since this is a free-running clock, it always succeeds.
/// Multiple calls are reference-counted.
///
/// # Arguments
/// * `interface_ip` - Network interface IP (ignored, kept for API compatibility)
///
/// # Returns
/// * BASS_SYS_OK on success
#[no_mangle]
pub unsafe extern "C" fn BASS_SYS_Start(_interface_ip: *const c_char) -> i32 {
    let count = REF_COUNT.fetch_add(1, Ordering::SeqCst);
    if count > 0 {
        // Already running
        return BASS_SYS_OK;
    }

    RUNNING.store(true, Ordering::SeqCst);

    // Initialize stats string
    let stats = STATS_STRING.get_or_init(|| Mutex::new(String::new()));
    *stats.lock() = "System Clock (free-running)".to_string();

    BASS_SYS_OK
}

/// Stop the system clock.
///
/// Decrements reference count. Only actually stops when count reaches 0.
///
/// # Returns
/// * BASS_SYS_OK always
#[no_mangle]
pub unsafe extern "C" fn BASS_SYS_Stop() -> i32 {
    let prev = REF_COUNT.fetch_sub(1, Ordering::SeqCst);
    if prev > 1 {
        return BASS_SYS_OK;
    }

    // Last reference, stop
    force_stop();
    BASS_SYS_OK
}

/// Force stop the system clock regardless of reference count.
///
/// # Returns
/// * BASS_SYS_OK always
#[no_mangle]
pub unsafe extern "C" fn BASS_SYS_ForceStop() -> i32 {
    force_stop();
    BASS_SYS_OK
}

/// Internal force stop function.
fn force_stop() {
    REF_COUNT.store(0, Ordering::SeqCst);
    RUNNING.store(false, Ordering::SeqCst);

    // Stop timer if running
    if let Some(timer_mutex) = TIMER.get() {
        let mut timer_guard = timer_mutex.lock();
        if let Some(timer) = timer_guard.take() {
            timer.stop();
        }
    }
}

/// Check if system clock is running.
///
/// # Returns
/// * 1 if running, 0 if not
#[no_mangle]
pub unsafe extern "C" fn BASS_SYS_IsRunning() -> i32 {
    if RUNNING.load(Ordering::SeqCst) { 1 } else { 0 }
}

/// Get current clock offset in nanoseconds.
///
/// System clock has no offset correction - always returns 0.
///
/// # Returns
/// * 0 (no offset)
#[no_mangle]
pub unsafe extern "C" fn BASS_SYS_GetOffset() -> i64 {
    0
}

/// Get current frequency adjustment in PPM.
///
/// System clock runs at nominal rate - always returns 0.0.
///
/// # Returns
/// * 0.0 (nominal rate)
#[no_mangle]
pub unsafe extern "C" fn BASS_SYS_GetFrequencyPPM() -> f64 {
    0.0
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
pub unsafe extern "C" fn BASS_SYS_GetStatsString(buffer: *mut c_char, buffer_size: i32) -> i32 {
    if buffer.is_null() || buffer_size <= 0 {
        return 0;
    }

    let stats_str = STATS_STRING
        .get()
        .map(|m| m.lock().clone())
        .unwrap_or_else(|| "System Clock (not started)".to_string());

    let bytes = stats_str.as_bytes();
    let max_len = (buffer_size - 1) as usize;
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
pub unsafe extern "C" fn BASS_SYS_GetVersion() -> u32 {
    BASS_SYS_VERSION
}

/// Get system clock state.
///
/// # Returns
/// * 0 = Disabled, 3 = Slave (always reports Slave when running)
#[no_mangle]
pub unsafe extern "C" fn BASS_SYS_GetState() -> u8 {
    if RUNNING.load(Ordering::SeqCst) {
        3 // Slave state (synchronized)
    } else {
        0 // Disabled
    }
}

/// Check if system clock is locked (stable synchronization).
///
/// System clock is always "locked" - it's a free-running reference.
///
/// # Returns
/// * 1 if running (always locked when running), 0 if not running
#[no_mangle]
pub unsafe extern "C" fn BASS_SYS_IsLocked() -> i32 {
    if RUNNING.load(Ordering::SeqCst) { 1 } else { 0 }
}

// ============================================================================
// Timer C API Functions
// ============================================================================

/// Timer callback function type
#[allow(non_camel_case_types)]
pub type BASS_SYS_TimerProc = unsafe extern "C" fn(*mut c_void);

/// Start the precision timer.
///
/// # Arguments
/// * `interval_ms` - Timer period in milliseconds (1-1000)
/// * `callback` - Function to call on each tick (can be NULL)
/// * `user` - User data passed to callback
///
/// # Returns
/// * BASS_SYS_OK on success
/// * BASS_SYS_ERROR_INVALID if interval is out of range
/// * BASS_SYS_ERROR_ALREADY if timer is already running
#[no_mangle]
pub unsafe extern "C" fn BASS_SYS_TimerStart(
    interval_ms: u32,
    callback: Option<BASS_SYS_TimerProc>,
    user: *mut c_void,
) -> i32 {
    if interval_ms == 0 || interval_ms > 1000 {
        return BASS_SYS_ERROR_INVALID;
    }

    let timer_mutex = TIMER.get_or_init(|| Mutex::new(None));
    let mut timer_guard = timer_mutex.lock();

    if timer_guard.is_some() {
        return BASS_SYS_ERROR_ALREADY;
    }

    let cb: Option<TimerCallback> = callback.map(|f| {
        let user_ptr = user as usize;
        Box::new(move || {
            f(user_ptr as *mut c_void);
        }) as TimerCallback
    });

    match Timer::start(interval_ms, cb) {
        Ok(timer) => {
            *timer_guard = Some(timer);
            BASS_SYS_OK
        }
        Err(_) => BASS_SYS_ERROR_INVALID,
    }
}

/// Stop the precision timer.
///
/// # Returns
/// * BASS_SYS_OK always
#[no_mangle]
pub unsafe extern "C" fn BASS_SYS_TimerStop() -> i32 {
    if let Some(timer_mutex) = TIMER.get() {
        let mut timer_guard = timer_mutex.lock();
        if let Some(timer) = timer_guard.take() {
            timer.stop();
        }
    }
    BASS_SYS_OK
}

/// Check if timer is running.
///
/// # Returns
/// * 1 if running, 0 if not
#[no_mangle]
pub unsafe extern "C" fn BASS_SYS_TimerIsRunning() -> i32 {
    TIMER
        .get()
        .map(|m| m.lock().is_some())
        .unwrap_or(false) as i32
}

/// Set timer interval (can change while running).
///
/// # Arguments
/// * `interval_ms` - New interval in milliseconds (1-1000)
///
/// # Returns
/// * BASS_SYS_OK on success
/// * BASS_SYS_ERROR_INVALID if interval is out of range
#[no_mangle]
pub unsafe extern "C" fn BASS_SYS_TimerSetInterval(interval_ms: u32) -> i32 {
    if interval_ms == 0 || interval_ms > 1000 {
        return BASS_SYS_ERROR_INVALID;
    }

    if let Some(timer_mutex) = TIMER.get() {
        let timer_guard = timer_mutex.lock();
        if let Some(ref timer) = *timer_guard {
            timer.set_interval(interval_ms);
        }
    }
    BASS_SYS_OK
}

/// Get current timer interval.
///
/// # Returns
/// * Current interval in milliseconds, or 0 if not running
#[no_mangle]
pub unsafe extern "C" fn BASS_SYS_TimerGetInterval() -> u32 {
    TIMER
        .get()
        .and_then(|m| m.lock().as_ref().map(|t| t.get_interval()))
        .unwrap_or(0)
}

/// Enable or disable PLL frequency adjustment.
///
/// For system clock, PLL has no effect (always runs at nominal rate).
///
/// # Arguments
/// * `enabled` - 1 to enable, 0 to disable (ignored)
///
/// # Returns
/// * BASS_SYS_OK always
#[no_mangle]
pub unsafe extern "C" fn BASS_SYS_TimerSetPLL(_enabled: i32) -> i32 {
    // PLL has no effect on system clock
    BASS_SYS_OK
}

/// Check if PLL adjustment is enabled.
///
/// # Returns
/// * 0 (PLL not applicable for system clock)
#[no_mangle]
pub unsafe extern "C" fn BASS_SYS_TimerIsPLLEnabled() -> i32 {
    0
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
            // Initialize - nothing needed
        }
        DLL_PROCESS_DETACH => {
            // Cleanup - stop timer
            force_stop();
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
        force_stop();
    }
    fini
};
