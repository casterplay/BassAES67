//! AES67 Input Lab - Test AES67 input with soundcard output.
//!
//! Usage: cargo run --example aes67_input_lab
//!
//! This example uses BASS with the default soundcard (BASS_Init(-1)) to isolate
//! and test the AES67 network receiver. With a real soundcard driving the timing,
//! we can verify the input side works correctly before tackling the loopback case.
//!
//! Only requires: bass.dll, bass_aes67.dll
//!
//! Expected behavior:
//! - Buffer STABLE = soundcard consumes at same rate as network provides
//! - Buffer GROWING = soundcard slower than network (will overflow)
//! - Buffer SHRINKING = soundcard faster than network (will underrun)

use std::collections::VecDeque;
use std::ffi::{c_void, CString};
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
    fn BASS_ChannelPlay(handle: DWORD, restart: BOOL) -> BOOL;
    fn BASS_StreamFree(handle: HSTREAM) -> BOOL;
    fn BASS_PluginFree(handle: HPLUGIN) -> BOOL;
    fn BASS_SetConfig(option: DWORD, value: DWORD) -> BOOL;
    fn BASS_SetConfigPtr(option: DWORD, value: *const c_void) -> BOOL;
    fn BASS_GetConfig(option: DWORD) -> DWORD;
}

// AES67 config options
const BASS_CONFIG_AES67_INTERFACE: DWORD = 0x20001;
const BASS_CONFIG_AES67_JITTER: DWORD = 0x20002;
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
    println!("AES67 Input Lab");
    println!("===============\n");
    println!("Testing AES67 input with soundcard output.");
    println!("This isolates the input receiver for debugging.\n");

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

        // Initialize BASS with DEFAULT SOUNDCARD (device=-1)
        // This lets BASS/soundcard drive the timing
        println!("\nInitializing BASS (default soundcard, 20ms update)...");
        if BASS_Init(-1, 48000, 0, ptr::null_mut(), ptr::null()) == FALSE {
            println!("ERROR: Failed to initialize BASS (error {})", BASS_ErrorGetCode());
            return;
        }
        println!("  BASS initialized (device=-1, default soundcard)");

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
        // Use 500ms jitter buffer to match BASS default buffer size
        let interface = CString::new("192.168.60.102").unwrap();
        BASS_SetConfigPtr(BASS_CONFIG_AES67_INTERFACE, interface.as_ptr() as *const c_void);
        BASS_SetConfig(BASS_CONFIG_AES67_JITTER, 500);
        println!("  AES67 configured (interface=192.168.60.102, jitter=500ms)");

        // Create AES67 INPUT stream (normal playback mode, NOT decode)
        println!("\nCreating AES67 input stream...");
        let input_url = CString::new("aes67://239.192.76.52:5004").unwrap();
        let input_stream = BASS_StreamCreateURL(
            input_url.as_ptr(),
            0,
            0,  // Normal playback mode (no BASS_STREAM_DECODE)
            ptr::null(),
            ptr::null_mut()
        );

        if input_stream == 0 {
            println!("ERROR: Failed to create input stream (error {})", BASS_ErrorGetCode());
            BASS_PluginFree(plugin);
            BASS_Free();
            return;
        }
        println!("  Input stream created (handle: {}, source: 239.192.76.52:5004)", input_stream);

        // Start playback to soundcard
        println!("\nStarting playback...");
        if BASS_ChannelPlay(input_stream, FALSE) == FALSE {
            println!("ERROR: Failed to start playback (error {})", BASS_ErrorGetCode());
            BASS_StreamFree(input_stream);
            BASS_PluginFree(plugin);
            BASS_Free();
            return;
        }
        println!("  Playback started");

        println!("\n==========================================");
        println!("AES67 Input Lab running:");
        println!("  INPUT:  239.192.76.52:5004 (Livewire source)");
        println!("  OUTPUT: Default soundcard");
        println!("==========================================");
        println!("Press Ctrl+C to stop\n");

        // Buffer level history for trend calculation
        let mut level_history: VecDeque<u32> = VecDeque::with_capacity(10);

        // Monitor loop
        while RUNNING.load(Ordering::SeqCst) {
            let state = BASS_ChannelIsActive(input_stream);
            if state == BASS_ACTIVE_STOPPED {
                println!("\nInput stream ended");
                break;
            }

            // Get input buffer stats
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

            // Display status
            print!("\r\x1b[K");
            print!("buf={}/{} rcv={} late={} und={} | {}",
                buffer_packets,
                target_packets,
                packets_received,
                packets_late,
                jitter_underruns,
                trend);
            use std::io::Write;
            std::io::stdout().flush().unwrap();

            thread::sleep(Duration::from_millis(500));
        }

        // Cleanup - free stream first to stop receiver thread cleanly
        println!("\n\nCleaning up...");
        BASS_StreamFree(input_stream);
        // Give receiver thread time to exit (100ms socket timeout + margin)
        thread::sleep(Duration::from_millis(200));
        BASS_PluginFree(plugin);
        BASS_Free();
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
