//! No-soundcard test for BASS AES67 plugin.
//!
//! Usage: cargo run --example no_soundcard
//!
//! This example demonstrates using BASS without a soundcard (device 0).
//! Audio is driven by a PTP-synchronized timer from bass_ptp.dll.
//! The audio data can be sent to an Icecast server or other output.

use std::ffi::{c_char, c_void, CString};
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

// BASS types
type DWORD = u32;
type BOOL = i32;
type HSTREAM = DWORD;
type HPLUGIN = DWORD;

const FALSE: BOOL = 0;

// BASS functions
#[link(name = "bass")]
extern "system" {
    fn BASS_Init(device: i32, freq: DWORD, flags: DWORD, win: *mut c_void, dsguid: *const c_void) -> BOOL;
    fn BASS_Free() -> BOOL;
    fn BASS_GetVersion() -> DWORD;
    fn BASS_ErrorGetCode() -> i32;
    fn BASS_PluginLoad(file: *const i8, flags: DWORD) -> HPLUGIN;
    fn BASS_StreamCreateURL(url: *const i8, offset: DWORD, flags: DWORD, proc: *const c_void, user: *mut c_void) -> HSTREAM;
    fn BASS_ChannelGetData(handle: DWORD, buffer: *mut c_void, length: DWORD) -> DWORD;
    fn BASS_ChannelStop(handle: DWORD) -> BOOL;
    fn BASS_ChannelIsActive(handle: DWORD) -> DWORD;
    fn BASS_StreamFree(handle: HSTREAM) -> BOOL;
    fn BASS_PluginFree(handle: HPLUGIN) -> BOOL;
    fn BASS_SetConfig(option: DWORD, value: DWORD) -> BOOL;
    fn BASS_SetConfigPtr(option: DWORD, value: *const c_void) -> BOOL;
}

// bass_ptp function types (loaded dynamically)
type PtpStartFn = unsafe extern "C" fn(*const c_char, u8) -> i32;
type PtpStopFn = unsafe extern "C" fn() -> i32;
type PtpGetStatsStringFn = unsafe extern "C" fn(*mut c_char, i32) -> i32;
type PtpIsLockedFn = unsafe extern "C" fn() -> i32;
type PtpTimerStartFn = unsafe extern "C" fn(u32, Option<TimerCallback>, *mut c_void) -> i32;
type PtpTimerStopFn = unsafe extern "C" fn() -> i32;
type PtpTimerSetPllFn = unsafe extern "C" fn(i32) -> i32;

type TimerCallback = unsafe extern "C" fn(*mut c_void);

// Windows API for dynamic loading
#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(lpLibFileName: *const u16) -> *mut c_void;
    fn GetProcAddress(hModule: *mut c_void, lpProcName: *const i8) -> *mut c_void;
}

/// Convert Rust string to wide string
#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// bass_ptp function pointers
struct PtpFunctions {
    start: PtpStartFn,
    stop: PtpStopFn,
    get_stats_string: PtpGetStatsStringFn,
    is_locked: PtpIsLockedFn,
    timer_start: PtpTimerStartFn,
    timer_stop: PtpTimerStopFn,
    timer_set_pll: PtpTimerSetPllFn,
}

/// Load bass_ptp.dll dynamically
#[cfg(windows)]
unsafe fn load_ptp_dll() -> Option<PtpFunctions> {
    let handle = LoadLibraryW(to_wide("bass_ptp.dll").as_ptr());
    if handle.is_null() {
        return None;
    }

    macro_rules! load_fn {
        ($name:expr, $ty:ty) => {{
            let ptr = GetProcAddress(handle, concat!($name, "\0").as_ptr() as *const i8);
            if ptr.is_null() {
                return None;
            }
            std::mem::transmute::<*mut c_void, $ty>(ptr)
        }};
    }

    Some(PtpFunctions {
        start: load_fn!("BASS_PTP_Start", PtpStartFn),
        stop: load_fn!("BASS_PTP_Stop", PtpStopFn),
        get_stats_string: load_fn!("BASS_PTP_GetStatsString", PtpGetStatsStringFn),
        is_locked: load_fn!("BASS_PTP_IsLocked", PtpIsLockedFn),
        timer_start: load_fn!("BASS_PTP_TimerStart", PtpTimerStartFn),
        timer_stop: load_fn!("BASS_PTP_TimerStop", PtpTimerStopFn),
        timer_set_pll: load_fn!("BASS_PTP_TimerSetPLL", PtpTimerSetPllFn),
    })
}

