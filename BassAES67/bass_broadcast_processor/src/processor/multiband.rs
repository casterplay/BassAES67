//! N-band multiband processor implementation.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::dsp::agc::{ThreeStageAGC, WidebandAGC};
use crate::dsp::compressor::Compressor;
use crate::dsp::gain::{apply_gain, db_to_linear, peak_level};
use crate::dsp::lufs_meter::LufsMeter;
use crate::dsp::multiband::MultibandCrossover;
use crate::dsp::parametric_eq::ParametricEq;
use crate::dsp::soft_clipper::SoftClipper;
use crate::dsp::stereo_enhancer::StereoEnhancer;
use crate::ffi::{BASS_ChannelGetData, BASS_DATA_FLOAT, DWORD};

use super::config::{Agc3StageConfig, AgcConfig, CompressorConfig, MultibandConfig, ParametricEqConfig, SoftClipperConfig, StereoEnhancerConfig, AGC_MODE_THREE_STAGE};
use super::stats::{MultibandAtomicStats, MultibandStatsHeader};

/// N-band multiband audio processor.
/// Processes audio from a BASS source channel through an N-band compressor chain.
pub struct MultibandProcessor {
    /// Configuration
    config: MultibandConfig,
    /// Wideband AGC (before multiband split) - Phase 3 (single-stage)
    agc: WidebandAGC,
    /// 3-stage cascaded AGC (Omnia 9 style) - Phase 3.1b (optional)
    agc_3stage: Option<ThreeStageAGC>,
    /// AGC mode: false = single-stage (default), true = 3-stage
    agc_use_3stage: bool,
    /// N-band crossover
    crossover: MultibandCrossover,
    /// Per-band compressors
    compressors: Vec<Compressor>,
    /// Multiband stereo enhancer (Omnia 9 style) - Phase 3.2
    stereo_enhancer: StereoEnhancer,
    /// Per-band parametric EQ - Phase 2
    parametric_eq: ParametricEq,
    /// Soft clipper with oversampling - Phase 3
    soft_clipper: SoftClipper,
    /// LUFS meter (ITU-R BS.1770) - Phase 3
    lufs_meter: LufsMeter,
    /// Input gain (linear)
    input_gain: f32,
    /// Output gain (linear)
    output_gain: f32,
    /// Lock-free statistics
    stats: Arc<MultibandAtomicStats>,
    /// Source BASS channel handle
    source_channel: DWORD,
    /// Output BASS stream handle (set after creation)
    pub output_handle: DWORD,
    /// Temporary buffer for reading from source
    temp_buffer: Vec<f32>,
    /// Temporary buffer for band samples (per frame)
    band_buffer: Vec<f32>,
    /// Temporary buffer for band samples (right channel)
    band_buffer_r: Vec<f32>,
    /// Bypass mode - when true, audio passes through unprocessed
    pub bypass: bool,
}

