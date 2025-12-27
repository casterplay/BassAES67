//! Broadcast Audio Processor for BASS
//!
//! A multiband broadcast audio processor that sits between BASS audio streams.
//! Provides crossover filtering and per-band compression.
//!
//! Two processor types available:
//! - `BASS_Processor_*` - Original 2-band processor (backward compatible)
//! - `BASS_MultibandProcessor_*` - N-band processor (2, 5, 8, or any number of bands)

use std::ffi::c_void;
use std::ptr;

mod dsp;
mod ffi;
mod processor;

use ffi::*;
use processor::{BroadcastProcessor, MultibandProcessor};

// Re-export types for external use
pub use processor::{
    Agc3StageConfig, AgcConfig, CompressorConfig, MultibandConfig, MultibandConfigHeader,
    MultibandStatsHeader, ParametricEqBandConfig, ParametricEqConfig, ProcessorConfig,
    ProcessorStats, SoftClipperConfig, StereoEnhancerBandConfig, StereoEnhancerConfig,
    AGC_MODE_SINGLE, AGC_MODE_THREE_STAGE, CLIP_MODE_HARD, CLIP_MODE_SOFT, CLIP_MODE_TANH,
};

/// STREAMPROC callback - called by BASS when output stream needs samples.
unsafe extern "system" fn processor_stream_proc(
    _handle: HSTREAM,
    buffer: *mut c_void,
    length: DWORD,
    user: *mut c_void,
) -> DWORD {
    if user.is_null() {
        return 0;
    }

    let processor = &mut *(user as *mut BroadcastProcessor);
    let samples = length as usize / 4; // 4 bytes per f32
    let float_buffer = std::slice::from_raw_parts_mut(buffer as *mut f32, samples);

    let written = processor.read_samples(float_buffer);

    (written * 4) as DWORD
}

/// Create a new broadcast processor.
///
/// # Arguments
/// * `source_channel` - BASS channel handle to pull audio from
/// * `config` - Pointer to ProcessorConfig structure
///
/// # Returns
/// Opaque handle (pointer) to the processor, or null on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_Processor_Create(
    source_channel: DWORD,
    config: *const ProcessorConfig,
) -> *mut c_void {
    if config.is_null() {
        return ptr::null_mut();
    }

    let cfg = (*config).clone();
    match BroadcastProcessor::new(source_channel, cfg) {
        Ok(processor) => {
            // Get config values before boxing
            let sample_rate = processor.get_config().sample_rate;
            let channels = processor.get_config().channels as DWORD;
            let decode_output = processor.get_config().decode_output != 0;

            // Box and leak to get stable pointer
            let boxed = Box::new(processor);
            let ptr = Box::into_raw(boxed);

            // Build stream flags
            let mut flags = BASS_SAMPLE_FLOAT;
            if decode_output {
                flags |= BASS_STREAM_DECODE;
            }

            // Create output BASS stream with processor pointer
            let handle = BASS_StreamCreate(
                sample_rate,
                channels,
                flags,
                processor_stream_proc,
                ptr as *mut c_void,
            );

            if handle == 0 {
                // Cleanup on failure
                let _ = Box::from_raw(ptr);
                return ptr::null_mut();
            }

            (*ptr).output_handle = handle;
            ptr as *mut c_void
        }
        Err(_) => ptr::null_mut(),
    }
}

/// Get the output BASS stream handle.
///
/// This handle can be used as input to BASS_AES67_OutputCreate or any other
/// BASS function that reads from a channel.
#[no_mangle]
pub unsafe extern "system" fn BASS_Processor_GetOutput(handle: *mut c_void) -> HSTREAM {
    if handle.is_null() {
        return 0;
    }
    let processor = &*(handle as *const BroadcastProcessor);
    processor.output_handle
}

/// Get processor statistics (lock-free).
///
/// # Arguments
/// * `handle` - Processor handle from BASS_Processor_Create
/// * `stats` - Pointer to ProcessorStats structure to fill
///
/// # Returns
/// TRUE on success, FALSE on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_Processor_GetStats(
    handle: *mut c_void,
    stats: *mut ProcessorStats,
) -> BOOL {
    if handle.is_null() || stats.is_null() {
        return FALSE;
    }

    let processor = &*(handle as *const BroadcastProcessor);
    *stats = processor.get_stats();
    TRUE
}

