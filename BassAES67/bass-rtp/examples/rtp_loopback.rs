//! RTP Test for BASS RTP Plugin
//!
//! This example tests the bass-rtp plugin in two modes:
//! 1. Loopback mode (default): Sends RTP packets to itself
//! 2. Z/IP ONE mode: Connects to a Telos Z/IP ONE codec
//!
//! Usage:
//!   # Loopback mode (default)
//!   cargo run --release --example rtp_loopback
//!
//!   # Z/IP ONE mode - send PCM16 to port 9152 (returns same codec)
//!   cargo run --release --example rtp_loopback -- 192.168.50.155 9152 0
//!
//!   # Z/IP ONE mode - send PCM16 to port 9153 (returns MP2)
//!   cargo run --release --example rtp_loopback -- 192.168.50.155 9153 0
//!
//!   # Z/IP ONE mode - receive only from port 9150
//!   cargo run --release --example rtp_loopback -- 192.168.50.155 9150 0
//!
//! Arguments:
//!   [1] Remote IP address (default: 127.0.0.1 for loopback)
//!   [2] Remote port (default: 5004)
//!       Z/IP ONE ports:
//!         9150 = Receive only (no reply)
//!         9151 = Reply with G.722
//!         9152 = Reply with same codec as sent
//!         9153 = Reply with current codec setting (often MP2)
//!   [3] Output codec (default: 0)
//!         0 = PCM16, 1 = PCM24, 2 = MP2, 3 = OPUS, 4 = FLAC
//!   [4] Local port (default: 5004)
//!
//! On Windows, ensure bass.dll and bass_rtp.dll are in the PATH or current directory.

use std::ffi::{c_void, CString};
use std::io::Write;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// ============================================================================
// BASS FFI Types and Functions
// ============================================================================

type DWORD = u32;
type BOOL = i32;
type HSTREAM = DWORD;
type HPLUGIN = DWORD;

#[allow(dead_code)]
const TRUE: BOOL = 1;
const FALSE: BOOL = 0;

// BASS channel states
const BASS_ACTIVE_STOPPED: DWORD = 0;
const BASS_ACTIVE_PLAYING: DWORD = 1;
const BASS_ACTIVE_STALLED: DWORD = 2;
const BASS_ACTIVE_PAUSED: DWORD = 3;

// BASS stream flags
const BASS_SAMPLE_FLOAT: DWORD = 0x100;
const BASS_STREAM_DECODE: DWORD = 0x200000;

#[link(name = "bass")]
extern "system" {
    fn BASS_Init(
        device: i32,
        freq: DWORD,
        flags: DWORD,
        win: *mut c_void,
        dsguid: *const c_void,
    ) -> BOOL;
    fn BASS_Free() -> BOOL;
    fn BASS_GetVersion() -> DWORD;
    fn BASS_ErrorGetCode() -> i32;
    fn BASS_PluginLoad(file: *const i8, flags: DWORD) -> HPLUGIN;
    fn BASS_PluginFree(handle: HPLUGIN) -> BOOL;
    fn BASS_StreamCreate(
        freq: DWORD,
        chans: DWORD,
        flags: DWORD,
        proc: Option<StreamProc>,
        user: *mut c_void,
    ) -> HSTREAM;
    fn BASS_StreamFree(handle: HSTREAM) -> BOOL;
    fn BASS_ChannelPlay(handle: DWORD, restart: BOOL) -> BOOL;
    #[allow(dead_code)]
    fn BASS_ChannelStop(handle: DWORD) -> BOOL;
    fn BASS_ChannelIsActive(handle: DWORD) -> DWORD;
    fn BASS_ChannelGetLevel(handle: DWORD) -> DWORD;
}

// BASS stream callback type
type StreamProc = unsafe extern "system" fn(HSTREAM, *mut c_void, DWORD, *mut c_void) -> DWORD;

// ============================================================================
// RTP Plugin FFI Types (loaded dynamically)
// ============================================================================

