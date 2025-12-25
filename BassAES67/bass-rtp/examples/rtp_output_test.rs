//! RTP Output Test for BASS RTP Module
//!
//! This example tests the OUTPUT module of bass-rtp where Z/IP ONE connects TO us.
//! - We LISTEN on a port for incoming connections
//! - We RECEIVE audio FROM Z/IP ONE (incoming)
//! - We SEND backfeed audio TO Z/IP ONE
//!
//! The remote address is auto-detected from the first incoming RTP packet.
//!
//! Usage:
//!   # Listen on port 5004 for Z/IP ONE to connect
//!   cargo run --release --example rtp_output_test -- 5004
//!
//!   # Listen with specific backfeed codec (0=PCM16, 1=PCM20, 2=PCM24, 3=MP2, 4=G.711, 5=G.722)
//!   cargo run --release --example rtp_output_test -- 5004 --backfeed-codec 3
//!
//!   # With buffer settings
//!   cargo run --release --example rtp_output_test -- 5004 --buffer 100
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
// RTP Output FFI Types (loaded dynamically)
// ============================================================================

/// Buffer mode constants
const BASS_RTP_BUFFER_MODE_SIMPLE: u8 = 0;
const BASS_RTP_BUFFER_MODE_MINMAX: u8 = 1;

/// Connection state for callback
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionState {
    Disconnected = 0,
    Connected = 1,
}

/// Connection callback type
type ConnectionCallback = extern "C" fn(state: ConnectionState, user_data: *mut c_void);

/// Configuration for creating an RTP Output stream (Z/IP ONE connects TO us)
#[repr(C)]
struct RtpOutputConfigFFI {
    /// Local port to listen on (Z/IP ONE connects here)
    local_port: u16,
    /// Network interface IP address (4 bytes, 0.0.0.0 = any)
    interface_addr: [u8; 4],
    /// Sample rate (48000)
    sample_rate: u32,
    /// Number of channels (1 or 2)
    channels: u16,
    /// Backfeed codec (BASS_RTP_CODEC_*)
    backfeed_codec: u8,
    /// Backfeed bitrate in kbps (for MP2, 0 = default)
    backfeed_bitrate: u32,
    /// Frame duration in milliseconds (1-5, 0 = default 1)
    frame_duration_ms: u32,
    /// Clock mode (0=PTP, 1=Livewire, 2=System)
    clock_mode: u8,
    /// PTP domain (0-127)
    ptp_domain: u8,
    /// Incoming audio buffer mode (0=simple, 1=min/max)
    buffer_mode: u8,
    /// Incoming audio buffer size in ms (simple mode target, minmax mode min)
    buffer_ms: u32,
    /// Incoming audio max buffer size in ms (minmax mode only)
    max_buffer_ms: u32,
    /// Connection state callback (optional)
    connection_callback: Option<ConnectionCallback>,
    /// User data for callback
    callback_user_data: *mut c_void,
}

/// Statistics for an RTP Output stream
#[repr(C)]
#[derive(Debug, Default)]
struct RtpOutputStatsFFI {
    /// RX packets received (incoming audio)
    rx_packets: u64,
    /// RX bytes received
    rx_bytes: u64,
    /// RX decode errors
    rx_decode_errors: u64,
    /// RX packets dropped (buffer full)
    rx_dropped: u64,
    /// TX packets sent (backfeed)
    tx_packets: u64,
    /// TX bytes sent
    tx_bytes: u64,
    /// TX encode errors
    tx_encode_errors: u64,
    /// TX buffer underruns
    tx_underruns: u64,
    /// Current incoming buffer level (samples)
    buffer_level: u32,
    /// Detected incoming audio payload type
    detected_incoming_pt: u8,
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
    unsafe extern "system" fn(backfeed_channel: DWORD, config: *const RtpOutputConfigFFI) -> *mut c_void;
type FnBassRtpOutputStart = unsafe extern "system" fn(handle: *mut c_void) -> i32;
type FnBassRtpOutputStop = unsafe extern "system" fn(handle: *mut c_void) -> i32;
type FnBassRtpOutputGetInputStream = unsafe extern "system" fn(handle: *mut c_void) -> HSTREAM;
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
    get_input_stream: FnBassRtpOutputGetInputStream,
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
                get_input_stream: lib.get_fn("BASS_RTP_OutputGetInputStream")?,
                get_stats: lib.get_fn("BASS_RTP_OutputGetStats")?,
                free: lib.get_fn("BASS_RTP_OutputFree")?,
            })
        }
    }
}

