//! Audio compressor with envelope follower and optional lookahead.

use super::gain::{db_to_linear, linear_to_db};

/// Maximum lookahead in samples (10ms at 48kHz = 480 samples)
const MAX_LOOKAHEAD_SAMPLES: usize = 512;

/// Per-band compressor with peak envelope follower and optional lookahead.
///
/// Lookahead delays the audio signal while computing gain reduction ahead of time,
/// allowing the compressor to respond to transients before they arrive. This provides
/// transparent limiting without distortion on fast transients.
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
    /// Lookahead enabled flag
    lookahead_enabled: bool,
    /// Lookahead delay in samples
    lookahead_samples: usize,
    /// Delay buffer for left channel
    delay_buffer_l: [f32; MAX_LOOKAHEAD_SAMPLES],
    /// Delay buffer for right channel
    delay_buffer_r: [f32; MAX_LOOKAHEAD_SAMPLES],
    /// Delay buffer write position
    delay_pos: usize,
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
            lookahead_enabled: false,
            lookahead_samples: 0,
            delay_buffer_l: [0.0; MAX_LOOKAHEAD_SAMPLES],
            delay_buffer_r: [0.0; MAX_LOOKAHEAD_SAMPLES],
            delay_pos: 0,
        }
    }

    /// Enable or disable lookahead.
    ///
    /// # Arguments
    /// * `enabled` - Whether lookahead is enabled
    /// * `lookahead_ms` - Lookahead time in milliseconds (0.0 to 10.0)
    pub fn set_lookahead(&mut self, enabled: bool, lookahead_ms: f32) {
        self.lookahead_enabled = enabled;
        if enabled {
            // Convert ms to samples, clamp to max
            let samples = (lookahead_ms * self.sample_rate / 1000.0) as usize;
            self.lookahead_samples = samples.min(MAX_LOOKAHEAD_SAMPLES - 1);
        } else {
            self.lookahead_samples = 0;
        }
    }

    /// Get current lookahead in milliseconds.
    pub fn lookahead_ms(&self) -> f32 {
        if self.lookahead_enabled {
            self.lookahead_samples as f32 * 1000.0 / self.sample_rate
        } else {
            0.0
        }
    }

    /// Check if lookahead is enabled.
    pub fn is_lookahead_enabled(&self) -> bool {
        self.lookahead_enabled
    }

    /// Process a single sample.
    /// Returns the compressed sample.
    ///
    /// When lookahead is enabled, the audio is delayed while gain reduction
    /// is computed from the undelayed signal, allowing transparent limiting.
    #[inline]
    pub fn process(&mut self, input: f32, channel: usize) -> f32 {
        let input_abs = input.abs();

        // Peak envelope follower (always uses undelayed input for detection)
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

        // If lookahead is enabled, delay the audio signal
        let output_sample = if self.lookahead_enabled && self.lookahead_samples > 0 {
            // Get the delayed sample from the buffer
            let read_pos = (self.delay_pos + MAX_LOOKAHEAD_SAMPLES - self.lookahead_samples)
                % MAX_LOOKAHEAD_SAMPLES;

            let delayed = if channel == 0 {
                let out = self.delay_buffer_l[read_pos];
                self.delay_buffer_l[self.delay_pos] = input;
                out
            } else {
                let out = self.delay_buffer_r[read_pos];
                self.delay_buffer_r[self.delay_pos] = input;
                out
            };

            // Advance write position (only once per stereo pair, on channel 1)
            if channel == 1 {
                self.delay_pos = (self.delay_pos + 1) % MAX_LOOKAHEAD_SAMPLES;
            }

            delayed
        } else {
            input
        };

        output_sample * gain * self.makeup_gain
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

    /// Reset envelope state and delay buffers (for discontinuities).
    pub fn reset(&mut self) {
        self.envelope = [0.0; 2];
        self.gain_reduction = 1.0;
        self.delay_buffer_l = [0.0; MAX_LOOKAHEAD_SAMPLES];
        self.delay_buffer_r = [0.0; MAX_LOOKAHEAD_SAMPLES];
        self.delay_pos = 0;
    }
}
