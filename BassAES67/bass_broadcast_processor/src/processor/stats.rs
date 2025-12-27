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
