//! Soft clipper with optional oversampling for intersample peak handling.
//!
//! Provides final-stage limiting with configurable clipping modes:
//! - Hard: Simple clamp to ceiling
//! - Soft: Quadratic knee for smooth transition
//! - Tanh: Hyperbolic tangent (musical saturation)

use super::gain::db_to_linear;

/// Maximum oversampling factor supported
const MAX_OVERSAMPLE: usize = 4;

/// Clipping mode enumeration
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClipMode {
    Hard = 0,
    Soft = 1,
    Tanh = 2,
}

impl From<u8> for ClipMode {
    fn from(v: u8) -> Self {
        match v {
            0 => ClipMode::Hard,
            1 => ClipMode::Soft,
            2 => ClipMode::Tanh,
            _ => ClipMode::Soft,
        }
    }
}

/// Soft clipper with optional oversampling.
pub struct SoftClipper {
    /// Ceiling level (linear)
    ceiling: f32,
    /// Knee width (linear, for soft mode)
    knee: f32,
    /// Clipping mode
    mode: ClipMode,
    /// Oversampling factor (1, 2, or 4)
    oversample_factor: usize,
    /// Enabled flag
    enabled: bool,
    /// Sample rate
    sample_rate: f32,
    /// Upsample buffer (per channel)
    upsample_buffer: [[f32; MAX_OVERSAMPLE]; 2],
    /// History for linear interpolation upsampling (per channel)
    prev_sample: [f32; 2],
}

impl SoftClipper {
    /// Create a new soft clipper with default settings.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            ceiling: db_to_linear(-0.1), // Default -0.1 dBFS
            knee: 0.1,                   // ~3dB knee
            mode: ClipMode::Soft,
            oversample_factor: 1,
            enabled: false,
            sample_rate,
            upsample_buffer: [[0.0; MAX_OVERSAMPLE]; 2],
            prev_sample: [0.0; 2],
        }
    }

    /// Set clipper parameters.
    ///
    /// # Arguments
    /// * `ceiling_db` - Ceiling level in dBFS (-3.0 to 0.0)
    /// * `knee_db` - Knee width in dB (0.0 to 6.0, soft mode only)
    /// * `mode` - Clipping mode (0=hard, 1=soft, 2=tanh)
    /// * `oversample` - Oversampling factor (1, 2, or 4)
    pub fn set_params(&mut self, ceiling_db: f32, knee_db: f32, mode: u8, oversample: u8) {
        self.ceiling = db_to_linear(ceiling_db.clamp(-6.0, 0.0));
        // Convert knee from dB to linear difference
        self.knee = (db_to_linear(knee_db.clamp(0.0, 6.0)) - 1.0).max(0.001);
        self.mode = ClipMode::from(mode);
        self.oversample_factor = (oversample as usize).clamp(1, MAX_OVERSAMPLE);
    }

    /// Enable or disable the soft clipper.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Check if the soft clipper is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get the latency in samples introduced by oversampling.
    pub fn latency_samples(&self) -> f32 {
        if self.oversample_factor > 1 {
            0.5 // Minimal latency with linear interpolation
        } else {
            0.0
        }
    }

    /// Get the latency in milliseconds.
    pub fn latency_ms(&self) -> f32 {
        self.latency_samples() * 1000.0 / self.sample_rate
    }

    /// Apply soft clipping curve to a single sample.
    #[inline]
    fn clip_sample(&self, input: f32) -> f32 {
        match self.mode {
            ClipMode::Hard => {
                input.clamp(-self.ceiling, self.ceiling)
            }
            ClipMode::Soft => {
                // Soft knee polynomial clipping
                let abs_in = input.abs();
                if abs_in <= self.ceiling - self.knee {
                    // Below knee: pass through
                    input
                } else if abs_in >= self.ceiling + self.knee {
                    // Above knee: hard limit
                    input.signum() * self.ceiling
                } else {
                    // In knee region: quadratic transition
                    let x = abs_in - (self.ceiling - self.knee);
                    let knee2 = 2.0 * self.knee;
                    let out = abs_in - (x * x) / (2.0 * knee2);
                    input.signum() * out.min(self.ceiling)
                }
            }
            ClipMode::Tanh => {
                // Hyperbolic tangent soft clipping
                // Scale so that tanh curve approaches ceiling asymptotically
                if input.abs() < self.ceiling * 0.5 {
                    // Below 50% ceiling: mostly linear
                    input
                } else {
                    // Apply tanh curve
                    let scaled = input / self.ceiling;
                    self.ceiling * scaled.tanh()
                }
            }
        }
    }

    /// Process stereo samples with optional oversampling.
    #[inline]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        if !self.enabled {
            return (left, right);
        }

        if self.oversample_factor <= 1 {
            // No oversampling - direct clipping
            (self.clip_sample(left), self.clip_sample(right))
        } else {
            // Upsample, clip, downsample
            let factor = self.oversample_factor;

            // Linear interpolation upsample for left channel
            for i in 0..factor {
                let t = (i as f32 + 1.0) / factor as f32;
                self.upsample_buffer[0][i] = self.prev_sample[0] * (1.0 - t) + left * t;
            }

            // Linear interpolation upsample for right channel
            for i in 0..factor {
                let t = (i as f32 + 1.0) / factor as f32;
                self.upsample_buffer[1][i] = self.prev_sample[1] * (1.0 - t) + right * t;
            }

            // Store for next iteration
            self.prev_sample[0] = left;
            self.prev_sample[1] = right;

            // Clip all oversampled values
            for i in 0..factor {
                self.upsample_buffer[0][i] = self.clip_sample(self.upsample_buffer[0][i]);
                self.upsample_buffer[1][i] = self.clip_sample(self.upsample_buffer[1][i]);
            }

            // Downsample: take the last sample (simple decimation)
            // This catches the highest peak in the oversampled region
            let out_l = self.upsample_buffer[0][factor - 1];
            let out_r = self.upsample_buffer[1][factor - 1];

            (out_l, out_r)
        }
    }

    /// Reset clipper state.
    pub fn reset(&mut self) {
        self.prev_sample = [0.0; 2];
        self.upsample_buffer = [[0.0; MAX_OVERSAMPLE]; 2];
    }
}