// ============================================================================
// Tone Generator (for backfeed)
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

// Global connection state
static CONNECTION_STATE: AtomicBool = AtomicBool::new(false);

/// Connection state callback - called when connection is established or lost
extern "C" fn on_connection_state_change(state: ConnectionState, _user_data: *mut c_void) {
    let connected = state == ConnectionState::Connected;
    CONNECTION_STATE.store(connected, Ordering::SeqCst);

    if connected {
        println!("\n>>> CONNECTED - Receiving RTP stream");
    } else {
        println!("\n>>> DISCONNECTED - RTP stream lost");
    }
}

/// BASS stream callback that generates a 440Hz sine wave for backfeed
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

fn print_usage() {
    println!("BASS RTP Output Test");
    println!("====================\n");
    println!("Usage: rtp_output_test <local_port> [options]\n");
    println!("Mode: OUTPUT - Z/IP ONE connects TO us\n");
    println!("       Remote address is auto-detected from first incoming packet.\n");
    println!("Required arguments:");
    println!("  local_port - Port to listen on (Z/IP ONE connects here)\n");
    println!("Options:");
    println!("  --backfeed-codec <n>  Backfeed codec: 0=PCM16, 1=PCM20, 2=PCM24, 3=MP2, 4=G.711, 5=G.722");
    println!("  --bitrate <kbps>      Bitrate for MP2 (default: 256)");
    println!("  --clock <mode>        Clock mode: ptp, livewire, system (default: system)");
    println!("  --buffer <ms>         Incoming audio buffer size (default: 100ms)");
    println!("  --min-buffer <ms>     Min/Max mode: minimum buffer (target)");
    println!("  --max-buffer <ms>     Min/Max mode: maximum buffer (ceiling)");
    println!();
    println!("Examples:");
    println!("  rtp_output_test 5004");
    println!("  rtp_output_test 5004 --backfeed-codec 3 --bitrate 384");
    println!("  rtp_output_test 5004 --buffer 150");
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    // Parse command-line arguments
    let args: Vec<String> = std::env::args().collect();

    // Show help if requested or not enough arguments
    if args.len() < 2 || args.iter().any(|a| a == "-h" || a == "--help") {
        print_usage();
        return;
    }

    // Parse required arguments
    let local_port: u16 = match args[1].parse() {
        Ok(p) => p,
        Err(_) => {
            println!("ERROR: Invalid port: {}", args[1]);
            print_usage();
            return;
        }
    };

    // Parse optional arguments
    let mut backfeed_codec: u8 = BASS_RTP_CODEC_PCM16;
    let mut backfeed_bitrate: u32 = 256;
    let mut clock_mode: u8 = CLOCK_MODE_SYSTEM;
    let mut buffer_ms: u32 = 100;
    let mut min_buffer_ms: u32 = 0;
    let mut max_buffer_ms: u32 = 0;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--backfeed-codec" => {
                if i + 1 < args.len() {
                    backfeed_codec = args[i + 1].parse().unwrap_or(0);
                    i += 1;
                }
            }
            "--bitrate" => {
                if i + 1 < args.len() {
                    backfeed_bitrate = args[i + 1].parse().unwrap_or(256);
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

    println!("Mode: OUTPUT (Z/IP ONE connects TO us)");
    println!("      Remote address will be auto-detected from first incoming packet.");

    println!();
    println!("Listen port:    {}", local_port);
    println!("Backfeed codec: {} ({}kbps)", codec_name(backfeed_codec), backfeed_bitrate);
    println!("Clock mode:     {}", clock_mode_name(clock_mode));
    if use_minmax_mode {
        println!("Buffer:         Min/Max mode (min: {}ms, max: {}ms)", effective_min, effective_max);
    } else {
        println!("Buffer:         Simple mode ({}ms)", effective_min);
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

        // Load the RTP library
        println!("\nLoading RTP library...");

        #[cfg(windows)]
        let lib_paths = [
            "bass_rtp.dll",
            "./bass_rtp.dll",
            "./target/release/bass_rtp.dll",
            "../target/release/bass_rtp.dll",
        ];

        #[cfg(not(windows))]
        let lib_paths = [
            "libbass_rtp.so",
            "./libbass_rtp.so",
            "./target/release/libbass_rtp.so",
            "../target/release/libbass_rtp.so",
        ];

        // Load RTP Output function pointers
        let rtp_lib = match Library::load(&lib_paths) {
            Some(lib) => lib,
            None => {
                println!("ERROR: Failed to load bass_rtp library");
                println!("Tried paths: {:?}", lib_paths);
                BASS_Free();
                return;
            }
        };

        let rtp = match RtpOutputFunctions::load(&rtp_lib) {
            Some(f) => f,
            None => {
                println!("ERROR: Failed to load RTP Output functions");
                BASS_Free();
                return;
            }
        };

        // Create tone generator stream (440Hz sine wave) for backfeed
        println!("\nCreating 440Hz tone generator for backfeed...");
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
            BASS_Free();
            return;
        }
        println!("Tone generator created");

        // Configure RTP Output stream
        let config = RtpOutputConfigFFI {
            local_port,
            interface_addr: [0, 0, 0, 0], // default interface
            sample_rate: 48000,
            channels: 2,
            backfeed_codec,
            backfeed_bitrate,
            frame_duration_ms: 1, // 1ms frames
            clock_mode,
            ptp_domain: 0,
            buffer_mode,
            buffer_ms: effective_min,
            max_buffer_ms: effective_max,
            connection_callback: Some(on_connection_state_change),
            callback_user_data: ptr::null_mut(),
        };

        println!("\nCreating RTP Output stream...");
        let rtp_handle = (rtp.create)(tone_stream, &config);
        if rtp_handle.is_null() {
            println!(
                "ERROR: Failed to create RTP Output stream (error code: {})",
                BASS_ErrorGetCode()
            );
            BASS_StreamFree(tone_stream);
            BASS_Free();
            return;
        }
        println!("RTP Output stream created");

        // Start the RTP Output stream
        println!("Starting RTP Output stream...");
        if (rtp.start)(rtp_handle) == 0 {
            println!(
                "ERROR: Failed to start RTP Output stream (error code: {})",
                BASS_ErrorGetCode()
            );
            (rtp.free)(rtp_handle);
            BASS_StreamFree(tone_stream);
            BASS_Free();
            return;
        }
        println!("RTP Output stream started - listening for connections");

        // Get the incoming stream handle and start playback
        let input_stream = (rtp.get_input_stream)(rtp_handle);
        if input_stream == 0 {
            println!("Incoming stream not available yet (will receive when data arrives)");
        } else {
            println!("Incoming stream ready (handle: {})", input_stream);

            if BASS_ChannelPlay(input_stream, FALSE) == FALSE {
                println!(
                    "WARNING: Failed to start incoming audio playback (error code: {})",
                    BASS_ErrorGetCode()
                );
            } else {
                println!("Incoming audio playback started");
            }
        }

        println!("\n--- Running (Ctrl+C to stop) ---\n");
        println!("Waiting for Z/IP ONE to connect on port {}...\n", local_port);

        // Monitor loop
        let start_time = std::time::Instant::now();
        let mut stats = RtpOutputStatsFFI::default();
        let mut last_rx = 0u64;
        let mut last_tx = 0u64;

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

            // Calculate packets per second
            let rx_pps = (stats.rx_packets - last_rx) * 2; // 500ms intervals
            let tx_pps = (stats.tx_packets - last_tx) * 2;
            last_rx = stats.rx_packets;
            last_tx = stats.tx_packets;

            // PPM display
            let ppm = stats.current_ppm_x1000 as f32 / 1000.0;

            // Connection state
            let conn_str = if CONNECTION_STATE.load(Ordering::SeqCst) { "CONN" } else { "----" };

            // Print status line
            print!(
                "\r\x1b[K[{:02}:{:02}] {} {} RX:{:6}({:3}pps) TX:{:6}({:4}pps) Buf:{:5} Drop:{} [{}][{}] In:{} PPM:{:+.1}",
                mins,
                secs,
                conn_str,
                state_str,
                stats.rx_packets,
                rx_pps,
                stats.tx_packets,
                tx_pps,
                stats.buffer_level,
                stats.rx_dropped,
                left_meter,
                right_meter,
                payload_type_name(stats.detected_incoming_pt),
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
