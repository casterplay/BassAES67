//! Processor configuration structures.

// ============================================================================
// AGC Configuration (Phase 3)
// ============================================================================

/// AGC mode constants
pub const AGC_MODE_SINGLE: u8 = 0;
pub const AGC_MODE_THREE_STAGE: u8 = 1;

/// AGC (Automatic Gain Control) configuration.
/// Used for wideband level normalization before multiband processing.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct AgcConfig {
    /// Target output level in dBFS (-24.0 to -12.0)
    pub target_level_db: f32,
    /// Compression threshold in dBFS (-30.0 to -6.0)
    pub threshold_db: f32,
    /// Compression ratio (2.0 to 8.0)
    pub ratio: f32,
    /// Soft knee width in dB (0.0 to 20.0)
    pub knee_db: f32,
    /// Attack time in milliseconds (10.0 to 100.0 for single, up to 5000 for 3-stage slow)
    pub attack_ms: f32,
    /// Release time in milliseconds (100.0 to 2000.0 for single, up to 10000 for 3-stage slow)
    pub release_ms: f32,
    /// Enable flag (1 = enabled, 0 = bypassed)
    pub enabled: u8,
    /// AGC mode: 0 = single-stage (default), 1 = 3-stage cascaded
    pub mode: u8,
    /// Padding for alignment
    pub _pad: [u8; 2],
}

impl Default for AgcConfig {
    fn default() -> Self {
        Self {
            target_level_db: -18.0,
            threshold_db: -24.0,
            ratio: 3.0,
            knee_db: 10.0,
            attack_ms: 50.0,
            release_ms: 500.0,
            enabled: 1,
            mode: AGC_MODE_SINGLE,
            _pad: [0; 2],
        }
    }
}

/// 3-stage cascaded AGC configuration (Omnia 9 style).
/// Each stage handles different time scales:
/// - Slow: Song-to-song level changes (seconds)
/// - Medium: Phrase-level dynamics (hundreds of ms)
/// - Fast: Syllable/transient control (tens of ms)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Agc3StageConfig {
    /// Stage 1: Slow AGC for song-level normalization
    pub slow: AgcConfig,
    /// Stage 2: Medium AGC for phrase-level dynamics
    pub medium: AgcConfig,
    /// Stage 3: Fast AGC for syllable/transient control
    pub fast: AgcConfig,
}

impl Default for Agc3StageConfig {
    fn default() -> Self {
        Self {
            // Stage 1: Slow (song-level)
            slow: AgcConfig {
                target_level_db: -20.0,
                threshold_db: -28.0,
                ratio: 2.0,
                knee_db: 12.0,
                attack_ms: 3000.0,  // 3 seconds
                release_ms: 8000.0, // 8 seconds
                enabled: 1,
                mode: AGC_MODE_THREE_STAGE,
                _pad: [0; 2],
            },
            // Stage 2: Medium (phrase-level)
            medium: AgcConfig {
                target_level_db: -18.0,
                threshold_db: -24.0,
                ratio: 2.5,
                knee_db: 10.0,
                attack_ms: 300.0,  // 300 ms
                release_ms: 800.0, // 800 ms
                enabled: 1,
                mode: AGC_MODE_THREE_STAGE,
                _pad: [0; 2],
            },
            // Stage 3: Fast (syllable-level)
            fast: AgcConfig {
                target_level_db: -16.0,
                threshold_db: -22.0,
                ratio: 3.0,
                knee_db: 8.0,
                attack_ms: 30.0,   // 30 ms
                release_ms: 150.0, // 150 ms
                enabled: 1,
                mode: AGC_MODE_THREE_STAGE,
                _pad: [0; 2],
            },
        }
    }
}

// ============================================================================
// Parametric EQ Configuration (Phase 2)
// ============================================================================

/// Per-band parametric EQ configuration.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ParametricEqBandConfig {
    /// Center frequency in Hz (20.0 to 20000.0)
    pub frequency: f32,
    /// Q factor (0.1 to 10.0, higher = narrower bandwidth)
    pub q: f32,
    /// Gain in dB (-12.0 to +12.0)
    pub gain_db: f32,
    /// Enable flag (1 = enabled, 0 = bypassed)
    pub enabled: u8,
    /// Padding for alignment
    pub _pad: [u8; 3],
}

impl Default for ParametricEqBandConfig {
    fn default() -> Self {
        Self {
            frequency: 1000.0,
            q: 1.0,
            gain_db: 0.0,
            enabled: 0,
            _pad: [0; 3],
        }
    }
}

