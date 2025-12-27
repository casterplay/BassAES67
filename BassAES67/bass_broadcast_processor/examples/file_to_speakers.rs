//! File to Speakers test - plays a local file through the broadcast processor.
//!
//! Usage: cargo run --example file_to_speakers --release
//!
//! This tests the broadcast processor with direct speaker output.
//! Source: Local MP3 file
//! Output: Default audio device (speakers)

use std::ffi::{c_void, CString};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

// Use the library directly
use bass_broadcast_processor::{
    BASS_Processor_Create, BASS_Processor_Free, BASS_Processor_GetOutput, BASS_Processor_GetStats,
    BASS_Processor_Prefill, BASS_Processor_SetBypass, CompressorConfig, ProcessorConfig, ProcessorStats,
};

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
const BASS_ACTIVE_PLAYING: DWORD = 1;
const BASS_ACTIVE_STALLED: DWORD = 2;
const BASS_ACTIVE_PAUSED: DWORD = 3;
const BASS_ACTIVE_PAUSED_DEVICE: DWORD = 4;

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
    fn BASS_ChannelGetPosition(handle: DWORD, mode: DWORD) -> u64;
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

fn channel_state_string(state: DWORD) -> &'static str {
    match state {
        BASS_ACTIVE_STOPPED => "STOPPED",
        BASS_ACTIVE_PLAYING => "PLAYING",
        BASS_ACTIVE_STALLED => "STALLED",
        BASS_ACTIVE_PAUSED => "PAUSED",
        BASS_ACTIVE_PAUSED_DEVICE => "PAUSED_DEVICE",
        _ => "UNKNOWN",
    }
}

