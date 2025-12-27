//! Linkwitz-Riley 4th order crossover filter.
//! Provides perfect reconstruction when low and high bands are summed.

use super::biquad::Biquad;

/// Linkwitz-Riley 4th order (LR4) crossover.
/// Consists of two cascaded 2nd-order Butterworth filters per band.
/// When low and high outputs are summed, the result is perfectly flat.
pub struct LR4Crossover {
    /// First-stage lowpass
    lp1: Biquad,
    /// Second-stage lowpass (cascade)
    lp2: Biquad,
    /// First-stage highpass
    hp1: Biquad,
    /// Second-stage highpass (cascade)
    hp2: Biquad,
}

impl LR4Crossover {
    /// Create a new LR4 crossover at the given frequency.
    pub fn new(crossover_freq: f32, sample_rate: f32) -> Self {
        Self {
            lp1: Biquad::lowpass(crossover_freq, sample_rate),
            lp2: Biquad::lowpass(crossover_freq, sample_rate),
            hp1: Biquad::highpass(crossover_freq, sample_rate),
            hp2: Biquad::highpass(crossover_freq, sample_rate),
        }
    }

    /// Split input sample into low and high bands.
    /// Returns (low, high) for the given channel.
    #[inline]
    pub fn split(&mut self, input: f32, channel: usize) -> (f32, f32) {
        // LR4 = two cascaded 2nd-order Butterworth filters
        let low1 = self.lp1.process(input, channel);
        let low = self.lp2.process(low1, channel);

        let high1 = self.hp1.process(input, channel);
        let high = self.hp2.process(high1, channel);

        (low, high)
    }

    /// Reset all filter states to zero.
    pub fn reset(&mut self) {
        self.lp1.reset();
        self.lp2.reset();
        self.hp1.reset();
        self.hp2.reset();
    }
}
