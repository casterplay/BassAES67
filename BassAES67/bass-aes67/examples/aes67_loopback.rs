//! AES67 loopback example - receives AES67, routes through BASS, transmits as AES67.
//!
//! Usage:
//!   cargo run --example aes67_loopback           # Use PTP clock (default)
//!   cargo run --example aes67_loopback -- ptp    # Use PTP clock
//!   cargo run --example aes67_loopback -- lw     # Use Livewire clock
//!   cargo run --example aes67_loopback -- sys    # Use System clock (free-running)
//!
//! This demonstrates the production use case:
//!   AES67 INPUT -> BASS (decode/mixer/effects) -> AES67 OUTPUT
//!
//! When both input and output use network timing (PTP or Livewire), there is NO clock drift
//! to compensate for - they share the same reference clock.
//!
//! System clock mode is useful for testing or when no network clock is available.
//! It runs at nominal rate with no synchronization.
//!
//! Test with Livewire/xNode:
//! - Input: Livewire stream on 239.192.76.49:5004
//! - Output: New multicast 239.192.1.100:5004 (configure xNode to receive)

use std::collections::VecDeque;
use std::env;
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
    fn BASS_GetConfigPtr(option: DWORD) -> *const c_void;
}

/// Clock mode selection
#[derive(Clone, Copy, PartialEq)]
enum ClockMode {
    Ptp,
    Livewire,
    System,
}

impl ClockMode {
    fn name(&self) -> &'static str {
        match self {
            ClockMode::Ptp => "PTP",
            ClockMode::Livewire => "Livewire",
            ClockMode::System => "System",
        }
    }

    fn config_value(&self) -> DWORD {
        match self {
            ClockMode::Ptp => 0,
            ClockMode::Livewire => 1,
            ClockMode::System => 2,
        }
    }
}

// Config constants
const BASS_STREAM_DECODE: DWORD = 0x200000;
const BASS_CONFIG_BUFFER: DWORD = 0;
const BASS_CONFIG_UPDATEPERIOD: DWORD = 6;

// AES67 config options
const BASS_CONFIG_AES67_INTERFACE: DWORD = 0x20001;
const BASS_CONFIG_AES67_JITTER: DWORD = 0x20002;
const BASS_CONFIG_AES67_PTP_DOMAIN: DWORD = 0x20003;
const BASS_CONFIG_AES67_PTP_STATS: DWORD = 0x20004;
const BASS_CONFIG_AES67_JITTER_UNDERRUNS: DWORD = 0x20011;
const BASS_CONFIG_AES67_PACKETS_RECEIVED: DWORD = 0x20012;
const BASS_CONFIG_AES67_PACKETS_LATE: DWORD = 0x20013;
const BASS_CONFIG_AES67_BUFFER_PACKETS: DWORD = 0x20014;
const BASS_CONFIG_AES67_TARGET_PACKETS: DWORD = 0x20015;
const BASS_CONFIG_AES67_PTP_LOCKED: DWORD = 0x20017;
const BASS_CONFIG_AES67_CLOCK_MODE: DWORD = 0x20019;

// Channel states
const BASS_ACTIVE_STOPPED: DWORD = 0;

// Global running flag for clean shutdown
static RUNNING: AtomicBool = AtomicBool::new(true);