/// Configuration for creating an RTP stream
#[repr(C)]
struct RtpStreamConfigFFI {
    local_port: u16,
    remote_addr: [u8; 4],
    remote_port: u16,
    sample_rate: u32,
    channels: u16,
    output_codec: u8,
    output_bitrate: u32,
    jitter_ms: u32,
    interface_addr: [u8; 4],
}

/// Statistics for an RTP stream
#[repr(C)]
#[derive(Debug, Default)]
struct RtpStatsFFI {
    input_packets: u64,
    output_packets: u64,
    input_dropped: u64,
    output_errors: u64,
    detected_codec: u32,
    buffer_level: u32,
}

// Codec constants
const BASS_RTP_CODEC_PCM16: u8 = 0;
const BASS_RTP_CODEC_PCM24: u8 = 1;
const BASS_RTP_CODEC_MP2: u8 = 2;
const BASS_RTP_CODEC_OPUS: u8 = 3;
const BASS_RTP_CODEC_FLAC: u8 = 4;

// Function pointer types for dynamically loaded functions
type FnBassRtpCreate =
    unsafe extern "system" fn(bass_channel: DWORD, config: *const RtpStreamConfigFFI) -> *mut c_void;
type FnBassRtpStart = unsafe extern "system" fn(handle: *mut c_void) -> i32;
type FnBassRtpStop = unsafe extern "system" fn(handle: *mut c_void) -> i32;
type FnBassRtpGetInputStream = unsafe extern "system" fn(handle: *mut c_void) -> HSTREAM;
type FnBassRtpGetStats =
    unsafe extern "system" fn(handle: *mut c_void, stats: *mut RtpStatsFFI) -> i32;
#[allow(dead_code)]
type FnBassRtpIsRunning = unsafe extern "system" fn(handle: *mut c_void) -> i32;
type FnBassRtpFree = unsafe extern "system" fn(handle: *mut c_void) -> i32;

/// Holds function pointers loaded from bass_rtp.dll
struct RtpFunctions {
    create: FnBassRtpCreate,
    start: FnBassRtpStart,
    stop: FnBassRtpStop,
    get_input_stream: FnBassRtpGetInputStream,
    get_stats: FnBassRtpGetStats,
    free: FnBassRtpFree,
}

// ============================================================================
// Dynamic Library Loading
// ============================================================================

#[cfg(windows)]
mod dynlib {
    use super::*;

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryA(lpLibFileName: *const i8) -> *mut c_void;
        fn GetProcAddress(hModule: *mut c_void, lpProcName: *const i8) -> *mut c_void;
        fn FreeLibrary(hLibModule: *mut c_void) -> i32;
    }

    pub struct Library {
        handle: *mut c_void,
    }

    impl Library {
        pub fn load(paths: &[&str]) -> Option<Self> {
            for path in paths {
                let c_path = CString::new(*path).unwrap();
                let handle = unsafe { LoadLibraryA(c_path.as_ptr()) };
                if !handle.is_null() {
                    println!("Loaded library from: {}", path);
                    return Some(Self { handle });
                }
            }
            None
        }

        pub unsafe fn get_fn<T>(&self, name: &str) -> Option<T> {
            let c_name = CString::new(name).unwrap();
            let ptr = GetProcAddress(self.handle, c_name.as_ptr());
            if ptr.is_null() {
                None
            } else {
                Some(std::mem::transmute_copy(&ptr))
            }
        }
    }

    impl Drop for Library {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                unsafe {
                    FreeLibrary(self.handle);
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod dynlib {
    use super::*;

    extern "C" {
        fn dlopen(filename: *const i8, flags: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const i8) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> i32;
    }

    const RTLD_NOW: i32 = 2;

    pub struct Library {
        handle: *mut c_void,
    }

    impl Library {
        pub fn load(paths: &[&str]) -> Option<Self> {
            for path in paths {
                let c_path = CString::new(*path).unwrap();
                let handle = unsafe { dlopen(c_path.as_ptr(), RTLD_NOW) };
                if !handle.is_null() {
                    println!("Loaded library from: {}", path);
                    return Some(Self { handle });
                }
            }
            None
        }

        pub unsafe fn get_fn<T>(&self, name: &str) -> Option<T> {
            let c_name = CString::new(name).unwrap();
            let ptr = dlsym(self.handle, c_name.as_ptr());
            if ptr.is_null() {
                None
            } else {
                Some(std::mem::transmute_copy(&ptr))
            }
        }
    }

    impl Drop for Library {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                unsafe {
                    dlclose(self.handle);
                }
            }
        }
    }
}

