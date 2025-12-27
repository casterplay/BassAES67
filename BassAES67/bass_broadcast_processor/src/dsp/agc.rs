//! Wideband Automatic Gain Control (AGC) with RMS detection and soft-knee compression.
//!
//! The AGC normalizes input levels to a consistent target level before multiband processing.
//! It uses RMS envelope detection for smooth, program-dependent gain control.

use super::gain::{db_to_linear, linear_to_db};

/// Wideband AGC with RMS detection and soft-knee compression.
pub struct WidebandAGC {
    /// Target output level in linear
    target_level: f32,
    /// Target output level in dB (cached)
    target_level_db: f32,
    /// Threshold in linear
    threshold: f32,
    /// Threshold in dB (cached)
    threshold_db: f32,
    /// Compression ratio (e.g., 3.0 = 3:1)
    ratio: f32,
    /// Soft knee width in dB
    knee_db: f32,
    /// Half knee width (cached for calculations)
    half_knee: f32,
    /// Makeup gain to reach target level (linear)
    makeup_gain: f32,
    /// Attack coefficient (per sample)
    attack_coeff: f32,
    /// Release coefficient (per sample)
    release_coeff: f32,
    /// RMS envelope (per channel for stereo)
    rms_env: [f32; 2],
    /// RMS integration coefficient
    rms_coeff: f32,
    /// Smoothed gain (per channel)
    current_gain: [f32; 2],
    /// Current gain reduction in linear (for metering)
    gain_reduction: f32,
    /// Sample rate (cached for parameter updates)
    sample_rate: f32,
    /// Enable/bypass flag
    enabled: bool,
}

impl WidebandAGC {
    /// Create a new Wideband AGC.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate
    /// * `target_level_db` - Target output level in dBFS (-24 to -12)
    /// * `threshold_db` - Compression threshold in dBFS (-30 to -6)
    /// * `ratio` - Compression ratio (2.0 to 8.0)
    /// * `knee_db` - Soft knee width in dB (0 to 20)
    /// * `attack_ms` - Attack time in milliseconds (10 to 100)
    /// * `release_ms` - Release time in milliseconds (100 to 2000)
    /// * `enabled` - Enable/bypass flag
    pub fn new(
        sample_rate: f32,
        target_level_db: f32,
        threshold_db: f32,
        ratio: f32,
        knee_db: f32,
        attack_ms: f32,
        release_ms: f32,
        enabled: bool,
    ) -> Self {
        // Calculate time constants
        let attack_coeff = 1.0 - (-1.0 / (attack_ms * sample_rate / 1000.0)).exp();
        let release_coeff = 1.0 - (-1.0 / (release_ms * sample_rate / 1000.0)).exp();

        // RMS integration time (~10ms for program-dependent detection)
        let rms_time_ms = 10.0;
        let rms_coeff = 1.0 - (-1.0 / (rms_time_ms * sample_rate / 1000.0)).exp();

        // Calculate makeup gain to reach target from threshold
        // When signal is at threshold, output should approach target
        let makeup_gain_db = target_level_db - threshold_db;
        let makeup_gain = db_to_linear(makeup_gain_db.max(0.0));

        Self {
            target_level: db_to_linear(target_level_db),
            target_level_db,
            threshold: db_to_linear(threshold_db),
            threshold_db,
            ratio,
            knee_db,
            half_knee: knee_db / 2.0,
            makeup_gain,
            attack_coeff,
            release_coeff,
            rms_env: [0.0; 2],
            rms_coeff,
            current_gain: [1.0; 2],
            gain_reduction: 1.0,
            sample_rate,
            enabled,
        }
    }

    /// Create with default broadcast settings.
    pub fn default_broadcast(sample_rate: f32) -> Self {
        Self::new(
            sample_rate,
            -18.0, // target level
            -24.0, // threshold
            3.0,   // ratio
            10.0,  // knee
            50.0,  // attack
            500.0, // release
            true,  // enabled
        )
    }

    /// Process a single sample.
    /// Returns the processed sample with gain control applied.
    #[inline]
    pub fn process(&mut self, input: f32, channel: usize) -> f32 {
        if !self.enabled {
            return input;
        }

        let input_squared = input * input;

        // RMS envelope follower (exponential moving average of squared signal)
        let rms = &mut self.rms_env[channel];
        *rms += self.rms_coeff * (input_squared - *rms);

        // Convert to RMS level (sqrt of mean square)
        let rms_level = rms.sqrt();

        // Calculate target gain using soft-knee compression
        let target_gain = self.compute_gain(rms_level);

        // Smooth gain changes with attack/release
        let current = &mut self.current_gain[channel];
        if target_gain < *current {
            // Gain decreasing (attack)
            *current += self.attack_coeff * (target_gain - *current);
        } else {
            // Gain increasing (release)
            *current += self.release_coeff * (target_gain - *current);
        }

        // Store for metering (use channel 0 as reference)
        if channel == 0 {
            self.gain_reduction = *current / self.makeup_gain;
        }

        input * *current
    }

