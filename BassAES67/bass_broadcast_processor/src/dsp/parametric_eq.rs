//! Per-band parametric EQ for multiband processor.
//!
//! Each frequency band can have its own parametric EQ section with
//! configurable center frequency, Q factor, and gain.

use super::biquad::Biquad;

/// Single parametric EQ band using a peaking filter.
pub struct ParametricEqBand {
    /// Peaking filter
    filter: Biquad,
    /// Enabled flag
    enabled: bool,
    /// Center frequency in Hz
    frequency: f32,
    /// Q factor
    q: f32,
    /// Gain in dB
    gain_db: f32,
    /// Sample rate (cached for reconfiguration)
    sample_rate: f32,
}

impl ParametricEqBand {
    /// Create a new parametric EQ band.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate
    /// * `frequency` - Center frequency in Hz
    /// * `q` - Q factor (0.1 to 10.0)
    /// * `gain_db` - Gain in dB (-12 to +12)
    /// * `enabled` - Whether the EQ is active
    pub fn new(sample_rate: f32, frequency: f32, q: f32, gain_db: f32, enabled: bool) -> Self {
        Self {
            filter: Biquad::peaking(frequency, q, gain_db, sample_rate),
            enabled,
            frequency,
            q,
            gain_db,
            sample_rate,
        }
    }

    /// Process a single sample.
    /// Returns input unchanged if disabled or gain is near 0 dB.
    #[inline]
    pub fn process(&mut self, input: f32, channel: usize) -> f32 {
        if !self.enabled || self.gain_db.abs() < 0.01 {
            return input;
        }
        self.filter.process(input, channel)
    }

    /// Update EQ parameters.
    pub fn set_params(&mut self, frequency: f32, q: f32, gain_db: f32, enabled: bool) {
        self.frequency = frequency;
        self.q = q;
        self.gain_db = gain_db;
        self.enabled = enabled;
        self.filter = Biquad::peaking(frequency, q, gain_db, self.sample_rate);
    }

    /// Check if the EQ band is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get the current gain in dB.
    pub fn gain_db(&self) -> f32 {
        self.gain_db
    }

    /// Reset filter state.
    pub fn reset(&mut self) {
        self.filter.reset();
    }
}

/// Per-band parametric EQ for 5-band multiband processor.
/// Each band has its own independent EQ section.
pub struct ParametricEq {
    /// Per-band EQ sections (5 bands)
    bands: [ParametricEqBand; 5],
    /// Global enable flag
    enabled: bool,
    /// Sample rate
    sample_rate: f32,
}

impl ParametricEq {
    /// Create a new parametric EQ with default (flat) settings.
    /// All bands are disabled by default (0 dB gain).
    pub fn new(sample_rate: f32) -> Self {
        Self {
            bands: [
                // Band 0: Sub-bass (default center 60 Hz)
                ParametricEqBand::new(sample_rate, 60.0, 1.0, 0.0, false),
                // Band 1: Bass (default center 250 Hz)
                ParametricEqBand::new(sample_rate, 250.0, 1.0, 0.0, false),
                // Band 2: Midrange (default center 1000 Hz)
                ParametricEqBand::new(sample_rate, 1000.0, 1.0, 0.0, false),
                // Band 3: Presence (default center 4000 Hz)
                ParametricEqBand::new(sample_rate, 4000.0, 1.0, 0.0, false),
                // Band 4: Brilliance (default center 12000 Hz)
                ParametricEqBand::new(sample_rate, 12000.0, 1.0, 0.0, false),
            ],
            enabled: false,
            sample_rate,
        }
    }

    /// Process a sample for a specific band.
    /// Returns input unchanged if globally disabled or band index is invalid.
    #[inline]
    pub fn process_band(&mut self, band_idx: usize, input: f32, channel: usize) -> f32 {
        if !self.enabled || band_idx >= 5 {
            return input;
        }
        self.bands[band_idx].process(input, channel)
    }

    /// Set parameters for a specific band.
    pub fn set_band(&mut self, band_idx: usize, frequency: f32, q: f32, gain_db: f32, enabled: bool) {
        if band_idx < 5 {
            self.bands[band_idx].set_params(frequency, q, gain_db, enabled);
        }
    }

    /// Enable or disable the entire parametric EQ.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Check if the parametric EQ is globally enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Reset all filter states.
    pub fn reset(&mut self) {
        for band in &mut self.bands {
            band.reset();
        }
    }
}