fn main() {
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    let clock_mode = if args.len() > 1 {
        match args[1].to_lowercase().as_str() {
            "lw" | "livewire" => ClockMode::Livewire,
            "sys" | "system" => ClockMode::System,
            "ptp" | _ => ClockMode::Ptp,
        }
    } else {
        ClockMode::Ptp // Default to PTP
    };

    println!("BASS AES67 Loopback Example");
    println!("============================\n");
    println!("Clock Mode: {}", clock_mode.name());
    println!("\nThis example receives AES67 audio, routes through BASS,");
    println!("and transmits it on a different multicast group.\n");
    println!("Usage: aes67_loopback [ptp|lw|sys]");
    println!("  ptp - Use IEEE 1588v2 PTP (default)");
    println!("  lw  - Use Axia Livewire Clock");
    println!("  sys - Use System Clock (free-running, no sync)\n");

    // Install Ctrl+C handler
    ctrlc_handler();

    unsafe {
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

        // Configure AES67
        // Set clock mode BEFORE creating streams
        BASS_SetConfig(BASS_CONFIG_AES67_CLOCK_MODE, clock_mode.config_value());
        println!("  Clock mode set to: {} ({})", clock_mode.name(), clock_mode.config_value());

        // Interface for the AoIP network
        let interface = CString::new("192.168.60.102").unwrap();
        BASS_SetConfigPtr(BASS_CONFIG_AES67_INTERFACE, interface.as_ptr() as *const c_void);

        // Livewire uses 200 packets/sec (5ms)
        // 10ms jitter buffer - minimal latency
        BASS_SetConfig(BASS_CONFIG_AES67_JITTER, 10);

        // PTP domain 1 for Livewire (ignored in Livewire clock mode)
        BASS_SetConfig(BASS_CONFIG_AES67_PTP_DOMAIN, 1);

        println!("  AES67 configured (interface=192.168.60.102, jitter=10ms, domain=1)");

        // Create AES67 INPUT stream (decode mode - we'll pull audio from it)
        // Note: This will start the clock client automatically based on CLOCK_MODE
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
            BASS_PluginFree(plugin);
            BASS_Free();
            return;
        }
        println!("  Input stream created (handle: {}, source: 239.192.76.49:5004)", input_stream);
        println!("  {} clock started automatically by bass_aes67", clock_mode.name());

        // Wait for clock to lock (use BASS config API)
        println!("\nWaiting for {} lock...", clock_mode.name());
        let mut wait_count = 0;
        while BASS_GetConfig(BASS_CONFIG_AES67_PTP_LOCKED) == 0 && wait_count < 100 {
            thread::sleep(Duration::from_millis(100));
            wait_count += 1;
            if wait_count % 10 == 0 {
                print!(".");
                use std::io::Write;
                std::io::stdout().flush().unwrap();
            }
        }
        if BASS_GetConfig(BASS_CONFIG_AES67_PTP_LOCKED) != 0 {
            println!("\n  {} locked!", clock_mode.name());
        } else {
            println!("\n  {} not locked yet (continuing anyway)", clock_mode.name());
        }

        // Create AES67 OUTPUT stream
        // Note: Both input and output use network timing for sync
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
            BASS_PluginFree(plugin);
            BASS_Free();
            return;
        }
        println!("  Output stream started");

        println!("\n==========================================");
        println!("Loopback running ({} sync):", clock_mode.name());
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

            // Get clock stats from BASS config API
            let clock_stats = {
                let stats_ptr = BASS_GetConfigPtr(BASS_CONFIG_AES67_PTP_STATS) as *const c_char;
                if !stats_ptr.is_null() {
                    std::ffi::CStr::from_ptr(stats_ptr)
                        .to_string_lossy()
                        .into_owned()
                } else {
                    format!("{}: (stats unavailable)", clock_mode.name())
                }
            };

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

            // Display enhanced status
            print!("\r\x1b[K");
            print!("IN: {}/{} rcv={} late={} und={} | OUT: pkt={} und={} | {} | {}",
                buffer_packets,
                target_packets,
                packets_received,
                packets_late,
                jitter_underruns,
                output_stats.packets_sent,
                output_stats.underruns,
                clock_stats,
                trend);
            use std::io::Write;
            std::io::stdout().flush().unwrap();

            thread::sleep(Duration::from_millis(500));
        }

        // Cleanup
        println!("\n\nCleaning up...");
        println!("  Stopping output stream...");
        output_stream.stop();
        println!("  Output stream stopped");
        // Give threads time to exit cleanly
        thread::sleep(Duration::from_millis(200));
        println!("  Freeing input stream...");
        BASS_StreamFree(input_stream);
        println!("  Input stream freed");
        // Give input receiver thread time to exit (100ms socket timeout + margin)
        thread::sleep(Duration::from_millis(200));
        // Clock is stopped automatically when bass_aes67 plugin unloads
        println!("  Unloading plugin...");
        BASS_PluginFree(plugin);
        println!("  Plugin unloaded");
        println!("  Freeing BASS...");
        BASS_Free();
        println!("  BASS freed");

        // Final stats
        let final_stats = output_stream.stats();
        println!("\nFinal Statistics:");
        println!("  Clock mode: {}", clock_mode.name());
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
