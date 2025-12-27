//! ITU-R BS.1770 LUFS Loudness Meter implementation.
//!
//! Provides K-weighted loudness measurement with:
//! - Momentary loudness (400ms window)
//! - Short-term loudness (3s window)
//! - Integrated loudness (gated, full program)

use super::biquad::Biquad;
use std::collections::VecDeque;

/// Absolute gate threshold in LUFS
const ABSOLUTE_GATE_LUFS: f32 = -70.0;

/// Relative gate offset in LU (below ungated average)
const RELATIVE_GATE_LU: f32 = -10.0;

/// Block duration for gated measurement (100ms)
const BLOCK_DURATION_MS: f32 = 100.0;

/// Momentary window (400ms = 4 blocks)
const MOMENTARY_BLOCKS: usize = 4;

/// Short-term window (3000ms = 30 blocks)
const SHORT_TERM_BLOCKS: usize = 30;

/// K-weighting filter (2-stage: high shelf + high pass).
/// ITU-R BS.1770 specifies these filters for loudness measurement.
pub struct KWeightingFilter {
    /// Stage 1: High shelf (+4dB above ~1500Hz)
    shelf: Biquad,
    /// Stage 2: High pass (~38Hz, removes DC and subsonic)
    highpass: Biquad,
}

impl KWeightingFilter {
    /// Create a new K-weighting filter for the given sample rate.
    pub fn new(sample_rate: f32) -> Self {
        // ITU-R BS.1770-4 specifies these filter characteristics:
        // Stage 1: High shelf, +4dB gain, corner ~1681Hz
        // Stage 2: High pass, ~38Hz, Q ~0.5
        let shelf = Biquad::high_shelf(1681.974, 4.0, sample_rate);
        let highpass = Biquad::highpass_q(38.135, 0.5, sample_rate);

        Self { shelf, highpass }
    }

    /// Process a sample through both filter stages.
    #[inline]
    pub fn process(&mut self, input: f32, channel: usize) -> f32 {
        let stage1 = self.shelf.process(input, channel);
        self.highpass.process(stage1, channel)
    }

    /// Reset filter states.
    pub fn reset(&mut self) {
        self.shelf.reset();
        self.highpass.reset();
    }
}

/// LUFS Meter implementing ITU-R BS.1770.
pub struct LufsMeter {
    /// K-weighting filter
    k_filter: KWeightingFilter,
    /// Sample rate
    sample_rate: f32,
    /// Samples per 100ms block
    samples_per_block: usize,
    /// Current block sample counter
    block_sample_count: usize,
    /// Current block mean square accumulator (per channel)
    block_ms_sum: [f64; 2],
    /// Ring buffer of recent block powers (for momentary/short-term)
    block_powers: VecDeque<f64>,
    /// All blocks for integrated measurement (gated)
    all_blocks: Vec<f64>,
    /// Enabled flag
    enabled: bool,
    /// Cached momentary loudness (LUFS)
    momentary_lufs: f32,
    /// Cached short-term loudness (LUFS)
    short_term_lufs: f32,
    /// Cached integrated loudness (LUFS)
    integrated_lufs: f32,
}

impl LufsMeter {
    /// Create a new LUFS meter for the given sample rate.
    pub fn new(sample_rate: f32) -> Self {
        let samples_per_block = (BLOCK_DURATION_MS * sample_rate / 1000.0) as usize;

        Self {
            k_filter: KWeightingFilter::new(sample_rate),
            sample_rate,
            samples_per_block,
            block_sample_count: 0,
            block_ms_sum: [0.0; 2],
            block_powers: VecDeque::with_capacity(SHORT_TERM_BLOCKS + 1),
            all_blocks: Vec::with_capacity(1024),
            enabled: true,
            momentary_lufs: -100.0,
            short_term_lufs: -100.0,
            integrated_lufs: -100.0,
        }
    }

    /// Enable or disable LUFS metering.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Check if LUFS metering is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Process a stereo sample pair and update loudness measurements.
    #[inline]
    pub fn process(&mut self, left: f32, right: f32) {
        if !self.enabled {
            return;
        }

        // Apply K-weighting filter
        let filtered_l = self.k_filter.process(left, 0);
        let filtered_r = self.k_filter.process(right, 1);

        // Accumulate mean square (sum of squares)
        self.block_ms_sum[0] += (filtered_l * filtered_l) as f64;
        self.block_ms_sum[1] += (filtered_r * filtered_r) as f64;
        self.block_sample_count += 1;

        // Check if 100ms block is complete
        if self.block_sample_count >= self.samples_per_block {
            self.complete_block();
        }
    }

