//! Multiband Stereo Enhancer (Omnia 9 style).
//!
//! Dynamically controls stereo width per frequency band using Mid-Side processing.
//! The enhancer adjusts the L-R (side) vs L+R (mid) ratio to achieve a target stereo width.
//!
//! Key features:
//! - Per-band stereo width control
//! - Band 1 (bass) always bypassed to avoid phase issues
//! - Attack/release envelope following for smooth transitions
//! - Maximum gain/attenuation limits to prevent extreme stereo images

use super::gain::db_to_linear;

/// Single-band stereo enhancer with dynamic width control.
pub struct StereoEnhancerBand {
    /// Target stereo width ratio (S/M). 1.0 = natural, >1.0 = wider, <1.0 = narrower
    target_width: f32,
    /// Maximum gain boost to side signal (linear)
    max_gain: f32,
    /// Maximum attenuation to side signal (linear, stored as 1/attenuation for clamping)
    min_gain: f32,
    /// Attack coefficient (per sample) - for narrowing
    attack_coeff: f32,
    /// Release coefficient (per sample) - for widening
    release_coeff: f32,
    /// Enable flag
    enabled: bool,
    /// Current width factor (smoothed, linear)
    current_width_factor: f32,
    /// Sample rate (cached for parameter updates)
    sample_rate: f32,
}

impl StereoEnhancerBand {
    /// Create a new stereo enhancer band.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate
    /// * `target_width` - Target stereo width ratio (1.0 = natural)
    /// * `max_gain_db` - Maximum gain boost to side signal in dB (0-18)
    /// * `max_atten_db` - Maximum attenuation to side signal in dB (0-18)
    /// * `attack_ms` - Attack time in milliseconds (narrowing speed)
    /// * `release_ms` - Release time in milliseconds (widening speed)
    /// * `enabled` - Enable/bypass flag
    pub fn new(
        sample_rate: f32,
        target_width: f32,
        max_gain_db: f32,
        max_atten_db: f32,
        attack_ms: f32,
        release_ms: f32,
        enabled: bool,
    ) -> Self {
        // Calculate time constants
        let attack_coeff = 1.0 - (-1.0 / (attack_ms * sample_rate / 1000.0)).exp();
        let release_coeff = 1.0 - (-1.0 / (release_ms * sample_rate / 1000.0)).exp();

        // Convert dB limits to linear
        let max_gain = db_to_linear(max_gain_db);
        let min_gain = db_to_linear(-max_atten_db);

        Self {
            target_width,
            max_gain,
            min_gain,
            attack_coeff,
            release_coeff,
            enabled,
            current_width_factor: 1.0, // Start at natural width
            sample_rate,
        }
    }

    /// Create with default broadcast settings for a given band index.
    /// Band 0 (bass) returns a bypassed instance.
    pub fn default_for_band(sample_rate: f32, band_index: usize) -> Self {
        match band_index {
            0 => {
                // Band 1 (Bass): Always bypassed
                Self::new(sample_rate, 1.0, 0.0, 0.0, 50.0, 200.0, false)
            }
            1 => {
                // Band 2 (Low-Mid): Gentle enhancement
                Self::new(sample_rate, 1.0, 6.0, 6.0, 50.0, 200.0, true)
            }
            2 => {
                // Band 3 (Mid): Moderate enhancement
                Self::new(sample_rate, 1.2, 9.0, 9.0, 30.0, 150.0, true)
            }
            3 => {
                // Band 4 (Presence): More enhancement
                Self::new(sample_rate, 1.3, 12.0, 12.0, 20.0, 100.0, true)
            }
            _ => {
                // Band 5+ (Brilliance): Most enhancement
                Self::new(sample_rate, 1.4, 12.0, 12.0, 15.0, 80.0, true)
            }
        }
    }

    /// Process a stereo pair (left, right) and return enhanced (left, right).
    ///
    /// Uses Mid-Side processing:
    /// - Mid (M) = (L + R) / 2 (mono/center content)
    /// - Side (S) = (L - R) / 2 (stereo/side content)
    /// - Adjust width by scaling S relative to M
    /// - Convert back: L = M + S, R = M - S
    #[inline]
    pub fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        if !self.enabled {
            return (left, right);
        }

