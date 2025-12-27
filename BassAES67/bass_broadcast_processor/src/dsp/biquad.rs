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
}