impl MultibandProcessor {
    /// Create a new multiband processor.
    ///
    /// # Arguments
    /// * `source_channel` - BASS channel handle to pull audio from
    /// * `config` - Multiband processor configuration
    pub fn new(source_channel: DWORD, config: MultibandConfig) -> Result<Self, String> {
        // Validate configuration
        config.validate()?;

        let num_bands = config.header.num_bands as usize;
        let sample_rate = config.header.sample_rate as f32;

        // Create wideband AGC with default broadcast settings
        let agc = WidebandAGC::default_broadcast(sample_rate);

        // Create multiband crossover
        let crossover = MultibandCrossover::new(&config.crossover_freqs, sample_rate);

        // Create compressors for each band
        let compressors: Vec<Compressor> = config
            .bands
            .iter()
            .map(|band_cfg| {
                let mut comp = Compressor::new(
                    band_cfg.threshold_db,
                    band_cfg.ratio,
                    band_cfg.attack_ms,
                    band_cfg.release_ms,
                    band_cfg.makeup_gain_db,
                    sample_rate,
                );
                // Enable lookahead if configured (> 0.0)
                if band_cfg.lookahead_ms > 0.0 {
                    comp.set_lookahead(true, band_cfg.lookahead_ms);
                }
                comp
            })
            .collect();

        // Create stereo enhancer with default broadcast settings
        let stereo_enhancer = StereoEnhancer::new(sample_rate);

        // Create per-band parametric EQ (default: disabled, flat response)
        let parametric_eq = ParametricEq::new(sample_rate);

        // Create soft clipper (default: disabled)
        let soft_clipper = SoftClipper::new(sample_rate);

        // Create LUFS meter for loudness measurement
        let lufs_meter = LufsMeter::new(sample_rate);

        // Pre-allocate buffers
        let temp_buffer = vec![0.0f32; 32768];
        let band_buffer = vec![0.0f32; num_bands];
        let band_buffer_r = vec![0.0f32; num_bands];

        Ok(Self {
            input_gain: db_to_linear(config.header.input_gain_db),
            output_gain: db_to_linear(config.header.output_gain_db),
            config,
            agc,
            agc_3stage: None,     // Not created until explicitly enabled
            agc_use_3stage: false, // Default to single-stage
            crossover,
            compressors,
            stereo_enhancer,
            parametric_eq,
            soft_clipper,
            lufs_meter,
            stats: Arc::new(MultibandAtomicStats::new(num_bands)),
            source_channel,
            output_handle: 0,
            temp_buffer,
            band_buffer,
            band_buffer_r,
            bypass: false,
        })
    }