    /// Complete current 100ms block and update measurements.
    fn complete_block(&mut self) {
        let n = self.block_sample_count as f64;
        if n == 0.0 {
            return;
        }

        // Calculate mean square for this block
        // ITU-R BS.1770: For stereo, L and R have equal weight (1.0)
        let mean_l = self.block_ms_sum[0] / n;
        let mean_r = self.block_ms_sum[1] / n;
        let block_power = mean_l + mean_r; // Sum for stereo

        // Add to ring buffer for momentary/short-term
        self.block_powers.push_back(block_power);
        if self.block_powers.len() > SHORT_TERM_BLOCKS {
            self.block_powers.pop_front();
        }

        // Add to all blocks for integrated measurement
        self.all_blocks.push(block_power);

        // Calculate momentary (400ms = 4 blocks)
        self.momentary_lufs = self.calculate_windowed_lufs(MOMENTARY_BLOCKS);

        // Calculate short-term (3s = 30 blocks)
        self.short_term_lufs = self.calculate_windowed_lufs(SHORT_TERM_BLOCKS);

        // Calculate integrated (gated)
        self.integrated_lufs = self.calculate_integrated_lufs();

        // Reset for next block
        self.block_sample_count = 0;
        self.block_ms_sum = [0.0; 2];
    }

    /// Calculate windowed loudness from recent blocks.
    fn calculate_windowed_lufs(&self, num_blocks: usize) -> f32 {
        let count = self.block_powers.len().min(num_blocks);
        if count == 0 {
            return -100.0;
        }

        let sum: f64 = self.block_powers.iter().rev().take(count).sum();
        let mean = sum / count as f64;

        if mean <= 0.0 {
            -100.0
        } else {
            // LUFS = -0.691 + 10 * log10(mean_square)
            (-0.691 + 10.0 * mean.log10()) as f32
        }
    }

    /// Calculate integrated loudness with gating per ITU-R BS.1770.
    fn calculate_integrated_lufs(&self) -> f32 {
        if self.all_blocks.is_empty() {
            return -100.0;
        }

        // First pass: absolute gate (-70 LUFS)
        let abs_threshold = 10.0f64.powf((ABSOLUTE_GATE_LUFS as f64 + 0.691) / 10.0);
        let above_abs: Vec<f64> = self.all_blocks.iter()
            .filter(|&&p| p > abs_threshold)
            .copied()
            .collect();

        if above_abs.is_empty() {
            return -100.0;
        }

        // Calculate ungated mean (above absolute threshold)
        let ungated_mean: f64 = above_abs.iter().sum::<f64>() / above_abs.len() as f64;
        let ungated_lufs = -0.691 + 10.0 * ungated_mean.log10();

        // Second pass: relative gate (-10 LU below ungated mean)
        let rel_threshold_lufs = ungated_lufs + RELATIVE_GATE_LU as f64;
        let rel_threshold = 10.0f64.powf((rel_threshold_lufs + 0.691) / 10.0);

        let gated: Vec<f64> = above_abs.iter()
            .filter(|&&p| p > rel_threshold)
            .copied()
            .collect();

        if gated.is_empty() {
            return -100.0;
        }

        // Calculate gated mean (final integrated loudness)
        let gated_mean: f64 = gated.iter().sum::<f64>() / gated.len() as f64;
        (-0.691 + 10.0 * gated_mean.log10()) as f32
    }

    /// Get momentary loudness (400ms window).
    pub fn momentary_lufs(&self) -> f32 {
        self.momentary_lufs
    }

    /// Get short-term loudness (3s window).
    pub fn short_term_lufs(&self) -> f32 {
        self.short_term_lufs
    }

    /// Get integrated loudness (gated, full program).
    pub fn integrated_lufs(&self) -> f32 {
        self.integrated_lufs
    }

    /// Reset all measurements (for new program).
    pub fn reset(&mut self) {
        self.k_filter.reset();
        self.block_sample_count = 0;
        self.block_ms_sum = [0.0; 2];
        self.block_powers.clear();
        self.all_blocks.clear();
        self.momentary_lufs = -100.0;
        self.short_term_lufs = -100.0;
        self.integrated_lufs = -100.0;
    }

    /// Reset only the integrated measurement (keep short-term/momentary).
    pub fn reset_integrated(&mut self) {
        self.all_blocks.clear();
        self.integrated_lufs = -100.0;
    }
}
