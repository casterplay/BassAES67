//! N-band multiband crossover using cascaded LR4 crossovers.
//! Supports any number of bands (2, 5, 8, etc.).

use super::crossover::LR4Crossover;

/// N-band crossover using cascaded Linkwitz-Riley 4th order filters.
///
/// For N bands, uses N-1 crossover points. Each crossover splits the signal
/// into low and high components, with the high component fed to the next
/// crossover in the chain.
///
/// Example for 5 bands with frequencies [100, 400, 2000, 8000]:
/// ```text
/// Input ──┬── LP 100Hz ──────────────────────────► Band 0 (sub-bass)
///         └── HP 100Hz ──┬── LP 400Hz ───────────► Band 1 (bass)
///                        └── HP 400Hz ──┬── LP 2kHz ► Band 2 (mid)
///                                       └── HP 2kHz ──┬── LP 8kHz ► Band 3 (presence)
///                                                     └── HP 8kHz ► Band 4 (brilliance)
/// ```
pub struct MultibandCrossover {
    /// N-1 crossovers for N bands
    crossovers: Vec<LR4Crossover>,
    /// Number of output bands
    num_bands: usize,
    /// Sample rate
    sample_rate: f32,
}

impl MultibandCrossover {
    /// Create a new N-band crossover.
    ///
    /// # Arguments
    /// * `freqs` - Crossover frequencies in Hz. Length must be N-1 for N bands.
    ///             Must be in ascending order.
    /// * `sample_rate` - Audio sample rate in Hz.
    ///
    /// # Examples
    /// ```
    /// // 2-band crossover at 400 Hz
    /// let xover = MultibandCrossover::new(&[400.0], 48000.0);
    ///
    /// // 5-band crossover
    /// let xover = MultibandCrossover::new(&[100.0, 400.0, 2000.0, 8000.0], 48000.0);
    /// ```
    pub fn new(freqs: &[f32], sample_rate: f32) -> Self {
        let num_bands = freqs.len() + 1;
        let crossovers = freqs
            .iter()
            .map(|&freq| LR4Crossover::new(freq, sample_rate))
            .collect();

        Self {
            crossovers,
            num_bands,
            sample_rate,
        }
    }

    /// Split input sample into N bands.
    ///
    /// # Arguments
    /// * `input` - Input sample
    /// * `channel` - Channel index (0 for left, 1 for right in stereo)
    /// * `out` - Output slice to receive band samples. Must have length >= num_bands.
    ///
    /// # Panics
    /// Panics if `out.len() < num_bands`.
    #[inline]
    pub fn split(&mut self, input: f32, channel: usize, out: &mut [f32]) {
        debug_assert!(out.len() >= self.num_bands, "Output buffer too small");

        if self.crossovers.is_empty() {
            // Single band - pass through
            out[0] = input;
            return;
        }

        // Cascade through crossovers
        // Each crossover splits: low goes to output, high goes to next crossover
        let mut signal = input;

        for (i, xover) in self.crossovers.iter_mut().enumerate() {
            let (low, high) = xover.split(signal, channel);
            out[i] = low;
            signal = high;
        }

        // Last band gets the remaining high frequency content
        out[self.num_bands - 1] = signal;
    }

    /// Get the number of output bands.
    #[inline]
    pub fn num_bands(&self) -> usize {
        self.num_bands
    }

    /// Get the sample rate.
    #[inline]
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Reset all filter states to zero.
    pub fn reset(&mut self) {
        for xover in &mut self.crossovers {
            xover.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_2band_crossover() {
        let mut xover = MultibandCrossover::new(&[400.0], 48000.0);
        assert_eq!(xover.num_bands(), 2);

        let mut out = [0.0f32; 2];
        xover.split(1.0, 0, &mut out);

        // Both bands should have some signal
        assert!(out[0] != 0.0 || out[1] != 0.0);
    }

    #[test]
    fn test_5band_crossover() {
        let mut xover = MultibandCrossover::new(&[100.0, 400.0, 2000.0, 8000.0], 48000.0);
        assert_eq!(xover.num_bands(), 5);

        let mut out = [0.0f32; 5];
        xover.split(1.0, 0, &mut out);

        // All bands should have some signal (impulse response)
        let sum: f32 = out.iter().sum();
        assert!(sum != 0.0);
    }

    #[test]
    fn test_perfect_reconstruction() {
        // LR4 crossovers should sum to unity (perfect reconstruction)
        let mut xover = MultibandCrossover::new(&[1000.0], 48000.0);
        let mut out = [0.0f32; 2];

        // Process several samples to let filters settle
        for _ in 0..1000 {
            xover.split(1.0, 0, &mut out);
        }

        // Sum should be close to 1.0 at steady state
        let sum = out[0] + out[1];
        assert!((sum - 1.0).abs() < 0.01, "Sum was {} but expected ~1.0", sum);
    }
}