    /// Process samples directly from source to output buffer.
    /// Called by STREAMPROC with the exact buffer BASS needs filled.
    pub fn read_samples(&mut self, buffer: &mut [f32]) -> usize {
        let start_time = std::time::Instant::now();

        let samples_needed = buffer.len();
        let channels = self.config.header.channels as usize;
        let num_bands = self.config.header.num_bands as usize;

        // Ensure temp buffer is large enough
        if self.temp_buffer.len() < samples_needed {
            self.temp_buffer.resize(samples_needed, 0.0);
        }

        // Pull samples from source channel
        let bytes_needed = (samples_needed * 4) as DWORD;
        let bytes_read = unsafe {
            BASS_ChannelGetData(
                self.source_channel,
                self.temp_buffer.as_mut_ptr() as *mut std::ffi::c_void,
                bytes_needed | BASS_DATA_FLOAT,
            )
        };

        // Handle error or end of stream
        let samples_read = if bytes_read == 0xFFFFFFFF || bytes_read == 0 {
            self.temp_buffer[..samples_needed].fill(0.0);
            self.stats.underruns.fetch_add(1, Ordering::Relaxed);
            samples_needed
        } else {
            (bytes_read as usize / 4).min(samples_needed)
        };

        // Track input peak
        let in_peak = peak_level(&self.temp_buffer[..samples_read]);
        self.stats
            .input_peak_x1000
            .store((in_peak * 1000.0) as i32, Ordering::Relaxed);

        let frames = samples_read / channels;

        if self.bypass {
            // Bypass mode: copy input directly to output (no processing)
            buffer[..samples_read].copy_from_slice(&self.temp_buffer[..samples_read]);

            // Track output peak (same as input in bypass)
            self.stats
                .output_peak_x1000
                .store((in_peak * 1000.0) as i32, Ordering::Relaxed);

            // Clear all GR meters (AGC + bands)
            self.stats.agc_gr_x100.store(0, Ordering::Relaxed);
            for gr in &self.stats.band_gr_x100 {
                gr.store(0, Ordering::Relaxed);
            }
        } else {
            // Apply input gain
            apply_gain(&mut self.temp_buffer[..samples_read], self.input_gain);

            // Process each stereo frame: AGC -> split -> compress -> stereo enhance -> sum
            // Note: Stereo enhancer requires processing L/R pairs together
            for i in 0..frames {
                // Get stereo pair indices
                let idx_l = i * channels;
                let idx_r = i * channels + 1;

                // Get input samples
                let mut sample_l = self.temp_buffer[idx_l];
                let mut sample_r = if channels >= 2 {
                    self.temp_buffer[idx_r]
                } else {
                    sample_l // Mono: duplicate
                };

                // Wideband AGC (before multiband split)
                // Use 3-stage if enabled, otherwise single-stage
                if self.agc_use_3stage {
                    if let Some(ref mut agc3) = self.agc_3stage {
                        sample_l = agc3.process(sample_l, 0);
                        sample_r = agc3.process(sample_r, 1);
                    }
                } else {
                    sample_l = self.agc.process(sample_l, 0);
                    sample_r = self.agc.process(sample_r, 1);
                }

                // Split into N bands (both channels)
                self.crossover.split(sample_l, 0, &mut self.band_buffer);
                self.crossover.split(sample_r, 1, &mut self.band_buffer_r);

                // Process each band: compress -> stereo enhance
                let mut output_l = 0.0f32;
                let mut output_r = 0.0f32;

                for band_idx in 0..num_bands.min(self.band_buffer.len()) {
                    // Compress both channels
                    let comp_l = self.compressors[band_idx].process(self.band_buffer[band_idx], 0);
                    let comp_r = self.compressors[band_idx].process(self.band_buffer_r[band_idx], 1);

                    // Apply per-band parametric EQ
                    let eq_l = self.parametric_eq.process_band(band_idx, comp_l, 0);
                    let eq_r = self.parametric_eq.process_band(band_idx, comp_r, 1);

                    // Apply stereo enhancer to this band (Band 0/bass is auto-bypassed internally)
                    let (enh_l, enh_r) = self.stereo_enhancer.process_band(band_idx, eq_l, eq_r);

                    // Sum bands
                    output_l += enh_l;
                    output_r += enh_r;
                }

                // Write output
                buffer[idx_l] = output_l;
                if channels >= 2 {
                    buffer[idx_r] = output_r;
                }
            }

            // Apply output gain
            apply_gain(&mut buffer[..samples_read], self.output_gain);

            // Apply soft clipping with oversampling (final stage limiting)
            for i in 0..frames {
                let idx_l = i * channels;
                let idx_r = i * channels + 1;
                let (clipped_l, clipped_r) = self.soft_clipper.process_stereo(
                    buffer[idx_l],
                    if channels >= 2 { buffer[idx_r] } else { buffer[idx_l] },
                );
                buffer[idx_l] = clipped_l;
                if channels >= 2 {
                    buffer[idx_r] = clipped_r;
                }
            }

            // Track output peak (after soft clipper)
            let out_peak = peak_level(&buffer[..samples_read]);
            self.stats
                .output_peak_x1000
                .store((out_peak * 1000.0) as i32, Ordering::Relaxed);

            // Feed LUFS meter (metering only, no audio modification)
            for i in 0..frames {
                let idx_l = i * channels;
                let idx_r = i * channels + 1;
                self.lufs_meter.process(
                    buffer[idx_l],
                    if channels >= 2 { buffer[idx_r] } else { buffer[idx_l] },
                );
            }

            // Update LUFS stats
            self.stats.lufs_momentary_x100.store(
                (self.lufs_meter.momentary_lufs() * 100.0) as i32,
                Ordering::Relaxed,
            );
            self.stats.lufs_short_x100.store(
                (self.lufs_meter.short_term_lufs() * 100.0) as i32,
                Ordering::Relaxed,
            );
            self.stats.lufs_integrated_x100.store(
                (self.lufs_meter.integrated_lufs() * 100.0) as i32,
                Ordering::Relaxed,
            );

            // Track AGC gain reduction (use 3-stage total if enabled)
            let agc_gr = if self.agc_use_3stage {
                self.agc_3stage
                    .as_ref()
                    .map(|a| a.total_gain_reduction_db())
                    .unwrap_or(0.0)
            } else {
                self.agc.gain_reduction_db()
            };
            self.stats
                .agc_gr_x100
                .store((agc_gr * 100.0) as i32, Ordering::Relaxed);

            // Track per-band gain reduction
            for (i, compressor) in self.compressors.iter().enumerate() {
                if i < num_bands {
                    self.stats.band_gr_x100[i].store(
                        (compressor.gain_reduction_db() * 100.0) as i32,
                        Ordering::Relaxed,
                    );
                }
            }
        }

        self.stats
            .samples_processed
            .fetch_add(frames as u64, Ordering::Relaxed);

        // Record processing time
        let elapsed = start_time.elapsed();
        self.stats
            .process_time_us
            .store(elapsed.as_micros() as u64, Ordering::Relaxed);

        samples_read
    }

