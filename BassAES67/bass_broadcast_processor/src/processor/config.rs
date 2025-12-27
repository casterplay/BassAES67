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
