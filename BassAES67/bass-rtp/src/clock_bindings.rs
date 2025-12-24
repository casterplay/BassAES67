//! Unified clock bindings for RTP audio synchronization.
//!
//! Provides runtime dynamic loading of bass_ptp.dll, bass_livewire_clock.dll,
//! and bass_system_clock.dll, allowing applications to select between:
//! - PTP (IEEE 1588v2)
//! - Axia Livewire clock
//! - System clock (free-running fallback)
//!
//! Supports automatic fallback to system clock when primary clock loses lock.

use std::ffi::{c_char, c_void, CString};
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

// ============================================================================
// Clock Mode Selection
// ============================================================================

/// Clock synchronization mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ClockMode {
    /// IEEE 1588v2 PTP (default)
    Ptp = 0,
    /// Axia Livewire Clock
    Livewire = 1,
    /// System clock (free-running, no sync)
    System = 2,
}

impl From<u32> for ClockMode {
    fn from(value: u32) -> Self {
        match value {
            1 => ClockMode::Livewire,
            2 => ClockMode::System,
            _ => ClockMode::Ptp,
        }
    }
}

/// Currently active clock (0=none, 1=PTP, 2=Livewire, 3=System)
static ACTIVE_CLOCK: AtomicU8 = AtomicU8::new(0);

// ============================================================================
// Fallback State Tracking
// ============================================================================

/// Is fallback to system clock currently active?
static FALLBACK_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Timestamp (in ms since start) of last successful lock
static LAST_LOCK_TIME_MS: AtomicU64 = AtomicU64::new(0);

/// Fallback timeout in seconds (0 = disabled)
static FALLBACK_TIMEOUT_SECS: AtomicU32 = AtomicU32::new(5);

/// Start time for elapsed time tracking
static START_TIME: OnceLock<Instant> = OnceLock::new();

/// Get milliseconds since start
fn elapsed_ms() -> u64 {
    START_TIME
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis() as u64
}

// ============================================================================
// Function pointer types (same for both PTP and Livewire)
// ============================================================================

type ClockStartPtpFn = unsafe extern "C" fn(*const c_char, u8) -> i32;
type ClockStartLwFn = unsafe extern "C" fn(*const c_char) -> i32;
type ClockStopFn = unsafe extern "C" fn() -> i32;
type ClockForceStopFn = unsafe extern "C" fn() -> i32;
type ClockIsRunningFn = unsafe extern "C" fn() -> i32;
type ClockGetOffsetFn = unsafe extern "C" fn() -> i64;
type ClockGetFrequencyPpmFn = unsafe extern "C" fn() -> f64;
type ClockGetStatsStringFn = unsafe extern "C" fn(*mut c_char, i32) -> i32;
type ClockGetVersionFn = unsafe extern "C" fn() -> u32;
type ClockGetStateFn = unsafe extern "C" fn() -> u8;
type ClockIsLockedFn = unsafe extern "C" fn() -> i32;

/// Timer callback type
pub type ClockTimerCallback = unsafe extern "C" fn(*mut c_void);

type ClockTimerStartFn = unsafe extern "C" fn(u32, Option<ClockTimerCallback>, *mut c_void) -> i32;
type ClockTimerStopFn = unsafe extern "C" fn() -> i32;
type ClockTimerIsRunningFn = unsafe extern "C" fn() -> i32;
type ClockTimerSetIntervalFn = unsafe extern "C" fn(u32) -> i32;
type ClockTimerGetIntervalFn = unsafe extern "C" fn() -> u32;
type ClockTimerSetPllFn = unsafe extern "C" fn(i32) -> i32;
type ClockTimerIsPllEnabledFn = unsafe extern "C" fn() -> i32;

// ============================================================================
// Error codes
// ============================================================================

pub const CLOCK_OK: i32 = 0;
#[allow(dead_code)]
pub const CLOCK_ERROR_ALREADY: i32 = 1;
pub const CLOCK_ERROR_NOT_INIT: i32 = 2;
#[allow(dead_code)]
pub const CLOCK_ERROR_SOCKET: i32 = 3;
#[allow(dead_code)]
pub const CLOCK_ERROR_INVALID: i32 = 4;

// ============================================================================
// Clock state values (same for PTP and Livewire)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum ClockState {
    #[default]
    Disabled = 0,
    Listening = 1,
    Uncalibrated = 2,
    Slave = 3,
}

impl From<u8> for ClockState {
    fn from(value: u8) -> Self {
        match value {
            0 => ClockState::Disabled,
            1 => ClockState::Listening,
            2 => ClockState::Uncalibrated,
            3 => ClockState::Slave,
            _ => ClockState::Disabled,
        }
    }
}

// ============================================================================
// PTP Function Table
// ============================================================================

struct PtpFunctions {
    start: ClockStartPtpFn,
    #[allow(dead_code)]
    stop: ClockStopFn,
    force_stop: ClockForceStopFn,
    is_running: ClockIsRunningFn,
    get_offset: ClockGetOffsetFn,
    get_frequency_ppm: ClockGetFrequencyPpmFn,
    get_stats_string: ClockGetStatsStringFn,
    get_version: ClockGetVersionFn,
    get_state: ClockGetStateFn,
    is_locked: ClockIsLockedFn,
    timer_start: ClockTimerStartFn,
    timer_stop: ClockTimerStopFn,
    timer_is_running: ClockTimerIsRunningFn,
    timer_set_interval: ClockTimerSetIntervalFn,
    timer_get_interval: ClockTimerGetIntervalFn,
    timer_set_pll: ClockTimerSetPllFn,
    timer_is_pll_enabled: ClockTimerIsPllEnabledFn,
}

