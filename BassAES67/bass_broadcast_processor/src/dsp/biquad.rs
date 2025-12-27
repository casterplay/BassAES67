//! Biquad filter implementation (Direct Form II Transposed).
//! Used for crossover filters and EQ.

use std::f32::consts::PI;

/// Butterworth Q factor (1/sqrt(2)) for maximally flat response.
const BUTTERWORTH_Q: f32 = 0.7071067811865476;

/// Generic biquad filter supporting stereo processing.
#[derive(Clone)]
pub struct Biquad {
    // Normalized coefficients
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    // State per channel (supports stereo)
    z1: [f32; 2],
    z2: [f32; 2],
}

impl Biquad {
    /// Create a lowpass Butterworth filter.
    pub fn lowpass(freq: f32, sample_rate: f32) -> Self {
        let omega = 2.0 * PI * freq / sample_rate;
        let cos_omega = omega.cos();
        let sin_omega = omega.sin();
        let alpha = sin_omega / (2.0 * BUTTERWORTH_Q);

        let b0 = (1.0 - cos_omega) / 2.0;
        let b1 = 1.0 - cos_omega;
        let b2 = (1.0 - cos_omega) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_omega;
        let a2 = 1.0 - alpha;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: [0.0; 2],
            z2: [0.0; 2],
        }
    }

    /// Create a highpass Butterworth filter.
    pub fn highpass(freq: f32, sample_rate: f32) -> Self {
        let omega = 2.0 * PI * freq / sample_rate;
        let cos_omega = omega.cos();
        let sin_omega = omega.sin();
        let alpha = sin_omega / (2.0 * BUTTERWORTH_Q);

        let b0 = (1.0 + cos_omega) / 2.0;
        let b1 = -(1.0 + cos_omega);
        let b2 = (1.0 + cos_omega) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_omega;
        let a2 = 1.0 - alpha;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: [0.0; 2],
            z2: [0.0; 2],
        }
    }

    /// Process a single sample for the given channel.
    /// Uses Direct Form II Transposed for numerical stability.
    #[inline]
    pub fn process(&mut self, input: f32, channel: usize) -> f32 {
        let output = self.b0 * input + self.z1[channel];
        self.z1[channel] = self.b1 * input - self.a1 * output + self.z2[channel];
        self.z2[channel] = self.b2 * input - self.a2 * output;
        output
    }

    /// Reset filter state to zero.
    pub fn reset(&mut self) {
        self.z1 = [0.0; 2];
        self.z2 = [0.0; 2];
    }

    /// Create a biquad from pre-computed normalized coefficients.
    /// Used for K-weighting filters and other custom filter designs.
    pub fn from_coefficients(b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) -> Self {
        Self {
            b0,
            b1,
            b2,
            a1,
            a2,
            z1: [0.0; 2],
            z2: [0.0; 2],
        }
    }

    /// Create a peaking (parametric) EQ filter using RBJ Audio EQ Cookbook.
    ///
    /// # Arguments
    /// * `freq` - Center frequency in Hz
    /// * `q` - Q factor (bandwidth), higher = narrower (0.1 to 10.0 typical)
    /// * `gain_db` - Gain in dB (-12 to +12 typical)
    /// * `sample_rate` - Audio sample rate
    pub fn peaking(freq: f32, q: f32, gain_db: f32, sample_rate: f32) -> Self {
        let a = 10.0f32.powf(gain_db / 40.0); // sqrt of linear gain
        let omega = 2.0 * PI * freq / sample_rate;
        let cos_omega = omega.cos();
        let sin_omega = omega.sin();
        let alpha = sin_omega / (2.0 * q);

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_omega;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_omega;
        let a2 = 1.0 - alpha / a;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: [0.0; 2],
            z2: [0.0; 2],
        }
    }

    /// Create a high shelf filter using RBJ Audio EQ Cookbook.
    /// Used for K-weighting filter stage 1.
    ///
    /// # Arguments
    /// * `freq` - Shelf corner frequency in Hz
    /// * `gain_db` - Gain in dB above shelf frequency
    /// * `sample_rate` - Audio sample rate
    pub fn high_shelf(freq: f32, gain_db: f32, sample_rate: f32) -> Self {
        let a = 10.0f32.powf(gain_db / 40.0);
        let omega = 2.0 * PI * freq / sample_rate;
        let cos_omega = omega.cos();
        let sin_omega = omega.sin();
        // Use slope S=1 for standard shelf
        let alpha = sin_omega / 2.0 * ((a + 1.0 / a) * (1.0 / 1.0 - 1.0) + 2.0).sqrt();

        let b0 = a * ((a + 1.0) + (a - 1.0) * cos_omega + 2.0 * a.sqrt() * alpha);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_omega);
        let b2 = a * ((a + 1.0) + (a - 1.0) * cos_omega - 2.0 * a.sqrt() * alpha);
        let a0 = (a + 1.0) - (a - 1.0) * cos_omega + 2.0 * a.sqrt() * alpha;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_omega);
        let a2 = (a + 1.0) - (a - 1.0) * cos_omega - 2.0 * a.sqrt() * alpha;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: [0.0; 2],
            z2: [0.0; 2],
        }
    }

    /// Create a high-pass filter with configurable Q.
    /// Used for K-weighting filter stage 2.
    ///
    /// # Arguments
    /// * `freq` - Cutoff frequency in Hz
    /// * `q` - Q factor (0.5 for K-weighting)
    /// * `sample_rate` - Audio sample rate
    pub fn highpass_q(freq: f32, q: f32, sample_rate: f32) -> Self {
        let omega = 2.0 * PI * freq / sample_rate;
        let cos_omega = omega.cos();
        let sin_omega = omega.sin();
        let alpha = sin_omega / (2.0 * q);

        let b0 = (1.0 + cos_omega) / 2.0;
        let b1 = -(1.0 + cos_omega);
        let b2 = (1.0 + cos_omega) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_omega;
        let a2 = 1.0 - alpha;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: [0.0; 2],
            z2: [0.0; 2],
        }
    }
}
