//! AES67 loopback example - receives AES67, routes through BASS, transmits as AES67.
//!
//! Usage: cargo run --example aes67_loopback
//!
//! This demonstrates the production use case:
//!   AES67 INPUT → BASS (decode/mixer/effects) → AES67 OUTPUT
//!
//! When both input and output use PTP network timing, there is NO clock drift
//! to compensate for - they share the same reference clock.
//!
//! Test with Livewire/xNode:
//! - Input: Livewire stream on 239.192.76.52:5004
//! - Output: New multicast 239.192.76.53:5004 (configure xNode to receive)

use std::collections::VecDeque;
use std::ffi::{c_char, c_void, CString};
use std::net::Ipv4Addr;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
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
    fn BASS_ChannelIsActive(handle: DWORD) -> DWORD;
    fn BASS_StreamFree(handle: HSTREAM) -> BOOL;
    fn BASS_PluginFree(handle: HPLUGIN) -> BOOL;
    fn BASS_SetConfig(option: DWORD, value: DWORD) -> BOOL;
    fn BASS_SetConfigPtr(option: DWORD, value: *const c_void) -> BOOL;
    fn BASS_GetConfig(option: DWORD) -> DWORD;
}

// bass_ptp function types (loaded dynamically)
type PtpStartFn = unsafe extern "C" fn(*const c_char, u8) -> i32;
type PtpStopFn = unsafe extern "C" fn() -> i32;
type PtpGetStatsStringFn = unsafe extern "C" fn(*mut c_char, i32) -> i32;
type PtpIsLockedFn = unsafe extern "C" fn() -> i32;

// Windows API for dynamic loading
#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(lpLibFileName: *const u16) -> *mut c_void;
    fn GetProcAddress(hModule: *mut c_void, lpProcName: *const i8) -> *mut c_void;
}

#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct PtpFunctions {
    start: PtpStartFn,
    stop: PtpStopFn,
    get_stats_string: PtpGetStatsStringFn,
    is_locked: PtpIsLockedFn,
}

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
    })
}

#[cfg(not(windows))]
unsafe fn load_ptp_dll() -> Option<PtpFunctions> {
    None
}

// Config constants
const BASS_STREAM_DECODE: DWORD = 0x200000;
const BASS_CONFIG_BUFFER: DWORD = 0;
const BASS_CONFIG_UPDATEPERIOD: DWORD = 6;

// AES67 config options
const BASS_CONFIG_AES67_INTERFACE: DWORD = 0x20001;
const BASS_CONFIG_AES67_JITTER: DWORD = 0x20002;
const BASS_CONFIG_AES67_PTP_DOMAIN: DWORD = 0x20003;
const BASS_CONFIG_AES67_JITTER_UNDERRUNS: DWORD = 0x20011;
const BASS_CONFIG_AES67_PACKETS_RECEIVED: DWORD = 0x20012;
const BASS_CONFIG_AES67_PACKETS_LATE: DWORD = 0x20013;
const BASS_CONFIG_AES67_BUFFER_PACKETS: DWORD = 0x20014;
const BASS_CONFIG_AES67_TARGET_PACKETS: DWORD = 0x20015;

// Channel states
const BASS_ACTIVE_STOPPED: DWORD = 0;

// Global running flag for clean shutdown
static RUNNING: AtomicBool = AtomicBool::new(true);