use dynlib::Library;

impl RtpFunctions {
    fn load(lib: &Library) -> Option<Self> {
        unsafe {
            Some(Self {
                create: lib.get_fn("BASS_RTP_Create")?,
                start: lib.get_fn("BASS_RTP_Start")?,
                stop: lib.get_fn("BASS_RTP_Stop")?,
                get_input_stream: lib.get_fn("BASS_RTP_GetInputStream")?,
                get_stats: lib.get_fn("BASS_RTP_GetStats")?,
                free: lib.get_fn("BASS_RTP_Free")?,
            })
        }
    }
}

// ============================================================================
// Tone Generator
// ============================================================================

/// State for generating a sine wave test tone
struct ToneGenerator {
    phase: f32,
    phase_increment: f32,
    amplitude: f32,
}

impl ToneGenerator {
    fn new(frequency: f32, sample_rate: f32, amplitude: f32) -> Self {
        Self {
            phase: 0.0,
            phase_increment: 2.0 * std::f32::consts::PI * frequency / sample_rate,
            amplitude,
        }
    }

    /// Generate stereo float samples into the buffer
    fn generate(&mut self, buffer: &mut [f32]) {
        for chunk in buffer.chunks_mut(2) {
            let sample = self.phase.sin() * self.amplitude;
            chunk[0] = sample; // Left
            chunk[1] = sample; // Right
            self.phase += self.phase_increment;
            if self.phase > 2.0 * std::f32::consts::PI {
                self.phase -= 2.0 * std::f32::consts::PI;
            }
        }
    }
}

// Global tone generator for the BASS callback
static mut TONE_GEN: Option<ToneGenerator> = None;

/// BASS stream callback that generates a 440Hz sine wave
unsafe extern "system" fn tone_stream_proc(
    _handle: HSTREAM,
    buffer: *mut c_void,
    length: DWORD,
    _user: *mut c_void,
) -> DWORD {
    if let Some(ref mut gen) = TONE_GEN {
        let samples = length as usize / 4; // 4 bytes per float sample
        let slice = std::slice::from_raw_parts_mut(buffer as *mut f32, samples);
        gen.generate(slice);
    }
    length
}

// ============================================================================
// Helpers
// ============================================================================

fn codec_name(codec: u8) -> &'static str {
    match codec {
        BASS_RTP_CODEC_PCM16 => "PCM16",
        BASS_RTP_CODEC_PCM24 => "PCM24",
        BASS_RTP_CODEC_MP2 => "MP2",
        BASS_RTP_CODEC_OPUS => "OPUS",
        BASS_RTP_CODEC_FLAC => "FLAC",
        _ => "Unknown",
    }
}

fn payload_type_name(pt: u32) -> &'static str {
    match pt {
        0 => "G.711u",
        9 => "G.722",
        14 => "MP2",
        21 => "PCM16",
        22 => "PCM24",
        96 => "MP2(dyn)",
        116 => "PCM20",
        _ if pt > 0 => "Other",
        _ => "-",
    }
}

fn parse_ip(s: &str) -> Option<[u8; 4]> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    Some([
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
        parts[3].parse().ok()?,
    ])
}

