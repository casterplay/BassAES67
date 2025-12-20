//! FFI bindings to bass_ptp.dll
//!
//! Provides runtime dynamic loading of bass_ptp.dll so that bass_aes67.dll
//! can work even if PTP support is not needed.

use std::ffi::{c_char, c_void, CString};
use std::net::Ipv4Addr;
use std::sync::OnceLock;

// ============================================================================
// Function pointer types
// ============================================================================

type PtpStartFn = unsafe extern "C" fn(*const c_char, u8) -> i32;
type PtpStopFn = unsafe extern "C" fn() -> i32;
type PtpForceStopFn = unsafe extern "C" fn() -> i32;
type PtpIsRunningFn = unsafe extern "C" fn() -> i32;
type PtpGetOffsetFn = unsafe extern "C" fn() -> i64;
type PtpGetFrequencyPpmFn = unsafe extern "C" fn() -> f64;
type PtpGetStatsStringFn = unsafe extern "C" fn(*mut c_char, i32) -> i32;
type PtpGetVersionFn = unsafe extern "C" fn() -> u32;
type PtpGetStateFn = unsafe extern "C" fn() -> u8;
type PtpIsLockedFn = unsafe extern "C" fn() -> i32;

// Timer callback type
pub type PtpTimerCallback = unsafe extern "C" fn(*mut c_void);

// Timer function types
type PtpTimerStartFn = unsafe extern "C" fn(u32, Option<PtpTimerCallback>, *mut c_void) -> i32;
type PtpTimerStopFn = unsafe extern "C" fn() -> i32;
type PtpTimerIsRunningFn = unsafe extern "C" fn() -> i32;
type PtpTimerSetIntervalFn = unsafe extern "C" fn(u32) -> i32;
type PtpTimerGetIntervalFn = unsafe extern "C" fn() -> u32;
type PtpTimerSetPllFn = unsafe extern "C" fn(i32) -> i32;
type PtpTimerIsPllEnabledFn = unsafe extern "C" fn() -> i32;

// ============================================================================
// Error codes (must match bass_ptp)
// ============================================================================

pub const BASS_PTP_OK: i32 = 0;
#[allow(dead_code)]
pub const BASS_PTP_ERROR_ALREADY: i32 = 1;
pub const BASS_PTP_ERROR_NOT_INIT: i32 = 2;
#[allow(dead_code)]
pub const BASS_PTP_ERROR_SOCKET: i32 = 3;
#[allow(dead_code)]
pub const BASS_PTP_ERROR_INVALID: i32 = 4;

// ============================================================================
// PTP state values (must match bass_ptp)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum PtpState {
    #[default]
    Disabled = 0,
    Listening = 1,
    Uncalibrated = 2,
    Slave = 3,
}

impl From<u8> for PtpState {
    fn from(value: u8) -> Self {
        match value {
            0 => PtpState::Disabled,
            1 => PtpState::Listening,
            2 => PtpState::Uncalibrated,
            3 => PtpState::Slave,
            _ => PtpState::Disabled,
        }
    }
}

// ============================================================================
// Function table
// ============================================================================

struct PtpFunctions {
    start: PtpStartFn,
    stop: PtpStopFn,
    force_stop: PtpForceStopFn,
    is_running: PtpIsRunningFn,
    get_offset: PtpGetOffsetFn,
    get_frequency_ppm: PtpGetFrequencyPpmFn,
    get_stats_string: PtpGetStatsStringFn,
    get_version: PtpGetVersionFn,
    get_state: PtpGetStateFn,
    is_locked: PtpIsLockedFn,
    // Timer functions
    timer_start: PtpTimerStartFn,
    timer_stop: PtpTimerStopFn,
    timer_is_running: PtpTimerIsRunningFn,
    timer_set_interval: PtpTimerSetIntervalFn,
    timer_get_interval: PtpTimerGetIntervalFn,
    timer_set_pll: PtpTimerSetPllFn,
    timer_is_pll_enabled: PtpTimerIsPllEnabledFn,
}