        // Convert to Mid-Side
        let mid = (left + right) * 0.5;
        let side = (left - right) * 0.5;

        // Measure current stereo width (ratio of side to mid)
        let mid_abs = mid.abs() + 1e-10; // Avoid division by zero
        let side_abs = side.abs();
        let current_width = side_abs / mid_abs;

        // Calculate desired width factor to achieve target width
        // If current_width is low and target is high, we need to boost side (factor > 1)
        // If current_width is high and target is low, we need to reduce side (factor < 1)
        let target_factor = if current_width < 1e-6 {
            // Nearly mono input - use target directly as factor
            self.target_width
        } else {
            self.target_width / current_width
        };

        // Clamp to limits
        let target_factor = target_factor.clamp(self.min_gain, self.max_gain);

        // Smooth width factor with envelope follower
        // Attack = narrowing (factor decreasing), Release = widening (factor increasing)
        let coeff = if target_factor < self.current_width_factor {
            self.attack_coeff // Narrowing
        } else {
            self.release_coeff // Widening
        };
        self.current_width_factor += coeff * (target_factor - self.current_width_factor);

        // Apply width factor to side signal
        let side_new = side * self.current_width_factor;

        // Convert back to L/R
        let left_out = mid + side_new;
        let right_out = mid - side_new;

        (left_out, right_out)
    }

    /// Get current width factor (for metering).
    pub fn current_width_factor(&self) -> f32 {
        self.current_width_factor
    }

    /// Check if band is enabled.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable or disable band.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Update parameters at runtime.
    pub fn set_params(
        &mut self,
        target_width: f32,
        max_gain_db: f32,
        max_atten_db: f32,
        attack_ms: f32,
        release_ms: f32,
        enabled: bool,
    ) {
        self.target_width = target_width;
        self.max_gain = db_to_linear(max_gain_db);
        self.min_gain = db_to_linear(-max_atten_db);
        self.attack_coeff = 1.0 - (-1.0 / (attack_ms * self.sample_rate / 1000.0)).exp();
        self.release_coeff = 1.0 - (-1.0 / (release_ms * self.sample_rate / 1000.0)).exp();
        self.enabled = enabled;
    }

    /// Reset envelope state.
    pub fn reset(&mut self) {
        self.current_width_factor = 1.0;
    }
}

/// Multiband stereo enhancer with up to 5 bands.
/// Band 0 (bass) is always bypassed to avoid phase issues.
pub struct StereoEnhancer {
    /// Per-band enhancers (up to 5)
    bands: [StereoEnhancerBand; 5],
    /// Global enable flag
    enabled: bool,
}