fn main() {
    println!("BASS Broadcast Processor - File to Speakers Test");
    println!("=================================================\n");

    // Install Ctrl+C handler
    ctrlc_handler();

    // Test file path
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
        println!("  BASS initialized (device=-1, default output)");

        // Create file stream in DECODE mode (processor will pull samples)
        println!("\nCreating file stream (decode mode)...");
        let file_cstring = CString::new(file_path).unwrap();
        let input_stream = BASS_StreamCreateFile(
            FALSE,
            file_cstring.as_ptr() as *const c_void,
            0,
            0,
            BASS_SAMPLE_FLOAT | BASS_STREAM_DECODE, // Decode mode - processor pulls samples
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
        println!(
            "  File stream created (handle: {}, error: {} = {})",
            input_stream,
            err,
            bass_error_string(err)
        );

        let length = BASS_ChannelGetLength(input_stream, BASS_POS_BYTE);
        println!("  Source: {}", file_path);
        println!("  Length: {} bytes", length);

        // Create processor config - AGGRESSIVE PODCAST PRESET
        // This preset is intentionally aggressive so you can HEAR the processing
        let config = ProcessorConfig {
            sample_rate: 48000,
            channels: 2,
            block_size: 256,
            decode_output: 0, // NOT decode mode - playable output!
            _pad: 0,
            input_gain_db: 6.0,   // Boost input to hit compressors harder
            output_gain_db: -3.0, // Pull back output to avoid clipping
            crossover_freq: 800.0, // Higher crossover for voice (splits bass from voice)
            low_band: CompressorConfig {
                threshold_db: -24.0,  // Low threshold - compress almost everything
                ratio: 8.0,           // Heavy compression on bass
                attack_ms: 20.0,      // Slower attack to let transients through
                release_ms: 150.0,    // Moderate release
                makeup_gain_db: 6.0,  // Boost compressed signal back up
                lookahead_ms: 0.0,
            },
            high_band: CompressorConfig {
                threshold_db: -20.0,  // Aggressive threshold for voice
                ratio: 6.0,           // Heavy compression on voice
                attack_ms: 5.0,       // Fast attack for voice
                release_ms: 100.0,    // Quick release for natural speech
                makeup_gain_db: 8.0,  // Significant makeup gain
                lookahead_ms: 0.0,
            },
        };

        // Create processor
        println!("\nCreating broadcast processor...");
        let processor = BASS_Processor_Create(input_stream, &config);
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
        println!(
            "  Processor created (error: {} = {})",
            err,
            bass_error_string(err)
        );

        // Get output stream
        let output_stream = BASS_Processor_GetOutput(processor);
        let err = BASS_ErrorGetCode();
        if output_stream == 0 {
            println!(
                "ERROR: Failed to get output stream (error {} = {})",
                err,
                bass_error_string(err)
            );
            BASS_Processor_Free(processor);
            BASS_StreamFree(input_stream);
            BASS_Free();
            return;
        }
        println!(
            "  Output stream: {} (error: {} = {})",
            output_stream,
            err,
            bass_error_string(err)
        );

        // Pre-fill the processor buffer before starting playback
        println!("\nPre-filling processor buffer...");
        BASS_Processor_Prefill(processor);
        println!("  Buffer pre-filled");

        // Start playback
        println!("\nStarting playback...");
        use std::io::Write;
        std::io::stdout().flush().unwrap();

        let play_result = BASS_ChannelPlay(output_stream, TRUE);
        let err = BASS_ErrorGetCode();
        if play_result == FALSE {
            println!(
                "ERROR: Failed to start playback (error {} = {})",
                err,
                bass_error_string(err)
            );
            BASS_Processor_Free(processor);
            BASS_StreamFree(input_stream);
            BASS_Free();
            return;
        }
        println!(
            "  Playback started (error: {} = {})",
            err,
            bass_error_string(err)
        );
        std::io::stdout().flush().unwrap();

        println!("\n==========================================");
        println!("Broadcast Processor Test Running:");
        println!("  INPUT:  {} (local file)", file_path);
        println!("  OUTPUT: Default speakers");
        println!("  Crossover: {} Hz", config.crossover_freq);
        println!("  Low band:  {}dB thresh, {}:1 ratio", config.low_band.threshold_db, config.low_band.ratio);
        println!("  High band: {}dB thresh, {}:1 ratio", config.high_band.threshold_db, config.high_band.ratio);
        println!("  Mode: Toggling BYPASS every 10 seconds");
        println!("==========================================");
        println!("Press Ctrl+C to stop\n");
        std::io::stdout().flush().unwrap();

        // Monitor loop
        let mut stats = ProcessorStats::default();
        let mut loop_count = 0u32;
        let mut bypass_on = false;
        let start_time = std::time::Instant::now();

        while RUNNING.load(Ordering::SeqCst) {
            let state = BASS_ChannelIsActive(output_stream);
            if state == BASS_ACTIVE_STOPPED {
                println!("\nPlayback ended");
                break;
            }

            // Toggle bypass every 10 seconds
            let elapsed_secs = start_time.elapsed().as_secs();
            let should_bypass = (elapsed_secs / 10) % 2 == 1;
            if should_bypass != bypass_on {
                bypass_on = should_bypass;
                BASS_Processor_SetBypass(processor, if bypass_on { TRUE } else { FALSE });
                if bypass_on {
                    println!("\n>>> BYPASS ON (unprocessed audio) <<<\n");
                } else {
                    println!("\n>>> PROCESSING ON (compressed audio) <<<\n");
                }
            }

            // Get position
            let position = BASS_ChannelGetPosition(output_stream, BASS_POS_BYTE);
            let progress = if length > 0 {
                (position * 100 / length) as u32
            } else {
                0
            };

            // Get processor stats
            BASS_Processor_GetStats(processor, &mut stats);

            // Display status every 10 loops (1 second) for cleaner batch output
            loop_count += 1;
            if loop_count % 10 == 1 {
                let mode_str = if bypass_on { "BYPASS" } else { "PROCESS" };
                // Convert microseconds to milliseconds for display
                let process_time_ms = stats.process_time_us as f64 / 1000.0;
                println!(
                    "[{:7}] InPk: {:5.3} | OutPk: {:5.3} | LowGR: {:+5.1}dB | HighGR: {:+5.1}dB | Time: {:5.2}ms",
                    mode_str,
                    stats.input_peak,
                    stats.output_peak,
                    stats.low_band_gr_db,
                    stats.high_band_gr_db,
                    process_time_ms,
                );
            }

            thread::sleep(Duration::from_millis(100));
        }

        // Cleanup
        println!("\n\nCleaning up...");
        BASS_Processor_Free(processor);
        BASS_StreamFree(input_stream);
        BASS_Free();

        // Final stats
        println!("\nFinal Statistics:");
        println!("  Samples processed: {}", stats.samples_processed);
        println!("  Input peak: {:.3}", stats.input_peak);
        println!("  Output peak: {:.3}", stats.output_peak);
        println!("  Low band GR: {:.1} dB", stats.low_band_gr_db);
        println!("  High band GR: {:.1} dB", stats.high_band_gr_db);
        println!("  Underruns: {}", stats.underruns);
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
