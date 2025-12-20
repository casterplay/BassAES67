//! High-precision timer with PLL adjustment for no-soundcard mode.
//!
//! Provides a timer that fires at configurable intervals (default 20ms) with
//! frequency adjustment based on PTP servo output.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};

use crate::client::{get_ptp_stats, is_ptp_running};

/// Timer callback function type
pub type TimerCallback = unsafe extern "C" fn(*mut c_void);

/// Global timer instance
static TIMER: OnceLock<Mutex<Option<TimerHandle>>> = OnceLock::new();

/// Timer configuration
static TIMER_INTERVAL_MS: AtomicU32 = AtomicU32::new(20);
static TIMER_PLL_ENABLED: AtomicBool = AtomicBool::new(true);

/// Handle to a running timer
struct TimerHandle {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    callback: Option<TimerCallback>,
    user_data: *mut c_void,
}

// Safety: user_data is only accessed from the timer thread
unsafe impl Send for TimerHandle {}
unsafe impl Sync for TimerHandle {}

/// Start the precision timer.
///
/// # Arguments
/// * `interval_ms` - Timer period in milliseconds (1-1000)
/// * `callback` - Function to call on each tick
/// * `user` - User data passed to callback
///
/// # Returns
/// * 0 on success, non-zero on error
pub fn start_timer(
    interval_ms: u32,
    callback: Option<TimerCallback>,
    user: *mut c_void,
) -> i32 {
    // Validate interval
    if interval_ms == 0 || interval_ms > 1000 {
        return -1;
    }

    // Store interval
    TIMER_INTERVAL_MS.store(interval_ms, Ordering::SeqCst);

    let timer_mutex = TIMER.get_or_init(|| Mutex::new(None));
    let mut timer_guard = match timer_mutex.lock() {
        Ok(g) => g,
        Err(_) => return -2,
    };

    // Check if already running
    if timer_guard.is_some() {
        return -3;
    }

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    // Store callback info for the thread
    let cb = callback;
    let user_ptr = user as usize; // Convert to usize for thread safety

    let thread = thread::spawn(move || {
        timer_thread(running_clone, cb, user_ptr as *mut c_void);
    });

    *timer_guard = Some(TimerHandle {
        running,
        thread: Some(thread),
        callback,
        user_data: user,
    });

    0
}

/// Stop the timer
pub fn stop_timer() -> i32 {
    let timer_mutex = match TIMER.get() {
        Some(m) => m,
        None => return 0,
    };

    let mut timer_guard = match timer_mutex.lock() {
        Ok(g) => g,
        Err(_) => return -1,
    };

    if let Some(mut handle) = timer_guard.take() {
        handle.running.store(false, Ordering::SeqCst);
        if let Some(thread) = handle.thread.take() {
            let _ = thread.join();
        }
    }

    0
}

/// Check if timer is running
pub fn is_timer_running() -> bool {
    let timer_mutex = match TIMER.get() {
        Some(m) => m,
        None => return false,
    };
    let timer_guard = match timer_mutex.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    timer_guard.is_some()
}

/// Set timer interval (can change while running)
pub fn set_interval(interval_ms: u32) -> i32 {
    if interval_ms == 0 || interval_ms > 1000 {
        return -1;
    }
    TIMER_INTERVAL_MS.store(interval_ms, Ordering::SeqCst);
    0
}

/// Get current timer interval
pub fn get_interval() -> u32 {
    TIMER_INTERVAL_MS.load(Ordering::SeqCst)
}

/// Enable/disable PLL adjustment
pub fn set_pll_enabled(enabled: bool) {
    TIMER_PLL_ENABLED.store(enabled, Ordering::SeqCst);
}

/// Check if PLL adjustment is enabled
pub fn is_pll_enabled() -> bool {
    TIMER_PLL_ENABLED.load(Ordering::SeqCst)
}