/// Update low band compressor parameters at runtime.
#[no_mangle]
pub unsafe extern "system" fn BASS_Processor_SetLowBand(
    handle: *mut c_void,
    config: *const CompressorConfig,
) -> BOOL {
    if handle.is_null() || config.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut BroadcastProcessor);
    let cfg = &*config;
    processor.low_comp.set_params(
        cfg.threshold_db,
        cfg.ratio,
        cfg.attack_ms,
        cfg.release_ms,
        cfg.makeup_gain_db,
    );
    TRUE
}

/// Update high band compressor parameters at runtime.
#[no_mangle]
pub unsafe extern "system" fn BASS_Processor_SetHighBand(
    handle: *mut c_void,
    config: *const CompressorConfig,
) -> BOOL {
    if handle.is_null() || config.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut BroadcastProcessor);
    let cfg = &*config;
    processor.high_comp.set_params(
        cfg.threshold_db,
        cfg.ratio,
        cfg.attack_ms,
        cfg.release_ms,
        cfg.makeup_gain_db,
    );
    TRUE
}

/// Set input and output gains.
///
/// # Arguments
/// * `handle` - Processor handle
/// * `input_gain_db` - Input gain in dB (-20 to +20)
/// * `output_gain_db` - Output gain in dB (-20 to +20)
#[no_mangle]
pub unsafe extern "system" fn BASS_Processor_SetGains(
    handle: *mut c_void,
    input_gain_db: f32,
    output_gain_db: f32,
) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut BroadcastProcessor);
    processor.input_gain = dsp::gain::db_to_linear(input_gain_db);
    processor.output_gain = dsp::gain::db_to_linear(output_gain_db);
    TRUE
}

/// Reset processor state (clear filter history, envelope followers).
#[no_mangle]
pub unsafe extern "system" fn BASS_Processor_Reset(handle: *mut c_void) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut BroadcastProcessor);
    processor.reset();
    TRUE
}

/// Set bypass mode.
/// When bypass is TRUE (1), audio passes through unprocessed.
/// When bypass is FALSE (0), audio is processed normally.
#[no_mangle]
pub unsafe extern "system" fn BASS_Processor_SetBypass(handle: *mut c_void, bypass: BOOL) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut BroadcastProcessor);
    processor.bypass = bypass != 0;
    TRUE
}

/// Pre-fill the processor buffer before starting playback.
/// Call this before BASS_ChannelPlay to avoid initial stall.
#[no_mangle]
pub unsafe extern "system" fn BASS_Processor_Prefill(handle: *mut c_void) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut BroadcastProcessor);
    processor.prefill();
    TRUE
}

/// Free the processor and associated BASS stream.
#[no_mangle]
pub unsafe extern "system" fn BASS_Processor_Free(handle: *mut c_void) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = Box::from_raw(handle as *mut BroadcastProcessor);
    if processor.output_handle != 0 {
        BASS_StreamFree(processor.output_handle);
    }
    // Box drops here, freeing processor
    TRUE
}

/// Get a default configuration.
/// Useful for C callers to get sensible defaults.
#[no_mangle]
pub unsafe extern "system" fn BASS_Processor_GetDefaultConfig(config: *mut ProcessorConfig) -> BOOL {
    if config.is_null() {
        return FALSE;
    }

    *config = ProcessorConfig::default();
    TRUE
}

// ============================================================================
// N-Band Multiband Processor FFI
// ============================================================================

/// STREAMPROC callback for multiband processor.
unsafe extern "system" fn multiband_stream_proc(
    _handle: HSTREAM,
    buffer: *mut c_void,
    length: DWORD,
    user: *mut c_void,
) -> DWORD {
    if user.is_null() {
        return 0;
    }

    let processor = &mut *(user as *mut MultibandProcessor);
    let samples = length as usize / 4; // 4 bytes per f32
    let float_buffer = std::slice::from_raw_parts_mut(buffer as *mut f32, samples);

    let written = processor.read_samples(float_buffer);

    (written * 4) as DWORD
}