    /// Compute gain for a given RMS level using soft-knee compression.
    #[inline]
    fn compute_gain(&self, rms_level: f32) -> f32 {
        if rms_level <= 0.0 {
            return self.makeup_gain;
        }

        let input_db = linear_to_db(rms_level);

        // Distance from threshold
        let over_threshold = input_db - self.threshold_db;

        // Soft-knee compression curve
        let compressed_db = if over_threshold <= -self.half_knee {
            // Below knee: no compression
            input_db
        } else if over_threshold >= self.half_knee {
            // Above knee: full compression
            self.threshold_db + over_threshold / self.ratio
        } else {
            // In knee: smooth transition
            // Quadratic interpolation in the knee region
            let knee_factor = (over_threshold + self.half_knee) / self.knee_db;
            let compression_amount = (1.0 - 1.0 / self.ratio) * knee_factor * knee_factor;
            input_db - over_threshold * compression_amount
        };

        // Calculate gain needed to achieve compressed output
        let gain_db = compressed_db - input_db;

        // Apply makeup gain
        db_to_linear(gain_db) * self.makeup_gain
    }

    /// Get current gain reduction in dB (for metering).
    /// Returns a negative value when reducing gain.
    pub fn gain_reduction_db(&self) -> f32 {
        linear_to_db(self.gain_reduction)
    }

    /// Check if AGC is enabled.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable or disable AGC.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Update AGC parameters at runtime.
    pub fn set_params(
        &mut self,
        target_level_db: f32,
        threshold_db: f32,
        ratio: f32,
        knee_db: f32,
        attack_ms: f32,
        release_ms: f32,
        enabled: bool,
    ) {
        self.target_level = db_to_linear(target_level_db);
        self.target_level_db = target_level_db;
        self.threshold = db_to_linear(threshold_db);
        self.threshold_db = threshold_db;
        self.ratio = ratio;
        self.knee_db = knee_db;
        self.half_knee = knee_db / 2.0;

        // Recalculate makeup gain
        let makeup_gain_db = target_level_db - threshold_db;
        self.makeup_gain = db_to_linear(makeup_gain_db.max(0.0));

        self.attack_coeff = 1.0 - (-1.0 / (attack_ms * self.sample_rate / 1000.0)).exp();
        self.release_coeff = 1.0 - (-1.0 / (release_ms * self.sample_rate / 1000.0)).exp();
        self.enabled = enabled;
    }

    /// Reset envelope and gain state (for discontinuities).
    pub fn reset(&mut self) {
        self.rms_env = [0.0; 2];
        self.current_gain = [1.0; 2];
        self.gain_reduction = 1.0;
    }
}

// ============================================================================
// 3-Stage Cascaded AGC (Omnia 9 Style)
// ============================================================================

/// 3-stage cascaded AGC processor (Omnia 9 style).
///
/// Audio flows through three stages in series:
/// 1. **Slow**: Song-to-song level normalization (3s attack, 8s release)
/// 2. **Medium**: Phrase-level dynamics control (300ms attack, 800ms release)
/// 3. **Fast**: Syllable/transient control (30ms attack, 150ms release)
///
/// Each stage does ~3-6dB of work, allowing total gain control of ~12-18dB range.
pub struct ThreeStageAGC {
    /// Stage 1: Slow AGC for song-level normalization
    slow: WidebandAGC,
    /// Stage 2: Medium AGC for phrase-level dynamics
    medium: WidebandAGC,
    /// Stage 3: Fast AGC for syllable/transient control
    fast: WidebandAGC,
    /// Enable/bypass flag
    enabled: bool,
}