// ============================================================================
// Global state
// ============================================================================

static PTP_LIB: OnceLock<Option<PtpLibrary>> = OnceLock::new();

struct PtpLibrary {
    #[cfg(windows)]
    _handle: *mut c_void,
    functions: PtpFunctions,
}

// SAFETY: The library handle and function pointers are valid for the lifetime of the process
unsafe impl Send for PtpLibrary {}
unsafe impl Sync for PtpLibrary {}

// ============================================================================
// Windows-specific loading
// ============================================================================

#[cfg(windows)]
mod windows_loader {
    use super::*;

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryW(lpLibFileName: *const u16) -> *mut c_void;
        fn GetProcAddress(hModule: *mut c_void, lpProcName: *const i8) -> *mut c_void;
        fn GetModuleHandleW(lpModuleName: *const u16) -> *mut c_void;
        fn GetModuleFileNameW(hModule: *mut c_void, lpFilename: *mut u16, nSize: u32) -> u32;
    }

    /// Convert Rust string to wide string
    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Get directory of current DLL
    fn get_dll_directory() -> Option<String> {
        unsafe {
            // Get handle to our own DLL
            let module = GetModuleHandleW(to_wide("bass_aes67.dll").as_ptr());
            if module.is_null() {
                return None;
            }

            // Get full path
            let mut path = vec![0u16; 260];
            let len = GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32);
            if len == 0 {
                return None;
            }

            // Convert to string and get directory
            let path_str = String::from_utf16_lossy(&path[..len as usize]);
            let path = std::path::Path::new(&path_str);
            path.parent().map(|p| p.to_string_lossy().into_owned())
        }
    }

    /// Load bass_ptp.dll from same directory as bass_aes67.dll
    pub fn load_ptp_library() -> Option<PtpLibrary> {
        unsafe {
            // Try to load from same directory as bass_aes67.dll
            let dll_path = if let Some(dir) = get_dll_directory() {
                format!("{}\\bass_ptp.dll", dir)
            } else {
                "bass_ptp.dll".to_string()
            };

            let wide_path = to_wide(&dll_path);
            let handle = LoadLibraryW(wide_path.as_ptr());

            if handle.is_null() {
                // Try current directory
                let handle = LoadLibraryW(to_wide("bass_ptp.dll").as_ptr());
                if handle.is_null() {
                    return None;
                }
                return load_functions(handle);
            }

            load_functions(handle)
        }
    }

    unsafe fn load_functions(handle: *mut c_void) -> Option<PtpLibrary> {
        macro_rules! load_fn {
            ($name:expr, $ty:ty) => {{
                let ptr = GetProcAddress(handle, concat!($name, "\0").as_ptr() as *const i8);
                if ptr.is_null() {
                    return None;
                }
                std::mem::transmute::<*mut c_void, $ty>(ptr)
            }};
        }

        let functions = PtpFunctions {
            start: load_fn!("BASS_PTP_Start", PtpStartFn),
            stop: load_fn!("BASS_PTP_Stop", PtpStopFn),
            force_stop: load_fn!("BASS_PTP_ForceStop", PtpForceStopFn),
            is_running: load_fn!("BASS_PTP_IsRunning", PtpIsRunningFn),
            get_offset: load_fn!("BASS_PTP_GetOffset", PtpGetOffsetFn),
            get_frequency_ppm: load_fn!("BASS_PTP_GetFrequencyPPM", PtpGetFrequencyPpmFn),
            get_stats_string: load_fn!("BASS_PTP_GetStatsString", PtpGetStatsStringFn),
            get_version: load_fn!("BASS_PTP_GetVersion", PtpGetVersionFn),
            get_state: load_fn!("BASS_PTP_GetState", PtpGetStateFn),
            is_locked: load_fn!("BASS_PTP_IsLocked", PtpIsLockedFn),
            // Timer functions
            timer_start: load_fn!("BASS_PTP_TimerStart", PtpTimerStartFn),
            timer_stop: load_fn!("BASS_PTP_TimerStop", PtpTimerStopFn),
            timer_is_running: load_fn!("BASS_PTP_TimerIsRunning", PtpTimerIsRunningFn),
            timer_set_interval: load_fn!("BASS_PTP_TimerSetInterval", PtpTimerSetIntervalFn),
            timer_get_interval: load_fn!("BASS_PTP_TimerGetInterval", PtpTimerGetIntervalFn),
            timer_set_pll: load_fn!("BASS_PTP_TimerSetPLL", PtpTimerSetPllFn),
            timer_is_pll_enabled: load_fn!("BASS_PTP_TimerIsPLLEnabled", PtpTimerIsPllEnabledFn),
        };

        Some(PtpLibrary {
            _handle: handle,
            functions,
        })
    }
}