/// Full parametric EQ configuration for 5 bands.
/// Each band gets its own parametric EQ section.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ParametricEqConfig {
    /// Global enable flag (1 = enabled, 0 = bypassed)
    pub enabled: u8,
    /// Padding for alignment
    pub _pad: [u8; 3],
    /// Per-band EQ settings (5 bands)
    pub bands: [ParametricEqBandConfig; 5],
}

impl Default for ParametricEqConfig {
    fn default() -> Self {
        Self {
            enabled: 0, // Disabled by default (flat response)
            _pad: [0; 3],
            bands: [
                // Band 0: Sub-bass
                ParametricEqBandConfig { frequency: 60.0, q: 1.0, gain_db: 0.0, enabled: 0, _pad: [0; 3] },
                // Band 1: Bass
                ParametricEqBandConfig { frequency: 250.0, q: 1.0, gain_db: 0.0, enabled: 0, _pad: [0; 3] },
                // Band 2: Midrange
                ParametricEqBandConfig { frequency: 1000.0, q: 1.0, gain_db: 0.0, enabled: 0, _pad: [0; 3] },
                // Band 3: Presence
                ParametricEqBandConfig { frequency: 4000.0, q: 1.0, gain_db: 0.0, enabled: 0, _pad: [0; 3] },
                // Band 4: Brilliance
                ParametricEqBandConfig { frequency: 12000.0, q: 1.0, gain_db: 0.0, enabled: 0, _pad: [0; 3] },
            ],
        }
    }
}

// ============================================================================
// Soft Clipper Configuration (Phase 3)
// ============================================================================

/// Soft clipper mode constants
pub const CLIP_MODE_HARD: u8 = 0;
pub const CLIP_MODE_SOFT: u8 = 1;
pub const CLIP_MODE_TANH: u8 = 2;

/// Soft clipper configuration.
/// Final-stage limiting with optional oversampling for intersample peak handling.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SoftClipperConfig {
    /// Ceiling level in dBFS (-6.0 to 0.0)
    pub ceiling_db: f32,
    /// Knee width in dB (0.0 to 6.0, only for soft mode)
    pub knee_db: f32,
    /// Clipping mode: 0=hard, 1=soft, 2=tanh
    pub mode: u8,
    /// Oversampling factor: 1, 2, or 4
    pub oversample: u8,
    /// Enable flag (1 = enabled, 0 = bypassed)
    pub enabled: u8,
    /// Padding for alignment
    pub _pad: u8,
}

impl Default for SoftClipperConfig {
    fn default() -> Self {
        Self {
            ceiling_db: -0.1,
            knee_db: 3.0,
            mode: CLIP_MODE_SOFT,
            oversample: 1,
            enabled: 0, // Disabled by default
            _pad: 0,
        }
    }
}

// ============================================================================
// Stereo Enhancer Configuration (Phase 3.2)
// ============================================================================

/// Per-band stereo enhancer configuration.
/// Controls stereo width dynamically using Mid-Side processing.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct StereoEnhancerBandConfig {
    /// Target stereo width ratio (S/M). 0.0 = mono, 1.0 = natural, 2.0 = enhanced
    pub target_width: f32,
    /// Maximum gain boost to side signal in dB (0.0 to 18.0)
    pub max_gain_db: f32,
    /// Maximum attenuation to side signal in dB (0.0 to 18.0)
    pub max_atten_db: f32,
    /// Attack time in ms (narrowing speed, 1.0 to 200.0)
    pub attack_ms: f32,
    /// Release time in ms (widening speed, 10.0 to 500.0)
    pub release_ms: f32,
    /// Enable flag (1 = enabled, 0 = bypassed)
    pub enabled: u8,
    /// Padding for alignment
    pub _pad: [u8; 3],
}

impl Default for StereoEnhancerBandConfig {
    fn default() -> Self {
        Self {
            target_width: 1.0,
            max_gain_db: 6.0,
            max_atten_db: 6.0,
            attack_ms: 30.0,
            release_ms: 150.0,
            enabled: 1,
            _pad: [0; 3],
        }
    }
}

/// Multiband stereo enhancer configuration (Omnia 9 style).
/// Contains settings for up to 5 bands. Band 0 (bass) is always bypassed.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct StereoEnhancerConfig {
    /// Global enable flag (1 = enabled, 0 = bypassed)
    pub enabled: u8,
    /// Padding for alignment
    pub _pad: [u8; 3],
    /// Per-band configurations (5 bands)
    /// Band 0 (bass) is always bypassed internally to avoid phase issues
    pub bands: [StereoEnhancerBandConfig; 5],
}

