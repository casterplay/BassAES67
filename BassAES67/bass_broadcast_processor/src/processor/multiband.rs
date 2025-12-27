//! N-band multiband processor implementation.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::dsp::agc::{ThreeStageAGC, WidebandAGC};
use crate::dsp::compressor::Compressor;
use crate::dsp::gain::{apply_gain, db_to_linear, peak_level};
use crate::dsp::multiband::MultibandCrossover;
use crate::ffi::{BASS_ChannelGetData, BASS_DATA_FLOAT, DWORD};

use super::config::{Agc3StageConfig, AgcConfig, CompressorConfig, MultibandConfig, AGC_MODE_THREE_STAGE};
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
                Compressor::new(
                    band_cfg.threshold_db,
                    band_cfg.ratio,
                    band_cfg.attack_ms,
                    band_cfg.release_ms,
                    band_cfg.makeup_gain_db,
                    sample_rate,
                )
            })
            .collect();

        // Pre-allocate buffers
        let temp_buffer = vec![0.0f32; 32768];
        let band_buffer = vec![0.0f32; num_bands];

        Ok(Self {
            input_gain: db_to_linear(config.header.input_gain_db),
            output_gain: db_to_linear(config.header.output_gain_db),
            config,
            agc,
            agc_3stage: None,     // Not created until explicitly enabled
            agc_use_3stage: false, // Default to single-stage
            crossover,
            compressors,
            stats: Arc::new(MultibandAtomicStats::new(num_bands)),
            source_channel,
            output_handle: 0,
            temp_buffer,
            band_buffer,
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

            // Process each sample: AGC -> split -> compress -> sum
            for i in 0..frames {
                for ch in 0..channels {
                    let idx = i * channels + ch;
                    let mut sample = self.temp_buffer[idx];

                    // Wideband AGC (before multiband split)
                    // Use 3-stage if enabled, otherwise single-stage
                    if self.agc_use_3stage {
                        if let Some(ref mut agc3) = self.agc_3stage {
                            sample = agc3.process(sample, ch);
                        }
                    } else {
                        sample = self.agc.process(sample, ch);
                    }

                    // Split into N bands
                    self.crossover.split(sample, ch, &mut self.band_buffer);

                    // Compress each band and sum
                    let mut output = 0.0f32;
                    for (band_idx, band_sample) in self.band_buffer.iter().enumerate() {
                        let compressed = self.compressors[band_idx].process(*band_sample, ch);
                        output += compressed;
                    }

                    buffer[idx] = output;
                }
            }

            // Apply output gain
            apply_gain(&mut buffer[..samples_read], self.output_gain);

            // Track output peak
            let out_peak = peak_level(&buffer[..samples_read]);
            self.stats
                .output_peak_x1000
                .store((out_peak * 1000.0) as i32, Ordering::Relaxed);

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
            self.config.bands[band] = *config;
        }
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
    }

    /// Pre-fill is no longer needed with direct processing.
    /// Kept for API compatibility.
    pub fn prefill(&mut self) {
        // No-op - direct processing doesn't need prefill
    }
}