fn print_usage() {
    println!("BASS RTP Plugin Test");
    println!("====================\n");
    println!("Usage: rtp_loopback [remote_ip] [remote_port] [codec] [local_port]\n");
    println!("Arguments:");
    println!("  remote_ip   - Remote IP address (default: 127.0.0.1 for loopback)");
    println!("  remote_port - Remote port (default: 5004)");
    println!("                Z/IP ONE ports:");
    println!("                  9150 = Receive only (no reply)");
    println!("                  9151 = Reply with G.722");
    println!("                  9152 = Reply with same codec as sent");
    println!("                  9153 = Reply with current codec setting");
    println!("  codec       - Output codec: 0=PCM16, 1=PCM24, 2=MP2, 3=OPUS, 4=FLAC");
    println!("  local_port  - Local port to bind (default: 5004)\n");
    println!("Examples:");
    println!("  rtp_loopback                              # Loopback test");
    println!("  rtp_loopback 192.168.50.155 9152 0        # Z/IP ONE, same codec reply");
    println!("  rtp_loopback 192.168.50.155 9153 0        # Z/IP ONE, MP2 reply");
    println!("  rtp_loopback 192.168.50.155 9150 0 5004   # Z/IP ONE, receive only");
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    // Parse command-line arguments
    let args: Vec<String> = std::env::args().collect();

    // Show help if requested
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_usage();
        return;
    }

    // Parse arguments with defaults
    let remote_ip_str = args.get(1).map(|s| s.as_str()).unwrap_or("127.0.0.1");
    let remote_ip = match parse_ip(remote_ip_str) {
        Some(ip) => ip,
        None => {
            println!("ERROR: Invalid IP address: {}", remote_ip_str);
            print_usage();
            return;
        }
    };

    let remote_port: u16 = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(5004);

    let codec: u8 = args
        .get(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(BASS_RTP_CODEC_PCM16);

    let local_port: u16 = args
        .get(4)
        .and_then(|s| s.parse().ok())
        .unwrap_or(5004);

    // Determine mode
    let is_loopback = remote_ip == [127, 0, 0, 1];
    let is_receive_only = remote_port == 9150;

    println!("BASS RTP Plugin Test");
    println!("====================\n");

    if is_loopback {
        println!("Mode: LOOPBACK (self-test)");
    } else if is_receive_only {
        println!("Mode: RECEIVE ONLY from Z/IP ONE");
    } else {
        println!("Mode: BIDIRECTIONAL with Z/IP ONE");
        println!("      Port {} = {}", remote_port, match remote_port {
            9151 => "Reply with G.722",
            9152 => "Reply with same codec",
            9153 => "Reply with current codec (often MP2)",
            _ => "Custom port",
        });
    }

    println!();
    println!("Local port:  {}", local_port);
    println!("Remote:      {}.{}.{}.{}:{}",
        remote_ip[0], remote_ip[1], remote_ip[2], remote_ip[3], remote_port);
    println!("Send codec:  {}", codec_name(codec));
    println!();

    // Set up Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    ctrlc_handler(move || {
        running_clone.store(false, Ordering::SeqCst);
    });

    unsafe {
        // Get BASS version
        let version = BASS_GetVersion();
        println!(
            "BASS version: {}.{}.{}.{}",
            (version >> 24) & 0xFF,
            (version >> 16) & 0xFF,
            (version >> 8) & 0xFF,
            version & 0xFF
        );

        // Initialize BASS
        println!("Initializing BASS...");
        if BASS_Init(-1, 48000, 0, ptr::null_mut(), ptr::null()) == FALSE {
            println!(
                "ERROR: Failed to initialize BASS (error code: {})",
                BASS_ErrorGetCode()
            );
            return;
        }
        println!("BASS initialized successfully");

        // Load the RTP plugin via BASS (registers the plugin)
        println!("\nLoading RTP plugin...");

        #[cfg(windows)]
        let plugin_paths = [
            "bass_rtp.dll",
            "./bass_rtp.dll",
            "./target/release/bass_rtp.dll",
            "../target/release/bass_rtp.dll",
        ];

        #[cfg(not(windows))]
        let plugin_paths = [
            "libbass_rtp.so",
            "./libbass_rtp.so",
            "./target/release/libbass_rtp.so",
            "../target/release/libbass_rtp.so",
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
            println!(
                "ERROR: Failed to load plugin (error code: {})",
                BASS_ErrorGetCode()
            );
            println!("Tried paths: {:?}", plugin_paths);
            BASS_Free();
            return;
        }

        // Load RTP function pointers
        let rtp_lib = match Library::load(&plugin_paths) {
            Some(lib) => lib,
            None => {
                println!("ERROR: Failed to load bass_rtp library");
                BASS_PluginFree(plugin);
                BASS_Free();
                return;
            }
        };

        let rtp = match RtpFunctions::load(&rtp_lib) {
            Some(f) => f,
            None => {
                println!("ERROR: Failed to load RTP functions");
                BASS_PluginFree(plugin);
                BASS_Free();
                return;
            }
        };

        // Create tone generator stream (440Hz sine wave)
        let tone_stream = if !is_receive_only {
            println!("\nCreating 440Hz tone generator...");
            TONE_GEN = Some(ToneGenerator::new(440.0, 48000.0, 0.5));

            let stream = BASS_StreamCreate(
                48000,
                2,
                BASS_SAMPLE_FLOAT | BASS_STREAM_DECODE,
                Some(tone_stream_proc),
                ptr::null_mut(),
            );

            if stream == 0 {
                println!(
                    "ERROR: Failed to create tone stream (error code: {})",
                    BASS_ErrorGetCode()
                );
                BASS_PluginFree(plugin);
                BASS_Free();
                return;
            }
            println!("Tone generator created");
            stream
        } else {
            println!("\nReceive-only mode: No tone generator");
            0
        };

        // Configure RTP stream
        // Z/IP ONE recommends: min buffer = 2x jitter, max buffer = 5x jitter
        // For typical network jitter of 10-20ms, use 50-100ms buffer
        let config = RtpStreamConfigFFI {
            local_port,
            remote_addr: remote_ip,
            remote_port,
            sample_rate: 48000,
            channels: 2,
            output_codec: codec,
            output_bitrate: 384, // for MP2 (broadcast quality)
            jitter_ms: 100,      // 100ms buffer for network jitter
            interface_addr: [0, 0, 0, 0], // default interface
        };

        println!("\nCreating RTP stream...");
        let rtp_handle = (rtp.create)(tone_stream, &config);
        if rtp_handle.is_null() {
            println!(
                "ERROR: Failed to create RTP stream (error code: {})",
                BASS_ErrorGetCode()
            );
            if tone_stream != 0 {
                BASS_StreamFree(tone_stream);
            }
            BASS_PluginFree(plugin);
            BASS_Free();
            return;
        }
        println!("RTP stream created");

        // Start the RTP stream
        println!("Starting RTP stream...");
        if (rtp.start)(rtp_handle) == 0 {
            println!(
                "ERROR: Failed to start RTP stream (error code: {})",
                BASS_ErrorGetCode()
            );
            (rtp.free)(rtp_handle);
            if tone_stream != 0 {
                BASS_StreamFree(tone_stream);
            }
            BASS_PluginFree(plugin);
            BASS_Free();
            return;
        }
        println!("RTP stream started");

        // Get the input stream handle and start playback
        let input_stream = (rtp.get_input_stream)(rtp_handle);
        if input_stream == 0 {
            println!("Input stream not available yet (will receive when data arrives)");
        } else {
            println!("Input stream ready (handle: {})", input_stream);

            if BASS_ChannelPlay(input_stream, FALSE) == FALSE {
                println!(
                    "WARNING: Failed to start playback (error code: {})",
                    BASS_ErrorGetCode()
                );
            } else {
                println!("Audio playback started");
            }
        }

        println!("\n--- Running (Ctrl+C to stop) ---\n");
        if is_receive_only {
            println!("Waiting for incoming RTP packets from Z/IP ONE...\n");
        }

        // Monitor loop
        let start_time = std::time::Instant::now();
        let mut stats = RtpStatsFFI::default();
        let mut last_rx = 0u64;

        while running.load(Ordering::SeqCst) {
            // Get statistics
            (rtp.get_stats)(rtp_handle, &mut stats);

            // Get input stream level if available
            let (left_level, right_level) = if input_stream != 0 {
                let level = BASS_ChannelGetLevel(input_stream);
                let left = (level & 0xFFFF) as f32 / 32768.0 * 100.0;
                let right = ((level >> 16) & 0xFFFF) as f32 / 32768.0 * 100.0;
                (left, right)
            } else {
                (0.0, 0.0)
            };

            // Get channel state
            let state = if input_stream != 0 {
                BASS_ChannelIsActive(input_stream)
            } else {
                BASS_ACTIVE_STOPPED
            };

            let state_str = match state {
                BASS_ACTIVE_STOPPED => "Stop",
                BASS_ACTIVE_PLAYING => "Play",
                BASS_ACTIVE_STALLED => "Stal",
                BASS_ACTIVE_PAUSED => "Paus",
                _ => "????",
            };

            // Format elapsed time
            let elapsed = start_time.elapsed().as_secs();
            let mins = elapsed / 60;
            let secs = elapsed % 60;

            // Create level meter
            let meter_width = 10;
            let left_bars = (left_level as usize * meter_width / 100).min(meter_width);
            let right_bars = (right_level as usize * meter_width / 100).min(meter_width);
            let left_meter: String =
                "|".repeat(left_bars) + &" ".repeat(meter_width - left_bars);
            let right_meter: String =
                "|".repeat(right_bars) + &" ".repeat(meter_width - right_bars);

            // Calculate packets per second (RX)
            let rx_pps = (stats.input_packets - last_rx) * 2; // 500ms intervals
            last_rx = stats.input_packets;

            // Print status line
            print!(
                "\r\x1b[K[{:02}:{:02}] {} TX:{:6} RX:{:6} ({:3}pps) Buf:{:3}% Drop:{} [{}][{}] {}",
                mins,
                secs,
                state_str,
                stats.output_packets,
                stats.input_packets,
                rx_pps,
                stats.buffer_level,
                stats.input_dropped,
                left_meter,
                right_meter,
                payload_type_name(stats.detected_codec),
            );
            std::io::stdout().flush().unwrap();

            thread::sleep(Duration::from_millis(500));
        }

        // Cleanup
        println!("\n\nStopping...");
        (rtp.stop)(rtp_handle);
        (rtp.free)(rtp_handle);
        if tone_stream != 0 {
            BASS_StreamFree(tone_stream);
        }
        BASS_PluginFree(plugin);
        BASS_Free();
        println!("Done!");
    }
}

