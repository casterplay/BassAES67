//! File to Speakers Stereo Enhancer Test
//!
//! Usage: cargo run --example file_to_speakers_stereo_enhancer --release
//!
//! This tests the multiband Stereo Enhancer (Omnia 9 style) added in Phase 3.2.
//! The enhancer uses Mid-Side processing to control stereo width per frequency band.
//!
//! Key features:
//! - Band 0 (bass) is always bypassed to avoid phase issues
//! - Per-band stereo width control with target width ratios
//! - Attack/release dynamics for smooth transitions
//!
//! Source: Local MP3 file
//! Output: Default audio device (speakers)
//!
//! Features demonstrated:
//! - Toggle stereo enhancer on/off to hear the difference
//! - Comparison of processed vs bypassed stereo image

use std::ffi::{c_void, CString};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

// Use the library directly
use bass_broadcast_processor::{
    AgcConfig, CompressorConfig, MultibandConfigHeader, MultibandStatsHeader,
    StereoEnhancerBandConfig, StereoEnhancerConfig, AGC_MODE_SINGLE,
};

// FFI imports for multiband processor
extern "system" {
    fn BASS_MultibandProcessor_Create(
        source: u32,
        header: *const MultibandConfigHeader,
        crossover_freqs: *const f32,
        bands: *const CompressorConfig,
    ) -> *mut c_void;
    fn BASS_MultibandProcessor_GetOutput(handle: *mut c_void) -> u32;
    fn BASS_MultibandProcessor_GetStats(
        handle: *mut c_void,
        header_out: *mut MultibandStatsHeader,
        band_gr_out: *mut f32,
    ) -> i32;
    fn BASS_MultibandProcessor_SetAGC(handle: *mut c_void, config: *const AgcConfig) -> i32;
    fn BASS_MultibandProcessor_SetStereoEnhancer(
        handle: *mut c_void,
        config: *const StereoEnhancerConfig,
    ) -> i32;
    fn BASS_MultibandProcessor_SetStereoEnhancerEnabled(
        handle: *mut c_void,
        enabled: i32,
    ) -> i32;
    fn BASS_MultibandProcessor_IsStereoEnhancerEnabled(handle: *mut c_void) -> i32;
    fn BASS_MultibandProcessor_Prefill(handle: *mut c_void) -> i32;
    fn BASS_MultibandProcessor_Free(handle: *mut c_void) -> i32;
}

// BASS types
type DWORD = u32;
type BOOL = i32;
type HSTREAM = DWORD;

const FALSE: BOOL = 0;
const TRUE: BOOL = 1;

// BASS flags
const BASS_SAMPLE_FLOAT: DWORD = 0x100;
const BASS_STREAM_DECODE: DWORD = 0x200000;
const BASS_POS_BYTE: DWORD = 0;

// Channel states
const BASS_ACTIVE_STOPPED: DWORD = 0;

// BASS functions
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
    fn BASS_StreamCreateFile(
        mem: BOOL,
        file: *const c_void,
        offset: u64,
        length: u64,
        flags: DWORD,
    ) -> HSTREAM;
    fn BASS_ChannelPlay(handle: DWORD, restart: BOOL) -> BOOL;
    fn BASS_ChannelIsActive(handle: DWORD) -> DWORD;
    fn BASS_StreamFree(handle: HSTREAM) -> BOOL;
    fn BASS_ChannelGetLength(handle: DWORD, mode: DWORD) -> u64;
}

// Global running flag for clean shutdown
static RUNNING: AtomicBool = AtomicBool::new(true);

fn bass_error_string(code: i32) -> &'static str {
    match code {
        0 => "OK",
        1 => "MEM",
        2 => "FILEOPEN",
        3 => "DRIVER",
        4 => "BUFLOST",
        5 => "HANDLE",
        6 => "FORMAT",
        7 => "POSITION",
        8 => "INIT",
        9 => "START",
        14 => "ALREADY",
        18 => "NOTAUDIO",
        20 => "NOCHAN",
        21 => "ILLTYPE",
        22 => "ILLPARAM",
        23 => "NO3D",
        24 => "NOEAX",
        25 => "DEVICE",
        27 => "NOPLAY",
        29 => "FREQ",
        31 => "NOTFILE",
        32 => "NOHW",
        33 => "EMPTY",
        34 => "NONET",
        35 => "CREATE",
        36 => "NOFX",
        37 => "NOTAVAIL",
        38 => "DECODE",
        39 => "DX",
        40 => "TIMEOUT",
        41 => "FILEFORM",
        42 => "SPEAKER",
        43 => "VERSION",
        44 => "CODEC",
        45 => "ENDED",
        46 => "BUSY",
        47 => "UNSTREAMABLE",
        -1 => "UNKNOWN",
        _ => "?",
    }
}