/// Create a new N-band multiband processor.
///
/// # Arguments
/// * `source_channel` - BASS channel handle to pull audio from
/// * `header` - Pointer to MultibandConfigHeader structure
/// * `crossover_freqs` - Pointer to array of crossover frequencies (num_bands - 1 elements)
/// * `bands` - Pointer to array of CompressorConfig (num_bands elements)
///
/// # Returns
/// Opaque handle (pointer) to the processor, or null on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_Create(
    source_channel: DWORD,
    header: *const MultibandConfigHeader,
    crossover_freqs: *const f32,
    bands: *const CompressorConfig,
) -> *mut c_void {
    if header.is_null() || crossover_freqs.is_null() || bands.is_null() {
        return ptr::null_mut();
    }

    let hdr = *header;
    let num_bands = hdr.num_bands as usize;

    if num_bands < 2 {
        return ptr::null_mut();
    }

    // Copy crossover frequencies (num_bands - 1)
    let freqs = std::slice::from_raw_parts(crossover_freqs, num_bands - 1).to_vec();

    // Copy band configs (num_bands)
    let band_cfgs = std::slice::from_raw_parts(bands, num_bands).to_vec();

    let config = MultibandConfig {
        header: hdr,
        crossover_freqs: freqs,
        bands: band_cfgs,
    };

    match MultibandProcessor::new(source_channel, config) {
        Ok(processor) => {
            // Get config values before boxing
            let sample_rate = processor.get_config().header.sample_rate;
            let channels = processor.get_config().header.channels as DWORD;
            let decode_output = processor.is_decode_output();

            // Box and leak to get stable pointer
            let boxed = Box::new(processor);
            let ptr = Box::into_raw(boxed);

            // Build stream flags
            let mut flags = BASS_SAMPLE_FLOAT;
            if decode_output {
                flags |= BASS_STREAM_DECODE;
            }

            // Create output BASS stream with processor pointer
            let handle = BASS_StreamCreate(
                sample_rate,
                channels,
                flags,
                multiband_stream_proc,
                ptr as *mut c_void,
            );

            if handle == 0 {
                // Cleanup on failure
                let _ = Box::from_raw(ptr);
                return ptr::null_mut();
            }

            (*ptr).output_handle = handle;
            ptr as *mut c_void
        }
        Err(_) => ptr::null_mut(),
    }
}

/// Get the output BASS stream handle for multiband processor.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_GetOutput(handle: *mut c_void) -> HSTREAM {
    if handle.is_null() {
        return 0;
    }
    let processor = &*(handle as *const MultibandProcessor);
    processor.output_handle
}

/// Get multiband processor statistics (lock-free).
///
/// # Arguments
/// * `handle` - Processor handle from BASS_MultibandProcessor_Create
/// * `header_out` - Pointer to MultibandStatsHeader to fill
/// * `band_gr_out` - Pointer to f32 array for per-band gain reduction (num_bands elements)
///
/// # Returns
/// TRUE on success, FALSE on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_GetStats(
    handle: *mut c_void,
    header_out: *mut MultibandStatsHeader,
    band_gr_out: *mut f32,
) -> BOOL {
    if handle.is_null() || header_out.is_null() || band_gr_out.is_null() {
        return FALSE;
    }

    let processor = &*(handle as *const MultibandProcessor);
    let num_bands = processor.num_bands();

    let band_gr_slice = std::slice::from_raw_parts_mut(band_gr_out, num_bands);
    *header_out = processor.get_stats(band_gr_slice);

    TRUE
}

/// Update a specific band's compressor settings.
///
/// # Arguments
/// * `handle` - Processor handle
/// * `band` - Band index (0-based)
/// * `config` - Pointer to CompressorConfig
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_SetBand(
    handle: *mut c_void,
    band: u32,
    config: *const CompressorConfig,
) -> BOOL {
    if handle.is_null() || config.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut MultibandProcessor);
    let cfg = &*config;

    if band as usize >= processor.num_bands() {
        return FALSE;
    }

    processor.set_band(band as usize, cfg);
    TRUE
}

