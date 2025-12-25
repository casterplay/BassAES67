//! WebRTC MediaMTX Test
//!
//! Tests bass-webrtc with MediaMTX server:
//! - WHIP: Push audio TO MediaMTX (browser can receive via WHEP)
//! - WHEP: Pull audio FROM MediaMTX (browser sends via WHIP)
//!
//! Usage:
//!   # Start MediaMTX first, then:
//!   cargo run --release --example webrtc_mediamtx_test -- --whip http://localhost:8889/mystream/whip
//!   cargo run --release --example webrtc_mediamtx_test -- --whep http://localhost:8889/mystream/whep
//!   cargo run --release --example webrtc_mediamtx_test -- --whip http://localhost:8889/out/whip --whep http://localhost:8889/in/whep

use std::ffi::c_void;
use std::io::Write;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// Use the library directly
use bass_webrtc::{
    BASS_WEBRTC_ConnectWhip, BASS_WEBRTC_ConnectWhep,
    BASS_WEBRTC_WhipStart, BASS_WEBRTC_WhipStop, BASS_WEBRTC_WhipFree,
    BASS_WEBRTC_WhepGetStream, BASS_WEBRTC_WhepFree,
};

// BASS FFI types
type DWORD = u32;
type BOOL = i32;
type HSTREAM = DWORD;

const FALSE: BOOL = 0;
const BASS_SAMPLE_FLOAT: DWORD = 0x100;
const BASS_STREAM_DECODE: DWORD = 0x200000;

#[link(name = "bass")]
extern "system" {
    fn BASS_Init(device: i32, freq: DWORD, flags: DWORD, win: *mut c_void, dsguid: *const c_void) -> BOOL;
    fn BASS_Free() -> BOOL;
    fn BASS_ErrorGetCode() -> i32;
    fn BASS_StreamCreate(freq: DWORD, chans: DWORD, flags: DWORD, proc: Option<StreamProc>, user: *mut c_void) -> HSTREAM;
    fn BASS_StreamFree(handle: HSTREAM) -> BOOL;
    fn BASS_ChannelPlay(handle: DWORD, restart: BOOL) -> BOOL;
    fn BASS_ChannelStop(handle: DWORD) -> BOOL;
}

type StreamProc = unsafe extern "system" fn(HSTREAM, *mut c_void, DWORD, *mut c_void) -> DWORD;

// Tone generator for test audio
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

    fn generate(&mut self, buffer: &mut [f32]) {
        for chunk in buffer.chunks_mut(2) {
            let sample = self.phase.sin() * self.amplitude;
            chunk[0] = sample;
            if chunk.len() > 1 {
                chunk[1] = sample;
            }
            self.phase += self.phase_increment;
            if self.phase > 2.0 * std::f32::consts::PI {
                self.phase -= 2.0 * std::f32::consts::PI;
            }
        }
    }
}

static mut TONE_GEN: Option<ToneGenerator> = None;