#[cfg(not(windows))]
unsafe fn load_ptp_dll() -> Option<PtpFunctions> {
    None
}

// Config constants
const BASS_CONFIG_BUFFER: DWORD = 0;
const BASS_CONFIG_UPDATEPERIOD: DWORD = 6;
const BASS_STREAM_DECODE: DWORD = 0x200000;

// AES67 config options
const BASS_CONFIG_AES67_INTERFACE: DWORD = 0x20001;
const BASS_CONFIG_AES67_JITTER: DWORD = 0x20002;
const BASS_CONFIG_AES67_PTP_DOMAIN: DWORD = 0x20003;

// Channel states
const BASS_ACTIVE_STOPPED: DWORD = 0;
const BASS_ACTIVE_PLAYING: DWORD = 1;
const BASS_ACTIVE_STALLED: DWORD = 2;

// Global counters for statistics
static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);
static BYTES_PULLED: AtomicU64 = AtomicU64::new(0);

// Timer callback - called every 20ms by bass_ptp timer
unsafe extern "C" fn timer_callback(user: *mut c_void) {
    TIMER_TICKS.fetch_add(1, Ordering::Relaxed);

    // user points to stream handle
    let stream = *(user as *const DWORD);
    if stream == 0 {
        return;
    }

    // Pull audio data from the stream
    // 20ms at 48kHz stereo float = 48000 * 0.020 * 2 * 4 = 7680 bytes
    let mut buffer = [0u8; 8192];
    let bytes = BASS_ChannelGetData(stream, buffer.as_mut_ptr() as *mut c_void, buffer.len() as DWORD);

    if bytes != 0xFFFFFFFF {
        BYTES_PULLED.fetch_add(bytes as u64, Ordering::Relaxed);

        // Here you would send the audio data to:
        // - Icecast encoder
        // - AES67 sender (future)
        // - File writer
        // - etc.
    }
}

