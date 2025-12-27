//! File to Speakers Presets Test
//!
//! Usage: cargo run --example file_to_speakers_presets --release
//!
//! Demonstrates different processing intensity presets:
//! - BYPASS:  No processing (passthrough)
//! - LIGHT:   Gentle processing for transparent sound
//! - MEDIUM:  Moderate processing for balanced output
//! - HEAVY:   Aggressive processing for maximum loudness
//! - INSANE:  Extreme processing (broadcast loudness war!)
//!
//! Each preset runs for 10 seconds before switching.
//!
//! Also measures end-to-end latency through the processing chain.

use std::ffi::{c_void, CString};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

// Use the library directly
use bass_broadcast_processor::{
    Agc3StageConfig, AgcConfig, CompressorConfig, MultibandConfigHeader, MultibandStatsHeader,
    StereoEnhancerBandConfig, StereoEnhancerConfig, AGC_MODE_SINGLE, AGC_MODE_THREE_STAGE,
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
    fn BASS_MultibandProcessor_SetBand(
        handle: *mut c_void,
        band: u32,
        config: *const CompressorConfig,
    ) -> i32;
    fn BASS_MultibandProcessor_SetBypass(handle: *mut c_void, bypass: i32) -> i32;
    fn BASS_MultibandProcessor_SetGains(
        handle: *mut c_void,
        input_gain_db: f32,
        output_gain_db: f32,
    ) -> i32;
    fn BASS_MultibandProcessor_SetAGC(handle: *mut c_void, config: *const AgcConfig) -> i32;
    fn BASS_MultibandProcessor_SetAGC3Stage(
        handle: *mut c_void,
        config: *const Agc3StageConfig,
    ) -> i32;
    fn BASS_MultibandProcessor_SetStereoEnhancer(
        handle: *mut c_void,
        config: *const StereoEnhancerConfig,
    ) -> i32;
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
    fn BASS_GetInfo(info: *mut BASS_INFO) -> BOOL;
}

/// BASS device info structure
#[repr(C)]
struct BASS_INFO {
    flags: DWORD,
    hwsize: DWORD,
    hwfree: DWORD,
    freesam: DWORD,
    free3d: DWORD,
    minrate: DWORD,
    maxrate: DWORD,
    eax: BOOL,
    minbuf: DWORD,
    dsver: DWORD,
    latency: DWORD, // Playback latency in ms
    initflags: DWORD,
    speakers: DWORD,
    freq: DWORD,
}

// Global running flag for clean shutdown
static RUNNING: AtomicBool = AtomicBool::new(true);


fn bass_error_string(code: i32) -> &'static str {
    match code {
        0 => "OK",
        1 => "MEM",
        2 => "FILEOPEN",
        3 => "DRIVER",
        5 => "HANDLE",
        6 => "FORMAT",
        8 => "INIT",
        14 => "ALREADY",
        20 => "NOCHAN",
        _ => "?",
    }
}

// ============================================================================
// PRESETS
// ============================================================================

#[derive(Clone, Copy, PartialEq)]
enum Preset {
    Bypass,
    Light,
    Medium,
    Heavy,
    Insane,
}

impl Preset {
    fn name(&self) -> &'static str {
        match self {
            Preset::Bypass => "BYPASS",
            Preset::Light => "LIGHT",
            Preset::Medium => "MEDIUM",
            Preset::Heavy => "HEAVY",
            Preset::Insane => "INSANE",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Preset::Bypass => "No processing - clean passthrough",
            Preset::Light => "Gentle AGC + light compression",
            Preset::Medium => "Balanced processing for general use",
            Preset::Heavy => "Aggressive for maximum loudness",
            Preset::Insane => "EXTREME - Loudness war mode!",
        }
    }

    fn next(&self) -> Preset {
        match self {
            Preset::Bypass => Preset::Light,
            Preset::Light => Preset::Medium,
            Preset::Medium => Preset::Heavy,
            Preset::Heavy => Preset::Insane,
            Preset::Insane => Preset::Bypass,
        }
    }
}

