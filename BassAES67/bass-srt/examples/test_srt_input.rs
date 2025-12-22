//! Test application for BASS SRT plugin input.
//!
//! Usage:
//!   Terminal 1 (start sender first):
//!     cd BassAES67/bass-srt
//!     ./target/release/examples/srt_sender
//!
//!   Terminal 2 (start receiver):
//!     cd BassAES67/bass-srt
//!     export LD_LIBRARY_PATH=./target/release:../bass-aes67/target/release:$LD_LIBRARY_PATH
//!     ./target/release/examples/test_srt_input
//!
//! The Rust sender transmits a 440Hz sine wave as raw L16 PCM over SRT.
//! This receiver connects to the SRT stream and plays it through BASS.
//!
//! For custom SRT URL: ./target/release/examples/test_srt_input srt://host:port
//!
//! NOTE: This receiver expects raw L16 PCM. ffmpeg sends MPEG-TS which is not
//!       currently supported. Use the Rust srt_sender example for testing.

use std::ffi::CString;
use std::ptr;
use std::thread;
use std::time::Duration;
use std::io::Write;

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
    fn BASS_ChannelGetLevel(handle: DWORD) -> DWORD;
    fn BASS_StreamFree(handle: HSTREAM) -> BOOL;
    fn BASS_PluginFree(handle: HPLUGIN) -> BOOL;
    fn BASS_GetConfig(option: DWORD) -> DWORD;
}

// Channel states
const BASS_ACTIVE_STOPPED: DWORD = 0;
const BASS_ACTIVE_PLAYING: DWORD = 1;
const BASS_ACTIVE_STALLED: DWORD = 2;
const BASS_ACTIVE_PAUSED: DWORD = 3;

// SRT config options (from lib.rs)
const BASS_CONFIG_SRT_BUFFER_LEVEL: DWORD = 0x21001;
const BASS_CONFIG_SRT_PACKETS_RECEIVED: DWORD = 0x21002;
const BASS_CONFIG_SRT_PACKETS_DROPPED: DWORD = 0x21003;
const BASS_CONFIG_SRT_UNDERRUNS: DWORD = 0x21004;
const BASS_CONFIG_SRT_CODEC: DWORD = 0x21005;
const BASS_CONFIG_SRT_BITRATE: DWORD = 0x21006;

// SRT transport statistics
const BASS_CONFIG_SRT_RTT: DWORD = 0x21020;
const BASS_CONFIG_SRT_BANDWIDTH: DWORD = 0x21021;
const BASS_CONFIG_SRT_LOSS_TOTAL: DWORD = 0x21024;
const BASS_CONFIG_SRT_RETRANS_TOTAL: DWORD = 0x21025;
const BASS_CONFIG_SRT_UPTIME: DWORD = 0x21029;

// Codec values
const CODEC_UNKNOWN: DWORD = 0;
const CODEC_PCM: DWORD = 1;
const CODEC_OPUS: DWORD = 2;
const CODEC_MP2: DWORD = 3;
const CODEC_FLAC: DWORD = 4;