struct PtpLibrary {
    _handle: *mut c_void,
    functions: PtpFunctions,
}

unsafe impl Send for PtpLibrary {}
unsafe impl Sync for PtpLibrary {}

static PTP_LIB: OnceLock<Option<PtpLibrary>> = OnceLock::new();

// ============================================================================
// Livewire Function Table
// ============================================================================

struct LwFunctions {
    start: ClockStartLwFn,
    #[allow(dead_code)]
    stop: ClockStopFn,
    force_stop: ClockForceStopFn,
    is_running: ClockIsRunningFn,
    get_offset: ClockGetOffsetFn,
    get_frequency_ppm: ClockGetFrequencyPpmFn,
    get_stats_string: ClockGetStatsStringFn,
    get_version: ClockGetVersionFn,
    get_state: ClockGetStateFn,
    is_locked: ClockIsLockedFn,
    timer_start: ClockTimerStartFn,
    timer_stop: ClockTimerStopFn,
    timer_is_running: ClockTimerIsRunningFn,
    timer_set_interval: ClockTimerSetIntervalFn,
    timer_get_interval: ClockTimerGetIntervalFn,
    timer_set_pll: ClockTimerSetPllFn,
    timer_is_pll_enabled: ClockTimerIsPllEnabledFn,
}

struct LwLibrary {
    _handle: *mut c_void,
    functions: LwFunctions,
}

unsafe impl Send for LwLibrary {}
unsafe impl Sync for LwLibrary {}

static LW_LIB: OnceLock<Option<LwLibrary>> = OnceLock::new();

// ============================================================================
// System Clock Function Table
// ============================================================================

/// System clock has simpler API - always succeeds, no network
type SysStartFn = unsafe extern "C" fn(*const c_char) -> i32;
type SysStopFn = unsafe extern "C" fn() -> i32;
type SysForceStopFn = unsafe extern "C" fn() -> i32;
type SysIsRunningFn = unsafe extern "C" fn() -> i32;
type SysGetOffsetFn = unsafe extern "C" fn() -> i64;
type SysGetFrequencyPpmFn = unsafe extern "C" fn() -> f64;
type SysGetStatsStringFn = unsafe extern "C" fn(*mut c_char, i32) -> i32;
type SysGetVersionFn = unsafe extern "C" fn() -> u32;
type SysGetStateFn = unsafe extern "C" fn() -> u8;
type SysIsLockedFn = unsafe extern "C" fn() -> i32;

struct SysFunctions {
    start: SysStartFn,
    #[allow(dead_code)]
    stop: SysStopFn,
    force_stop: SysForceStopFn,
    is_running: SysIsRunningFn,
    get_offset: SysGetOffsetFn,
    get_frequency_ppm: SysGetFrequencyPpmFn,
    get_stats_string: SysGetStatsStringFn,
    get_version: SysGetVersionFn,
    get_state: SysGetStateFn,
    #[allow(dead_code)]
    is_locked: SysIsLockedFn,
    timer_start: ClockTimerStartFn,
    timer_stop: ClockTimerStopFn,
    timer_is_running: ClockTimerIsRunningFn,
    timer_set_interval: ClockTimerSetIntervalFn,
    timer_get_interval: ClockTimerGetIntervalFn,
    timer_set_pll: ClockTimerSetPllFn,
    timer_is_pll_enabled: ClockTimerIsPllEnabledFn,
}

struct SysLibrary {
    _handle: *mut c_void,
    functions: SysFunctions,
}

unsafe impl Send for SysLibrary {}
unsafe impl Sync for SysLibrary {}