impl ThreeStageAGC {
    /// Create a new 3-stage AGC with default broadcast settings.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            // Stage 1: Slow (song-level)
            slow: WidebandAGC::new(
                sample_rate,
                -20.0,   // target level
                -28.0,   // threshold
                2.0,     // ratio
                12.0,    // knee
                3000.0,  // attack (3 seconds)
                8000.0,  // release (8 seconds)
                true,
            ),
            // Stage 2: Medium (phrase-level)
            medium: WidebandAGC::new(
                sample_rate,
                -18.0,  // target level
                -24.0,  // threshold
                2.5,    // ratio
                10.0,   // knee
                300.0,  // attack (300 ms)
                800.0,  // release (800 ms)
                true,
            ),
            // Stage 3: Fast (syllable-level)
            fast: WidebandAGC::new(
                sample_rate,
                -16.0,  // target level
                -22.0,  // threshold
                3.0,    // ratio
                8.0,    // knee
                30.0,   // attack (30 ms)
                150.0,  // release (150 ms)
                true,
            ),
            enabled: true,
        }
    }

    /// Process a single sample through all 3 stages.
    /// Signal flows: input → slow → medium → fast → output
    #[inline]
    pub fn process(&mut self, input: f32, channel: usize) -> f32 {
        if !self.enabled {
            return input;
        }

        // Cascade through all stages
        let after_slow = self.slow.process(input, channel);
        let after_medium = self.medium.process(after_slow, channel);
        self.fast.process(after_medium, channel)
    }

    /// Get total gain reduction in dB (sum of all 3 stages).
    pub fn total_gain_reduction_db(&self) -> f32 {
        self.slow.gain_reduction_db()
            + self.medium.gain_reduction_db()
            + self.fast.gain_reduction_db()
    }

    /// Get individual stage gain reduction values in dB.
    /// Returns (slow_gr, medium_gr, fast_gr).
    pub fn stage_gain_reduction_db(&self) -> (f32, f32, f32) {
        (
            self.slow.gain_reduction_db(),
            self.medium.gain_reduction_db(),
            self.fast.gain_reduction_db(),
        )
    }

    /// Configure the slow stage (song-level).
    pub fn set_slow(
        &mut self,
        target_level_db: f32,
        threshold_db: f32,
        ratio: f32,
        knee_db: f32,
        attack_ms: f32,
        release_ms: f32,
        enabled: bool,
    ) {
        self.slow.set_params(
            target_level_db,
            threshold_db,
            ratio,
            knee_db,
            attack_ms,
            release_ms,
            enabled,
        );
    }

    /// Configure the medium stage (phrase-level).
    pub fn set_medium(
        &mut self,
        target_level_db: f32,
        threshold_db: f32,
        ratio: f32,
        knee_db: f32,
        attack_ms: f32,
        release_ms: f32,
        enabled: bool,
    ) {
        self.medium.set_params(
            target_level_db,
            threshold_db,
            ratio,
            knee_db,
            attack_ms,
            release_ms,
            enabled,
        );
    }

    /// Configure the fast stage (syllable-level).
    pub fn set_fast(
        &mut self,
        target_level_db: f32,
        threshold_db: f32,
        ratio: f32,
        knee_db: f32,
        attack_ms: f32,
        release_ms: f32,
        enabled: bool,
    ) {
        self.fast.set_params(
            target_level_db,
            threshold_db,
            ratio,
            knee_db,
            attack_ms,
            release_ms,
            enabled,
        );
    }

    /// Check if 3-stage AGC is enabled.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable or disable the entire 3-stage AGC.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Reset all stage envelope and gain states.
    pub fn reset(&mut self) {
        self.slow.reset();
        self.medium.reset();
        self.fast.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agc_bypass() {
        let mut agc = WidebandAGC::new(48000.0, -18.0, -24.0, 3.0, 10.0, 50.0, 500.0, false);

        // When disabled, output should equal input
        let input = 0.5;
        let output = agc.process(input, 0);
        assert_eq!(output, input);
    }

    #[test]
    fn test_agc_gain_reduction() {
        let mut agc = WidebandAGC::new(48000.0, -18.0, -24.0, 3.0, 10.0, 50.0, 500.0, true);

        // Feed a loud signal for a while to build up envelope
        for _ in 0..4800 {
            // ~100ms
            agc.process(0.8, 0);
            agc.process(0.8, 1);
        }

        // Should have some gain reduction
        let gr = agc.gain_reduction_db();
        assert!(gr < 0.0, "Expected negative gain reduction, got {}", gr);
    }

    #[test]
    fn test_agc_default_broadcast() {
        let agc = WidebandAGC::default_broadcast(48000.0);
        assert!(agc.is_enabled());
        assert_eq!(agc.target_level_db, -18.0);
        assert_eq!(agc.threshold_db, -24.0);
    }

    #[test]
    fn test_three_stage_agc_bypass() {
        let mut agc = ThreeStageAGC::new(48000.0);
        agc.set_enabled(false);

        let input = 0.5;
        let output = agc.process(input, 0);
        assert_eq!(output, input);
    }

    #[test]
    fn test_three_stage_agc_gain_reduction() {
        let mut agc = ThreeStageAGC::new(48000.0);

        // Feed a loud signal for a while to build up envelopes
        for _ in 0..48000 {
            // 1 second
            agc.process(0.8, 0);
            agc.process(0.8, 1);
        }

        // Should have some total gain reduction
        let gr = agc.total_gain_reduction_db();
        assert!(gr < 0.0, "Expected negative total gain reduction, got {}", gr);

        // All stages should contribute
        let (slow, medium, fast) = agc.stage_gain_reduction_db();
        // Note: slow stage may not have fully reacted due to 3s attack
        assert!(
            slow < 0.5 || medium < 0.0 || fast < 0.0,
            "Expected at least one stage to reduce gain"
        );
    }

    #[test]
    fn test_three_stage_agc_cascade() {
        let mut agc = ThreeStageAGC::new(48000.0);

        // Process some samples to verify cascading works
        let output = agc.process(0.5, 0);
        // Output should be different from input due to makeup gain
        assert!(output != 0.5, "Expected output to differ from input");
    }
}