    /// Get statistics header and per-band gain reduction.
    ///
    /// # Arguments
    /// * `band_gr_out` - Output slice for per-band gain reduction in dB.
    ///                   Must have length >= num_bands.
    ///
    /// # Returns
    /// Statistics header with peak levels, sample count, etc.
    pub fn get_stats(&self, band_gr_out: &mut [f32]) -> MultibandStatsHeader {
        let num_bands = self.stats.num_bands();

        // Fill per-band gain reduction
        for (i, gr) in self.stats.band_gr_x100.iter().enumerate() {
            if i < band_gr_out.len() {
                band_gr_out[i] = gr.load(Ordering::Relaxed) as f32 / 100.0;
            }
        }

        MultibandStatsHeader {
            samples_processed: self.stats.samples_processed.load(Ordering::Relaxed),
            input_peak: self.stats.input_peak_x1000.load(Ordering::Relaxed) as f32 / 1000.0,
            output_peak: self.stats.output_peak_x1000.load(Ordering::Relaxed) as f32 / 1000.0,
            num_bands: num_bands as u32,
            agc_gr_db: self.stats.agc_gr_x100.load(Ordering::Relaxed) as f32 / 100.0,
            underruns: self.stats.underruns.load(Ordering::Relaxed),
            process_time_us: self.stats.process_time_us.load(Ordering::Relaxed),
            lufs_momentary: self.stats.lufs_momentary_x100.load(Ordering::Relaxed) as f32 / 100.0,
            lufs_short_term: self.stats.lufs_short_x100.load(Ordering::Relaxed) as f32 / 100.0,
            lufs_integrated: self.stats.lufs_integrated_x100.load(Ordering::Relaxed) as f32 / 100.0,
            _pad: 0,
        }
    }

    /// Get the number of bands.
    pub fn num_bands(&self) -> usize {
        self.config.header.num_bands as usize
    }

    /// Update a specific band's compressor settings.
    ///
    /// # Arguments
    /// * `band` - Band index (0-based)
    /// * `config` - New compressor configuration
    pub fn set_band(&mut self, band: usize, config: &CompressorConfig) {
        if band < self.compressors.len() {
            self.compressors[band].set_params(
                config.threshold_db,
                config.ratio,
                config.attack_ms,
                config.release_ms,
                config.makeup_gain_db,
            );
            // Update lookahead setting
            self.compressors[band].set_lookahead(config.lookahead_ms > 0.0, config.lookahead_ms);
            self.config.bands[band] = *config;
        }
    }

    /// Set lookahead for all bands at once.
    /// This is a convenience method for enabling/disabling lookahead globally.
    ///
    /// # Arguments
    /// * `enabled` - Whether lookahead is enabled
    /// * `lookahead_ms` - Lookahead time in milliseconds (0.0 to 10.0)
    pub fn set_lookahead(&mut self, enabled: bool, lookahead_ms: f32) {
        for (i, comp) in self.compressors.iter_mut().enumerate() {
            comp.set_lookahead(enabled, lookahead_ms);
            self.config.bands[i].lookahead_ms = if enabled { lookahead_ms } else { 0.0 };
        }
    }

    /// Get the total lookahead latency in milliseconds.
    /// Returns the maximum lookahead across all bands (they should all be the same).
    pub fn get_lookahead_ms(&self) -> f32 {
        self.compressors
            .iter()
            .map(|c| c.lookahead_ms())
            .fold(0.0f32, |a, b| a.max(b))
    }

    /// Update input and output gains.
    pub fn set_gains(&mut self, input_gain_db: f32, output_gain_db: f32) {
        self.input_gain = db_to_linear(input_gain_db);
        self.output_gain = db_to_linear(output_gain_db);
        self.config.header.input_gain_db = input_gain_db;
        self.config.header.output_gain_db = output_gain_db;
    }

