//! RTP Output Test for BASS RTP Plugin
//!
//! This example tests the output module of bass-rtp where WE connect TO Z/IP ONE.
//! Unlike rtp_loopback which waits for incoming connections, this example
//! actively connects to a specified Z/IP ONE and sends audio while receiving return audio.
//!
//! Usage:
//!   # Connect to Z/IP ONE port 9152 (returns same codec as sent)
//!   cargo run --release --example rtp_output_test -- 192.168.50.155 9152
//!
//!   # Connect with specific send codec (0=PCM16, 1=PCM20, 2=PCM24, 3=MP2, 4=G.711, 5=G.722)
//!   cargo run --release --example rtp_output_test -- 192.168.50.155 9152 --codec 3
//!
//!   # With custom local port
//!   cargo run --release --example rtp_output_test -- 192.168.50.155 9152 --local-port 5006
//!
//!   # With buffer settings
//!   cargo run --release --example rtp_output_test -- 192.168.50.155 9152 --buffer 100
//!
//! Z/IP ONE Reciprocal Ports:
//!   9150 = Receive only (no reply from Z/IP ONE)
//!   9151 = Reply with G.722
//!   9152 = Reply with same codec as sent
//!   9153 = Reply with current codec setting (often MP2)
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
// RTP Output Plugin FFI Types (loaded dynamically)
// ============================================================================

/// Buffer mode constants
const BASS_RTP_BUFFER_MODE_SIMPLE: u8 = 0;
const BASS_RTP_BUFFER_MODE_MINMAX: u8 = 1;

/// Configuration for creating an RTP output stream
#[repr(C)]
struct RtpOutputConfigFFI {
    /// Remote IP address (Z/IP ONE) as 4 bytes
    remote_addr: [u8; 4],
    /// Remote port (9150-9153 for Z/IP ONE, or custom)
    remote_port: u16,
    /// Local port to bind (0 = auto-assign)
    local_port: u16,
    /// Network interface IP address (4 bytes, 0.0.0.0 = default)
    interface_addr: [u8; 4],
    /// Sample rate (48000)
    sample_rate: u32,
    /// Number of channels (1 or 2)
    channels: u16,
    /// Send codec (BASS_RTP_CODEC_*)
    send_codec: u8,
    /// Send bitrate in kbps (for MP2/OPUS, 0 = default)
    send_bitrate: u32,
    /// Frame duration in milliseconds
    frame_duration_ms: u32,
    /// Clock mode (0=PTP, 1=Livewire, 2=System)
    clock_mode: u8,
    /// Return audio buffer mode (0=simple, 1=min/max)
    return_buffer_mode: u8,
    /// Return audio buffer size in ms (simple mode target, minmax mode min)
    return_buffer_ms: u32,
    /// Return audio max buffer size in ms (minmax mode only)
    return_max_buffer_ms: u32,
}

/// Statistics for an RTP output stream
#[repr(C)]
#[derive(Debug, Default)]
struct RtpOutputStatsFFI {
    /// TX packets sent
    tx_packets: u64,
    /// TX bytes sent
    tx_bytes: u64,
    /// TX encode errors
    tx_encode_errors: u64,
    /// TX buffer underruns
    tx_underruns: u64,
    /// RX packets received (return audio)
    rx_packets: u64,
    /// RX bytes received
    rx_bytes: u64,
    /// RX decode errors
    rx_decode_errors: u64,
    /// RX packets dropped (buffer full)
    rx_dropped: u64,
    /// Current return buffer level (samples)
    buffer_level: u32,
    /// Detected return audio payload type
    detected_return_pt: u8,
    /// Current PPM adjustment * 1000
    current_ppm_x1000: i32,
}

// Codec constants
const BASS_RTP_CODEC_PCM16: u8 = 0;
const BASS_RTP_CODEC_PCM20: u8 = 1;
const BASS_RTP_CODEC_PCM24: u8 = 2;
const BASS_RTP_CODEC_MP2: u8 = 3;
const BASS_RTP_CODEC_G711: u8 = 4;
const BASS_RTP_CODEC_G722: u8 = 5;

// Clock mode constants
const CLOCK_MODE_PTP: u8 = 0;
const CLOCK_MODE_LIVEWIRE: u8 = 1;
const CLOCK_MODE_SYSTEM: u8 = 2;

// Function pointer types for dynamically loaded functions
type FnBassRtpOutputCreate =
    unsafe extern "system" fn(source_channel: DWORD, config: *const RtpOutputConfigFFI) -> *mut c_void;
