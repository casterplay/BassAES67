//! Lock-free statistics for real-time metering.

use std::sync::atomic::{AtomicI32, AtomicU64};

/// Lock-free atomic statistics for real-time metering.
/// Uses atomics to avoid locking in the audio path.
pub struct AtomicStats {
    /// Total samples processed
    pub samples_processed: AtomicU64,
    /// Input peak level (scaled: value * 1000 for 3 decimal precision)
    pub input_peak_x1000: AtomicI32,
    /// Output peak level (scaled: value * 1000)
    pub output_peak_x1000: AtomicI32,
    /// Low band gain reduction in dB * 100
    pub low_gr_x100: AtomicI32,
    /// High band gain reduction in dB * 100
    pub high_gr_x100: AtomicI32,
    /// Underrun count (source returned less data than requested)
    pub underruns: AtomicU64,
    /// Last processing time in microseconds
    pub process_time_us: AtomicU64,
}

impl AtomicStats {
    /// Create new zeroed statistics.
    pub fn new() -> Self {
        Self {
            samples_processed: AtomicU64::new(0),
            input_peak_x1000: AtomicI32::new(0),
            output_peak_x1000: AtomicI32::new(0),
            low_gr_x100: AtomicI32::new(0),
            high_gr_x100: AtomicI32::new(0),
            underruns: AtomicU64::new(0),
            process_time_us: AtomicU64::new(0),
        }
    }
}

impl Default for AtomicStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics snapshot for external access (FFI-compatible).
#[repr(C)]
#[derive(Default, Clone, Debug)]
pub struct ProcessorStats {
    /// Total samples processed
    pub samples_processed: u64,
    /// Input peak level (linear, 0.0 to 1.0+)
    pub input_peak: f32,
    /// Output peak level (linear, 0.0 to 1.0+)
    pub output_peak: f32,
    /// Low band gain reduction in dB (negative when compressing)
    pub low_band_gr_db: f32,
    /// High band gain reduction in dB (negative when compressing)
    pub high_band_gr_db: f32,
    /// Number of source underruns
    pub underruns: u64,
    /// Last processing time in microseconds
    pub process_time_us: u64,
}

// ============================================================================
// N-Band Multiband Processor Statistics
// ============================================================================

/// Lock-free atomic statistics for N-band multiband processor.
/// Uses atomics to avoid locking in the audio path.
pub struct MultibandAtomicStats {
    /// Total samples (frames) processed
    pub samples_processed: AtomicU64,
    /// Input peak level (scaled: value * 1000 for 3 decimal precision)
    pub input_peak_x1000: AtomicI32,
    /// Output peak level (scaled: value * 1000)
    pub output_peak_x1000: AtomicI32,
    /// Per-band gain reduction in dB * 100
    pub band_gr_x100: Vec<AtomicI32>,
    /// AGC gain reduction in dB * 100 (Phase 3)
    pub agc_gr_x100: AtomicI32,
    /// Underrun count (source returned less data than requested)
    pub underruns: AtomicU64,
    /// Last processing time in microseconds
    pub process_time_us: AtomicU64,
    /// LUFS momentary loudness * 100 (Phase 3 - LUFS metering)
    pub lufs_momentary_x100: AtomicI32,
    /// LUFS short-term loudness * 100
    pub lufs_short_x100: AtomicI32,
    /// LUFS integrated loudness * 100
    pub lufs_integrated_x100: AtomicI32,
}

impl MultibandAtomicStats {
    /// Create new zeroed statistics for N bands.
    pub fn new(num_bands: usize) -> Self {
        let mut band_gr = Vec::with_capacity(num_bands);
        for _ in 0..num_bands {
            band_gr.push(AtomicI32::new(0));
        }

        Self {
            samples_processed: AtomicU64::new(0),
            input_peak_x1000: AtomicI32::new(0),
            output_peak_x1000: AtomicI32::new(0),
            band_gr_x100: band_gr,
            agc_gr_x100: AtomicI32::new(0),
            underruns: AtomicU64::new(0),
            process_time_us: AtomicU64::new(0),
            lufs_momentary_x100: AtomicI32::new(-10000), // -100.0 LUFS
            lufs_short_x100: AtomicI32::new(-10000),
            lufs_integrated_x100: AtomicI32::new(-10000),
        }
    }

    /// Get the number of bands.
    pub fn num_bands(&self) -> usize {
        self.band_gr_x100.len()
    }
}

/// FFI-compatible statistics header for N-band multiband processor.
/// Per-band gain reduction values are returned in a separate buffer.
#[repr(C)]
#[derive(Clone, Debug)]
pub struct MultibandStatsHeader {
    /// Total samples (frames) processed
    pub samples_processed: u64,
    /// Input peak level (linear, 0.0 to 1.0+)
    pub input_peak: f32,
    /// Output peak level (linear, 0.0 to 1.0+)
    pub output_peak: f32,
    /// Number of bands (for caller to know buffer size)
    pub num_bands: u32,
    /// AGC gain reduction in dB (negative when compressing) - Phase 3
    pub agc_gr_db: f32,
    /// Number of source underruns
    pub underruns: u64,
    /// Last processing time in microseconds
    pub process_time_us: u64,
    /// Momentary loudness (LUFS, 400ms window) - Phase 3
    pub lufs_momentary: f32,
    /// Short-term loudness (LUFS, 3s window)
    pub lufs_short_term: f32,
    /// Integrated loudness (LUFS, gated)
    pub lufs_integrated: f32,
    /// Padding for alignment
    pub _pad: u32,
}

impl Default for MultibandStatsHeader {
    fn default() -> Self {
        Self {
            samples_processed: 0,
            input_peak: 0.0,
            output_peak: 0.0,
            num_bands: 0,
            agc_gr_db: 0.0,
            underruns: 0,
            process_time_us: 0,
            lufs_momentary: -100.0,
            lufs_short_term: -100.0,
            lufs_integrated: -100.0,
            _pad: 0,
        }
    }
}