/// Set bypass mode for multiband processor.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_SetBypass(
    handle: *mut c_void,
    bypass: BOOL,
) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut MultibandProcessor);
    processor.bypass = bypass != 0;
    TRUE
}

/// Set input and output gains for multiband processor.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_SetGains(
    handle: *mut c_void,
    input_gain_db: f32,
    output_gain_db: f32,
) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut MultibandProcessor);
    processor.set_gains(input_gain_db, output_gain_db);
    TRUE
}

/// Reset multiband processor state.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_Reset(handle: *mut c_void) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut MultibandProcessor);
    processor.reset();
    TRUE
}

/// Pre-fill the multiband processor buffer.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_Prefill(handle: *mut c_void) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut MultibandProcessor);
    processor.prefill();
    TRUE
}

/// Free the multiband processor and associated BASS stream.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_Free(handle: *mut c_void) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = Box::from_raw(handle as *mut MultibandProcessor);
    if processor.output_handle != 0 {
        BASS_StreamFree(processor.output_handle);
    }
    // Box drops here, freeing processor
    TRUE
}

/// Get the number of bands in the processor.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_GetNumBands(handle: *mut c_void) -> DWORD {
    if handle.is_null() {
        return 0;
    }

    let processor = &*(handle as *const MultibandProcessor);
    processor.num_bands() as DWORD
}

// ============================================================================
// Phase 3: AGC (Automatic Gain Control) FFI Functions
// ============================================================================

/// Set AGC parameters for multiband processor.
///
/// # Arguments
/// * `handle` - Processor handle from BASS_MultibandProcessor_Create
/// * `config` - Pointer to AgcConfig structure
///
/// # Returns
/// TRUE on success, FALSE on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_SetAGC(
    handle: *mut c_void,
    config: *const AgcConfig,
) -> BOOL {
    if handle.is_null() || config.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut MultibandProcessor);
    processor.set_agc(&*config);
    TRUE
}

/// Get default AGC configuration.
///
/// # Arguments
/// * `config` - Pointer to AgcConfig structure to fill with defaults
///
/// # Returns
/// TRUE on success, FALSE on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_GetDefaultAGC(
    config: *mut AgcConfig,
) -> BOOL {
    if config.is_null() {
        return FALSE;
    }

    *config = AgcConfig::default();
    TRUE
}

// ============================================================================
// Phase 3.1b: 3-Stage AGC (Omnia 9 Style) FFI Functions
// ============================================================================

/// Set 3-stage AGC configuration for multiband processor.
/// This enables 3-stage AGC mode with individual slow/medium/fast stages.
///
/// # Arguments
/// * `handle` - Processor handle from BASS_MultibandProcessor_Create
/// * `config` - Pointer to Agc3StageConfig structure
///
/// # Returns
/// TRUE on success, FALSE on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_SetAGC3Stage(
    handle: *mut c_void,
    config: *const Agc3StageConfig,
) -> BOOL {
    if handle.is_null() || config.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut MultibandProcessor);
    processor.set_agc_3stage(&*config);
    TRUE
}

/// Get default 3-stage AGC configuration.
///
/// # Arguments
/// * `config` - Pointer to Agc3StageConfig structure to fill with defaults
///
/// # Returns
/// TRUE on success, FALSE on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_GetDefaultAGC3Stage(
    config: *mut Agc3StageConfig,
) -> BOOL {
    if config.is_null() {
        return FALSE;
    }

    *config = Agc3StageConfig::default();
    TRUE
}

/// Check if 3-stage AGC mode is active.
///
/// # Arguments
/// * `handle` - Processor handle
///
/// # Returns
/// TRUE (1) if 3-stage mode is active, FALSE (0) if single-stage.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_IsAGC3Stage(handle: *mut c_void) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = &*(handle as *const MultibandProcessor);
    if processor.is_agc_3stage() {
        TRUE
    } else {
        FALSE
    }
}

