//! Broadcast Audio Processor for BASS
//!
//! A 2-band broadcast audio processor that sits between BASS audio streams.
//! Provides crossover filtering and per-band compression.

use std::ffi::c_void;
use std::ptr;

mod dsp;
mod ffi;
mod processor;

use ffi::*;
use processor::BroadcastProcessor;

// Re-export types for external use
pub use processor::{CompressorConfig, ProcessorConfig, ProcessorStats};

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
