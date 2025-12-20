//! AES67 Low Latency Example - 1ms packets for live microphone/headphone use.
//!
//! Usage: cargo run --example aes67_low_latency
//!
//! This example is configured for minimum latency scenarios:
//!   - Live microphone to headphone monitoring
//!   - Real-time audio processing with tight timing requirements
//!
//! Packet rates and latency:
//!   - AES67 1ms:       1000 pkt/sec, ~5-10ms total latency
//!   - Livewire Live:   4000 pkt/sec, ~2-5ms total latency (not yet supported)
//!   - Livewire Standard: 200 pkt/sec, ~20-50ms latency (use aes67_loopback.rs)
//!
//! Test with AES67-compatible devices configured for 1ms packet time.

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

// AES67 config options
const BASS_CONFIG_AES67_INTERFACE: DWORD = 0x20001;
const BASS_CONFIG_AES67_JITTER: DWORD = 0x20002;
const BASS_CONFIG_AES67_PTP_DOMAIN: DWORD = 0x20003;
const BASS_CONFIG_AES67_JITTER_UNDERRUNS: DWORD = 0x20011;
const BASS_CONFIG_AES67_PACKETS_RECEIVED: DWORD = 0x20012;
const BASS_CONFIG_AES67_PACKETS_LATE: DWORD = 0x20013;
const BASS_CONFIG_AES67_BUFFER_PACKETS: DWORD = 0x20014;
const BASS_CONFIG_AES67_TARGET_PACKETS: DWORD = 0x20015;
const BASS_CONFIG_AES67_PACKET_TIME: DWORD = 0x20016;

// Channel states
const BASS_ACTIVE_STOPPED: DWORD = 0;

// Global running flag for clean shutdown
static RUNNING: AtomicBool = AtomicBool::new(true);