impl StereoEnhancer {
    /// Create a new multiband stereo enhancer with default broadcast settings.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            bands: [
                StereoEnhancerBand::default_for_band(sample_rate, 0),
                StereoEnhancerBand::default_for_band(sample_rate, 1),
                StereoEnhancerBand::default_for_band(sample_rate, 2),
                StereoEnhancerBand::default_for_band(sample_rate, 3),
                StereoEnhancerBand::default_for_band(sample_rate, 4),
            ],
            enabled: true,
        }
    }

    /// Process a stereo pair for a specific band.
    /// Band 0 always returns input unchanged (bass bypass).
    ///
    /// # Arguments
    /// * `band` - Band index (0-4)
    /// * `left` - Left channel sample
    /// * `right` - Right channel sample
    ///
    /// # Returns
    /// Processed (left, right) stereo pair
    #[inline]
    pub fn process_band(&mut self, band: usize, left: f32, right: f32) -> (f32, f32) {
        if !self.enabled || band >= 5 {
            return (left, right);
        }

        // Band 0 is always bypassed (handled by band's enabled flag)
        self.bands[band].process(left, right)
    }

    /// Check if globally enabled.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable or disable globally.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Get a reference to a specific band.
    pub fn band(&self, index: usize) -> Option<&StereoEnhancerBand> {
        self.bands.get(index)
    }

    /// Get a mutable reference to a specific band.
    pub fn band_mut(&mut self, index: usize) -> Option<&mut StereoEnhancerBand> {
        self.bands.get_mut(index)
    }

    /// Configure a specific band.
    pub fn set_band(
        &mut self,
        band_index: usize,
        target_width: f32,
        max_gain_db: f32,
        max_atten_db: f32,
        attack_ms: f32,
        release_ms: f32,
        enabled: bool,
    ) {
        if let Some(band) = self.bands.get_mut(band_index) {
            // Band 0 should never be enabled (bass protection)
            let effective_enabled = if band_index == 0 { false } else { enabled };
            band.set_params(
                target_width,
                max_gain_db,
                max_atten_db,
                attack_ms,
                release_ms,
                effective_enabled,
            );
        }
    }

    /// Reset all band states.
    pub fn reset(&mut self) {
        for band in &mut self.bands {
            band.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ms_conversion_identity() {
        let mut band = StereoEnhancerBand::new(48000.0, 1.0, 0.0, 0.0, 50.0, 200.0, true);

        // With target_width=1.0 and no gain limits, output should approximate input
        let left = 0.7;
        let right = 0.3;
        let (out_l, out_r) = band.process(left, right);

        // Allow small tolerance due to envelope following starting at 1.0
        assert!((out_l - left).abs() < 0.1, "Left: {} vs {}", out_l, left);
        assert!((out_r - right).abs() < 0.1, "Right: {} vs {}", out_r, right);
    }

    #[test]
    fn test_mono_stays_mono() {
        let mut band = StereoEnhancerBand::new(48000.0, 2.0, 18.0, 18.0, 10.0, 100.0, true);

        // Pure mono signal: L = R
        let mono_val = 0.5;
        let (out_l, out_r) = band.process(mono_val, mono_val);

        // Side signal is 0, so output should still be mono
        assert!(
            (out_l - out_r).abs() < 1e-6,
            "Mono should stay mono: L={}, R={}",
            out_l,
            out_r
        );
    }

    #[test]
    fn test_bypass() {
        let mut band = StereoEnhancerBand::new(48000.0, 2.0, 18.0, 18.0, 10.0, 100.0, false);

        let left = 0.7;
        let right = 0.3;
        let (out_l, out_r) = band.process(left, right);

        assert_eq!(out_l, left);
        assert_eq!(out_r, right);
    }

    #[test]
    fn test_bass_always_bypassed() {
        let enhancer = StereoEnhancer::new(48000.0);

        // Band 0 (bass) should always be disabled
        assert!(!enhancer.bands[0].is_enabled());
    }

    #[test]
    fn test_multiband_global_bypass() {
        let mut enhancer = StereoEnhancer::new(48000.0);
        enhancer.set_enabled(false);

        let left = 0.7;
        let right = 0.3;
        let (out_l, out_r) = enhancer.process_band(2, left, right);

        assert_eq!(out_l, left);
        assert_eq!(out_r, right);
    }

    #[test]
    fn test_stereo_widening() {
        let mut band = StereoEnhancerBand::new(48000.0, 2.0, 18.0, 18.0, 1.0, 1.0, true);

        // Feed stereo signal for a while to let envelope converge
        let left = 0.6;
        let right = 0.4;
        let mut out_l = 0.0;
        let mut out_r = 0.0;

        for _ in 0..48000 {
            // 1 second at 48kHz
            let result = band.process(left, right);
            out_l = result.0;
            out_r = result.1;
        }

        // Original stereo width: |L-R|/|L+R| = 0.2/1.0 = 0.2
        // With target_width=2.0, we should see more separation
        let original_diff = (left - right).abs();
        let new_diff = (out_l - out_r).abs();

        assert!(
            new_diff > original_diff,
            "Expected wider stereo image. Original diff: {}, New diff: {}",
            original_diff,
            new_diff
        );
    }

    #[test]
    fn test_stereo_narrowing() {
        let mut band = StereoEnhancerBand::new(48000.0, 0.5, 18.0, 18.0, 1.0, 1.0, true);

        // Feed stereo signal for a while
        let left = 0.8;
        let right = 0.2;
        let mut out_l = 0.0;
        let mut out_r = 0.0;

        for _ in 0..48000 {
            let result = band.process(left, right);
            out_l = result.0;
            out_r = result.1;
        }

        // With target_width=0.5, we should see less separation
        let original_diff = (left - right).abs();
        let new_diff = (out_l - out_r).abs();

        assert!(
            new_diff < original_diff,
            "Expected narrower stereo image. Original diff: {}, New diff: {}",
            original_diff,
            new_diff
        );
    }
}