fn main() {
    println!("BASS AES67 Loopback Example");
    println!("============================\n");
    println!("This example receives AES67 audio, routes through BASS,");
    println!("and transmits it on a different multicast group.\n");

    // Install Ctrl+C handler
    ctrlc_handler();

    unsafe {
        // Load bass_ptp.dll
        println!("Loading bass_ptp.dll...");
        let ptp = match load_ptp_dll() {
            Some(p) => {
                println!("  bass_ptp.dll loaded");
                p
            }
            None => {
                println!("ERROR: Failed to load bass_ptp.dll");
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

        // Initialize BASS in no-soundcard mode (device=0)
        // This is the production mode - no local audio device needed
        println!("\nInitializing BASS (no soundcard mode)...");

        // Configure BASS for manual data pulling (no automatic updates)
        BASS_SetConfig(BASS_CONFIG_BUFFER, 20);      // 20ms buffer
        BASS_SetConfig(BASS_CONFIG_UPDATEPERIOD, 0); // Disable auto-update

        if BASS_Init(0, 48000, 0, ptr::null_mut(), ptr::null()) == FALSE {
            println!("ERROR: Failed to initialize BASS (error {})", BASS_ErrorGetCode());
            return;
        }
        println!("  BASS initialized (device=0, no soundcard)");

        // Load AES67 plugin
        let plugin_path = CString::new("bass_aes67.dll").unwrap();
        let plugin = BASS_PluginLoad(plugin_path.as_ptr(), 0);
        if plugin == 0 {
            println!("ERROR: Failed to load bass_aes67.dll (error {})", BASS_ErrorGetCode());
            BASS_Free();
            return;
        }
        println!("  bass_aes67.dll loaded");

        // Configure AES67 for Livewire
        // Interface for the AoIP network
        let interface = CString::new("192.168.60.102").unwrap();
        BASS_SetConfigPtr(BASS_CONFIG_AES67_INTERFACE, interface.as_ptr() as *const c_void);
        // Livewire uses 200 packets/sec (5ms)
        // 10ms jitter buffer - minimal latency
        BASS_SetConfig(BASS_CONFIG_AES67_JITTER, 10);
        // Livewire uses PTP domain 1
        BASS_SetConfig(BASS_CONFIG_AES67_PTP_DOMAIN, 1);
        println!("  AES67 configured (interface=192.168.60.102, jitter=50ms, domain=1)");

        // Start PTP client
        let ptp_interface = CString::new("192.168.60.102").unwrap();
        let ptp_result = (ptp.start)(ptp_interface.as_ptr(), 1);  // Domain 1 for Livewire
        if ptp_result != 0 {
            println!("WARNING: PTP start returned {}", ptp_result);
        }
        println!("  PTP client started (domain=1)");

        // Wait for PTP to lock (optional but recommended for best sync)
        println!("\nWaiting for PTP lock...");
        let mut ptp_wait = 0;
        while (ptp.is_locked)() == 0 && ptp_wait < 100 {
            thread::sleep(Duration::from_millis(100));
            ptp_wait += 1;
            if ptp_wait % 10 == 0 {
                print!(".");
                use std::io::Write;
                std::io::stdout().flush().unwrap();
            }
        }
        if (ptp.is_locked)() != 0 {
            println!("\n  PTP locked!");
        } else {
            println!("\n  PTP not locked yet (continuing anyway)");
        }

        // Create AES67 INPUT stream (decode mode - we'll pull audio from it)
        println!("\nCreating AES67 input stream...");
        let input_url = CString::new("aes67://239.192.76.49:5004").unwrap();
        let input_stream = BASS_StreamCreateURL(
            input_url.as_ptr(),
            0,
            BASS_STREAM_DECODE,  // Decode mode - we pull samples manually
            ptr::null(),
            ptr::null_mut()
        );

        if input_stream == 0 {
            println!("ERROR: Failed to create input stream (error {})", BASS_ErrorGetCode());
            (ptp.stop)();
            BASS_PluginFree(plugin);
            BASS_Free();
            return;
        }
        println!("  Input stream created (handle: {}, source: 239.192.76.49:5004)", input_stream);

        // Create AES67 OUTPUT stream
        // Note: Both input and output use PTP timing for sync
        println!("\nCreating AES67 output stream...");
        let output_config = bass_aes67::Aes67OutputConfig {
            multicast_addr: Ipv4Addr::new(239, 192, 1, 100),  // Livewire destination
            port: 5004,
            interface: Some(Ipv4Addr::new(192, 168, 60, 102)),
            payload_type: 96,
            channels: 2,
            sample_rate: 48000,
            packet_time_us: 5000,  // 5ms = 200 packets/sec (Livewire standard)
        };

        let mut output_stream = match bass_aes67::Aes67OutputStream::new(input_stream, output_config) {
            Ok(s) => {
                println!("  Output stream created (dest: 239.192.1.100:5004, 5ms/200pkt/s)");
                s
            }
            Err(e) => {
                println!("ERROR: Failed to create output stream: {}", e);
                BASS_StreamFree(input_stream);
                (ptp.stop)();
                BASS_PluginFree(plugin);
                BASS_Free();
                return;
            }
        };

        // Wait for input buffer to fill before starting output
        // This prevents initial underruns while the jitter buffer fills
        println!("\nWaiting for input buffer to fill...");
        loop {
            let buffer_packets = BASS_GetConfig(BASS_CONFIG_AES67_BUFFER_PACKETS);
            let target_packets = BASS_GetConfig(BASS_CONFIG_AES67_TARGET_PACKETS);
            if buffer_packets >= target_packets / 2 {
                println!("  Input buffer ready ({}/{})", buffer_packets, target_packets);
                break;
            }
            print!("\r  Buffering: {}/{}", buffer_packets, target_packets);
            use std::io::Write;
            std::io::stdout().flush().unwrap();
            thread::sleep(Duration::from_millis(50));
        }

        // Start output transmission
        if let Err(e) = output_stream.start() {
            println!("ERROR: Failed to start output stream: {}", e);
            BASS_StreamFree(input_stream);
            (ptp.stop)();
            BASS_PluginFree(plugin);
            BASS_Free();
            return;
        }
        println!("  Output stream started");

        println!("\n==========================================");
        println!("Loopback running:");
        println!("  INPUT:  239.192.76.49:5004 (Livewire source)");
        println!("  OUTPUT: 239.192.1.100:5004 (200 pkt/sec)");
        println!("==========================================");
        println!("Press Ctrl+C to stop\n");

        // Buffer level history for trend calculation (last 10 readings = 5 seconds)
        let mut level_history: VecDeque<u32> = VecDeque::with_capacity(10);

        // Monitor loop
        while RUNNING.load(Ordering::SeqCst) {
            let state = BASS_ChannelIsActive(input_stream);
            if state == BASS_ACTIVE_STOPPED {
                println!("\nInput stream ended");
                break;
            }

            // Get PTP stats
            let mut ptp_buffer = [0i8; 256];
            (ptp.get_stats_string)(ptp_buffer.as_mut_ptr(), 256);
            let ptp_stats = std::ffi::CStr::from_ptr(ptp_buffer.as_ptr())
                .to_string_lossy()
                .into_owned();

            // Get detailed jitter buffer stats
            let buffer_packets = BASS_GetConfig(BASS_CONFIG_AES67_BUFFER_PACKETS);
            let target_packets = BASS_GetConfig(BASS_CONFIG_AES67_TARGET_PACKETS);
            let jitter_underruns = BASS_GetConfig(BASS_CONFIG_AES67_JITTER_UNDERRUNS);
            let packets_received = BASS_GetConfig(BASS_CONFIG_AES67_PACKETS_RECEIVED);
            let packets_late = BASS_GetConfig(BASS_CONFIG_AES67_PACKETS_LATE);

            // Track buffer level for trend calculation
            level_history.push_back(buffer_packets);
            if level_history.len() > 10 {
                level_history.pop_front();
            }

            // Calculate trend (compare first vs last in history)
            let trend = if level_history.len() >= 3 {
                let first = *level_history.front().unwrap() as i32;
                let last = *level_history.back().unwrap() as i32;
                let diff = last - first;
                if diff > 2 {
                    "GROW"  // Buffer growing
                } else if diff < -2 {
                    "SHRINK" // Buffer shrinking
                } else {
                    "STABLE" // Buffer stable
                }
            } else {
                "---" // Not enough data yet
            };

            // Get output stats
            let output_stats = output_stream.stats();

            // Display enhanced status on two lines for clarity
            print!("\r\x1b[K");
            print!("IN: {}/{} rcv={} late={} und={} | OUT: pkt={} und={} | {} | {}",
                buffer_packets,
                target_packets,
                packets_received,
                packets_late,
                jitter_underruns,
                output_stats.packets_sent,
                output_stats.underruns,
                ptp_stats,
                trend);
            use std::io::Write;
            std::io::stdout().flush().unwrap();

            thread::sleep(Duration::from_millis(500));
        }

        // Cleanup
        println!("\n\nCleaning up...");
        output_stream.stop();
        // Give threads time to exit cleanly
        thread::sleep(Duration::from_millis(200));
        BASS_StreamFree(input_stream);
        // Give input receiver thread time to exit (100ms socket timeout + margin)
        thread::sleep(Duration::from_millis(200));
        (ptp.stop)();
        BASS_PluginFree(plugin);
        BASS_Free();

        // Final stats
        let final_stats = output_stream.stats();
        println!("\nFinal Statistics:");
        println!("  Packets sent: {}", final_stats.packets_sent);
        println!("  Samples sent: {}", final_stats.samples_sent);
        println!("  Send errors: {}", final_stats.send_errors);
        println!("  Underruns: {}", final_stats.underruns);
    }

    println!("Done!");
}

/// Setup Ctrl+C handler for clean shutdown
fn ctrlc_handler() {
    #[cfg(windows)]
    {
        use std::os::raw::c_int;

        extern "system" {
            fn SetConsoleCtrlHandler(
                handler: Option<unsafe extern "system" fn(c_int) -> i32>,
                add: i32,
            ) -> i32;
        }

        unsafe extern "system" fn handler(_: c_int) -> i32 {
            RUNNING.store(false, Ordering::SeqCst);
            println!("\n\nShutting down...");
            1 // Return TRUE to indicate we handled it
        }

        unsafe {
            SetConsoleCtrlHandler(Some(handler), 1);
        }
    }
}