static SYS_LIB: OnceLock<Option<SysLibrary>> = OnceLock::new();

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

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn get_dll_directory() -> Option<String> {
        unsafe {
            let module = GetModuleHandleW(to_wide("bass_rtp.dll").as_ptr());
            if module.is_null() {
                return None;
            }

            let mut path = vec![0u16; 260];
            let len = GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32);
            if len == 0 {
                return None;
            }

            let path_str = String::from_utf16_lossy(&path[..len as usize]);
            let path = std::path::Path::new(&path_str);
            path.parent().map(|p| p.to_string_lossy().into_owned())
        }
    }

    fn load_library(dll_name: &str) -> *mut c_void {
        unsafe {
            let dll_path = if let Some(dir) = get_dll_directory() {
                format!("{}\\{}", dir, dll_name)
            } else {
                dll_name.to_string()
            };

            let wide_path = to_wide(&dll_path);
            let handle = LoadLibraryW(wide_path.as_ptr());

            if handle.is_null() {
                LoadLibraryW(to_wide(dll_name).as_ptr())
            } else {
                handle
            }
        }
    }

    pub fn load_ptp_library() -> Option<PtpLibrary> {
        let handle = load_library("bass_ptp.dll");
        if handle.is_null() {
            return None;
        }

        unsafe {
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
                start: load_fn!("BASS_PTP_Start", ClockStartPtpFn),
                stop: load_fn!("BASS_PTP_Stop", ClockStopFn),
                force_stop: load_fn!("BASS_PTP_ForceStop", ClockForceStopFn),
                is_running: load_fn!("BASS_PTP_IsRunning", ClockIsRunningFn),
                get_offset: load_fn!("BASS_PTP_GetOffset", ClockGetOffsetFn),
                get_frequency_ppm: load_fn!("BASS_PTP_GetFrequencyPPM", ClockGetFrequencyPpmFn),
                get_stats_string: load_fn!("BASS_PTP_GetStatsString", ClockGetStatsStringFn),
                get_version: load_fn!("BASS_PTP_GetVersion", ClockGetVersionFn),
                get_state: load_fn!("BASS_PTP_GetState", ClockGetStateFn),
                is_locked: load_fn!("BASS_PTP_IsLocked", ClockIsLockedFn),
                timer_start: load_fn!("BASS_PTP_TimerStart", ClockTimerStartFn),
                timer_stop: load_fn!("BASS_PTP_TimerStop", ClockTimerStopFn),
                timer_is_running: load_fn!("BASS_PTP_TimerIsRunning", ClockTimerIsRunningFn),
                timer_set_interval: load_fn!("BASS_PTP_TimerSetInterval", ClockTimerSetIntervalFn),
                timer_get_interval: load_fn!("BASS_PTP_TimerGetInterval", ClockTimerGetIntervalFn),
                timer_set_pll: load_fn!("BASS_PTP_TimerSetPLL", ClockTimerSetPllFn),
                timer_is_pll_enabled: load_fn!("BASS_PTP_TimerIsPLLEnabled", ClockTimerIsPllEnabledFn),
            };

            Some(PtpLibrary {
                _handle: handle,
                functions,
            })
        }
    }

    pub fn load_lw_library() -> Option<LwLibrary> {
        let handle = load_library("bass_livewire_clock.dll");
        if handle.is_null() {
            return None;
        }

        unsafe {
            macro_rules! load_fn {
                ($name:expr, $ty:ty) => {{
                    let ptr = GetProcAddress(handle, concat!($name, "\0").as_ptr() as *const i8);
                    if ptr.is_null() {
                        return None;
                    }
                    std::mem::transmute::<*mut c_void, $ty>(ptr)
                }};
            }

            let functions = LwFunctions {
                start: load_fn!("BASS_LW_Start", ClockStartLwFn),
                stop: load_fn!("BASS_LW_Stop", ClockStopFn),
                force_stop: load_fn!("BASS_LW_ForceStop", ClockForceStopFn),
                is_running: load_fn!("BASS_LW_IsRunning", ClockIsRunningFn),
                get_offset: load_fn!("BASS_LW_GetOffset", ClockGetOffsetFn),
                get_frequency_ppm: load_fn!("BASS_LW_GetFrequencyPPM", ClockGetFrequencyPpmFn),
                get_stats_string: load_fn!("BASS_LW_GetStatsString", ClockGetStatsStringFn),
                get_version: load_fn!("BASS_LW_GetVersion", ClockGetVersionFn),
                get_state: load_fn!("BASS_LW_GetState", ClockGetStateFn),
                is_locked: load_fn!("BASS_LW_IsLocked", ClockIsLockedFn),
                timer_start: load_fn!("BASS_LW_TimerStart", ClockTimerStartFn),
                timer_stop: load_fn!("BASS_LW_TimerStop", ClockTimerStopFn),
                timer_is_running: load_fn!("BASS_LW_TimerIsRunning", ClockTimerIsRunningFn),
                timer_set_interval: load_fn!("BASS_LW_TimerSetInterval", ClockTimerSetIntervalFn),
                timer_get_interval: load_fn!("BASS_LW_TimerGetInterval", ClockTimerGetIntervalFn),
                timer_set_pll: load_fn!("BASS_LW_TimerSetPLL", ClockTimerSetPllFn),
                timer_is_pll_enabled: load_fn!("BASS_LW_TimerIsPLLEnabled", ClockTimerIsPllEnabledFn),
            };

            Some(LwLibrary {
                _handle: handle,
                functions,
            })
        }
    }

    pub fn load_sys_library() -> Option<SysLibrary> {
        let handle = load_library("bass_system_clock.dll");
        if handle.is_null() {
            return None;
        }

        unsafe {
            macro_rules! load_fn {
                ($name:expr, $ty:ty) => {{
                    let ptr = GetProcAddress(handle, concat!($name, "\0").as_ptr() as *const i8);
                    if ptr.is_null() {
                        return None;
                    }
                    std::mem::transmute::<*mut c_void, $ty>(ptr)
                }};
            }

            let functions = SysFunctions {
                start: load_fn!("BASS_SYS_Start", SysStartFn),
                stop: load_fn!("BASS_SYS_Stop", SysStopFn),
                force_stop: load_fn!("BASS_SYS_ForceStop", SysForceStopFn),
                is_running: load_fn!("BASS_SYS_IsRunning", SysIsRunningFn),
                get_offset: load_fn!("BASS_SYS_GetOffset", SysGetOffsetFn),
                get_frequency_ppm: load_fn!("BASS_SYS_GetFrequencyPPM", SysGetFrequencyPpmFn),
                get_stats_string: load_fn!("BASS_SYS_GetStatsString", SysGetStatsStringFn),
                get_version: load_fn!("BASS_SYS_GetVersion", SysGetVersionFn),
                get_state: load_fn!("BASS_SYS_GetState", SysGetStateFn),
                is_locked: load_fn!("BASS_SYS_IsLocked", SysIsLockedFn),
                timer_start: load_fn!("BASS_SYS_TimerStart", ClockTimerStartFn),
                timer_stop: load_fn!("BASS_SYS_TimerStop", ClockTimerStopFn),
                timer_is_running: load_fn!("BASS_SYS_TimerIsRunning", ClockTimerIsRunningFn),
                timer_set_interval: load_fn!("BASS_SYS_TimerSetInterval", ClockTimerSetIntervalFn),
                timer_get_interval: load_fn!("BASS_SYS_TimerGetInterval", ClockTimerGetIntervalFn),
                timer_set_pll: load_fn!("BASS_SYS_TimerSetPLL", ClockTimerSetPllFn),
                timer_is_pll_enabled: load_fn!("BASS_SYS_TimerIsPLLEnabled", ClockTimerIsPllEnabledFn),
            };

            Some(SysLibrary {
                _handle: handle,
                functions,
            })
        }
    }
}