/// Get individual stage gain reduction values for 3-stage AGC.
///
/// # Arguments
/// * `handle` - Processor handle
/// * `slow_gr` - Pointer to receive slow stage gain reduction (dB)
/// * `medium_gr` - Pointer to receive medium stage gain reduction (dB)
/// * `fast_gr` - Pointer to receive fast stage gain reduction (dB)
///
/// # Returns
/// TRUE on success, FALSE on failure. Returns zeros if not in 3-stage mode.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_GetAGC3StageGR(
    handle: *mut c_void,
    slow_gr: *mut f32,
    medium_gr: *mut f32,
    fast_gr: *mut f32,
) -> BOOL {
    if handle.is_null() || slow_gr.is_null() || medium_gr.is_null() || fast_gr.is_null() {
        return FALSE;
    }

    let processor = &*(handle as *const MultibandProcessor);
    let (slow, medium, fast) = processor.get_agc_3stage_gr();
    *slow_gr = slow;
    *medium_gr = medium;
    *fast_gr = fast;
    TRUE
}

// ============================================================================
// Lookahead Control FFI Functions
// ============================================================================

/// Set lookahead for all compressor bands.
/// Lookahead adds latency but allows transparent limiting of fast transients.
///
/// # Arguments
/// * `handle` - Processor handle from BASS_MultibandProcessor_Create
/// * `enabled` - TRUE to enable lookahead, FALSE to disable
/// * `lookahead_ms` - Lookahead time in milliseconds (0.0 to 10.0)
///
/// # Returns
/// TRUE on success, FALSE on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_SetLookahead(
    handle: *mut c_void,
    enabled: BOOL,
    lookahead_ms: f32,
) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut MultibandProcessor);
    processor.set_lookahead(enabled != 0, lookahead_ms);
    TRUE
}

/// Get current lookahead latency in milliseconds.
///
/// # Arguments
/// * `handle` - Processor handle
///
/// # Returns
/// Lookahead latency in milliseconds, or 0.0 if disabled or on error.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_GetLookahead(handle: *mut c_void) -> f32 {
    if handle.is_null() {
        return 0.0;
    }

    let processor = &*(handle as *const MultibandProcessor);
    processor.get_lookahead_ms()
}

// ============================================================================
// Phase 3.2: Stereo Enhancer (Omnia 9 Style) FFI Functions
// ============================================================================

/// Set stereo enhancer configuration for multiband processor.
/// The stereo enhancer uses Mid-Side processing to control stereo width per band.
/// Band 0 (bass) is always bypassed internally to avoid phase issues.
///
/// # Arguments
/// * `handle` - Processor handle from BASS_MultibandProcessor_Create
/// * `config` - Pointer to StereoEnhancerConfig structure
///
/// # Returns
/// TRUE on success, FALSE on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_SetStereoEnhancer(
    handle: *mut c_void,
    config: *const StereoEnhancerConfig,
) -> BOOL {
    if handle.is_null() || config.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut MultibandProcessor);
    processor.set_stereo_enhancer(&*config);
    TRUE
}

/// Get default stereo enhancer configuration.
///
/// # Arguments
/// * `config` - Pointer to StereoEnhancerConfig structure to fill with defaults
///
/// # Returns
/// TRUE on success, FALSE on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_GetDefaultStereoEnhancer(
    config: *mut StereoEnhancerConfig,
) -> BOOL {
    if config.is_null() {
        return FALSE;
    }

    *config = StereoEnhancerConfig::default();
    TRUE
}

/// Check if stereo enhancer is enabled.
///
/// # Arguments
/// * `handle` - Processor handle
///
/// # Returns
/// TRUE (1) if stereo enhancer is enabled, FALSE (0) if bypassed.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_IsStereoEnhancerEnabled(
    handle: *mut c_void,
) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = &*(handle as *const MultibandProcessor);
    if processor.is_stereo_enhancer_enabled() {
        TRUE
    } else {
        FALSE
    }
}