fn main() {
    println!("=============================================================");
    println!("  BASS Broadcast Processor - Phase 3.2: Stereo Enhancer Test");
    println!("=============================================================\n");

    // Install Ctrl+C handler
    ctrlc_handler();

    // Test file path - adjust this to your audio file (preferably stereo content!)
    let file_path = r"F:\Audio\GlobalNewsPodcast-20251215.mp3";

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

        // Initialize BASS with default output device
        println!("\nInitializing BASS (default device)...");
        if BASS_Init(-1, 48000, 0, ptr::null_mut(), ptr::null()) == FALSE {
            let err = BASS_ErrorGetCode();
            println!(
                "ERROR: Failed to initialize BASS (error {} = {})",
                err,
                bass_error_string(err)
            );
            return;
        }
        println!("  BASS initialized");

        // Create file stream in DECODE mode
        println!("\nCreating file stream...");
        let file_cstring = CString::new(file_path).unwrap();
        let input_stream = BASS_StreamCreateFile(
            FALSE,
            file_cstring.as_ptr() as *const c_void,
            0,
            0,
            BASS_SAMPLE_FLOAT | BASS_STREAM_DECODE,
        );

        let err = BASS_ErrorGetCode();
        if input_stream == 0 {
            println!(
                "ERROR: Failed to create file stream (error {} = {})",
                err,
                bass_error_string(err)
            );
            println!("  File: {}", file_path);
            BASS_Free();
            return;
        }

        let length = BASS_ChannelGetLength(input_stream, BASS_POS_BYTE);
        println!("  Source: {}", file_path);
        println!(
            "  Length: {} bytes ({:.1} MB)",
            length,
            length as f64 / 1_000_000.0
        );

        // Create 5-band multiband processor config
        let header = MultibandConfigHeader {
            sample_rate: 48000,
            channels: 2,
            num_bands: 5,
            decode_output: 0, // Playable output
            _pad: [0; 3],
            input_gain_db: 0.0,
            output_gain_db: 0.0,
        };

        // Crossover frequencies: 100, 400, 2000, 8000 Hz
        let crossover_freqs: [f32; 4] = [100.0, 400.0, 2000.0, 8000.0];

        // Band compressor configs (moderate settings)
        let bands: [CompressorConfig; 5] = [
            // Sub-bass (< 100 Hz)
            CompressorConfig {
                threshold_db: -20.0,
                ratio: 3.0,
                attack_ms: 10.0,
                release_ms: 200.0,
                makeup_gain_db: 2.0,
            lookahead_ms: 0.0,
            },
            // Bass (100 - 400 Hz)
            CompressorConfig {
                threshold_db: -18.0,
                ratio: 3.0,
                attack_ms: 5.0,
                release_ms: 150.0,
                makeup_gain_db: 2.0,
            lookahead_ms: 0.0,
            },
            // Midrange (400 - 2000 Hz)
            CompressorConfig {
                threshold_db: -16.0,
                ratio: 2.5,
                attack_ms: 3.0,
                release_ms: 100.0,
                makeup_gain_db: 2.0,
            lookahead_ms: 0.0,
            },
            // Presence (2000 - 8000 Hz)
            CompressorConfig {
                threshold_db: -14.0,
                ratio: 3.0,
                attack_ms: 1.0,
                release_ms: 80.0,
                makeup_gain_db: 2.0,
            lookahead_ms: 0.0,
            },
            // Brilliance (> 8000 Hz)
            CompressorConfig {
                threshold_db: -12.0,
                ratio: 3.0,
                attack_ms: 0.5,
                release_ms: 50.0,
                makeup_gain_db: 1.0,
            lookahead_ms: 0.0,
            },
        ];

        // Create multiband processor
        println!("\nCreating 5-band multiband processor...");
        let processor = BASS_MultibandProcessor_Create(
            input_stream,
            &header,
            crossover_freqs.as_ptr(),
            bands.as_ptr(),
        );
        let err = BASS_ErrorGetCode();
        if processor.is_null() {
            println!(
                "ERROR: Failed to create processor (error {} = {})",
                err,
                bass_error_string(err)
            );
            BASS_StreamFree(input_stream);
            BASS_Free();
            return;
        }
        println!("  Processor created successfully");

        // Get output stream
        let output_stream = BASS_MultibandProcessor_GetOutput(processor);
        if output_stream == 0 {
            let err = BASS_ErrorGetCode();
            println!(
                "ERROR: Failed to get output stream (error {} = {})",
                err,
                bass_error_string(err)
            );
            BASS_MultibandProcessor_Free(processor);
            BASS_StreamFree(input_stream);
            BASS_Free();
            return;
        }

        // Enable AGC
        let agc_config = AgcConfig {
            target_level_db: -18.0,
            threshold_db: -24.0,
            ratio: 3.0,
            knee_db: 10.0,
            attack_ms: 50.0,
            release_ms: 500.0,
            enabled: 1,
            mode: AGC_MODE_SINGLE,
            _pad: [0; 2],
        };
        BASS_MultibandProcessor_SetAGC(processor, &agc_config);

        // Create stereo enhancer config with progressive enhancement
        let stereo_config = StereoEnhancerConfig {
            enabled: 1,
            _pad: [0; 3],
            bands: [
                // Band 0 (Bass): Always bypassed internally
                StereoEnhancerBandConfig {
                    target_width: 1.0,
                    max_gain_db: 0.0,
                    max_atten_db: 0.0,
                    attack_ms: 50.0,
                    release_ms: 200.0,
                    enabled: 0, // Always off for bass
                    _pad: [0; 3],
                },
                // Band 1 (Low-Mid): Gentle enhancement
                StereoEnhancerBandConfig {
                    target_width: 1.1,
                    max_gain_db: 6.0,
                    max_atten_db: 6.0,
                    attack_ms: 50.0,
                    release_ms: 200.0,
                    enabled: 1,
                    _pad: [0; 3],
                },
                // Band 2 (Mid): Moderate enhancement
                StereoEnhancerBandConfig {
                    target_width: 1.3,
                    max_gain_db: 9.0,
                    max_atten_db: 9.0,
                    attack_ms: 30.0,
                    release_ms: 150.0,
                    enabled: 1,
                    _pad: [0; 3],
                },
                // Band 3 (Presence): More enhancement
                StereoEnhancerBandConfig {
                    target_width: 1.5,
                    max_gain_db: 12.0,
                    max_atten_db: 12.0,
                    attack_ms: 20.0,
                    release_ms: 100.0,
                    enabled: 1,
                    _pad: [0; 3],
                },
                // Band 4 (Brilliance): Maximum enhancement
                StereoEnhancerBandConfig {
                    target_width: 1.6,
                    max_gain_db: 12.0,
                    max_atten_db: 12.0,
                    attack_ms: 15.0,
                    release_ms: 80.0,
                    enabled: 1,
                    _pad: [0; 3],
                },
            ],
        };
        BASS_MultibandProcessor_SetStereoEnhancer(processor, &stereo_config);

        // Print stereo enhancer settings
        println!("\nStereo Enhancer Configuration (Omnia 9 style):");
        println!("  Band 0 (Sub-bass):   BYPASSED - Avoids phase issues");
        println!("  Band 1 (Low-Mid):    Width=1.1 MaxGain=6dB  - Gentle");
        println!("  Band 2 (Mid):        Width=1.3 MaxGain=9dB  - Moderate");
        println!("  Band 3 (Presence):   Width=1.5 MaxGain=12dB - Enhanced");
        println!("  Band 4 (Brilliance): Width=1.6 MaxGain=12dB - Maximum");

        // Pre-fill and start playback
        BASS_MultibandProcessor_Prefill(processor);

        println!("\nStarting playback...");
        let play_result = BASS_ChannelPlay(output_stream, TRUE);
        if play_result == FALSE {
            let err = BASS_ErrorGetCode();
            println!(
                "ERROR: Failed to start playback (error {} = {})",
                err,
                bass_error_string(err)
            );
            BASS_MultibandProcessor_Free(processor);
            BASS_StreamFree(input_stream);
            BASS_Free();
            return;
        }

        println!("\n=============================================================");
        println!("  Stereo Enhancer Demo Running!");
        println!("=============================================================");
        println!("  INPUT:  {}", file_path);
        println!("  OUTPUT: Default speakers");
        println!("");
        println!("  Mode: Toggling Stereo Enhancer ON/OFF every 10 seconds");
        println!("");
        println!("  ENHANCED: Wider stereo image, especially in higher bands");
        println!("  BYPASSED: Original stereo width preserved");
        println!("");
        println!("  Listen for the difference in stereo separation!");
        println!("=============================================================");
        println!("Press Ctrl+C to stop\n");
        use std::io::Write;
        std::io::stdout().flush().unwrap();

        // Monitor loop
        let mut stats = MultibandStatsHeader::default();
        let mut band_gr = [0.0f32; 5];
        let mut loop_count = 0u32;
        let start_time = std::time::Instant::now();
        let mut enhancer_enabled = true;

        while RUNNING.load(Ordering::SeqCst) {
            let state = BASS_ChannelIsActive(output_stream);
            if state == BASS_ACTIVE_STOPPED {
                println!("\nPlayback ended");
                break;
            }

            // Toggle stereo enhancer every 10 seconds
            let elapsed_secs = start_time.elapsed().as_secs();
            let should_enable = (elapsed_secs / 10) % 2 == 0;
            if should_enable != enhancer_enabled {
                enhancer_enabled = should_enable;
                BASS_MultibandProcessor_SetStereoEnhancerEnabled(
                    processor,
                    if enhancer_enabled { TRUE } else { FALSE },
                );
                if enhancer_enabled {
                    println!("\n  >>> STEREO ENHANCER ON - Wider stereo image <<<\n");
                } else {
                    println!("\n  >>> STEREO ENHANCER OFF - Original width <<<\n");
                }
                std::io::stdout().flush().unwrap();
            }

            // Get processor stats
            BASS_MultibandProcessor_GetStats(processor, &mut stats, band_gr.as_mut_ptr());

            // Display status every 5 loops (500ms)
            loop_count += 1;
            if loop_count % 5 == 1 {
                let process_time_ms = stats.process_time_us as f64 / 1000.0;
                let mode_str = if BASS_MultibandProcessor_IsStereoEnhancerEnabled(processor) != 0 {
                    "ENHANCED"
                } else {
                    "BYPASSED"
                };

                println!(
                    "[{}] In:{:5.3} Out:{:5.3} | AGC:{:+5.1}dB | Bands: {:+4.1} {:+4.1} {:+4.1} {:+4.1} {:+4.1}dB | {:4.2}ms",
                    mode_str,
                    stats.input_peak,
                    stats.output_peak,
                    stats.agc_gr_db,
                    band_gr[0], band_gr[1], band_gr[2], band_gr[3], band_gr[4],
                    process_time_ms,
                );
                std::io::stdout().flush().unwrap();
            }

            thread::sleep(Duration::from_millis(100));
        }

        // Cleanup
        println!("\n\nCleaning up...");
        BASS_MultibandProcessor_Free(processor);
        BASS_StreamFree(input_stream);
        BASS_Free();

        // Final stats
        println!("\nFinal Statistics:");
        println!("  Samples processed: {}", stats.samples_processed);
        println!(
            "  Peak levels: In={:.3} Out={:.3}",
            stats.input_peak, stats.output_peak
        );
        println!("  AGC Gain Reduction: {:.1} dB", stats.agc_gr_db);
        println!(
            "  Band GR (dB): Sub={:.1} Bas={:.1} Mid={:.1} Pre={:.1} Bri={:.1}",
            band_gr[0], band_gr[1], band_gr[2], band_gr[3], band_gr[4]
        );
        println!("  Underruns: {}", stats.underruns);
    }

    println!("\nDone!");
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