#[cfg(not(windows))]
mod unix_loader {
    use super::*;

    // dlopen flags
    const RTLD_NOW: i32 = 2;
    const RTLD_LOCAL: i32 = 0;

    extern "C" {
        fn dlopen(filename: *const i8, flags: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const i8) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> i32;
    }

    /// Try to find the directory containing libbass_ptp.so (and other clock libraries)
    fn get_lib_directory() -> Option<String> {
        use std::path::Path;

        // Check if libraries are in the current directory
        if Path::new("./libbass_ptp.so").exists() {
            return Some(".".to_string());
        }

        // Check executable directory and parent directories
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(dir) = exe_path.parent() {
                let ptp_path = dir.join("libbass_ptp.so");
                if ptp_path.exists() {
                    return Some(dir.to_string_lossy().into_owned());
                }

                if let Some(parent) = dir.parent() {
                    let ptp_path = parent.join("libbass_ptp.so");
                    if ptp_path.exists() {
                        return Some(parent.to_string_lossy().into_owned());
                    }
                }
            }
        }

        if Path::new("target/release/libbass_ptp.so").exists() {
            return Some("target/release".to_string());
        }

        None
    }

    fn load_library(lib_name: &str) -> *mut c_void {
        unsafe {
            if let Some(dir) = get_lib_directory() {
                let full_path = format!("{}/{}", dir, lib_name);
                if let Ok(c_path) = CString::new(full_path) {
                    let handle = dlopen(c_path.as_ptr(), RTLD_NOW | RTLD_LOCAL);
                    if !handle.is_null() {
                        return handle;
                    }
                }
            }

            let c_name = match CString::new(lib_name) {
                Ok(s) => s,
                Err(_) => return std::ptr::null_mut(),
            };
            dlopen(c_name.as_ptr(), RTLD_NOW | RTLD_LOCAL)
        }
    }

    pub fn load_ptp_library() -> Option<PtpLibrary> {
        let handle = load_library("libbass_ptp.so");
        if handle.is_null() {
            return None;
        }

        unsafe {
            macro_rules! load_fn {
                ($name:expr, $ty:ty) => {{
                    let c_name = match CString::new($name) {
                        Ok(s) => s,
                        Err(_) => {
                            dlclose(handle);
                            return None;
                        }
                    };
                    let ptr = dlsym(handle, c_name.as_ptr());
                    if ptr.is_null() {
                        dlclose(handle);
                        return None;
                    }
                    std::mem::transmute::<*mut c_void, $ty>(ptr)
                }};
            }

            let functions = PtpFunctions {
                start: load_fn!("BASS_PTP_Start", ClockStartPtpFn),
                stop: load_fn!("BASS_PTP_Stop", ClockStopFn),
                force_stop: load_fn!("BASS_PTP_ForceStop", ClockForceStopFn),
                is_running: load_fn!("BASS_PTP_IsRunning", ClockIsRunningFn),
                get_offset: load_fn!("BASS_PTP_GetOffset", ClockGetOffsetFn),
                get_frequency_ppm: load_fn!("BASS_PTP_GetFrequencyPPM", ClockGetFrequencyPpmFn),
                get_stats_string: load_fn!("BASS_PTP_GetStatsString", ClockGetStatsStringFn),
                get_version: load_fn!("BASS_PTP_GetVersion", ClockGetVersionFn),
                get_state: load_fn!("BASS_PTP_GetState", ClockGetStateFn),
                is_locked: load_fn!("BASS_PTP_IsLocked", ClockIsLockedFn),
                timer_start: load_fn!("BASS_PTP_TimerStart", ClockTimerStartFn),
                timer_stop: load_fn!("BASS_PTP_TimerStop", ClockTimerStopFn),
                timer_is_running: load_fn!("BASS_PTP_TimerIsRunning", ClockTimerIsRunningFn),
                timer_set_interval: load_fn!("BASS_PTP_TimerSetInterval", ClockTimerSetIntervalFn),
                timer_get_interval: load_fn!("BASS_PTP_TimerGetInterval", ClockTimerGetIntervalFn),
                timer_set_pll: load_fn!("BASS_PTP_TimerSetPLL", ClockTimerSetPllFn),
                timer_is_pll_enabled: load_fn!("BASS_PTP_TimerIsPLLEnabled", ClockTimerIsPllEnabledFn),
            };

            Some(PtpLibrary {
                _handle: handle,
                functions,
            })
        }
    }

    pub fn load_lw_library() -> Option<LwLibrary> {
        let handle = load_library("libbass_livewire_clock.so");
        if handle.is_null() {
            return None;
        }

        unsafe {
            macro_rules! load_fn {
                ($name:expr, $ty:ty) => {{
                    let c_name = match CString::new($name) {
                        Ok(s) => s,
                        Err(_) => {
                            dlclose(handle);
                            return None;
                        }
                    };
                    let ptr = dlsym(handle, c_name.as_ptr());
                    if ptr.is_null() {
                        dlclose(handle);
                        return None;
                    }
                    std::mem::transmute::<*mut c_void, $ty>(ptr)
                }};
            }

            let functions = LwFunctions {
                start: load_fn!("BASS_LW_Start", ClockStartLwFn),
                stop: load_fn!("BASS_LW_Stop", ClockStopFn),
                force_stop: load_fn!("BASS_LW_ForceStop", ClockForceStopFn),
                is_running: load_fn!("BASS_LW_IsRunning", ClockIsRunningFn),
                get_offset: load_fn!("BASS_LW_GetOffset", ClockGetOffsetFn),
                get_frequency_ppm: load_fn!("BASS_LW_GetFrequencyPPM", ClockGetFrequencyPpmFn),
                get_stats_string: load_fn!("BASS_LW_GetStatsString", ClockGetStatsStringFn),
                get_version: load_fn!("BASS_LW_GetVersion", ClockGetVersionFn),
                get_state: load_fn!("BASS_LW_GetState", ClockGetStateFn),
                is_locked: load_fn!("BASS_LW_IsLocked", ClockIsLockedFn),
                timer_start: load_fn!("BASS_LW_TimerStart", ClockTimerStartFn),
                timer_stop: load_fn!("BASS_LW_TimerStop", ClockTimerStopFn),
                timer_is_running: load_fn!("BASS_LW_TimerIsRunning", ClockTimerIsRunningFn),
                timer_set_interval: load_fn!("BASS_LW_TimerSetInterval", ClockTimerSetIntervalFn),
                timer_get_interval: load_fn!("BASS_LW_TimerGetInterval", ClockTimerGetIntervalFn),
                timer_set_pll: load_fn!("BASS_LW_TimerSetPLL", ClockTimerSetPllFn),
                timer_is_pll_enabled: load_fn!("BASS_LW_TimerIsPLLEnabled", ClockTimerIsPllEnabledFn),
            };

            Some(LwLibrary {
                _handle: handle,
                functions,
            })
        }
    }

    pub fn load_sys_library() -> Option<SysLibrary> {
        let handle = load_library("libbass_system_clock.so");
        if handle.is_null() {
            return None;
        }

        unsafe {
            macro_rules! load_fn {
                ($name:expr, $ty:ty) => {{
                    let c_name = match CString::new($name) {
                        Ok(s) => s,
                        Err(_) => {
                            dlclose(handle);
                            return None;
                        }
                    };
                    let ptr = dlsym(handle, c_name.as_ptr());
                    if ptr.is_null() {
                        dlclose(handle);
                        return None;
                    }
                    std::mem::transmute::<*mut c_void, $ty>(ptr)
                }};
            }

            let functions = SysFunctions {
                start: load_fn!("BASS_SYS_Start", SysStartFn),
                stop: load_fn!("BASS_SYS_Stop", SysStopFn),
                force_stop: load_fn!("BASS_SYS_ForceStop", SysForceStopFn),
                is_running: load_fn!("BASS_SYS_IsRunning", SysIsRunningFn),
                get_offset: load_fn!("BASS_SYS_GetOffset", SysGetOffsetFn),
                get_frequency_ppm: load_fn!("BASS_SYS_GetFrequencyPPM", SysGetFrequencyPpmFn),
                get_stats_string: load_fn!("BASS_SYS_GetStatsString", SysGetStatsStringFn),
                get_version: load_fn!("BASS_SYS_GetVersion", SysGetVersionFn),
                get_state: load_fn!("BASS_SYS_GetState", SysGetStateFn),
                is_locked: load_fn!("BASS_SYS_IsLocked", SysIsLockedFn),
                timer_start: load_fn!("BASS_SYS_TimerStart", ClockTimerStartFn),
                timer_stop: load_fn!("BASS_SYS_TimerStop", ClockTimerStopFn),
                timer_is_running: load_fn!("BASS_SYS_TimerIsRunning", ClockTimerIsRunningFn),
                timer_set_interval: load_fn!("BASS_SYS_TimerSetInterval", ClockTimerSetIntervalFn),
                timer_get_interval: load_fn!("BASS_SYS_TimerGetInterval", ClockTimerGetIntervalFn),
                timer_set_pll: load_fn!("BASS_SYS_TimerSetPLL", ClockTimerSetPllFn),
                timer_is_pll_enabled: load_fn!("BASS_SYS_TimerIsPLLEnabled", ClockTimerIsPllEnabledFn),
            };

            Some(SysLibrary {
                _handle: handle,
                functions,
            })
        }
    }
}