#[cfg(not(windows))]
mod unix_loader {
    use super::*;

    pub fn load_ptp_library() -> Option<PtpLibrary> {
        // TODO: Implement for Linux/macOS using dlopen
        None
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Initialize PTP bindings by loading bass_ptp.dll
///
/// Call this once during plugin initialization. Returns true if library loaded.
pub fn init_ptp_bindings() -> bool {
    PTP_LIB
        .get_or_init(|| {
            #[cfg(windows)]
            {
                windows_loader::load_ptp_library()
            }
            #[cfg(not(windows))]
            {
                unix_loader::load_ptp_library()
            }
        })
        .is_some()
}

/// Check if PTP library is available
pub fn is_ptp_available() -> bool {
    PTP_LIB.get().map(|l| l.is_some()).unwrap_or(false)
}

/// Start PTP client
pub fn ptp_start(interface: Ipv4Addr, domain: u8) -> Result<(), i32> {
    let lib = match PTP_LIB.get().and_then(|l| l.as_ref()) {
        Some(l) => l,
        None => return Err(BASS_PTP_ERROR_NOT_INIT),
    };

    let ip_str = CString::new(interface.to_string()).map_err(|_| BASS_PTP_ERROR_NOT_INIT)?;

    let result = unsafe { (lib.functions.start)(ip_str.as_ptr(), domain) };

    if result == BASS_PTP_OK {
        Ok(())
    } else {
        Err(result)
    }
}

/// Stop PTP client
pub fn ptp_stop() {
    if let Some(Some(lib)) = PTP_LIB.get() {
        unsafe {
            (lib.functions.stop)();
        }
    }
}

/// Force stop PTP client
pub fn ptp_force_stop() {
    if let Some(Some(lib)) = PTP_LIB.get() {
        unsafe {
            (lib.functions.force_stop)();
        }
    }
}

/// Check if PTP is running
pub fn ptp_is_running() -> bool {
    PTP_LIB
        .get()
        .and_then(|l| l.as_ref())
        .map(|lib| unsafe { (lib.functions.is_running)() != 0 })
        .unwrap_or(false)
}

/// Get current offset in nanoseconds
pub fn ptp_get_offset() -> i64 {
    PTP_LIB
        .get()
        .and_then(|l| l.as_ref())
        .map(|lib| unsafe { (lib.functions.get_offset)() })
        .unwrap_or(0)
}

/// Get current frequency adjustment in ppm
pub fn ptp_get_frequency_ppm() -> f64 {
    PTP_LIB
        .get()
        .and_then(|l| l.as_ref())
        .map(|lib| unsafe { (lib.functions.get_frequency_ppm)() })
        .unwrap_or(0.0)
}

/// Get formatted stats string
pub fn ptp_get_stats_string() -> String {
    let lib = match PTP_LIB.get().and_then(|l| l.as_ref()) {
        Some(l) => l,
        None => return String::from("PTP: Not available"),
    };

    let mut buffer = vec![0i8; 256];
    let len = unsafe { (lib.functions.get_stats_string)(buffer.as_mut_ptr(), buffer.len() as i32) };

    if len > 0 {
        // Convert to Rust string
        let bytes: Vec<u8> = buffer[..len as usize].iter().map(|&b| b as u8).collect();
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        String::from("PTP: Error getting stats")
    }
}

/// Get PTP state
pub fn ptp_get_state() -> PtpState {
    PTP_LIB
        .get()
        .and_then(|l| l.as_ref())
        .map(|lib| unsafe { PtpState::from((lib.functions.get_state)()) })
        .unwrap_or(PtpState::Disabled)
}

/// Check if PTP is locked
pub fn ptp_is_locked() -> bool {
    PTP_LIB
        .get()
        .and_then(|l| l.as_ref())
        .map(|lib| unsafe { (lib.functions.is_locked)() != 0 })
        .unwrap_or(false)
}

/// Get PTP library version
pub fn ptp_get_version() -> u32 {
    PTP_LIB
        .get()
        .and_then(|l| l.as_ref())
        .map(|lib| unsafe { (lib.functions.get_version)() })
        .unwrap_or(0)
}

// ============================================================================
// Timer Public API
// ============================================================================

/// Start the precision timer
///
/// # Arguments
/// * `interval_ms` - Timer period in milliseconds (1-1000)
/// * `callback` - Function to call on each tick (can be None)
/// * `user` - User data passed to callback
pub fn ptp_timer_start(
    interval_ms: u32,
    callback: Option<PtpTimerCallback>,
    user: *mut c_void,
) -> Result<(), i32> {
    let lib = match PTP_LIB.get().and_then(|l| l.as_ref()) {
        Some(l) => l,
        None => return Err(BASS_PTP_ERROR_NOT_INIT),
    };

    let result = unsafe { (lib.functions.timer_start)(interval_ms, callback, user) };

    if result == BASS_PTP_OK {
        Ok(())
    } else {
        Err(result)
    }
}

/// Stop the timer
pub fn ptp_timer_stop() {
    if let Some(Some(lib)) = PTP_LIB.get() {
        unsafe {
            (lib.functions.timer_stop)();
        }
    }
}

/// Check if timer is running
pub fn ptp_timer_is_running() -> bool {
    PTP_LIB
        .get()
        .and_then(|l| l.as_ref())
        .map(|lib| unsafe { (lib.functions.timer_is_running)() != 0 })
        .unwrap_or(false)
}

/// Set timer interval (can change while running)
pub fn ptp_timer_set_interval(interval_ms: u32) -> Result<(), i32> {
    let lib = match PTP_LIB.get().and_then(|l| l.as_ref()) {
        Some(l) => l,
        None => return Err(BASS_PTP_ERROR_NOT_INIT),
    };

    let result = unsafe { (lib.functions.timer_set_interval)(interval_ms) };

    if result == BASS_PTP_OK {
        Ok(())
    } else {
        Err(result)
    }
}

/// Get current timer interval
pub fn ptp_timer_get_interval() -> u32 {
    PTP_LIB
        .get()
        .and_then(|l| l.as_ref())
        .map(|lib| unsafe { (lib.functions.timer_get_interval)() })
        .unwrap_or(20)
}

/// Enable/disable PLL adjustment
pub fn ptp_timer_set_pll(enabled: bool) {
    if let Some(Some(lib)) = PTP_LIB.get() {
        unsafe {
            (lib.functions.timer_set_pll)(if enabled { 1 } else { 0 });
        }
    }
}

/// Check if PLL adjustment is enabled
pub fn ptp_timer_is_pll_enabled() -> bool {
    PTP_LIB
        .get()
        .and_then(|l| l.as_ref())
        .map(|lib| unsafe { (lib.functions.timer_is_pll_enabled)() != 0 })
        .unwrap_or(true)
}