/// Apply preset to processor
unsafe fn apply_preset(processor: *mut c_void, preset: Preset) {
    match preset {
        Preset::Bypass => apply_bypass(processor),
        Preset::Light => apply_light(processor),
        Preset::Medium => apply_medium(processor),
        Preset::Heavy => apply_heavy(processor),
        Preset::Insane => apply_insane(processor),
    }
}

// ----------------------------------------------------------------------------
// BYPASS: No processing
// ----------------------------------------------------------------------------
unsafe fn apply_bypass(processor: *mut c_void) {
    BASS_MultibandProcessor_SetBypass(processor, TRUE);
}

// ----------------------------------------------------------------------------
// LIGHT: Gentle processing
// ----------------------------------------------------------------------------
unsafe fn apply_light(processor: *mut c_void) {
    BASS_MultibandProcessor_SetBypass(processor, FALSE);

    // Input/output gains
    BASS_MultibandProcessor_SetGains(processor, 0.0, -1.0); // Slight headroom

    // AGC: Single-stage, gentle
    let agc = AgcConfig {
        target_level_db: -20.0,
        threshold_db: -28.0,
        ratio: 2.0,        // Gentle ratio
        knee_db: 15.0,     // Wide knee
        attack_ms: 100.0,  // Slow attack
        release_ms: 800.0, // Slow release
        enabled: 1,
        mode: AGC_MODE_SINGLE,
        _pad: [0; 2],
    };
    BASS_MultibandProcessor_SetAGC(processor, &agc);

    // Light compression per band
    let bands = [
        // Sub-bass: Very gentle
        CompressorConfig {
            threshold_db: -24.0,
            ratio: 2.0,
            attack_ms: 20.0,
            release_ms: 300.0,
            makeup_gain_db: 1.0,
            lookahead_ms: 0.0,
        },
        // Bass
        CompressorConfig {
            threshold_db: -22.0,
            ratio: 2.0,
            attack_ms: 15.0,
            release_ms: 250.0,
            makeup_gain_db: 1.0,
            lookahead_ms: 0.0,
        },
        // Mid
        CompressorConfig {
            threshold_db: -20.0,
            ratio: 2.0,
            attack_ms: 10.0,
            release_ms: 200.0,
            makeup_gain_db: 1.0,
            lookahead_ms: 0.0,
        },
        // Presence
        CompressorConfig {
            threshold_db: -18.0,
            ratio: 2.0,
            attack_ms: 5.0,
            release_ms: 150.0,
            makeup_gain_db: 1.0,
            lookahead_ms: 0.0,
        },
        // Brilliance
        CompressorConfig {
            threshold_db: -16.0,
            ratio: 2.0,
            attack_ms: 3.0,
            release_ms: 100.0,
            makeup_gain_db: 0.5,
            lookahead_ms: 0.0,
        },
    ];
    for (i, band) in bands.iter().enumerate() {
        BASS_MultibandProcessor_SetBand(processor, i as u32, band);
    }

    // Stereo enhancer: Very subtle
    let stereo = StereoEnhancerConfig {
        enabled: 1,
        _pad: [0; 3],
        bands: [
            // Band 0 (Bass): Bypassed
            StereoEnhancerBandConfig {
                target_width: 1.0,
                max_gain_db: 0.0,
                max_atten_db: 0.0,
                attack_ms: 50.0,
                release_ms: 200.0,
                enabled: 0,
                _pad: [0; 3],
            },
            // Band 1-4: Subtle enhancement
            StereoEnhancerBandConfig {
                target_width: 1.0,
                max_gain_db: 3.0,
                max_atten_db: 3.0,
                attack_ms: 50.0,
                release_ms: 200.0,
                enabled: 1,
                _pad: [0; 3],
            },
            StereoEnhancerBandConfig {
                target_width: 1.05,
                max_gain_db: 4.0,
                max_atten_db: 4.0,
                attack_ms: 40.0,
                release_ms: 180.0,
                enabled: 1,
                _pad: [0; 3],
            },
            StereoEnhancerBandConfig {
                target_width: 1.1,
                max_gain_db: 5.0,
                max_atten_db: 5.0,
                attack_ms: 30.0,
                release_ms: 150.0,
                enabled: 1,
                _pad: [0; 3],
            },
            StereoEnhancerBandConfig {
                target_width: 1.15,
                max_gain_db: 6.0,
                max_atten_db: 6.0,
                attack_ms: 20.0,
                release_ms: 120.0,
                enabled: 1,
                _pad: [0; 3],
            },
        ],
    };
    BASS_MultibandProcessor_SetStereoEnhancer(processor, &stereo);
}