// ============================================================================
// Initialization
// ============================================================================

/// Initialize clock bindings by loading bass_ptp.dll, bass_livewire_clock.dll, and bass_system_clock.dll.
/// Call this once during plugin initialization. Returns true if at least one library loaded.
pub fn init_clock_bindings() -> bool {
    // Initialize start time for fallback tracking
    let _ = START_TIME.get_or_init(Instant::now);

    let ptp_loaded = PTP_LIB
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
        .is_some();

    let lw_loaded = LW_LIB
        .get_or_init(|| {
            #[cfg(windows)]
            {
                windows_loader::load_lw_library()
            }
            #[cfg(not(windows))]
            {
                unix_loader::load_lw_library()
            }
        })
        .is_some();

    let sys_loaded = SYS_LIB
        .get_or_init(|| {
            #[cfg(windows)]
            {
                windows_loader::load_sys_library()
            }
            #[cfg(not(windows))]
            {
                unix_loader::load_sys_library()
            }
        })
        .is_some();

    ptp_loaded || lw_loaded || sys_loaded
}

/// Check if PTP library is available
pub fn is_ptp_available() -> bool {
    PTP_LIB.get().map(|l| l.is_some()).unwrap_or(false)
}

/// Check if Livewire clock library is available
pub fn is_lw_available() -> bool {
    LW_LIB.get().map(|l| l.is_some()).unwrap_or(false)
}