impl Default for StereoEnhancerConfig {
    fn default() -> Self {
        Self {
            enabled: 1,
            _pad: [0; 3],
            bands: [
                // Band 0 (Bass): Always bypassed - settings ignored
                StereoEnhancerBandConfig {
                    target_width: 1.0,
                    max_gain_db: 0.0,
                    max_atten_db: 0.0,
                    attack_ms: 50.0,
                    release_ms: 200.0,
                    enabled: 0, // Always disabled for bass
                    _pad: [0; 3],
                },
                // Band 1 (Low-Mid): Gentle enhancement
                StereoEnhancerBandConfig {
                    target_width: 1.0,
                    max_gain_db: 6.0,
                    max_atten_db: 6.0,
                    attack_ms: 50.0,
                    release_ms: 200.0,
                    enabled: 1,
                    _pad: [0; 3],
                },
                // Band 2 (Mid): Moderate enhancement
                StereoEnhancerBandConfig {
                    target_width: 1.2,
                    max_gain_db: 9.0,
                    max_atten_db: 9.0,
                    attack_ms: 30.0,
                    release_ms: 150.0,
                    enabled: 1,
                    _pad: [0; 3],
                },
                // Band 3 (Presence): More enhancement
                StereoEnhancerBandConfig {
                    target_width: 1.3,
                    max_gain_db: 12.0,
                    max_atten_db: 12.0,
                    attack_ms: 20.0,
                    release_ms: 100.0,
                    enabled: 1,
                    _pad: [0; 3],
                },
                // Band 4 (Brilliance): Most enhancement
                StereoEnhancerBandConfig {
                    target_width: 1.4,
                    max_gain_db: 12.0,
                    max_atten_db: 12.0,
                    attack_ms: 15.0,
                    release_ms: 80.0,
                    enabled: 1,
                    _pad: [0; 3],
                },
            ],
        }
    }
}

// ============================================================================
// Compressor Configuration
// ============================================================================

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
    /// Lookahead time in milliseconds (0.0 to 10.0). Set to 0.0 to disable.
    /// Lookahead adds latency but allows transparent limiting of fast transients.
    pub lookahead_ms: f32,
}

impl Default for CompressorConfig {
    fn default() -> Self {
        Self {
            threshold_db: -20.0,
            ratio: 4.0,
            attack_ms: 10.0,
            release_ms: 100.0,
            makeup_gain_db: 0.0,
            lookahead_ms: 0.0, // Disabled by default (zero-latency)
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
                lookahead_ms: 0.0,
            },
            high_band: CompressorConfig {
                threshold_db: -18.0,
                ratio: 3.0,
                attack_ms: 5.0,
                release_ms: 80.0,
                makeup_gain_db: 0.0,
                lookahead_ms: 0.0,
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
                    lookahead_ms: 0.0,
                },
                CompressorConfig {
                    threshold_db: -18.0,
                    ratio: 3.0,
                    attack_ms: 5.0,
                    release_ms: 80.0,
                    makeup_gain_db: 0.0,
                    lookahead_ms: 0.0,
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
                    lookahead_ms: 0.0,
                },
                // Bass (100 - 400 Hz)
                CompressorConfig {
                    threshold_db: -20.0,
                    ratio: 5.0,
                    attack_ms: 5.0,
                    release_ms: 150.0,
                    makeup_gain_db: 4.0,
                    lookahead_ms: 0.0,
                },
                // Midrange (400 - 2000 Hz)
                CompressorConfig {
                    threshold_db: -18.0,
                    ratio: 3.0,
                    attack_ms: 3.0,
                    release_ms: 100.0,
                    makeup_gain_db: 3.0,
                    lookahead_ms: 0.0,
                },
                // Presence (2000 - 8000 Hz)
                CompressorConfig {
                    threshold_db: -16.0,
                    ratio: 4.0,
                    attack_ms: 1.0,
                    release_ms: 80.0,
                    makeup_gain_db: 4.0,
                    lookahead_ms: 0.0,
                },
                // Brilliance (> 8000 Hz)
                CompressorConfig {
                    threshold_db: -14.0,
                    ratio: 5.0,
                    attack_ms: 0.5,
                    release_ms: 50.0,
                    makeup_gain_db: 2.0,
                    lookahead_ms: 0.0,
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
