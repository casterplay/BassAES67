//! Processor configuration structures.

/// Compressor configuration (per-band).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CompressorConfig {
    /// Threshold in dBFS (-40.0 to 0.0)
    pub threshold_db: f32,
    /// Compression ratio (1.0 = no compression, 10.0 = heavy)
    pub ratio: f32,
    /// Attack time in milliseconds (0.5 to 100)
    pub attack_ms: f32,
    /// Release time in milliseconds (10 to 1000)
    pub release_ms: f32,
    /// Makeup gain in dB (0.0 to 20.0)
    pub makeup_gain_db: f32,
}

impl Default for CompressorConfig {
    fn default() -> Self {
        Self {
            threshold_db: -20.0,
            ratio: 4.0,
            attack_ms: 10.0,
            release_ms: 100.0,
            makeup_gain_db: 0.0,
        }
    }
}

/// Main processor configuration.
#[repr(C)]
#[derive(Clone, Debug)]
pub struct ProcessorConfig {
    /// Sample rate in Hz (typically 48000)
    pub sample_rate: u32,
    /// Number of channels (2 for stereo)
    pub channels: u16,
    /// Block size in samples per channel (64-512, default 256)
    pub block_size: u16,
    /// If true, output is decode-only (for feeding to AES67 output).
    /// If false, output is playable (for direct speaker output).
    pub decode_output: u8,
    /// Padding for alignment (set to 0)
    pub _pad: u8,
    /// Input gain in dB (-20.0 to +20.0)
    pub input_gain_db: f32,
    /// Output gain in dB (-20.0 to +20.0)
    pub output_gain_db: f32,
    /// Crossover frequency in Hz (default 400)
    pub crossover_freq: f32,
    /// Low band compressor settings
    pub low_band: CompressorConfig,
    /// High band compressor settings
    pub high_band: CompressorConfig,
}

impl Default for ProcessorConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            channels: 2,
            block_size: 256,
            decode_output: 1, // Default to decode mode (for AES67 pipeline)
            _pad: 0,
            input_gain_db: 0.0,
            output_gain_db: 0.0,
            crossover_freq: 400.0,
            low_band: CompressorConfig {
                threshold_db: -20.0,
                ratio: 4.0,
                attack_ms: 10.0,
                release_ms: 100.0,
                makeup_gain_db: 0.0,
            },
            high_band: CompressorConfig {
                threshold_db: -18.0,
                ratio: 3.0,
                attack_ms: 5.0,
                release_ms: 80.0,
                makeup_gain_db: 0.0,
            },
        }
    }
}

// ============================================================================
// N-Band Multiband Processor Configuration
// ============================================================================

/// FFI-compatible configuration header for N-band multiband processor.
/// Fixed size - crossover frequencies and band configs are passed separately.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MultibandConfigHeader {
    /// Sample rate in Hz (typically 48000)
    pub sample_rate: u32,
    /// Number of channels (2 for stereo)
    pub channels: u16,
    /// Number of frequency bands (2, 5, 8, etc.)
    pub num_bands: u16,
    /// If non-zero, output is decode-only (for feeding to AES67 output).
    /// If zero, output is playable (for direct speaker output).
    pub decode_output: u8,
    /// Padding for alignment
    pub _pad: [u8; 3],
    /// Input gain in dB (-20.0 to +20.0)
    pub input_gain_db: f32,
    /// Output gain in dB (-20.0 to +20.0)
    pub output_gain_db: f32,
}

impl Default for MultibandConfigHeader {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            channels: 2,
            num_bands: 5,
            decode_output: 0,
            _pad: [0; 3],
            input_gain_db: 0.0,
            output_gain_db: 0.0,
        }
    }
}

/// Rust-side configuration with owned data for N-band processor.
#[derive(Clone, Debug)]
pub struct MultibandConfig {
    /// Fixed header
    pub header: MultibandConfigHeader,
    /// Crossover frequencies in Hz (ascending order).
    /// Length = num_bands - 1
    pub crossover_freqs: Vec<f32>,
    /// Compressor configuration for each band.
    /// Length = num_bands
    pub bands: Vec<CompressorConfig>,
}

impl MultibandConfig {
    /// Create a 2-band configuration (lightweight).
    pub fn two_band(sample_rate: u32, crossover_freq: f32) -> Self {
        Self {
            header: MultibandConfigHeader {
                sample_rate,
                channels: 2,
                num_bands: 2,
                decode_output: 0,
                _pad: [0; 3],
                input_gain_db: 0.0,
                output_gain_db: 0.0,
            },
            crossover_freqs: vec![crossover_freq],
            bands: vec![
                CompressorConfig {
                    threshold_db: -20.0,
                    ratio: 4.0,
                    attack_ms: 10.0,
                    release_ms: 100.0,
                    makeup_gain_db: 0.0,
                },
                CompressorConfig {
                    threshold_db: -18.0,
                    ratio: 3.0,
                    attack_ms: 5.0,
                    release_ms: 80.0,
                    makeup_gain_db: 0.0,
                },
            ],
        }
    }

    /// Create a 5-band broadcast configuration.
    pub fn five_band_broadcast(sample_rate: u32) -> Self {
        Self {
            header: MultibandConfigHeader {
                sample_rate,
                channels: 2,
                num_bands: 5,
                decode_output: 0,
                _pad: [0; 3],
                input_gain_db: 0.0,
                output_gain_db: 0.0,
            },
            // Crossover frequencies: 100, 400, 2000, 8000 Hz
            crossover_freqs: vec![100.0, 400.0, 2000.0, 8000.0],
            bands: vec![
                // Sub-bass (< 100 Hz)
                CompressorConfig {
                    threshold_db: -24.0,
                    ratio: 4.0,
                    attack_ms: 10.0,
                    release_ms: 200.0,
                    makeup_gain_db: 3.0,
                },
                // Bass (100 - 400 Hz)
                CompressorConfig {
                    threshold_db: -20.0,
                    ratio: 5.0,
                    attack_ms: 5.0,
                    release_ms: 150.0,
                    makeup_gain_db: 4.0,
                },
                // Midrange (400 - 2000 Hz)
                CompressorConfig {
                    threshold_db: -18.0,
                    ratio: 3.0,
                    attack_ms: 3.0,
                    release_ms: 100.0,
                    makeup_gain_db: 3.0,
                },
                // Presence (2000 - 8000 Hz)
                CompressorConfig {
                    threshold_db: -16.0,
                    ratio: 4.0,
                    attack_ms: 1.0,
                    release_ms: 80.0,
                    makeup_gain_db: 4.0,
                },
                // Brilliance (> 8000 Hz)
                CompressorConfig {
                    threshold_db: -14.0,
                    ratio: 5.0,
                    attack_ms: 0.5,
                    release_ms: 50.0,
                    makeup_gain_db: 2.0,
                },
            ],
        }
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), String> {
        let n = self.header.num_bands as usize;

        if n < 2 {
            return Err("num_bands must be at least 2".to_string());
        }

        if self.crossover_freqs.len() != n - 1 {
            return Err(format!(
                "crossover_freqs length {} does not match num_bands {} (expected {})",
                self.crossover_freqs.len(),
                n,
                n - 1
            ));
        }

        if self.bands.len() != n {
            return Err(format!(
                "bands length {} does not match num_bands {}",
                self.bands.len(),
                n
            ));
        }

        // Check frequencies are in ascending order
        for i in 1..self.crossover_freqs.len() {
            if self.crossover_freqs[i] <= self.crossover_freqs[i - 1] {
                return Err("crossover_freqs must be in ascending order".to_string());
            }
        }

        Ok(())
    }
}