fn main() {
    println!("BASS AES67 No-Soundcard Test");
    println!("============================\n");

    unsafe {
        // Load bass_ptp.dll dynamically
        println!("Loading bass_ptp.dll...");
        let ptp = match load_ptp_dll() {
            Some(p) => {
                println!("bass_ptp.dll loaded successfully");
                p
            }
            None => {
                println!("ERROR: Failed to load bass_ptp.dll");
                println!("Make sure bass_ptp.dll is in the current directory");
                return;
            }
        };

        // Get BASS version
        let version = BASS_GetVersion();
        println!("BASS version: {}.{}.{}.{}",
            (version >> 24) & 0xFF,
            (version >> 16) & 0xFF,
            (version >> 8) & 0xFF,
            version & 0xFF);

        // Configure BASS for no-soundcard operation
        println!("\nConfiguring BASS for no-soundcard mode...");
        BASS_SetConfig(BASS_CONFIG_BUFFER, 20);      // 20ms buffer
        BASS_SetConfig(BASS_CONFIG_UPDATEPERIOD, 0); // Disable auto-update
        println!("  BASS_CONFIG_BUFFER: 20ms");
        println!("  BASS_CONFIG_UPDATEPERIOD: 0 (disabled)");

        // Initialize BASS with no device (0 = no sound)
        println!("\nInitializing BASS with device=0 (no soundcard)...");
        if BASS_Init(0, 48000, 0, ptr::null_mut(), ptr::null()) == FALSE {
            println!("ERROR: Failed to initialize BASS (error code: {})", BASS_ErrorGetCode());
            return;
        }
        println!("BASS initialized successfully (no soundcard)");

        // Load the AES67 plugin
        println!("\nLoading AES67 plugin...");
        let plugin_path = CString::new("bass_aes67.dll").unwrap();
        let plugin = BASS_PluginLoad(plugin_path.as_ptr(), 0);
        if plugin == 0 {
            println!("ERROR: Failed to load plugin (error code: {})", BASS_ErrorGetCode());
            println!("Make sure bass_aes67.dll is in the current directory");
            BASS_Free();
            return;
        }
        println!("Plugin loaded (handle: {})", plugin);

        // Configure AES67 settings
        println!("\nConfiguring AES67...");

        // Set network interface
        let interface = CString::new("192.168.60.102").unwrap();
        BASS_SetConfigPtr(BASS_CONFIG_AES67_INTERFACE, interface.as_ptr() as *const c_void);
        println!("  Interface: 192.168.60.102");

        // Set jitter buffer
        BASS_SetConfig(BASS_CONFIG_AES67_JITTER, 20);
        println!("  Jitter buffer: 20ms");

        // Set PTP domain
        BASS_SetConfig(BASS_CONFIG_AES67_PTP_DOMAIN, 10);
        println!("  PTP domain: 10");

        // Start PTP client directly
        println!("\nStarting PTP client...");
        let ptp_interface = CString::new("192.168.60.102").unwrap();
        let ptp_result = (ptp.start)(ptp_interface.as_ptr(), 10);
        if ptp_result != 0 {
            println!("WARNING: PTP start returned {}", ptp_result);
        } else {
            println!("PTP client started");
        }

        // Create decode stream from AES67 URL
        println!("\nCreating AES67 decode stream...");
        let url = CString::new("aes67://239.192.76.52:5004").unwrap();
        let stream = BASS_StreamCreateURL(url.as_ptr(), 0, BASS_STREAM_DECODE, ptr::null(), ptr::null_mut());

        if stream == 0 {
            println!("ERROR: Failed to create stream (error code: {})", BASS_ErrorGetCode());
            (ptp.stop)();
            BASS_PluginFree(plugin);
            BASS_Free();
            return;
        }
        println!("Decode stream created (handle: {})", stream);

        // Store stream handle for timer callback
        let mut stream_handle = stream;

        // Start the PTP-synchronized timer
        println!("\nStarting PTP timer (20ms interval)...");
        (ptp.timer_set_pll)(1); // Enable PLL adjustment
        let timer_result = (ptp.timer_start)(
            20, // 20ms interval
            Some(timer_callback),
            &mut stream_handle as *mut DWORD as *mut c_void,
        );

        if timer_result != 0 {
            println!("ERROR: Failed to start timer (error {})", timer_result);
            BASS_StreamFree(stream);
            (ptp.stop)();
            BASS_PluginFree(plugin);
            BASS_Free();
            return;
        }
        println!("Timer started with PLL enabled");

        println!("\nStreaming... Press Ctrl+C to stop\n");

        // Monitor loop
        let mut last_ticks: u64 = 0;
        let mut last_bytes: u64 = 0;

        loop {
            let state = BASS_ChannelIsActive(stream);
            let state_str = match state {
                BASS_ACTIVE_STOPPED => "Stopped",
                BASS_ACTIVE_PLAYING => "Playing",
                BASS_ACTIVE_STALLED => "Stalled",
                _ => "Active",
            };

            // Get PTP stats
            let mut ptp_buffer = [0i8; 256];
            (ptp.get_stats_string)(ptp_buffer.as_mut_ptr(), 256);
            let ptp_stats = std::ffi::CStr::from_ptr(ptp_buffer.as_ptr())
                .to_string_lossy()
                .into_owned();

            let ptp_locked = (ptp.is_locked)() != 0;

            // Calculate rates
            let ticks = TIMER_TICKS.load(Ordering::Relaxed);
            let bytes = BYTES_PULLED.load(Ordering::Relaxed);
            let tick_delta = ticks - last_ticks;
            let byte_delta = bytes - last_bytes;
            last_ticks = ticks;
            last_bytes = bytes;

            // Calculate actual rate (bytes per second)
            let bytes_per_sec = byte_delta * 2; // 500ms update interval

            // Clear line and print status
            print!("\r\x1b[K");
            print!("Stream: {:8} | Ticks: {:6} | {:.1} KB/s | PTP: {} | {}",
                state_str,
                tick_delta,
                bytes_per_sec as f64 / 1024.0,
                if ptp_locked { "LOCKED" } else { "unlocked" },
                ptp_stats);
            use std::io::Write;
            std::io::stdout().flush().unwrap();

            if state == BASS_ACTIVE_STOPPED {
                println!("\n\nStream ended");
                break;
            }

            thread::sleep(Duration::from_millis(500));
        }

        // Cleanup
        println!("\nCleaning up...");
        (ptp.timer_stop)();
        BASS_ChannelStop(stream);
        BASS_StreamFree(stream);
        (ptp.stop)();
        BASS_PluginFree(plugin);
        BASS_Free();
        println!("Done!");

        // Print final stats
        let total_ticks = TIMER_TICKS.load(Ordering::Relaxed);
        let total_bytes = BYTES_PULLED.load(Ordering::Relaxed);
        println!("\nStatistics:");
        println!("  Total timer ticks: {}", total_ticks);
        println!("  Total bytes pulled: {} ({:.2} MB)", total_bytes, total_bytes as f64 / (1024.0 * 1024.0));
    }
}