/// Check if System clock library is available
pub fn is_sys_available() -> bool {
    SYS_LIB.get().map(|l| l.is_some()).unwrap_or(false)
}

/// Get currently active clock mode (0=none, 1=PTP, 2=Livewire, 3=System)
pub fn get_active_clock() -> u8 {
    ACTIVE_CLOCK.load(Ordering::Relaxed)
}

/// Check if fallback to system clock is currently active
pub fn is_fallback_active() -> bool {
    FALLBACK_ACTIVE.load(Ordering::Relaxed)
}

/// Set fallback timeout in seconds (0 = disable fallback)
pub fn set_fallback_timeout(seconds: u32) {
    FALLBACK_TIMEOUT_SECS.store(seconds, Ordering::Relaxed);
}

/// Get current fallback timeout in seconds
pub fn get_fallback_timeout() -> u32 {
    FALLBACK_TIMEOUT_SECS.load(Ordering::Relaxed)
}

// ============================================================================
// Unified Clock API
// ============================================================================

/// Start the clock client based on mode selection.
pub fn clock_start(interface: Ipv4Addr, domain: u8, mode: ClockMode) -> Result<(), i32> {
    let ip_str = CString::new(interface.to_string()).map_err(|_| CLOCK_ERROR_NOT_INIT)?;

    // Reset fallback state
    FALLBACK_ACTIVE.store(false, Ordering::Relaxed);
    LAST_LOCK_TIME_MS.store(elapsed_ms(), Ordering::Relaxed);

    match mode {
        ClockMode::Ptp => {
            let lib = match PTP_LIB.get().and_then(|l| l.as_ref()) {
                Some(l) => l,
                None => return Err(CLOCK_ERROR_NOT_INIT),
            };
            let result = unsafe { (lib.functions.start)(ip_str.as_ptr(), domain) };
            if result == CLOCK_OK {
                ACTIVE_CLOCK.store(1, Ordering::Release);
                if let Some(Some(sys_lib)) = SYS_LIB.get() {
                    let _ = unsafe { (sys_lib.functions.start)(ip_str.as_ptr()) };
                }
                Ok(())
            } else {
                Err(result)
            }
        }
        ClockMode::Livewire => {
            let lib = match LW_LIB.get().and_then(|l| l.as_ref()) {
                Some(l) => l,
                None => return Err(CLOCK_ERROR_NOT_INIT),
            };
            let result = unsafe { (lib.functions.start)(ip_str.as_ptr()) };
            if result == CLOCK_OK {
                ACTIVE_CLOCK.store(2, Ordering::Release);
                if let Some(Some(sys_lib)) = SYS_LIB.get() {
                    let _ = unsafe { (sys_lib.functions.start)(ip_str.as_ptr()) };
                }
                Ok(())
            } else {
                Err(result)
            }
        }
        ClockMode::System => {
            let lib = match SYS_LIB.get().and_then(|l| l.as_ref()) {
                Some(l) => l,
                None => return Err(CLOCK_ERROR_NOT_INIT),
            };
            let result = unsafe { (lib.functions.start)(ip_str.as_ptr()) };
            if result == CLOCK_OK {
                ACTIVE_CLOCK.store(3, Ordering::Release);
                Ok(())
            } else {
                Err(result)
            }
        }
    }
}

/// Stop the currently active clock client.
pub fn clock_stop() {
    let active = ACTIVE_CLOCK.load(Ordering::Acquire);
    match active {
        1 => {
            if let Some(Some(lib)) = PTP_LIB.get() {
                unsafe { (lib.functions.force_stop)(); }
            }
        }
        2 => {
            if let Some(Some(lib)) = LW_LIB.get() {
                unsafe { (lib.functions.force_stop)(); }
            }
        }
        3 => {
            if let Some(Some(lib)) = SYS_LIB.get() {
                unsafe { (lib.functions.force_stop)(); }
            }
        }
        _ => {}
    }
    if active == 1 || active == 2 {
        if let Some(Some(lib)) = SYS_LIB.get() {
            unsafe { (lib.functions.force_stop)(); }
        }
    }
    FALLBACK_ACTIVE.store(false, Ordering::Relaxed);
    ACTIVE_CLOCK.store(0, Ordering::Release);
}

/// Check if any clock is running.
pub fn clock_is_running() -> bool {
    let active = ACTIVE_CLOCK.load(Ordering::Acquire);
    match active {
        1 => PTP_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.is_running)() != 0 })
            .unwrap_or(false),
        2 => LW_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.is_running)() != 0 })
            .unwrap_or(false),
        3 => SYS_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.is_running)() != 0 })
            .unwrap_or(false),
        _ => false,
    }
}

