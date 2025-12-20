//! Test application for BASS AES67 plugin.
//!
//! Usage: cargo run --example test_stream
//!
//! This example connects to an AES67 multicast stream and plays it through
//! the default audio output.

use std::ffi::CString;
use std::ptr;
use std::thread;
use std::time::Duration;

// BASS types
type DWORD = u32;
type BOOL = i32;
type HSTREAM = DWORD;
type HPLUGIN = DWORD;

const TRUE: BOOL = 1;
const FALSE: BOOL = 0;

// BASS functions
#[link(name = "bass")]
extern "system" {
    fn BASS_Init(device: i32, freq: DWORD, flags: DWORD, win: *mut std::ffi::c_void, dsguid: *const std::ffi::c_void) -> BOOL;
    fn BASS_Free() -> BOOL;
    fn BASS_GetVersion() -> DWORD;
    fn BASS_ErrorGetCode() -> i32;
    fn BASS_PluginLoad(file: *const i8, flags: DWORD) -> HPLUGIN;
    fn BASS_StreamCreateURL(url: *const i8, offset: DWORD, flags: DWORD, proc: *const std::ffi::c_void, user: *mut std::ffi::c_void) -> HSTREAM;
    fn BASS_ChannelPlay(handle: DWORD, restart: BOOL) -> BOOL;
    fn BASS_ChannelStop(handle: DWORD) -> BOOL;
    fn BASS_ChannelIsActive(handle: DWORD) -> DWORD;
    fn BASS_StreamFree(handle: HSTREAM) -> BOOL;
    fn BASS_PluginFree(handle: HPLUGIN) -> BOOL;
    fn BASS_SetConfig(option: DWORD, value: DWORD) -> BOOL;
    fn BASS_SetConfigPtr(option: DWORD, value: *const std::ffi::c_void) -> BOOL;
    fn BASS_GetConfigPtr(option: DWORD) -> *const std::ffi::c_void;
}

// Channel states
const BASS_ACTIVE_STOPPED: DWORD = 0;
const BASS_ACTIVE_PLAYING: DWORD = 1;
const BASS_ACTIVE_STALLED: DWORD = 2;
const BASS_ACTIVE_PAUSED: DWORD = 3;

// AES67 config options
const BASS_CONFIG_AES67_PT: DWORD = 0x20000;
const BASS_CONFIG_AES67_INTERFACE: DWORD = 0x20001;
const BASS_CONFIG_AES67_JITTER: DWORD = 0x20002;
const BASS_CONFIG_AES67_PTP_DOMAIN: DWORD = 0x20003;
const BASS_CONFIG_AES67_PTP_STATS: DWORD = 0x20004;

fn main() {
    println!("BASS AES67 Plugin Test");
    println!("======================\n");

    unsafe {
        // Get BASS version
        let version = BASS_GetVersion();
        println!("BASS version: {}.{}.{}.{}",
            (version >> 24) & 0xFF,
            (version >> 16) & 0xFF,
            (version >> 8) & 0xFF,
            version & 0xFF);

        // Initialize BASS with default device (-1)
        println!("\nInitializing BASS...");
        if BASS_Init(-1, 48000, 0, ptr::null_mut(), ptr::null()) == FALSE {
            println!("ERROR: Failed to initialize BASS (error code: {})", BASS_ErrorGetCode());
            return;
        }
        println!("BASS initialized successfully");

        // Load the AES67 plugin
        println!("\nLoading AES67 plugin...");
        let plugin_path = CString::new("bass_aes67.dll").unwrap();
        let plugin = BASS_PluginLoad(plugin_path.as_ptr(), 0);
        if plugin == 0 {
            println!("ERROR: Failed to load plugin (error code: {})", BASS_ErrorGetCode());
            println!("Make sure bass_aes67.dll is in the current directory or PATH");
            BASS_Free();
            return;
        }
        println!("Plugin loaded successfully (handle: {})", plugin);

        // Configure AES67 settings
        println!("\nConfiguring AES67...");

        // Set network interface
        let interface = CString::new("192.168.60.102").unwrap();
        BASS_SetConfigPtr(BASS_CONFIG_AES67_INTERFACE, interface.as_ptr() as *const std::ffi::c_void);
        println!("  Interface: 192.168.60.102");

        // Set payload type
        BASS_SetConfig(BASS_CONFIG_AES67_PT, 96);
        println!("  Payload type: 96");

        // Set jitter buffer
        BASS_SetConfig(BASS_CONFIG_AES67_JITTER, 20);
        println!("  Jitter buffer: 20ms");

        // Set PTP domain
        BASS_SetConfig(BASS_CONFIG_AES67_PTP_DOMAIN, 10);
        println!("  PTP domain: 10");

        // Create stream from AES67 URL
        println!("\nConnecting to AES67 stream...");
        let url = CString::new("aes67://239.192.76.52:5004").unwrap();
        let stream = BASS_StreamCreateURL(url.as_ptr(), 0, 0, ptr::null(), ptr::null_mut());

        if stream == 0 {
            println!("ERROR: Failed to create stream (error code: {})", BASS_ErrorGetCode());
            BASS_PluginFree(plugin);
            BASS_Free();
            return;
        }
        println!("Stream created (handle: {})", stream);

        // Start playback
        println!("\nStarting playback...");
        if BASS_ChannelPlay(stream, FALSE) == FALSE {
            println!("ERROR: Failed to start playback (error code: {})", BASS_ErrorGetCode());
            BASS_StreamFree(stream);
            BASS_PluginFree(plugin);
            BASS_Free();
            return;
        }
        println!("Playback started!");
        println!("\nPress Ctrl+C to stop...\n");

        // Monitor playback
        loop {
            let state = BASS_ChannelIsActive(stream);
            let state_str = match state {
                BASS_ACTIVE_STOPPED => "Stopped",
                BASS_ACTIVE_PLAYING => "Playing",
                BASS_ACTIVE_STALLED => "Stalled (buffering)",
                BASS_ACTIVE_PAUSED => "Paused",
                _ => "Unknown",
            };

            // Get PTP stats
            let ptp_stats_ptr = BASS_GetConfigPtr(BASS_CONFIG_AES67_PTP_STATS);
            let ptp_stats = if !ptp_stats_ptr.is_null() {
                let c_str = std::ffi::CStr::from_ptr(ptp_stats_ptr as *const i8);
                c_str.to_string_lossy().into_owned()
            } else {
                "PTP: N/A".to_string()
            };

            // Clear line and print status
            print!("\r\x1b[K");  // Clear line
            print!("Audio: {:12} | {}", state_str, ptp_stats);
            use std::io::Write;
            std::io::stdout().flush().unwrap();

            if state == BASS_ACTIVE_STOPPED {
                println!("\nStream ended");
                break;
            }

            thread::sleep(Duration::from_millis(500));
        }

        // Cleanup
        println!("\nCleaning up...");
        BASS_ChannelStop(stream);
        BASS_StreamFree(stream);
        BASS_PluginFree(plugin);
        BASS_Free();
        println!("Done!");
    }
}