fn main() {
    println!("AES67 Low Latency Example");
    println!("=========================\n");
    println!("Configured for 1ms packets (1000 pkt/sec) - AES67 standard low latency.");
    println!("Total latency target: ~5-10ms end-to-end.\n");

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
        println!("\nInitializing BASS (no soundcard mode)...");
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

        // Configure AES67 - use same settings as working aes67_loopback.rs
        let interface = CString::new("192.168.60.102").unwrap();
        BASS_SetConfigPtr(BASS_CONFIG_AES67_INTERFACE, interface.as_ptr() as *const c_void);
        // 500ms jitter buffer - same as working loopback
        BASS_SetConfig(BASS_CONFIG_AES67_JITTER, 500);
        // PTP domain 1 for Livewire (domain 0 for standard AES67)
        BASS_SetConfig(BASS_CONFIG_AES67_PTP_DOMAIN, 1);
        println!("  AES67 configured (interface=192.168.60.102, jitter=500ms, domain=1)");

        // Start PTP client
        let ptp_interface = CString::new("192.168.60.102").unwrap();
        let ptp_result = (ptp.start)(ptp_interface.as_ptr(), 1);  // Domain 1 for Livewire
        if ptp_result != 0 {
            println!("WARNING: PTP start returned {}", ptp_result);
        }
        println!("  PTP client started (domain=1)");

        // Wait for PTP to lock - CRITICAL for low latency (need accurate timing)
        println!("\nWaiting for PTP lock (required for low latency)...");
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
            println!("\n  WARNING: PTP not locked - latency may be unstable!");
        }

        // Create AES67 INPUT stream (decode mode)
        println!("\nCreating AES67 input stream...");
        // For low latency testing, use a 1ms source if available
        // Fall back to standard Livewire source for testing
        let input_url = CString::new("aes67://239.192.76.52:5004").unwrap();
        let input_stream = BASS_StreamCreateURL(
            input_url.as_ptr(),
            0,
            BASS_STREAM_DECODE,
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
        println!("  Input stream created (source: 239.192.76.52:5004)");

        // Wait for first packet to detect packet time
        println!("\nDetecting input packet time...");
        let mut detect_wait = 0;
        let detected_packet_time_us;
        loop {
            let packet_time = BASS_GetConfig(BASS_CONFIG_AES67_PACKET_TIME);
            if packet_time > 0 {
                detected_packet_time_us = packet_time;
                break;
            }
            thread::sleep(Duration::from_millis(50));
            detect_wait += 1;
            if detect_wait > 100 {  // 5 second timeout
                println!("ERROR: No packets received, cannot detect packet time");
                BASS_StreamFree(input_stream);
                (ptp.stop)();
                BASS_PluginFree(plugin);
                BASS_Free();
                return;
            }
        }

        let packets_per_sec = 1_000_000 / detected_packet_time_us;
        println!("  Detected: {}µs packets ({} pkt/sec)", detected_packet_time_us, packets_per_sec);

        // Create AES67 OUTPUT stream matching input packet time
        // Note: Both input and output use PTP timing for sync
        println!("\nCreating AES67 output stream (matching input)...");
        println!("  DEBUG: packet_time_us = {}", detected_packet_time_us);
        let output_config = bass_aes67::Aes67OutputConfig {
            multicast_addr: Ipv4Addr::new(239, 192, 1, 100),
            port: 5004,
            interface: Some(Ipv4Addr::new(192, 168, 60, 102)),
            payload_type: 96,
            channels: 2,
            sample_rate: 48000,
            packet_time_us: detected_packet_time_us,  // Match input packet time
        };
        println!("  DEBUG: config.packet_time_us = {}", output_config.packet_time_us);

        let mut output_stream = match bass_aes67::Aes67OutputStream::new(input_stream, output_config) {
            Ok(s) => {
                println!("  Output stream created (dest: 239.192.1.100:5004, {} pkt/sec)", packets_per_sec);
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
        // Fill to 50% - same as working loopback
        println!("\nWaiting for input buffer to fill...");
        loop {
            let buffer_packets = BASS_GetConfig(BASS_CONFIG_AES67_BUFFER_PACKETS);
            let target_packets = BASS_GetConfig(BASS_CONFIG_AES67_TARGET_PACKETS);
            if buffer_packets >= target_packets / 2 {
                println!("  Buffer ready ({}/{})", buffer_packets, target_packets);
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
        println!("  INPUT:  239.192.76.52:5004");
        println!("  OUTPUT: 239.192.1.100:5004 ({} pkt/sec)", packets_per_sec);
        println!("  JITTER: 10ms buffer");
        println!("==========================================");
        println!("Press Ctrl+C to stop\n");

        // Buffer level history for trend calculation
        let mut level_history: VecDeque<u32> = VecDeque::with_capacity(10);

        // Monitor loop - faster updates for low latency monitoring
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

            // Get jitter buffer stats
            let buffer_packets = BASS_GetConfig(BASS_CONFIG_AES67_BUFFER_PACKETS);
            let target_packets = BASS_GetConfig(BASS_CONFIG_AES67_TARGET_PACKETS);
            let jitter_underruns = BASS_GetConfig(BASS_CONFIG_AES67_JITTER_UNDERRUNS);
            let packets_received = BASS_GetConfig(BASS_CONFIG_AES67_PACKETS_RECEIVED);
            let packets_late = BASS_GetConfig(BASS_CONFIG_AES67_PACKETS_LATE);

            // Track buffer level for trend
            level_history.push_back(buffer_packets);
            if level_history.len() > 10 {
                level_history.pop_front();
            }

            // Calculate trend
            let trend = if level_history.len() >= 3 {
                let first = *level_history.front().unwrap() as i32;
                let last = *level_history.back().unwrap() as i32;
                let diff = last - first;
                if diff > 2 {
                    "GROW"
                } else if diff < -2 {
                    "SHRINK"
                } else {
                    "STABLE"
                }
            } else {
                "---"
            };

            // Get output stats
            let output_stats = output_stream.stats();

            // Calculate approximate latency (buffer level in ms + 1 packet)
            // For 1ms packets: latency ≈ buffer_packets + 1 ms
            let approx_latency_ms = buffer_packets + 1;

            // Display status
            print!("\r\x1b[K");
            print!("buf={}/{} lat~{}ms | rcv={} late={} und={} | OUT: {} | {} | {}",
                buffer_packets,
                target_packets,
                approx_latency_ms,
                packets_received,
                packets_late,
                jitter_underruns,
                output_stats.packets_sent,
                ptp_stats,
                trend);
            use std::io::Write;
            std::io::stdout().flush().unwrap();

            // Faster update rate for low latency monitoring
            thread::sleep(Duration::from_millis(200));
        }

        // Cleanup
        println!("\n\nCleaning up...");
        output_stream.stop();
        // Give output thread time to exit
        thread::sleep(Duration::from_millis(200));
        BASS_StreamFree(input_stream);
        // Give input receiver thread time to exit
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
            1
        }

        unsafe {
            SetConsoleCtrlHandler(Some(handler), 1);
        }
    }
}
