//! High-precision timer for no-soundcard mode.
//!
//! Provides a timer that fires at configurable intervals (default 20ms).
//! Since this is a system clock (free-running), no PLL adjustment is applied.
//! Cross-platform: Windows and Linux.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

/// Timer callback type (boxed closure)
pub type TimerCallback = Box<dyn Fn() + Send + 'static>;

/// Timer configuration
static TIMER_INTERVAL_MS: AtomicU32 = AtomicU32::new(20);

/// Timer handle
pub struct Timer {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Timer {
    /// Start a new timer with the given interval and optional callback.
    ///
    /// # Arguments
    /// * `interval_ms` - Timer period in milliseconds (1-1000)
    /// * `callback` - Optional function to call on each tick
    ///
    /// # Returns
    /// * Ok(Timer) on success
    /// * Err(String) on failure
    pub fn start(interval_ms: u32, callback: Option<TimerCallback>) -> Result<Self, String> {
        if interval_ms == 0 || interval_ms > 1000 {
            return Err("Invalid interval".to_string());
        }

        TIMER_INTERVAL_MS.store(interval_ms, Ordering::SeqCst);

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let thread = thread::Builder::new()
            .name("sys-clock-timer".to_string())
            .spawn(move || {
                timer_thread(running_clone, callback);
            })
            .map_err(|e| format!("Failed to spawn timer thread: {}", e))?;

        Ok(Timer {
            running,
            thread: Some(thread),
        })
    }

    /// Stop the timer.
    pub fn stop(mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    /// Set timer interval (can change while running).
    pub fn set_interval(&self, interval_ms: u32) {
        if interval_ms > 0 && interval_ms <= 1000 {
            TIMER_INTERVAL_MS.store(interval_ms, Ordering::SeqCst);
        }
    }

    /// Get current timer interval.
    pub fn get_interval(&self) -> u32 {
        TIMER_INTERVAL_MS.load(Ordering::SeqCst)
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        // Don't join in drop - the thread will exit on its own
    }
}

/// Windows timer thread using waitable timer.
#[cfg(windows)]
fn timer_thread(running: Arc<AtomicBool>, callback: Option<TimerCallback>) {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        CreateWaitableTimerW, SetWaitableTimer, WaitForSingleObject,
    };

    // Create waitable timer
    let timer_handle: HANDLE =
        unsafe { CreateWaitableTimerW(std::ptr::null(), 1, std::ptr::null()) };

    if timer_handle.is_null() || timer_handle == 0 as HANDLE {
        // Fallback to sleep-based timer
        return timer_thread_fallback(running, callback);
    }

    while running.load(Ordering::SeqCst) {
        let interval_ms = TIMER_INTERVAL_MS.load(Ordering::SeqCst);
        let interval_100ns = (interval_ms as i64) * 10_000;

        // Set timer (negative value = relative time in 100ns units)
        let due_time: i64 = -interval_100ns;
        let result =
            unsafe { SetWaitableTimer(timer_handle, &due_time, 0, None, std::ptr::null(), 0) };

        if result == 0 {
            break;
        }

        // Wait for timer with short timeout to check running flag
        let wait_result = unsafe { WaitForSingleObject(timer_handle, interval_ms + 100) };

        match wait_result {
            WAIT_OBJECT_0 => {
                // Timer fired, call callback
                if let Some(ref cb) = callback {
                    cb();
                }
            }
            WAIT_TIMEOUT => {
                // Timeout - check running flag and continue
                continue;
            }
            _ => {
                // Error
                break;
            }
        }
    }

    unsafe { CloseHandle(timer_handle) };
}

/// Windows fallback timer using Sleep.
#[cfg(windows)]
fn timer_thread_fallback(running: Arc<AtomicBool>, callback: Option<TimerCallback>) {
    use std::time::{Duration, Instant};

    let mut next_tick = Instant::now();

    while running.load(Ordering::SeqCst) {
        let interval_ms = TIMER_INTERVAL_MS.load(Ordering::SeqCst);
        next_tick += Duration::from_millis(interval_ms as u64);

        let now = Instant::now();
        if next_tick > now {
            thread::sleep(next_tick - now);
        }

        if let Some(ref cb) = callback {
            cb();
        }

        // Reset if we've fallen too far behind
        let now = Instant::now();
        if now > next_tick + Duration::from_millis(interval_ms as u64 * 2) {
            next_tick = now;
        }
    }
}

/// Linux/Unix timer thread using nanosleep.
#[cfg(unix)]
fn timer_thread(running: Arc<AtomicBool>, callback: Option<TimerCallback>) {
    use std::time::{Duration, Instant};

    let mut next_tick = Instant::now();

    while running.load(Ordering::SeqCst) {
        let interval_ms = TIMER_INTERVAL_MS.load(Ordering::SeqCst);
        next_tick += Duration::from_millis(interval_ms as u64);

        let now = Instant::now();
        if next_tick > now {
            thread::sleep(next_tick - now);
        }

        if let Some(ref cb) = callback {
            cb();
        }

        // Reset if we've fallen too far behind
        let now = Instant::now();
        if now > next_tick + Duration::from_millis(interval_ms as u64 * 2) {
            next_tick = now;
        }
    }
}

/// Fallback for other platforms.
#[cfg(not(any(windows, unix)))]
fn timer_thread(running: Arc<AtomicBool>, callback: Option<TimerCallback>) {
    use std::time::Duration;

    while running.load(Ordering::SeqCst) {
        let interval_ms = TIMER_INTERVAL_MS.load(Ordering::SeqCst);
        thread::sleep(Duration::from_millis(interval_ms as u64));

        if let Some(ref cb) = callback {
            cb();
        }
    }
}