type FnBassRtpOutputStart = unsafe extern "system" fn(handle: *mut c_void) -> i32;
type FnBassRtpOutputStop = unsafe extern "system" fn(handle: *mut c_void) -> i32;
type FnBassRtpOutputGetReturnStream = unsafe extern "system" fn(handle: *mut c_void) -> HSTREAM;
type FnBassRtpOutputGetStats =
    unsafe extern "system" fn(handle: *mut c_void, stats: *mut RtpOutputStatsFFI) -> i32;
#[allow(dead_code)]
type FnBassRtpOutputIsRunning = unsafe extern "system" fn(handle: *mut c_void) -> i32;
type FnBassRtpOutputFree = unsafe extern "system" fn(handle: *mut c_void) -> i32;

/// Holds function pointers loaded from bass_rtp.dll
struct RtpOutputFunctions {
    create: FnBassRtpOutputCreate,
    start: FnBassRtpOutputStart,
    stop: FnBassRtpOutputStop,
    get_return_stream: FnBassRtpOutputGetReturnStream,
    get_stats: FnBassRtpOutputGetStats,
    free: FnBassRtpOutputFree,
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

impl RtpOutputFunctions {
    fn load(lib: &Library) -> Option<Self> {
        unsafe {
            Some(Self {
                create: lib.get_fn("BASS_RTP_OutputCreate")?,
                start: lib.get_fn("BASS_RTP_OutputStart")?,
                stop: lib.get_fn("BASS_RTP_OutputStop")?,
                get_return_stream: lib.get_fn("BASS_RTP_OutputGetReturnStream")?,
                get_stats: lib.get_fn("BASS_RTP_OutputGetStats")?,
                free: lib.get_fn("BASS_RTP_OutputFree")?,
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
        BASS_RTP_CODEC_PCM20 => "PCM20",
        BASS_RTP_CODEC_PCM24 => "PCM24",
        BASS_RTP_CODEC_MP2 => "MP2",
        BASS_RTP_CODEC_G711 => "G.711",
        BASS_RTP_CODEC_G722 => "G.722",
        _ => "Unknown",
    }
}

fn clock_mode_name(mode: u8) -> &'static str {
    match mode {
        CLOCK_MODE_PTP => "PTP",
        CLOCK_MODE_LIVEWIRE => "Livewire",
        CLOCK_MODE_SYSTEM => "System",
        _ => "Unknown",
    }
}

fn payload_type_name(pt: u8) -> &'static str {
    match pt {
        0 => "G.711u",
        9 => "G.722",
        14 => "MP2",
        21 => "PCM16",
        22 => "PCM24",
        96 => "MP2(dyn)",
        99 => "AAC-X",
        116 => "PCM20",
        122 => "AAC(!)", // LATM format - not supported
        255 => "-",
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
    println!("BASS RTP Output Test");
    println!("====================\n");
    println!("Usage: rtp_output_test <remote_ip> <remote_port> [options]\n");
    println!("Required arguments:");
    println!("  remote_ip   - Z/IP ONE IP address");
    println!("  remote_port - Z/IP ONE port");
    println!("                  9150 = Receive only (no reply)");
    println!("                  9151 = Reply with G.722");
    println!("                  9152 = Reply with same codec as sent");
    println!("                  9153 = Reply with current codec setting\n");
    println!("Options:");
    println!("  --codec <n>        Send codec: 0=PCM16, 1=PCM20, 2=PCM24, 3=MP2, 4=G.711, 5=G.722");
    println!("  --bitrate <kbps>   Bitrate for MP2 (default: 384)");
    println!("  --local-port <n>   Local port to bind (default: 0 = auto)");
    println!("  --clock <mode>     Clock mode: ptp, livewire, system (default: system)");
    println!("  --buffer <ms>      Return audio buffer size (default: 100ms)");
    println!("  --min-buffer <ms>  Min/Max mode: minimum buffer (target)");
    println!("  --max-buffer <ms>  Min/Max mode: maximum buffer (ceiling)");
    println!();
    println!("Examples:");
    println!("  rtp_output_test 192.168.50.155 9152");
    println!("  rtp_output_test 192.168.50.155 9152 --codec 3 --bitrate 384");
    println!("  rtp_output_test 192.168.50.155 9151 --codec 5  # G.722 to G.722");
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    // Parse command-line arguments
    let args: Vec<String> = std::env::args().collect();

    // Show help if requested or not enough arguments
    if args.len() < 3 || args.iter().any(|a| a == "-h" || a == "--help") {
        print_usage();
        return;
    }

    // Parse required arguments
    let remote_ip_str = &args[1];
    let remote_ip = match parse_ip(remote_ip_str) {
        Some(ip) => ip,
        None => {
            println!("ERROR: Invalid IP address: {}", remote_ip_str);
            print_usage();
            return;
        }
    };

    let remote_port: u16 = match args[2].parse() {
        Ok(p) => p,
        Err(_) => {
            println!("ERROR: Invalid port: {}", args[2]);
            print_usage();
            return;
        }
    };

    // Parse optional arguments
    let mut send_codec: u8 = BASS_RTP_CODEC_PCM16;
    let mut send_bitrate: u32 = 384;
    let mut local_port: u16 = 0;
    let mut clock_mode: u8 = CLOCK_MODE_SYSTEM;
    let mut buffer_ms: u32 = 100;
    let mut min_buffer_ms: u32 = 0;
    let mut max_buffer_ms: u32 = 0;

    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--codec" => {
                if i + 1 < args.len() {
                    send_codec = args[i + 1].parse().unwrap_or(0);
                    i += 1;
                }
            }
            "--bitrate" => {
                if i + 1 < args.len() {
                    send_bitrate = args[i + 1].parse().unwrap_or(384);
                    i += 1;
                }
            }
            "--local-port" => {
                if i + 1 < args.len() {
                    local_port = args[i + 1].parse().unwrap_or(0);
                    i += 1;
                }
            }
            "--clock" => {
                if i + 1 < args.len() {
                    clock_mode = match args[i + 1].to_lowercase().as_str() {
                        "ptp" => CLOCK_MODE_PTP,
                        "livewire" => CLOCK_MODE_LIVEWIRE,
                        "system" | _ => CLOCK_MODE_SYSTEM,
                    };
                    i += 1;
                }
            }
            "--buffer" => {
                if i + 1 < args.len() {
                    buffer_ms = args[i + 1].parse().unwrap_or(100);
                    i += 1;
                }
            }
            "--min-buffer" => {
                if i + 1 < args.len() {
                    min_buffer_ms = args[i + 1].parse().unwrap_or(50);
                    i += 1;
                }
            }
            "--max-buffer" => {
                if i + 1 < args.len() {
                    max_buffer_ms = args[i + 1].parse().unwrap_or(200);
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }

    // Determine buffer mode
    let use_minmax_mode = min_buffer_ms > 0 && max_buffer_ms > 0;
    let (buffer_mode, effective_min, effective_max) = if use_minmax_mode {
        (BASS_RTP_BUFFER_MODE_MINMAX, min_buffer_ms, max_buffer_ms)
    } else {
        (BASS_RTP_BUFFER_MODE_SIMPLE, buffer_ms, 0)
    };

    println!("BASS RTP Output Test");
    println!("====================\n");

    println!("Mode: OUTPUT (WE connect to Z/IP ONE)");
    println!("      Port {} = {}", remote_port, match remote_port {
        9150 => "Receive only (no reply)",
        9151 => "Reply with G.722",
        9152 => "Reply with same codec",
        9153 => "Reply with current codec (often MP2)",
        _ => "Custom port",
    });

    println!();
    println!("Remote:      {}.{}.{}.{}:{}",
        remote_ip[0], remote_ip[1], remote_ip[2], remote_ip[3], remote_port);
    println!("Local port:  {}", if local_port == 0 { "auto".to_string() } else { local_port.to_string() });
    println!("Send codec:  {} ({}kbps)", codec_name(send_codec), send_bitrate);
    println!("Clock mode:  {}", clock_mode_name(clock_mode));
    if use_minmax_mode {
        println!("Ret buffer:  Min/Max mode (min: {}ms, max: {}ms)", effective_min, effective_max);
    } else {
        println!("Ret buffer:  Simple mode ({}ms)", effective_min);
    }
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

        // Load RTP Output function pointers
        let rtp_lib = match Library::load(&plugin_paths) {
            Some(lib) => lib,
            None => {
                println!("ERROR: Failed to load bass_rtp library");
                BASS_PluginFree(plugin);
                BASS_Free();
                return;
            }
        };

        let rtp = match RtpOutputFunctions::load(&rtp_lib) {
            Some(f) => f,
            None => {
                println!("ERROR: Failed to load RTP Output functions");
                BASS_PluginFree(plugin);
                BASS_Free();
                return;
            }
        };

        // Create tone generator stream (440Hz sine wave)
        println!("\nCreating 440Hz tone generator...");
        TONE_GEN = Some(ToneGenerator::new(440.0, 48000.0, 0.5));

        let tone_stream = BASS_StreamCreate(
            48000,
            2,
            BASS_SAMPLE_FLOAT | BASS_STREAM_DECODE,
            Some(tone_stream_proc),
            ptr::null_mut(),
        );

        if tone_stream == 0 {
            println!(
                "ERROR: Failed to create tone stream (error code: {})",
                BASS_ErrorGetCode()
            );
            BASS_PluginFree(plugin);
            BASS_Free();
            return;
        }
        println!("Tone generator created");

        // Configure RTP output stream
        let config = RtpOutputConfigFFI {
            remote_addr: remote_ip,
            remote_port,
            local_port,
            interface_addr: [0, 0, 0, 0], // default interface
            sample_rate: 48000,
            channels: 2,
            send_codec,
            send_bitrate,
            frame_duration_ms: 1, // 1ms frames
            clock_mode,
            return_buffer_mode: buffer_mode,
            return_buffer_ms: effective_min,
            return_max_buffer_ms: effective_max,
        };

        println!("\nCreating RTP output stream...");
        let rtp_handle = (rtp.create)(tone_stream, &config);
        if rtp_handle.is_null() {
            println!(
                "ERROR: Failed to create RTP output stream (error code: {})",
                BASS_ErrorGetCode()
            );
            BASS_StreamFree(tone_stream);
            BASS_PluginFree(plugin);
            BASS_Free();
            return;
        }
        println!("RTP output stream created");

        // Start the RTP output stream
        println!("Starting RTP output stream...");
        if (rtp.start)(rtp_handle) == 0 {
            println!(
                "ERROR: Failed to start RTP output stream (error code: {})",
                BASS_ErrorGetCode()
            );
            (rtp.free)(rtp_handle);
            BASS_StreamFree(tone_stream);
            BASS_PluginFree(plugin);
            BASS_Free();
            return;
        }
        println!("RTP output stream started - sending to Z/IP ONE");

        // Get the return stream handle and start playback
        let return_stream = (rtp.get_return_stream)(rtp_handle);
        if return_stream == 0 {
            println!("Return stream not available yet (will receive when data arrives)");
        } else {
            println!("Return stream ready (handle: {})", return_stream);

            if BASS_ChannelPlay(return_stream, FALSE) == FALSE {
                println!(
                    "WARNING: Failed to start return audio playback (error code: {})",
                    BASS_ErrorGetCode()
                );
            } else {
                println!("Return audio playback started");
            }
        }

        println!("\n--- Running (Ctrl+C to stop) ---\n");
        println!("Sending 440Hz tone to Z/IP ONE...\n");

        // Monitor loop
        let start_time = std::time::Instant::now();
        let mut stats = RtpOutputStatsFFI::default();
        let mut last_tx = 0u64;
        let mut last_rx = 0u64;

        while running.load(Ordering::SeqCst) {
            // Get statistics
            (rtp.get_stats)(rtp_handle, &mut stats);

            // Get return stream level if available
            let (left_level, right_level) = if return_stream != 0 {
                let level = BASS_ChannelGetLevel(return_stream);
                let left = (level & 0xFFFF) as f32 / 32768.0 * 100.0;
                let right = ((level >> 16) & 0xFFFF) as f32 / 32768.0 * 100.0;
                (left, right)
            } else {
                (0.0, 0.0)
            };

            // Get channel state
            let state = if return_stream != 0 {
                BASS_ChannelIsActive(return_stream)
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

            // Calculate packets per second
            let tx_pps = (stats.tx_packets - last_tx) * 2; // 500ms intervals
            let rx_pps = (stats.rx_packets - last_rx) * 2;
            last_tx = stats.tx_packets;
            last_rx = stats.rx_packets;

            // PPM display
            let ppm = stats.current_ppm_x1000 as f32 / 1000.0;

            // Print status line
            print!(
                "\r\x1b[K[{:02}:{:02}] {} TX:{:6}({:4}pps) RX:{:6}({:3}pps) Buf:{:5} Drop:{} [{}][{}] Ret:{} PPM:{:+.1}",
                mins,
                secs,
                state_str,
                stats.tx_packets,
                tx_pps,
                stats.rx_packets,
                rx_pps,
                stats.buffer_level,
                stats.rx_dropped,
                left_meter,
                right_meter,
                payload_type_name(stats.detected_return_pt),
                ppm,
            );
            std::io::stdout().flush().unwrap();

            thread::sleep(Duration::from_millis(500));
        }

        // Cleanup
        println!("\n\nStopping...");
        (rtp.stop)(rtp_handle);
        (rtp.free)(rtp_handle);
        BASS_StreamFree(tone_stream);
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