/// Enable or disable stereo enhancer globally.
///
/// # Arguments
/// * `handle` - Processor handle
/// * `enabled` - TRUE (1) to enable, FALSE (0) to bypass
///
/// # Returns
/// TRUE on success, FALSE on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_SetStereoEnhancerEnabled(
    handle: *mut c_void,
    enabled: BOOL,
) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut MultibandProcessor);
    processor.set_stereo_enhancer_enabled(enabled != 0);
    TRUE
}

// ============================================================================
// Per-Band Parametric EQ FFI Functions
// ============================================================================

/// Set parametric EQ configuration for multiband processor.
/// Each band can have its own parametric EQ section for frequency shaping.
///
/// # Arguments
/// * `handle` - Processor handle from BASS_MultibandProcessor_Create
/// * `config` - Pointer to ParametricEqConfig structure
///
/// # Returns
/// TRUE on success, FALSE on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_SetParametricEQ(
    handle: *mut c_void,
    config: *const ParametricEqConfig,
) -> BOOL {
    if handle.is_null() || config.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut MultibandProcessor);
    processor.set_parametric_eq(&*config);
    TRUE
}

/// Get default parametric EQ configuration.
///
/// # Arguments
/// * `config` - Pointer to ParametricEqConfig structure to fill with defaults
///
/// # Returns
/// TRUE on success, FALSE on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_GetDefaultParametricEQ(
    config: *mut ParametricEqConfig,
) -> BOOL {
    if config.is_null() {
        return FALSE;
    }

    *config = ParametricEqConfig::default();
    TRUE
}

/// Check if parametric EQ is enabled.
///
/// # Arguments
/// * `handle` - Processor handle
///
/// # Returns
/// TRUE (1) if parametric EQ is enabled, FALSE (0) if bypassed.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_IsParametricEQEnabled(
    handle: *mut c_void,
) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = &*(handle as *const MultibandProcessor);
    if processor.is_parametric_eq_enabled() {
        TRUE
    } else {
        FALSE
    }
}

/// Enable or disable parametric EQ globally.
///
/// # Arguments
/// * `handle` - Processor handle
/// * `enabled` - TRUE (1) to enable, FALSE (0) to bypass
///
/// # Returns
/// TRUE on success, FALSE on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_SetParametricEQEnabled(
    handle: *mut c_void,
    enabled: BOOL,
) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut MultibandProcessor);
    processor.set_parametric_eq_enabled(enabled != 0);
    TRUE
}

// ============================================================================
// Soft Clipper FFI Functions
// ============================================================================

/// Set soft clipper configuration for multiband processor.
/// The soft clipper provides final-stage limiting with optional oversampling
/// to catch intersample peaks.
///
/// # Arguments
/// * `handle` - Processor handle from BASS_MultibandProcessor_Create
/// * `config` - Pointer to SoftClipperConfig structure
///
/// # Returns
/// TRUE on success, FALSE on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_SetSoftClipper(
    handle: *mut c_void,
    config: *const SoftClipperConfig,
) -> BOOL {
    if handle.is_null() || config.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut MultibandProcessor);
    processor.set_soft_clipper(&*config);
    TRUE
}

/// Get default soft clipper configuration.
///
/// # Arguments
/// * `config` - Pointer to SoftClipperConfig structure to fill with defaults
///
/// # Returns
/// TRUE on success, FALSE on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_GetDefaultSoftClipper(
    config: *mut SoftClipperConfig,
) -> BOOL {
    if config.is_null() {
        return FALSE;
    }

    *config = SoftClipperConfig::default();
    TRUE
}

/// Check if soft clipper is enabled.
///
/// # Arguments
/// * `handle` - Processor handle
///
/// # Returns
/// TRUE (1) if soft clipper is enabled, FALSE (0) if bypassed.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_IsSoftClipperEnabled(
    handle: *mut c_void,
) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = &*(handle as *const MultibandProcessor);
    if processor.is_soft_clipper_enabled() {
        TRUE
    } else {
        FALSE
    }
}