    /// Update AGC parameters.
    /// If mode is AGC_MODE_THREE_STAGE (1), this enables 3-stage mode
    /// but does not configure the stages (use set_agc_3stage for that).
    pub fn set_agc(&mut self, config: &AgcConfig) {
        // Check if switching to 3-stage mode
        if config.mode == AGC_MODE_THREE_STAGE {
            // Enable 3-stage mode, create if needed
            if self.agc_3stage.is_none() {
                let sample_rate = self.config.header.sample_rate as f32;
                self.agc_3stage = Some(ThreeStageAGC::new(sample_rate));
            }
            if let Some(ref mut agc3) = self.agc_3stage {
                agc3.set_enabled(config.enabled != 0);
            }
            self.agc_use_3stage = true;
        } else {
            // Single-stage mode
            self.agc.set_params(
                config.target_level_db,
                config.threshold_db,
                config.ratio,
                config.knee_db,
                config.attack_ms,
                config.release_ms,
                config.enabled != 0,
            );
            self.agc_use_3stage = false;
        }
    }

    /// Configure 3-stage AGC with individual stage parameters.
    /// Automatically enables 3-stage mode.
    pub fn set_agc_3stage(&mut self, config: &Agc3StageConfig) {
        let sample_rate = self.config.header.sample_rate as f32;

        // Create 3-stage AGC if not already created
        if self.agc_3stage.is_none() {
            self.agc_3stage = Some(ThreeStageAGC::new(sample_rate));
        }

        if let Some(ref mut agc3) = self.agc_3stage {
            // Configure slow stage
            agc3.set_slow(
                config.slow.target_level_db,
                config.slow.threshold_db,
                config.slow.ratio,
                config.slow.knee_db,
                config.slow.attack_ms,
                config.slow.release_ms,
                config.slow.enabled != 0,
            );

            // Configure medium stage
            agc3.set_medium(
                config.medium.target_level_db,
                config.medium.threshold_db,
                config.medium.ratio,
                config.medium.knee_db,
                config.medium.attack_ms,
                config.medium.release_ms,
                config.medium.enabled != 0,
            );

            // Configure fast stage
            agc3.set_fast(
                config.fast.target_level_db,
                config.fast.threshold_db,
                config.fast.ratio,
                config.fast.knee_db,
                config.fast.attack_ms,
                config.fast.release_ms,
                config.fast.enabled != 0,
            );

            // Enable the overall 3-stage AGC
            agc3.set_enabled(true);
        }

        // Switch to 3-stage mode
        self.agc_use_3stage = true;
    }

    /// Check if 3-stage AGC mode is active.
    pub fn is_agc_3stage(&self) -> bool {
        self.agc_use_3stage
    }

    /// Get individual stage gain reduction values (if 3-stage mode).
    /// Returns (slow_gr, medium_gr, fast_gr) in dB, or (0, 0, 0) if not in 3-stage mode.
    pub fn get_agc_3stage_gr(&self) -> (f32, f32, f32) {
        if self.agc_use_3stage {
            self.agc_3stage
                .as_ref()
                .map(|a| a.stage_gain_reduction_db())
                .unwrap_or((0.0, 0.0, 0.0))
        } else {
            (0.0, 0.0, 0.0)
        }
    }

    /// Configure the stereo enhancer.
    /// Band 0 (bass) is always bypassed internally to avoid phase issues.
    pub fn set_stereo_enhancer(&mut self, config: &StereoEnhancerConfig) {
        self.stereo_enhancer.set_enabled(config.enabled != 0);

        for (i, band_cfg) in config.bands.iter().enumerate() {
            // Band 0 is always disabled internally, but we still pass the config
            self.stereo_enhancer.set_band(
                i,
                band_cfg.target_width,
                band_cfg.max_gain_db,
                band_cfg.max_atten_db,
                band_cfg.attack_ms,
                band_cfg.release_ms,
                band_cfg.enabled != 0,
            );
        }
    }