// Simple Ctrl+C handler (platform-specific)
fn ctrlc_handler<F: Fn() + Send + 'static>(handler: F) {
    #[cfg(windows)]
    {
        use std::sync::Mutex;
        static HANDLER: Mutex<Option<Box<dyn Fn() + Send>>> = Mutex::new(None);

        extern "system" fn ctrl_handler(_: u32) -> i32 {
            if let Ok(guard) = HANDLER.lock() {
                if let Some(ref f) = *guard {
                    f();
                }
            }
            1
        }

        *HANDLER.lock().unwrap() = Some(Box::new(handler));

        #[link(name = "kernel32")]
        extern "system" {
            fn SetConsoleCtrlHandler(handler: extern "system" fn(u32) -> i32, add: i32) -> i32;
        }

        unsafe {
            SetConsoleCtrlHandler(ctrl_handler, 1);
        }
    }

    #[cfg(not(windows))]
    {
        use std::sync::Mutex;
        static HANDLER: Mutex<Option<Box<dyn Fn() + Send>>> = Mutex::new(None);

        extern "C" fn signal_handler(_: i32) {
            if let Ok(guard) = HANDLER.lock() {
                if let Some(ref f) = *guard {
                    f();
                }
            }
        }

        *HANDLER.lock().unwrap() = Some(Box::new(handler));

        unsafe {
            libc::signal(libc::SIGINT, signal_handler as libc::sighandler_t);
        }
    }
}
