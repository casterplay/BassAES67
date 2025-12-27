//! Audio compressor with envelope follower.

use super::gain::{db_to_linear, linear_to_db};

/// Per-band compressor with peak envelope follower.
pub struct Compressor {
    /// Threshold in linear
    threshold: f32,
    /// Threshold in dB (cached for calculations)
    threshold_db: f32,
    /// Compression ratio (e.g., 4.0 = 4:1)
    ratio: f32,
    /// Attack coefficient (per sample)
    attack_coeff: f32,
    /// Release coefficient (per sample)
    release_coeff: f32,
    /// Makeup gain in linear
    makeup_gain: f32,
    /// Current envelope level (per channel for stereo)
    envelope: [f32; 2],
    /// Current gain reduction in linear (for metering)
    gain_reduction: f32,
    /// Sample rate (cached for parameter updates)
    sample_rate: f32,
}

impl Compressor {
    /// Create a new compressor.
    ///
    /// # Arguments
    /// * `threshold_db` - Threshold in dBFS (-40 to 0)
    /// * `ratio` - Compression ratio (1.0 = no compression, 10.0 = heavy)
    /// * `attack_ms` - Attack time in milliseconds (0.5 to 100)
    /// * `release_ms` - Release time in milliseconds (10 to 1000)
    /// * `makeup_gain_db` - Makeup gain in dB (0 to 20)
    /// * `sample_rate` - Audio sample rate
    pub fn new(
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        makeup_gain_db: f32,
        sample_rate: f32,
    ) -> Self {
        // Calculate time constants: coeff = 1 - exp(-1 / (time_ms * sample_rate / 1000))
        let attack_coeff = 1.0 - (-1.0 / (attack_ms * sample_rate / 1000.0)).exp();
        let release_coeff = 1.0 - (-1.0 / (release_ms * sample_rate / 1000.0)).exp();

        Self {
            threshold: db_to_linear(threshold_db),
            threshold_db,
            ratio,
            attack_coeff,
            release_coeff,
            makeup_gain: db_to_linear(makeup_gain_db),
            envelope: [0.0; 2],
            gain_reduction: 1.0,
            sample_rate,
        }
    }

    /// Process a single sample.
    /// Returns the compressed sample.
    #[inline]
    pub fn process(&mut self, input: f32, channel: usize) -> f32 {
        let input_abs = input.abs();

        // Peak envelope follower
        let env = &mut self.envelope[channel];
        if input_abs > *env {
            // Attack: envelope rising
            *env += self.attack_coeff * (input_abs - *env);
        } else {
            // Release: envelope falling
            *env += self.release_coeff * (input_abs - *env);
        }

        // Calculate gain reduction
        let gain = if *env > self.threshold && *env > 0.0 {
            // Above threshold: apply compression
            // output_db = threshold_db + (input_db - threshold_db) / ratio
            let input_db = linear_to_db(*env);
            let over_db = input_db - self.threshold_db;
            let compressed_db = self.threshold_db + over_db / self.ratio;
            db_to_linear(compressed_db) / *env
        } else {
            1.0
        };

        // Store for metering (use channel 0 as reference)
        if channel == 0 {
            self.gain_reduction = gain;
        }

        input * gain * self.makeup_gain
    }

    /// Get current gain reduction in dB (for metering).
    /// Returns a negative value when compressing.
    pub fn gain_reduction_db(&self) -> f32 {
        linear_to_db(self.gain_reduction)
    }

    /// Update compressor parameters at runtime.
    pub fn set_params(
        &mut self,
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        makeup_gain_db: f32,
    ) {
        self.threshold = db_to_linear(threshold_db);
        self.threshold_db = threshold_db;
        self.ratio = ratio;
        self.attack_coeff = 1.0 - (-1.0 / (attack_ms * self.sample_rate / 1000.0)).exp();
        self.release_coeff = 1.0 - (-1.0 / (release_ms * self.sample_rate / 1000.0)).exp();
        self.makeup_gain = db_to_linear(makeup_gain_db);
    }

    /// Reset envelope state (for discontinuities).
    pub fn reset(&mut self) {
        self.envelope = [0.0; 2];
        self.gain_reduction = 1.0;
    }
}