/// Timer thread implementation using multimedia timer (Windows)
#[cfg(windows)]
fn timer_thread(running: Arc<AtomicBool>, callback: Option<TimerCallback>, user: *mut c_void) {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        CreateWaitableTimerW, SetWaitableTimer, WaitForSingleObject, INFINITE,
    };

    // Create waitable timer (standard, not high-resolution for compatibility)
    let timer_handle: HANDLE = unsafe { CreateWaitableTimerW(std::ptr::null(), 1, std::ptr::null()) };

    if timer_handle.is_null() || timer_handle == 0 as HANDLE {
        // Fallback to sleep-based timer
        return timer_thread_fallback(running, callback, user);
    }

    while running.load(Ordering::SeqCst) {
        // Get current settings
        let base_interval_ms = TIMER_INTERVAL_MS.load(Ordering::SeqCst);
        let pll_enabled = TIMER_PLL_ENABLED.load(Ordering::SeqCst);

        // Calculate adjusted interval
        let adjusted_interval_100ns = calculate_adjusted_interval(base_interval_ms, pll_enabled);

        // Set timer (negative value = relative time in 100ns units)
        let due_time: i64 = -(adjusted_interval_100ns as i64);
        let result = unsafe {
            SetWaitableTimer(timer_handle, &due_time, 0, None, std::ptr::null(), 0)
        };

        if result == 0 {
            // Timer set failed
            break;
        }

        // Wait for timer
        let wait_result = unsafe { WaitForSingleObject(timer_handle, INFINITE) };
        if wait_result != WAIT_OBJECT_0 {
            break;
        }

        // Call user callback
        if let Some(cb) = callback {
            unsafe { cb(user) };
        }
    }

    // Cleanup
    unsafe { CloseHandle(timer_handle) };
}

/// Fallback timer using Sleep (less precise)
#[cfg(windows)]
fn timer_thread_fallback(
    running: Arc<AtomicBool>,
    callback: Option<TimerCallback>,
    user: *mut c_void,
) {
    while running.load(Ordering::SeqCst) {
        let base_interval_ms = TIMER_INTERVAL_MS.load(Ordering::SeqCst);
        let pll_enabled = TIMER_PLL_ENABLED.load(Ordering::SeqCst);

        // Calculate adjusted interval and convert to ms
        let adjusted_100ns = calculate_adjusted_interval(base_interval_ms, pll_enabled);
        let adjusted_ms = (adjusted_100ns / 10_000) as u32;

        // Sleep
        std::thread::sleep(std::time::Duration::from_millis(adjusted_ms as u64));

        // Call user callback
        if let Some(cb) = callback {
            unsafe { cb(user) };
        }
    }
}

/// Non-Windows fallback
#[cfg(not(windows))]
fn timer_thread(running: Arc<AtomicBool>, callback: Option<TimerCallback>, user: *mut c_void) {
    while running.load(Ordering::SeqCst) {
        let base_interval_ms = TIMER_INTERVAL_MS.load(Ordering::SeqCst);
        let pll_enabled = TIMER_PLL_ENABLED.load(Ordering::SeqCst);

        // Calculate adjusted interval
        let adjusted_100ns = calculate_adjusted_interval(base_interval_ms, pll_enabled);
        let adjusted_us = adjusted_100ns / 10;

        // Sleep
        std::thread::sleep(std::time::Duration::from_micros(adjusted_us as u64));

        // Call user callback
        if let Some(cb) = callback {
            unsafe { cb(user) };
        }
    }
}

/// Calculate PLL-adjusted interval in 100ns units
fn calculate_adjusted_interval(base_interval_ms: u32, pll_enabled: bool) -> u64 {
    let base_100ns = (base_interval_ms as u64) * 10_000; // ms to 100ns

    if !pll_enabled {
        return base_100ns;
    }

    // Only apply PLL when PTP is running and locked
    if !is_ptp_running() {
        return base_100ns;
    }

    let stats = match get_ptp_stats() {
        Some(s) => s,
        None => return base_100ns,
    };

    if !stats.locked {
        return base_100ns;
    }

    // Apply frequency correction
    // frequency_ppm > 0 means local clock is fast, so lengthen interval
    // frequency_ppm < 0 means local clock is slow, so shorten interval
    let freq_ppm = stats.frequency_ppm;
    let adjustment_factor = 1.0 + (freq_ppm / 1_000_000.0);
    let adjusted = (base_100ns as f64) * adjustment_factor;

    // Clamp to reasonable range (±10% of base)
    let min = (base_100ns as f64) * 0.9;
    let max = (base_100ns as f64) * 1.1;
    adjusted.clamp(min, max) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interval_calculation() {
        // 20ms base = 200,000 100ns units
        let base = calculate_adjusted_interval(20, false);
        assert_eq!(base, 200_000);
    }
}