/// Enable or disable soft clipper.
///
/// # Arguments
/// * `handle` - Processor handle
/// * `enabled` - TRUE (1) to enable, FALSE (0) to bypass
///
/// # Returns
/// TRUE on success, FALSE on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_SetSoftClipperEnabled(
    handle: *mut c_void,
    enabled: BOOL,
) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut MultibandProcessor);
    processor.set_soft_clipper_enabled(enabled != 0);
    TRUE
}

/// Get soft clipper latency in milliseconds.
/// This includes any latency from oversampling.
///
/// # Arguments
/// * `handle` - Processor handle
///
/// # Returns
/// Latency in milliseconds, or 0.0 on error.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_GetSoftClipperLatency(
    handle: *mut c_void,
) -> f32 {
    if handle.is_null() {
        return 0.0;
    }

    let processor = &*(handle as *const MultibandProcessor);
    processor.get_soft_clipper_latency() as f32
}

// ============================================================================
// LUFS Metering FFI Functions
// ============================================================================

/// Get LUFS loudness readings.
/// Returns momentary (400ms), short-term (3s), and integrated (gated) loudness.
///
/// # Arguments
/// * `handle` - Processor handle from BASS_MultibandProcessor_Create
/// * `momentary` - Pointer to receive momentary LUFS (-100.0 if no data)
/// * `short_term` - Pointer to receive short-term LUFS (-100.0 if no data)
/// * `integrated` - Pointer to receive integrated LUFS (-100.0 if no data)
///
/// # Returns
/// TRUE on success, FALSE on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_GetLUFS(
    handle: *mut c_void,
    momentary: *mut f32,
    short_term: *mut f32,
    integrated: *mut f32,
) -> BOOL {
    if handle.is_null() || momentary.is_null() || short_term.is_null() || integrated.is_null() {
        return FALSE;
    }

    let processor = &*(handle as *const MultibandProcessor);
    let (m, s, i) = processor.get_lufs();
    *momentary = m;
    *short_term = s;
    *integrated = i;
    TRUE
}

/// Reset LUFS meter measurements.
/// Clears the integrated loudness measurement for new program material.
///
/// # Arguments
/// * `handle` - Processor handle
///
/// # Returns
/// TRUE on success, FALSE on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_ResetLUFS(handle: *mut c_void) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut MultibandProcessor);
    processor.reset_lufs();
    TRUE
}

/// Check if LUFS metering is enabled.
///
/// # Arguments
/// * `handle` - Processor handle
///
/// # Returns
/// TRUE (1) if LUFS metering is enabled, FALSE (0) if disabled.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_IsLUFSEnabled(handle: *mut c_void) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = &*(handle as *const MultibandProcessor);
    if processor.is_lufs_enabled() {
        TRUE
    } else {
        FALSE
    }
}

/// Enable or disable LUFS metering.
/// Disabling LUFS metering can save CPU if loudness measurement is not needed.
///
/// # Arguments
/// * `handle` - Processor handle
/// * `enabled` - TRUE (1) to enable, FALSE (0) to disable
///
/// # Returns
/// TRUE on success, FALSE on failure.
#[no_mangle]
pub unsafe extern "system" fn BASS_MultibandProcessor_SetLUFSEnabled(
    handle: *mut c_void,
    enabled: BOOL,
) -> BOOL {
    if handle.is_null() {
        return FALSE;
    }

    let processor = &mut *(handle as *mut MultibandProcessor);
    processor.set_lufs_enabled(enabled != 0);
    TRUE
}

// Windows DLL entry point
#[cfg(windows)]
#[no_mangle]
pub unsafe extern "system" fn DllMain(
    _hinst: *mut c_void,
    reason: DWORD,
    _reserved: *mut c_void,
) -> BOOL {
    const DLL_PROCESS_ATTACH: DWORD = 1;

    if reason == DLL_PROCESS_ATTACH {
        // Verify BASS version
        let version = BASS_GetVersion();
        if (version >> 16) < 0x204 {
            return FALSE;
        }
    }
    TRUE
}