unsafe extern "system" fn tone_stream_proc(
    _handle: HSTREAM,
    buffer: *mut c_void,
    length: DWORD,
    _user: *mut c_void,
) -> DWORD {
    if let Some(ref mut gen) = TONE_GEN {
        let samples = length as usize / 4;
        let slice = std::slice::from_raw_parts_mut(buffer as *mut f32, samples);
        gen.generate(slice);
    }
    length
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut whip_url: Option<String> = None;
    let mut whep_url: Option<String> = None;

    // Parse arguments
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--whip" if i + 1 < args.len() => {
                whip_url = Some(args[i + 1].clone());
                i += 1;
            }
            "--whep" if i + 1 < args.len() => {
                whep_url = Some(args[i + 1].clone());
                i += 1;
            }
            "--help" | "-h" => {
                println!("WebRTC MediaMTX Test");
                println!();
                println!("Usage:");
                println!("  webrtc_mediamtx_test --whip <url>         Push audio to MediaMTX");
                println!("  webrtc_mediamtx_test --whep <url>         Pull audio from MediaMTX");
                println!("  webrtc_mediamtx_test --whip <url> --whep <url>  Both directions");
                println!();
                println!("Examples:");
                println!("  webrtc_mediamtx_test --whip http://localhost:8889/mystream/whip");
                println!("  webrtc_mediamtx_test --whep http://localhost:8889/mystream/whep");
                return;
            }
            _ => {}
        }
        i += 1;
    }

    if whip_url.is_none() && whep_url.is_none() {
        println!("Error: Must specify at least --whip or --whep URL");
        println!("Use --help for usage information");
        return;
    }

    println!("=================================");
    println!("  WebRTC MediaMTX Test");
    println!("=================================");
    println!();

    // Setup Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        println!("\nReceived Ctrl+C, stopping...");
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl-C handler");

    unsafe {
        // Initialize BASS
        if BASS_Init(-1, 48000, 0, ptr::null_mut(), ptr::null()) == FALSE {
            println!("ERROR: Failed to initialize BASS (error: {})", BASS_ErrorGetCode());
            return;
        }
        println!("[OK] BASS initialized");

        let mut whip_handle: *mut c_void = ptr::null_mut();
        let mut whep_handle: *mut c_void = ptr::null_mut();
        let mut tone_stream: HSTREAM = 0;
        let mut input_stream: HSTREAM = 0;

        // Setup WHIP (push audio to MediaMTX)
        if let Some(ref url) = whip_url {
            println!();
            println!("WHIP Mode: Pushing audio to {}", url);

            // Create tone generator
            TONE_GEN = Some(ToneGenerator::new(440.0, 48000.0, 0.5));
            tone_stream = BASS_StreamCreate(
                48000,
                2,
                BASS_SAMPLE_FLOAT | BASS_STREAM_DECODE,
                Some(tone_stream_proc),
                ptr::null_mut(),
            );

            if tone_stream == 0 {
                println!("ERROR: Failed to create tone stream");
                BASS_Free();
                return;
            }
            println!("[OK] Created 440Hz test tone");

            // Connect to WHIP
            let url_cstr = std::ffi::CString::new(url.as_str()).unwrap();
            whip_handle = BASS_WEBRTC_ConnectWhip(
                tone_stream,
                url_cstr.as_ptr(),
                48000,
                2,
                128, // 128 kbps
            );

            if whip_handle.is_null() {
                println!("ERROR: Failed to connect to WHIP endpoint");
                BASS_StreamFree(tone_stream);
                BASS_Free();
                return;
            }
            println!("[OK] Connected to WHIP server");

            // Start streaming
            if BASS_WEBRTC_WhipStart(whip_handle) == 0 {
                println!("ERROR: Failed to start WHIP streaming");
                BASS_WEBRTC_WhipFree(whip_handle);
                BASS_StreamFree(tone_stream);
                BASS_Free();
                return;
            }
            println!("[OK] Started WHIP streaming (sending 440Hz tone)");
        }

        // Setup WHEP (pull audio from MediaMTX)
        if let Some(ref url) = whep_url {
            println!();
            println!("WHEP Mode: Pulling audio from {}", url);

            let url_cstr = std::ffi::CString::new(url.as_str()).unwrap();
            whep_handle = BASS_WEBRTC_ConnectWhep(
                url_cstr.as_ptr(),
                48000,
                2,
                100, // 100ms buffer
                0,   // not decode-only (will play)
            );

            if whep_handle.is_null() {
                println!("ERROR: Failed to connect to WHEP endpoint");
                if !whip_handle.is_null() {
                    BASS_WEBRTC_WhipStop(whip_handle);
                    BASS_WEBRTC_WhipFree(whip_handle);
                }
                if tone_stream != 0 {
                    BASS_StreamFree(tone_stream);
                }
                BASS_Free();
                return;
            }
            println!("[OK] Connected to WHEP server");

            // Get and play input stream
            input_stream = BASS_WEBRTC_WhepGetStream(whep_handle);
            if input_stream != 0 {
                BASS_ChannelPlay(input_stream, FALSE);
                println!("[OK] Playing received audio");
            }
        }

        println!();
        println!("--- Running (Ctrl+C to stop) ---");
        println!();

        // Monitor loop
        let mut frame_count: u64 = 0;
        while running.load(Ordering::SeqCst) {
            frame_count += 1;

            // Print status every second
            if frame_count % 10 == 0 {
                let mut status = String::new();

                if whip_url.is_some() {
                    status.push_str("WHIP: sending");
                }

                if whep_url.is_some() {
                    if !status.is_empty() {
                        status.push_str(" | ");
                    }
                    status.push_str("WHEP: receiving");
                }

                print!("\r[{}s] {}   ", frame_count / 10, status);
                std::io::stdout().flush().unwrap();
            }

            thread::sleep(Duration::from_millis(100));
        }

        println!();
        println!();
        println!("Cleaning up...");

        // Cleanup WHIP
        if !whip_handle.is_null() {
            BASS_WEBRTC_WhipStop(whip_handle);
            BASS_WEBRTC_WhipFree(whip_handle);
            println!("[OK] WHIP disconnected");
        }

        // Cleanup WHEP
        if !whep_handle.is_null() {
            if input_stream != 0 {
                BASS_ChannelStop(input_stream);
            }
            BASS_WEBRTC_WhepFree(whep_handle);
            println!("[OK] WHEP disconnected");
        }

        // Cleanup BASS
        if tone_stream != 0 {
            BASS_StreamFree(tone_stream);
        }
        BASS_Free();
        println!("[OK] BASS freed");

        println!();
        println!("Done!");
    }
}