/// Get current frequency adjustment in ppm from the active clock.
pub fn clock_get_frequency_ppm() -> f64 {
    if FALLBACK_ACTIVE.load(Ordering::Relaxed) {
        return 0.0;
    }

    let active = ACTIVE_CLOCK.load(Ordering::Acquire);
    match active {
        1 => PTP_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.get_frequency_ppm)() })
            .unwrap_or(0.0),
        2 => LW_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.get_frequency_ppm)() })
            .unwrap_or(0.0),
        3 => SYS_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.get_frequency_ppm)() })
            .unwrap_or(0.0),
        _ => 0.0,
    }
}

/// Get current offset in nanoseconds from the active clock.
pub fn clock_get_offset() -> i64 {
    if FALLBACK_ACTIVE.load(Ordering::Relaxed) {
        return 0;
    }

    let active = ACTIVE_CLOCK.load(Ordering::Acquire);
    match active {
        1 => PTP_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.get_offset)() })
            .unwrap_or(0),
        2 => LW_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.get_offset)() })
            .unwrap_or(0),
        3 => SYS_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.get_offset)() })
            .unwrap_or(0),
        _ => 0,
    }
}

/// Check if the active clock is locked.
pub fn clock_is_locked() -> bool {
    let active = ACTIVE_CLOCK.load(Ordering::Acquire);

    if active == 3 {
        return true;
    }

    let primary_locked = match active {
        1 => PTP_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.is_locked)() != 0 })
            .unwrap_or(false),
        2 => LW_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.is_locked)() != 0 })
            .unwrap_or(false),
        _ => return false,
    };

    if primary_locked {
        LAST_LOCK_TIME_MS.store(elapsed_ms(), Ordering::Relaxed);
        if FALLBACK_ACTIVE.load(Ordering::Relaxed) {
            FALLBACK_ACTIVE.store(false, Ordering::Relaxed);
        }
        return true;
    }

    let timeout_secs = FALLBACK_TIMEOUT_SECS.load(Ordering::Relaxed);
    if timeout_secs == 0 {
        return false;
    }

    if SYS_LIB.get().and_then(|l| l.as_ref()).is_none() {
        return false;
    }

    let last_lock = LAST_LOCK_TIME_MS.load(Ordering::Relaxed);
    let now = elapsed_ms();
    let elapsed_secs = (now.saturating_sub(last_lock)) / 1000;

    if elapsed_secs >= timeout_secs as u64 {
        FALLBACK_ACTIVE.store(true, Ordering::Relaxed);
        return true;
    }

    false
}

/// Get formatted stats string from the active clock.
pub fn clock_get_stats_string() -> String {
    let active = ACTIVE_CLOCK.load(Ordering::Acquire);

    if active == 0 {
        return String::from("Clock: Not started");
    }

    if FALLBACK_ACTIVE.load(Ordering::Relaxed) {
        let last_lock = LAST_LOCK_TIME_MS.load(Ordering::Relaxed);
        let now = elapsed_ms();
        let elapsed_secs = (now.saturating_sub(last_lock)) / 1000;
        let primary = match active {
            1 => "PTP",
            2 => "Livewire",
            _ => "Network",
        };
        return format!(
            "FALLBACK: System Clock (free-running) - {} lost lock {}s ago",
            primary, elapsed_secs
        );
    }

    let mut buffer = vec![0i8; 256];

    let len = match active {
        1 => {
            if let Some(Some(lib)) = PTP_LIB.get() {
                unsafe { (lib.functions.get_stats_string)(buffer.as_mut_ptr(), buffer.len() as i32) }
            } else {
                return String::from("PTP: Not available");
            }
        }
        2 => {
            if let Some(Some(lib)) = LW_LIB.get() {
                unsafe { (lib.functions.get_stats_string)(buffer.as_mut_ptr(), buffer.len() as i32) }
            } else {
                return String::from("Livewire: Not available");
            }
        }
        3 => {
            if let Some(Some(lib)) = SYS_LIB.get() {
                unsafe { (lib.functions.get_stats_string)(buffer.as_mut_ptr(), buffer.len() as i32) }
            } else {
                return String::from("System: Not available");
            }
        }
        _ => return String::from("Clock: Not started"),
    };

    if len > 0 {
        let bytes: Vec<u8> = buffer[..len as usize].iter().map(|&b| b as u8).collect();
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        String::from("Clock: Error getting stats")
    }
}

/// Get state from the active clock.
pub fn clock_get_state() -> ClockState {
    if FALLBACK_ACTIVE.load(Ordering::Relaxed) {
        return ClockState::Slave;
    }

    let active = ACTIVE_CLOCK.load(Ordering::Acquire);
    match active {
        1 => PTP_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| ClockState::from(unsafe { (lib.functions.get_state)() }))
            .unwrap_or(ClockState::Disabled),
        2 => LW_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| ClockState::from(unsafe { (lib.functions.get_state)() }))
            .unwrap_or(ClockState::Disabled),
        3 => SYS_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| ClockState::from(unsafe { (lib.functions.get_state)() }))
            .unwrap_or(ClockState::Disabled),
        _ => ClockState::Disabled,
    }
}

/// Get library version from the active clock.
pub fn clock_get_version() -> u32 {
    let active = ACTIVE_CLOCK.load(Ordering::Acquire);
    match active {
        1 => PTP_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.get_version)() })
            .unwrap_or(0),
        2 => LW_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.get_version)() })
            .unwrap_or(0),
        3 => SYS_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.get_version)() })
            .unwrap_or(0),
        _ => 0,
    }
}

// ============================================================================
// Timer API
// ============================================================================