    /// Check if stereo enhancer is globally enabled.
    pub fn is_stereo_enhancer_enabled(&self) -> bool {
        self.stereo_enhancer.is_enabled()
    }

    /// Enable or disable stereo enhancer globally.
    pub fn set_stereo_enhancer_enabled(&mut self, enabled: bool) {
        self.stereo_enhancer.set_enabled(enabled);
    }

    // ========================================================================
    // Parametric EQ Methods
    // ========================================================================

    /// Configure the per-band parametric EQ.
    pub fn set_parametric_eq(&mut self, config: &ParametricEqConfig) {
        self.parametric_eq.set_enabled(config.enabled != 0);

        for (i, band_cfg) in config.bands.iter().enumerate() {
            self.parametric_eq.set_band(
                i,
                band_cfg.frequency,
                band_cfg.q,
                band_cfg.gain_db,
                band_cfg.enabled != 0,
            );
        }
    }

    /// Check if parametric EQ is globally enabled.
    pub fn is_parametric_eq_enabled(&self) -> bool {
        self.parametric_eq.is_enabled()
    }

    /// Enable or disable parametric EQ globally.
    pub fn set_parametric_eq_enabled(&mut self, enabled: bool) {
        self.parametric_eq.set_enabled(enabled);
    }

    // ========================================================================
    // Soft Clipper Methods
    // ========================================================================

    /// Configure the soft clipper.
    pub fn set_soft_clipper(&mut self, config: &SoftClipperConfig) {
        self.soft_clipper.set_params(
            config.ceiling_db,
            config.knee_db,
            config.mode,
            config.oversample,
        );
        self.soft_clipper.set_enabled(config.enabled != 0);
    }

    /// Check if soft clipper is enabled.
    pub fn is_soft_clipper_enabled(&self) -> bool {
        self.soft_clipper.is_enabled()
    }

    /// Enable or disable soft clipper.
    pub fn set_soft_clipper_enabled(&mut self, enabled: bool) {
        self.soft_clipper.set_enabled(enabled);
    }

    /// Get soft clipper latency in samples (due to oversampling).
    pub fn get_soft_clipper_latency(&self) -> usize {
        // Oversampling doesn't add latency in our implementation
        // (we use linear interpolation, not FIR filtering)
        0
    }

    // ========================================================================
    // LUFS Meter Methods
    // ========================================================================

    /// Check if LUFS metering is enabled.
    pub fn is_lufs_enabled(&self) -> bool {
        self.lufs_meter.is_enabled()
    }

    /// Enable or disable LUFS metering.
    pub fn set_lufs_enabled(&mut self, enabled: bool) {
        self.lufs_meter.set_enabled(enabled);
    }

    /// Reset LUFS meter (clears integrated measurement).
    pub fn reset_lufs(&mut self) {
        self.lufs_meter.reset();
    }

    /// Get current LUFS readings.
    /// Returns (momentary, short_term, integrated).
    pub fn get_lufs(&self) -> (f32, f32, f32) {
        (
            self.lufs_meter.momentary_lufs(),
            self.lufs_meter.short_term_lufs(),
            self.lufs_meter.integrated_lufs(),
        )
    }

    /// Get the processor configuration.
    pub fn get_config(&self) -> &MultibandConfig {
        &self.config
    }

    /// Check if decode output mode is enabled.
    pub fn is_decode_output(&self) -> bool {
        self.config.header.decode_output != 0
    }

    /// Reset all DSP state (filter history, envelope followers).
    pub fn reset(&mut self) {
        self.agc.reset();
        if let Some(ref mut agc3) = self.agc_3stage {
            agc3.reset();
        }
        self.crossover.reset();
        for comp in &mut self.compressors {
            comp.reset();
        }
        self.stereo_enhancer.reset();
        self.parametric_eq.reset();
        self.soft_clipper.reset();
        self.lufs_meter.reset();
    }

    /// Pre-fill is no longer needed with direct processing.
    /// Kept for API compatibility.
    pub fn prefill(&mut self) {
        // No-op - direct processing doesn't need prefill
    }
}
