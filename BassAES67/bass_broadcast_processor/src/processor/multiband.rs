//! N-band multiband processor implementation.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::dsp::compressor::Compressor;
use crate::dsp::gain::{apply_gain, db_to_linear, peak_level};
use crate::dsp::multiband::MultibandCrossover;
use crate::ffi::{BASS_ChannelGetData, BASS_DATA_FLOAT, DWORD};

use super::config::{CompressorConfig, MultibandConfig};
use super::stats::{MultibandAtomicStats, MultibandStatsHeader};

/// N-band multiband audio processor.
/// Processes audio from a BASS source channel through an N-band compressor chain.
pub struct MultibandProcessor {
    /// Configuration
    config: MultibandConfig,
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

            // Clear all band GR meters
            for gr in &self.stats.band_gr_x100 {
                gr.store(0, Ordering::Relaxed);
            }
        } else {
            // Apply input gain
            apply_gain(&mut self.temp_buffer[..samples_read], self.input_gain);

            // Process each sample: split -> compress -> sum
            for i in 0..frames {
                for ch in 0..channels {
                    let idx = i * channels + ch;
                    let sample = self.temp_buffer[idx];

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