/// Start the precision timer using the active clock's timer.
pub fn clock_timer_start(
    interval_ms: u32,
    callback: Option<ClockTimerCallback>,
    user: *mut c_void,
) -> Result<(), i32> {
    let active = ACTIVE_CLOCK.load(Ordering::Acquire);

    let result = match active {
        1 => {
            let lib = match PTP_LIB.get().and_then(|l| l.as_ref()) {
                Some(l) => l,
                None => return Err(CLOCK_ERROR_NOT_INIT),
            };
            unsafe { (lib.functions.timer_start)(interval_ms, callback, user) }
        }
        2 => {
            let lib = match LW_LIB.get().and_then(|l| l.as_ref()) {
                Some(l) => l,
                None => return Err(CLOCK_ERROR_NOT_INIT),
            };
            unsafe { (lib.functions.timer_start)(interval_ms, callback, user) }
        }
        3 => {
            let lib = match SYS_LIB.get().and_then(|l| l.as_ref()) {
                Some(l) => l,
                None => return Err(CLOCK_ERROR_NOT_INIT),
            };
            unsafe { (lib.functions.timer_start)(interval_ms, callback, user) }
        }
        _ => return Err(CLOCK_ERROR_NOT_INIT),
    };

    if result == CLOCK_OK {
        Ok(())
    } else {
        Err(result)
    }
}

/// Stop the timer.
pub fn clock_timer_stop() {
    let active = ACTIVE_CLOCK.load(Ordering::Acquire);
    match active {
        1 => {
            if let Some(Some(lib)) = PTP_LIB.get() {
                unsafe { (lib.functions.timer_stop)(); }
            }
        }
        2 => {
            if let Some(Some(lib)) = LW_LIB.get() {
                unsafe { (lib.functions.timer_stop)(); }
            }
        }
        3 => {
            if let Some(Some(lib)) = SYS_LIB.get() {
                unsafe { (lib.functions.timer_stop)(); }
            }
        }
        _ => {}
    }
}

/// Check if timer is running.
pub fn clock_timer_is_running() -> bool {
    let active = ACTIVE_CLOCK.load(Ordering::Acquire);
    match active {
        1 => PTP_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.timer_is_running)() != 0 })
            .unwrap_or(false),
        2 => LW_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.timer_is_running)() != 0 })
            .unwrap_or(false),
        3 => SYS_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.timer_is_running)() != 0 })
            .unwrap_or(false),
        _ => false,
    }
}

/// Set timer interval.
pub fn clock_timer_set_interval(interval_ms: u32) -> Result<(), i32> {
    let active = ACTIVE_CLOCK.load(Ordering::Acquire);

    let result = match active {
        1 => {
            let lib = match PTP_LIB.get().and_then(|l| l.as_ref()) {
                Some(l) => l,
                None => return Err(CLOCK_ERROR_NOT_INIT),
            };
            unsafe { (lib.functions.timer_set_interval)(interval_ms) }
        }
        2 => {
            let lib = match LW_LIB.get().and_then(|l| l.as_ref()) {
                Some(l) => l,
                None => return Err(CLOCK_ERROR_NOT_INIT),
            };
            unsafe { (lib.functions.timer_set_interval)(interval_ms) }
        }
        3 => {
            let lib = match SYS_LIB.get().and_then(|l| l.as_ref()) {
                Some(l) => l,
                None => return Err(CLOCK_ERROR_NOT_INIT),
            };
            unsafe { (lib.functions.timer_set_interval)(interval_ms) }
        }
        _ => return Err(CLOCK_ERROR_NOT_INIT),
    };

    if result == CLOCK_OK {
        Ok(())
    } else {
        Err(result)
    }
}

/// Get current timer interval.
pub fn clock_timer_get_interval() -> u32 {
    let active = ACTIVE_CLOCK.load(Ordering::Acquire);
    match active {
        1 => PTP_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.timer_get_interval)() })
            .unwrap_or(20),
        2 => LW_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.timer_get_interval)() })
            .unwrap_or(20),
        3 => SYS_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.timer_get_interval)() })
            .unwrap_or(20),
        _ => 20,
    }
}

/// Enable/disable PLL adjustment.
pub fn clock_timer_set_pll(enabled: bool) {
    let active = ACTIVE_CLOCK.load(Ordering::Acquire);
    match active {
        1 => {
            if let Some(Some(lib)) = PTP_LIB.get() {
                unsafe { (lib.functions.timer_set_pll)(if enabled { 1 } else { 0 }); }
            }
        }
        2 => {
            if let Some(Some(lib)) = LW_LIB.get() {
                unsafe { (lib.functions.timer_set_pll)(if enabled { 1 } else { 0 }); }
            }
        }
        3 => {
            if let Some(Some(lib)) = SYS_LIB.get() {
                unsafe { (lib.functions.timer_set_pll)(if enabled { 1 } else { 0 }); }
            }
        }
        _ => {}
    }
}

/// Check if PLL adjustment is enabled.
pub fn clock_timer_is_pll_enabled() -> bool {
    let active = ACTIVE_CLOCK.load(Ordering::Acquire);
    match active {
        1 => PTP_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.timer_is_pll_enabled)() != 0 })
            .unwrap_or(true),
        2 => LW_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.timer_is_pll_enabled)() != 0 })
            .unwrap_or(true),
        3 => SYS_LIB
            .get()
            .and_then(|l| l.as_ref())
            .map(|lib| unsafe { (lib.functions.timer_is_pll_enabled)() != 0 })
            .unwrap_or(false),
        _ => true,
    }
}