fn main() {
    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();

    let srt_url = if args.len() > 1 {
        args[1].clone()
    } else {
        "srt://127.0.0.1:9000".to_string()
    };

    println!("BASS SRT Plugin Test");
    println!("====================\n");

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

        // Load the SRT plugin
        println!("\nLoading SRT plugin...");

        // Try different paths for the plugin
        let plugin_paths = [
            "libbass_srt.so",
            "./libbass_srt.so",
            "./target/release/libbass_srt.so",
            "../target/release/libbass_srt.so",
        ];

        let mut plugin: HPLUGIN = 0;
        for path in &plugin_paths {
            let plugin_path = CString::new(*path).unwrap();
            plugin = BASS_PluginLoad(plugin_path.as_ptr(), 0);
            if plugin != 0 {
                println!("Plugin loaded from: {}", path);
                break;
            }
        }

        if plugin == 0 {
            println!("ERROR: Failed to load plugin (error code: {})", BASS_ErrorGetCode());
            println!("Make sure libbass_srt.so is in the current directory or PATH");
            println!("Tried paths: {:?}", plugin_paths);
            BASS_Free();
            return;
        }
        println!("Plugin loaded successfully (handle: {})", plugin);

        // Create stream from SRT URL
        println!("\nConnecting to SRT stream: {}", srt_url);
        println!("(Make sure an SRT sender is running on that address)");
        println!();

        let url = CString::new(srt_url.as_str()).unwrap();
        let stream = BASS_StreamCreateURL(url.as_ptr(), 0, 0, ptr::null(), ptr::null_mut());

        if stream == 0 {
            let error = BASS_ErrorGetCode();
            println!("ERROR: Failed to create stream (error code: {})", error);
            println!("\nTo test, start the SRT sender first in another terminal:");
            println!("  ./target/release/examples/srt_sender");
            println!("\nThen run this receiver again.");
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
        let mut last_packets: u64 = 0;
        loop {
            let state = BASS_ChannelIsActive(stream);
            let state_str = match state {
                BASS_ACTIVE_STOPPED => "Stopped",
                BASS_ACTIVE_PLAYING => "Playing",
                BASS_ACTIVE_STALLED => "Stalled",
                BASS_ACTIVE_PAUSED => "Paused",
                _ => "Unknown",
            };

            // Get audio level
            let level = BASS_ChannelGetLevel(stream);
            let left = (level & 0xFFFF) as f32 / 32768.0 * 100.0;
            let right = ((level >> 16) & 0xFFFF) as f32 / 32768.0 * 100.0;

            // Get SRT stats
            let buffer_level = BASS_GetConfig(BASS_CONFIG_SRT_BUFFER_LEVEL);
            let packets = BASS_GetConfig(BASS_CONFIG_SRT_PACKETS_RECEIVED) as u64;
            let dropped = BASS_GetConfig(BASS_CONFIG_SRT_PACKETS_DROPPED);
            let underruns = BASS_GetConfig(BASS_CONFIG_SRT_UNDERRUNS);
            let codec = BASS_GetConfig(BASS_CONFIG_SRT_CODEC);
            let bitrate = BASS_GetConfig(BASS_CONFIG_SRT_BITRATE);

            // Get SRT transport stats
            let rtt_x10 = BASS_GetConfig(BASS_CONFIG_SRT_RTT);
            let bandwidth = BASS_GetConfig(BASS_CONFIG_SRT_BANDWIDTH);
            let loss = BASS_GetConfig(BASS_CONFIG_SRT_LOSS_TOTAL);
            let retrans = BASS_GetConfig(BASS_CONFIG_SRT_RETRANS_TOTAL);
            let uptime = BASS_GetConfig(BASS_CONFIG_SRT_UPTIME);

            let codec_str = match codec {
                CODEC_PCM => "PCM",
                CODEC_OPUS => "OPUS",
                CODEC_MP2 => "MP2",
                CODEC_FLAC => "FLAC",
                _ => "?",
            };

            // Format bitrate display (only for encoded codecs)
            let bitrate_str = if bitrate > 0 {
                format!("{}k", bitrate)
            } else {
                "-".to_string()
            };

            // Format RTT (stored as ms × 10)
            let rtt_ms = rtt_x10 as f32 / 10.0;

            // Calculate packets per second
            let pps = packets.saturating_sub(last_packets) * 2;  // Updates every 500ms
            last_packets = packets;

            // Create level meter (shorter to fit more stats)
            let meter_width = 10;
            let left_bars = (left as usize * meter_width / 100).min(meter_width);
            let right_bars = (right as usize * meter_width / 100).min(meter_width);
            let left_meter: String = "█".repeat(left_bars) + &" ".repeat(meter_width - left_bars);
            let right_meter: String = "█".repeat(right_bars) + &" ".repeat(meter_width - right_bars);

            // Format uptime as mm:ss
            let uptime_min = uptime / 60;
            let uptime_sec = uptime % 60;

            // Clear line and print status (two lines for more detail)
            print!("\r\x1b[K");
            print!("{:8} [{:4} {:>4}] L[{}] R[{}] | RTT:{:.1}ms BW:{}k Loss:{} Retrans:{} Up:{}:{:02}",
                state_str,
                codec_str,
                bitrate_str,
                left_meter,
                right_meter,
                rtt_ms,
                bandwidth,
                loss,
                retrans,
                uptime_min,
                uptime_sec
            );
            std::io::stdout().flush().unwrap();

            if state == BASS_ACTIVE_STOPPED {
                println!("\n\nStream ended");
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