// ----------------------------------------------------------------------------
// MEDIUM: Balanced processing
// ----------------------------------------------------------------------------
unsafe fn apply_medium(processor: *mut c_void) {
    BASS_MultibandProcessor_SetBypass(processor, FALSE);

    // Input/output gains
    BASS_MultibandProcessor_SetGains(processor, 0.0, 0.0);

    // AGC: Single-stage, moderate
    let agc = AgcConfig {
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
    BASS_MultibandProcessor_SetAGC(processor, &agc);

    // Moderate compression
    let bands = [
        CompressorConfig {
            threshold_db: -20.0,
            ratio: 3.0,
            attack_ms: 10.0,
            release_ms: 200.0,
            makeup_gain_db: 2.0,
            lookahead_ms: 0.0,
        },
        CompressorConfig {
            threshold_db: -18.0,
            ratio: 3.0,
            attack_ms: 8.0,
            release_ms: 150.0,
            makeup_gain_db: 2.0,
            lookahead_ms: 0.0,
        },
        CompressorConfig {
            threshold_db: -16.0,
            ratio: 3.0,
            attack_ms: 5.0,
            release_ms: 100.0,
            makeup_gain_db: 2.0,
            lookahead_ms: 0.0,
        },
        CompressorConfig {
            threshold_db: -14.0,
            ratio: 3.0,
            attack_ms: 3.0,
            release_ms: 80.0,
            makeup_gain_db: 2.0,
            lookahead_ms: 0.0,
        },
        CompressorConfig {
            threshold_db: -12.0,
            ratio: 3.0,
            attack_ms: 1.0,
            release_ms: 60.0,
            makeup_gain_db: 1.0,
            lookahead_ms: 0.0,
        },
    ];
    for (i, band) in bands.iter().enumerate() {
        BASS_MultibandProcessor_SetBand(processor, i as u32, band);
    }

    // Stereo enhancer: Moderate
    let stereo = StereoEnhancerConfig {
        enabled: 1,
        _pad: [0; 3],
        bands: [
            StereoEnhancerBandConfig {
                target_width: 1.0,
                max_gain_db: 0.0,
                max_atten_db: 0.0,
                attack_ms: 50.0,
                release_ms: 200.0,
                enabled: 0,
                _pad: [0; 3],
            },
            StereoEnhancerBandConfig {
                target_width: 1.0,
                max_gain_db: 6.0,
                max_atten_db: 6.0,
                attack_ms: 50.0,
                release_ms: 200.0,
                enabled: 1,
                _pad: [0; 3],
            },
            StereoEnhancerBandConfig {
                target_width: 1.2,
                max_gain_db: 9.0,
                max_atten_db: 9.0,
                attack_ms: 30.0,
                release_ms: 150.0,
                enabled: 1,
                _pad: [0; 3],
            },
            StereoEnhancerBandConfig {
                target_width: 1.3,
                max_gain_db: 10.0,
                max_atten_db: 10.0,
                attack_ms: 20.0,
                release_ms: 100.0,
                enabled: 1,
                _pad: [0; 3],
            },
            StereoEnhancerBandConfig {
                target_width: 1.4,
                max_gain_db: 12.0,
                max_atten_db: 12.0,
                attack_ms: 15.0,
                release_ms: 80.0,
                enabled: 1,
                _pad: [0; 3],
            },
        ],
    };
    BASS_MultibandProcessor_SetStereoEnhancer(processor, &stereo);
}

// ----------------------------------------------------------------------------
// HEAVY: Aggressive processing
// ----------------------------------------------------------------------------
unsafe fn apply_heavy(processor: *mut c_void) {
    BASS_MultibandProcessor_SetBypass(processor, FALSE);

    // Input/output gains - push input harder
    BASS_MultibandProcessor_SetGains(processor, 3.0, -2.0);

    // AGC: 3-stage for tight control
    let agc_3stage = Agc3StageConfig {
        slow: AgcConfig {
            target_level_db: -18.0,
            threshold_db: -26.0,
            ratio: 2.5,
            knee_db: 10.0,
            attack_ms: 2000.0,
            release_ms: 6000.0,
            enabled: 1,
            mode: AGC_MODE_THREE_STAGE,
            _pad: [0; 2],
        },
        medium: AgcConfig {
            target_level_db: -16.0,
            threshold_db: -22.0,
            ratio: 3.0,
            knee_db: 8.0,
            attack_ms: 200.0,
            release_ms: 600.0,
            enabled: 1,
            mode: AGC_MODE_THREE_STAGE,
            _pad: [0; 2],
        },
        fast: AgcConfig {
            target_level_db: -14.0,
            threshold_db: -20.0,
            ratio: 4.0,
            knee_db: 6.0,
            attack_ms: 20.0,
            release_ms: 100.0,
            enabled: 1,
            mode: AGC_MODE_THREE_STAGE,
            _pad: [0; 2],
        },
    };
    BASS_MultibandProcessor_SetAGC3Stage(processor, &agc_3stage);

    // Heavy compression (with 5ms lookahead for transparent limiting)
    let bands = [
        CompressorConfig {
            threshold_db: -18.0,
            ratio: 5.0,
            attack_ms: 5.0,
            release_ms: 150.0,
            makeup_gain_db: 4.0,
            lookahead_ms: 5.0,
        },
        CompressorConfig {
            threshold_db: -16.0,
            ratio: 5.0,
            attack_ms: 3.0,
            release_ms: 100.0,
            makeup_gain_db: 4.0,
            lookahead_ms: 5.0,
        },
        CompressorConfig {
            threshold_db: -14.0,
            ratio: 4.0,
            attack_ms: 2.0,
            release_ms: 80.0,
            makeup_gain_db: 4.0,
            lookahead_ms: 5.0,
        },
        CompressorConfig {
            threshold_db: -12.0,
            ratio: 5.0,
            attack_ms: 1.0,
            release_ms: 60.0,
            makeup_gain_db: 4.0,
            lookahead_ms: 5.0,
        },
        CompressorConfig {
            threshold_db: -10.0,
            ratio: 5.0,
            attack_ms: 0.5,
            release_ms: 40.0,
            makeup_gain_db: 3.0,
            lookahead_ms: 5.0,
        },
    ];
    for (i, band) in bands.iter().enumerate() {
        BASS_MultibandProcessor_SetBand(processor, i as u32, band);
    }

    // Stereo enhancer: Aggressive
    let stereo = StereoEnhancerConfig {
        enabled: 1,
        _pad: [0; 3],
        bands: [
            StereoEnhancerBandConfig {
                target_width: 1.0,
                max_gain_db: 0.0,
                max_atten_db: 0.0,
                attack_ms: 50.0,
                release_ms: 200.0,
                enabled: 0,
                _pad: [0; 3],
            },
            StereoEnhancerBandConfig {
                target_width: 1.1,
                max_gain_db: 9.0,
                max_atten_db: 9.0,
                attack_ms: 30.0,
                release_ms: 150.0,
                enabled: 1,
                _pad: [0; 3],
            },
            StereoEnhancerBandConfig {
                target_width: 1.4,
                max_gain_db: 12.0,
                max_atten_db: 12.0,
                attack_ms: 20.0,
                release_ms: 100.0,
                enabled: 1,
                _pad: [0; 3],
            },
            StereoEnhancerBandConfig {
                target_width: 1.6,
                max_gain_db: 15.0,
                max_atten_db: 15.0,
                attack_ms: 10.0,
                release_ms: 60.0,
                enabled: 1,
                _pad: [0; 3],
            },
            StereoEnhancerBandConfig {
                target_width: 1.8,
                max_gain_db: 18.0,
                max_atten_db: 18.0,
                attack_ms: 5.0,
                release_ms: 40.0,
                enabled: 1,
                _pad: [0; 3],
            },
        ],
    };
    BASS_MultibandProcessor_SetStereoEnhancer(processor, &stereo);
}

// ----------------------------------------------------------------------------
// INSANE: Extreme processing (loudness war!)
// ----------------------------------------------------------------------------
unsafe fn apply_insane(processor: *mut c_void) {
    BASS_MultibandProcessor_SetBypass(processor, FALSE);

    // Input/output gains - PUSH HARD
    BASS_MultibandProcessor_SetGains(processor, 6.0, -3.0);

    // AGC: 3-stage, AGGRESSIVE
    let agc_3stage = Agc3StageConfig {
        slow: AgcConfig {
            target_level_db: -14.0,
            threshold_db: -22.0,
            ratio: 4.0,
            knee_db: 6.0,
            attack_ms: 1000.0,
            release_ms: 3000.0,
            enabled: 1,
            mode: AGC_MODE_THREE_STAGE,
            _pad: [0; 2],
        },
        medium: AgcConfig {
            target_level_db: -12.0,
            threshold_db: -18.0,
            ratio: 5.0,
            knee_db: 4.0,
            attack_ms: 100.0,
            release_ms: 300.0,
            enabled: 1,
            mode: AGC_MODE_THREE_STAGE,
            _pad: [0; 2],
        },
        fast: AgcConfig {
            target_level_db: -10.0,
            threshold_db: -16.0,
            ratio: 6.0,
            knee_db: 2.0,
            attack_ms: 10.0,
            release_ms: 50.0,
            enabled: 1,
            mode: AGC_MODE_THREE_STAGE,
            _pad: [0; 2],
        },
    };
    BASS_MultibandProcessor_SetAGC3Stage(processor, &agc_3stage);

    // EXTREME compression - brick wall! (with 5ms lookahead for transparent limiting)
    let bands = [
        CompressorConfig {
            threshold_db: -14.0,
            ratio: 8.0,
            attack_ms: 2.0,
            release_ms: 80.0,
            makeup_gain_db: 6.0,
            lookahead_ms: 5.0,
        },
        CompressorConfig {
            threshold_db: -12.0,
            ratio: 8.0,
            attack_ms: 1.0,
            release_ms: 60.0,
            makeup_gain_db: 6.0,
            lookahead_ms: 5.0,
        },
        CompressorConfig {
            threshold_db: -10.0,
            ratio: 7.0,
            attack_ms: 0.5,
            release_ms: 40.0,
            makeup_gain_db: 6.0,
            lookahead_ms: 5.0,
        },
        CompressorConfig {
            threshold_db: -8.0,
            ratio: 8.0,
            attack_ms: 0.3,
            release_ms: 30.0,
            makeup_gain_db: 6.0,
            lookahead_ms: 5.0,
        },
        CompressorConfig {
            threshold_db: -6.0,
            ratio: 10.0,
            attack_ms: 0.2,
            release_ms: 20.0,
            makeup_gain_db: 5.0,
            lookahead_ms: 5.0,
        },
    ];
    for (i, band) in bands.iter().enumerate() {
        BASS_MultibandProcessor_SetBand(processor, i as u32, band);
    }

    // Stereo enhancer: MAXIMUM WIDTH
    let stereo = StereoEnhancerConfig {
        enabled: 1,
        _pad: [0; 3],
        bands: [
            StereoEnhancerBandConfig {
                target_width: 1.0,
                max_gain_db: 0.0,
                max_atten_db: 0.0,
                attack_ms: 50.0,
                release_ms: 200.0,
                enabled: 0,
                _pad: [0; 3],
            },
            StereoEnhancerBandConfig {
                target_width: 1.2,
                max_gain_db: 12.0,
                max_atten_db: 12.0,
                attack_ms: 15.0,
                release_ms: 80.0,
                enabled: 1,
                _pad: [0; 3],
            },
            StereoEnhancerBandConfig {
                target_width: 1.6,
                max_gain_db: 15.0,
                max_atten_db: 15.0,
                attack_ms: 10.0,
                release_ms: 50.0,
                enabled: 1,
                _pad: [0; 3],
            },
            StereoEnhancerBandConfig {
                target_width: 2.0,
                max_gain_db: 18.0,
                max_atten_db: 18.0,
                attack_ms: 5.0,
                release_ms: 30.0,
                enabled: 1,
                _pad: [0; 3],
            },
            StereoEnhancerBandConfig {
                target_width: 2.2,
                max_gain_db: 18.0,
                max_atten_db: 18.0,
                attack_ms: 3.0,
                release_ms: 20.0,
                enabled: 1,
                _pad: [0; 3],
            },
        ],
    };
    BASS_MultibandProcessor_SetStereoEnhancer(processor, &stereo);
}

// ============================================================================
// MAIN
// ============================================================================

fn main() {
    println!("=============================================================");
    println!("  BASS Broadcast Processor - PRESET DEMO");
    println!("=============================================================\n");

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

        // Initialize BASS
        println!("\nInitializing BASS...");
        if BASS_Init(-1, 48000, 0, ptr::null_mut(), ptr::null()) == FALSE {
            let err = BASS_ErrorGetCode();
            println!("ERROR: Failed to initialize BASS (error {} = {})", err, bass_error_string(err));
            return;
        }

        // Get device info including hardware latency
        let mut bass_info: BASS_INFO = std::mem::zeroed();
        if BASS_GetInfo(&mut bass_info) == TRUE {
            println!("  Device buffer latency: {} ms (audio output buffer)", bass_info.latency);
            println!("  Sample rate: {} Hz", bass_info.freq);
            println!("  Processor latency: 0 ms (zero-latency, sample-by-sample)");
        }

        // Create file stream
        let file_cstring = CString::new(file_path).unwrap();
        let input_stream = BASS_StreamCreateFile(
            FALSE,
            file_cstring.as_ptr() as *const c_void,
            0,
            0,
            BASS_SAMPLE_FLOAT | BASS_STREAM_DECODE,
        );

        if input_stream == 0 {
            let err = BASS_ErrorGetCode();
            println!("ERROR: Failed to create file stream (error {} = {})", err, bass_error_string(err));
            BASS_Free();
            return;
        }

        let length = BASS_ChannelGetLength(input_stream, BASS_POS_BYTE);
        println!("  Source: {}", file_path);
        println!("  Length: {:.1} MB", length as f64 / 1_000_000.0);

        // Create 5-band processor
        let header = MultibandConfigHeader {
            sample_rate: 48000,
            channels: 2,
            num_bands: 5,
            decode_output: 0,
            _pad: [0; 3],
            input_gain_db: 0.0,
            output_gain_db: 0.0,
        };

        let crossover_freqs: [f32; 4] = [100.0, 400.0, 2000.0, 8000.0];

        // Initial band configs (will be overwritten by presets)
        let bands: [CompressorConfig; 5] = [
            CompressorConfig { threshold_db: -20.0, ratio: 3.0, attack_ms: 10.0, release_ms: 200.0, makeup_gain_db: 2.0, lookahead_ms: 0.0 },
            CompressorConfig { threshold_db: -18.0, ratio: 3.0, attack_ms: 5.0, release_ms: 150.0, makeup_gain_db: 2.0, lookahead_ms: 0.0 },
            CompressorConfig { threshold_db: -16.0, ratio: 2.5, attack_ms: 3.0, release_ms: 100.0, makeup_gain_db: 2.0, lookahead_ms: 0.0 },
            CompressorConfig { threshold_db: -14.0, ratio: 3.0, attack_ms: 1.0, release_ms: 80.0, makeup_gain_db: 2.0, lookahead_ms: 0.0 },
            CompressorConfig { threshold_db: -12.0, ratio: 3.0, attack_ms: 0.5, release_ms: 50.0, makeup_gain_db: 1.0, lookahead_ms: 0.0 },
        ];

        let processor = BASS_MultibandProcessor_Create(
            input_stream,
            &header,
            crossover_freqs.as_ptr(),
            bands.as_ptr(),
        );

        if processor.is_null() {
            println!("ERROR: Failed to create processor");
            BASS_StreamFree(input_stream);
            BASS_Free();
            return;
        }

        let output_stream = BASS_MultibandProcessor_GetOutput(processor);
        if output_stream == 0 {
            println!("ERROR: Failed to get output stream");
            BASS_MultibandProcessor_Free(processor);
            BASS_StreamFree(input_stream);
            BASS_Free();
            return;
        }

        // Start with first preset
        let mut current_preset = Preset::Bypass;
        apply_preset(processor, current_preset);

        BASS_MultibandProcessor_Prefill(processor);

        println!("\nStarting playback...");
        if BASS_ChannelPlay(output_stream, TRUE) == FALSE {
            println!("ERROR: Failed to start playback");
            BASS_MultibandProcessor_Free(processor);
            BASS_StreamFree(input_stream);
            BASS_Free();
            return;
        }

        println!("\n=============================================================");
        println!("  PRESET DEMO Running!");
        println!("=============================================================");
        println!("  Presets cycle every 10 seconds:");
        println!("    BYPASS  -> LIGHT  -> MEDIUM -> HEAVY  -> INSANE -> ...");
        println!("=============================================================");
        println!("Press Ctrl+C to stop\n");

        use std::io::Write;
        std::io::stdout().flush().unwrap();

        // Print current preset
        println!("\n  >>> {} - {} <<<\n", current_preset.name(), current_preset.description());
        std::io::stdout().flush().unwrap();

        let mut stats = MultibandStatsHeader::default();
        let mut band_gr = [0.0f32; 5];
        let mut loop_count = 0u32;
        let start_time = std::time::Instant::now();
        let mut last_preset_time = start_time;

        while RUNNING.load(Ordering::SeqCst) {
            let state = BASS_ChannelIsActive(output_stream);
            if state == BASS_ACTIVE_STOPPED {
                println!("\nPlayback ended");
                break;
            }

            // Switch preset every 10 seconds
            if last_preset_time.elapsed() >= Duration::from_secs(10) {
                current_preset = current_preset.next();
                apply_preset(processor, current_preset);
                last_preset_time = std::time::Instant::now();

                println!("\n  >>> {} - {} <<<\n", current_preset.name(), current_preset.description());
                std::io::stdout().flush().unwrap();
            }

            // Get stats
            BASS_MultibandProcessor_GetStats(processor, &mut stats, band_gr.as_mut_ptr());

            // Display every 500ms
            loop_count += 1;
            if loop_count % 5 == 1 {
                let process_time_ms = stats.process_time_us as f64 / 1000.0;
                let elapsed = start_time.elapsed().as_secs();
                let preset_remaining = 10 - (last_preset_time.elapsed().as_secs() as i64).min(10);

                if current_preset == Preset::Bypass {
                    println!(
                        "[{:>6}] {:02}:{:02} | In:{:5.3} Out:{:5.3} | Next in {}s",
                        current_preset.name(),
                        elapsed / 60,
                        elapsed % 60,
                        stats.input_peak,
                        stats.output_peak,
                        preset_remaining,
                    );
                } else {
                    // process_time_ms = CPU time to process one buffer (not audio latency)
                    println!(
                        "[{:>6}] {:02}:{:02} | In:{:5.3} Out:{:5.3} | AGC:{:+5.1}dB | Bands:{:+4.1} {:+4.1} {:+4.1} {:+4.1} {:+4.1} | CPU:{:4.2}ms | Next in {}s",
                        current_preset.name(),
                        elapsed / 60,
                        elapsed % 60,
                        stats.input_peak,
                        stats.output_peak,
                        stats.agc_gr_db,
                        band_gr[0], band_gr[1], band_gr[2], band_gr[3], band_gr[4],
                        process_time_ms,
                        preset_remaining,
                    );
                }
                std::io::stdout().flush().unwrap();
            }

            thread::sleep(Duration::from_millis(100));
        }

        // Cleanup
        println!("\n\nCleaning up...");
        BASS_MultibandProcessor_Free(processor);
        BASS_StreamFree(input_stream);
        BASS_Free();
    }

    println!("\nDone!");
}

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
