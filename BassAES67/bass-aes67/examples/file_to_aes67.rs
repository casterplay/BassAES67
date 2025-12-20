//! File to AES67 output test - plays a local file and transmits as AES67.
//!
//! Usage: cargo run --example file_to_aes67
//!
//! This isolates the AES67 OUTPUT for testing without AES67 INPUT.
//! Source: Local MP3 file played through BASS
//! Output: AES67 multicast to 239.192.1.100:5004

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

const FALSE: BOOL = 0;

// BASS functions
#[link(name = "bass")]
extern "system" {
    fn BASS_Init(device: i32, freq: DWORD, flags: DWORD, win: *mut c_void, dsguid: *const c_void) -> BOOL;
    fn BASS_Free() -> BOOL;
    fn BASS_GetVersion() -> DWORD;
    fn BASS_ErrorGetCode() -> i32;
    fn BASS_StreamCreateFile(mem: BOOL, file: *const c_void, offset: u64, length: u64, flags: DWORD) -> HSTREAM;
    fn BASS_ChannelIsActive(handle: DWORD) -> DWORD;
    fn BASS_StreamFree(handle: HSTREAM) -> BOOL;
    fn BASS_ChannelGetLength(handle: DWORD, mode: DWORD) -> u64;
    fn BASS_ChannelGetPosition(handle: DWORD, mode: DWORD) -> u64;
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
const BASS_POS_BYTE: DWORD = 0;

// Channel states
const BASS_ACTIVE_STOPPED: DWORD = 0;

// Global running flag for clean shutdown
static RUNNING: AtomicBool = AtomicBool::new(true);

fn main() {
    println!("BASS File to AES67 Output Test");
    println!("================================\n");
    println!("This example plays a local file and transmits via AES67.");
    println!("Used to test AES67 OUTPUT independently of AES67 INPUT.\n");

    // Install Ctrl+C handler
    ctrlc_handler();

    // Test file path
    let file_path = r"F:\Audio\GlobalNewsPodcast-20251215.mp3";

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

        // Start PTP client (for clock sync with Livewire network)
        let ptp_interface = CString::new("192.168.60.102").unwrap();
        let ptp_result = (ptp.start)(ptp_interface.as_ptr(), 1);  // Domain 1 for Livewire
        if ptp_result != 0 {
            println!("WARNING: PTP start returned {}", ptp_result);
        }
        println!("  PTP client started (domain=1)");

        // Wait for PTP to lock
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

        // Create file stream (decode mode - we pull samples manually)
        println!("\nCreating file stream...");
        let file_cstring = CString::new(file_path).unwrap();
        let input_stream = BASS_StreamCreateFile(
            FALSE,
            file_cstring.as_ptr() as *const c_void,
            0,
            0,
            BASS_STREAM_DECODE,  // Decode mode - we pull samples manually
        );

        if input_stream == 0 {
            println!("ERROR: Failed to create file stream (error {})", BASS_ErrorGetCode());
            println!("  File: {}", file_path);
            (ptp.stop)();
            BASS_Free();
            return;
        }

        let length = BASS_ChannelGetLength(input_stream, BASS_POS_BYTE);
        println!("  File stream created (handle: {}, length: {} bytes)", input_stream, length);
        println!("  Source: {}", file_path);

        // Create AES67 OUTPUT stream
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
                BASS_Free();
                return;
            }
        };

        // Start output transmission
        if let Err(e) = output_stream.start() {
            println!("ERROR: Failed to start output stream: {}", e);
            BASS_StreamFree(input_stream);
            (ptp.stop)();
            BASS_Free();
            return;
        }
        println!("  Output stream started");

        println!("\n==========================================");
        println!("File to AES67 running:");
        println!("  INPUT:  {} (local file)", file_path);
        println!("  OUTPUT: 239.192.1.100:5004 (200 pkt/sec)");
        println!("==========================================");
        println!("Press Ctrl+C to stop\n");

        // Monitor loop
        while RUNNING.load(Ordering::SeqCst) {
            let state = BASS_ChannelIsActive(input_stream);
            if state == BASS_ACTIVE_STOPPED {
                println!("\nFile playback ended");
                break;
            }

            // Get PTP stats
            let mut ptp_buffer = [0i8; 256];
            (ptp.get_stats_string)(ptp_buffer.as_mut_ptr(), 256);
            let ptp_stats = std::ffi::CStr::from_ptr(ptp_buffer.as_ptr())
                .to_string_lossy()
                .into_owned();

            // Get file position
            let position = BASS_ChannelGetPosition(input_stream, BASS_POS_BYTE);
            let progress = if length > 0 { (position * 100 / length) as u32 } else { 0 };

            // Get output stats
            let output_stats = output_stream.stats();
            let applied_ppm = output_stream.applied_ppm();

            // Display status
            print!("\r\x1b[K");
            print!("Progress: {:3}% | OutPkt: {:6} | Applied: {:+.2}ppm | {}",
                progress,
                output_stats.packets_sent,
                applied_ppm,
                ptp_stats);
            use std::io::Write;
            std::io::stdout().flush().unwrap();

            thread::sleep(Duration::from_millis(500));
        }

        // Cleanup
        println!("\n\nCleaning up...");
        output_stream.stop();
        BASS_StreamFree(input_stream);
        (ptp.stop)();
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
