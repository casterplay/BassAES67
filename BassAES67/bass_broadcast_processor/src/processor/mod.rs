//! Main broadcast processor implementation.

mod config;
pub mod multiband;
mod stats;

pub use config::*;
pub use multiband::MultibandProcessor;
pub use stats::*;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::dsp::compressor::Compressor;
use crate::dsp::crossover::LR4Crossover;
use crate::dsp::gain::{apply_gain, db_to_linear, peak_level};
use crate::ffi::{BASS_ChannelGetData, BASS_DATA_FLOAT, DWORD};

/// Main broadcast audio processor.
/// Processes audio from a BASS source channel through a 2-band compressor chain.
pub struct BroadcastProcessor {
    /// Configuration
    config: ProcessorConfig,
    /// Crossover filter
    crossover: LR4Crossover,
    /// Low band compressor
    pub low_comp: Compressor,
    /// High band compressor
    pub high_comp: Compressor,
    /// Input gain (linear)
    pub input_gain: f32,
    /// Output gain (linear)
    pub output_gain: f32,
    /// Lock-free statistics
    stats: Arc<AtomicStats>,
    /// Source BASS channel handle
    source_channel: DWORD,
    /// Output BASS stream handle (set after creation)
    pub output_handle: DWORD,
    /// Temporary buffer for reading from source (reused to avoid allocations)
    temp_buffer: Vec<f32>,
    /// Bypass mode - when true, audio passes through unprocessed
    pub bypass: bool,
}

impl BroadcastProcessor {
    /// Create a new broadcast processor.
    ///
    /// # Arguments
    /// * `source_channel` - BASS channel handle to pull audio from
    /// * `config` - Processor configuration
    pub fn new(source_channel: DWORD, config: ProcessorConfig) -> Result<Self, String> {
        let sample_rate = config.sample_rate as f32;

        // Create crossover
        let crossover = LR4Crossover::new(config.crossover_freq, sample_rate);

        // Create compressors
        let low_comp = Compressor::new(
            config.low_band.threshold_db,
            config.low_band.ratio,
            config.low_band.attack_ms,
            config.low_band.release_ms,
            config.low_band.makeup_gain_db,
            sample_rate,
        );
        let high_comp = Compressor::new(
            config.high_band.threshold_db,
            config.high_band.ratio,
            config.high_band.attack_ms,
            config.high_band.release_ms,
            config.high_band.makeup_gain_db,
            sample_rate,
        );

        // Pre-allocate temp buffer for typical BASS request size (about 20000 samples)
        let temp_buffer = vec![0.0f32; 32768];

        Ok(Self {
            input_gain: db_to_linear(config.input_gain_db),
            output_gain: db_to_linear(config.output_gain_db),
            config,
            crossover,
            low_comp,
            high_comp,
            stats: Arc::new(AtomicStats::new()),
            source_channel,
            output_handle: 0,
            temp_buffer,
            bypass: false,
        })
    }

    /// Process samples directly from source to output buffer.
    /// Called by STREAMPROC with the exact buffer BASS needs filled.
    pub fn read_samples(&mut self, buffer: &mut [f32]) -> usize {
        let start_time = std::time::Instant::now();

        let samples_needed = buffer.len();
        let channels = self.config.channels as usize;

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
            self.stats.low_gr_x100.store(0, Ordering::Relaxed);
            self.stats.high_gr_x100.store(0, Ordering::Relaxed);
        } else {
            // Apply input gain
            apply_gain(&mut self.temp_buffer[..samples_read], self.input_gain);

            // Process each sample: split -> compress -> sum
            for i in 0..frames {
                for ch in 0..channels {
                    let idx = i * channels + ch;
                    let sample = self.temp_buffer[idx];

                    // Split into bands
                    let (low, high) = self.crossover.split(sample, ch);

                    // Compress each band
                    let low_processed = self.low_comp.process(low, ch);
                    let high_processed = self.high_comp.process(high, ch);

                    // Sum bands and write to output buffer
                    buffer[idx] = low_processed + high_processed;
                }
            }

            // Apply output gain
            apply_gain(&mut buffer[..samples_read], self.output_gain);

            // Track output peak and gain reduction
            let out_peak = peak_level(&buffer[..samples_read]);
            self.stats
                .output_peak_x1000
                .store((out_peak * 1000.0) as i32, Ordering::Relaxed);
            self.stats.low_gr_x100.store(
                (self.low_comp.gain_reduction_db() * 100.0) as i32,
                Ordering::Relaxed,
            );
            self.stats.high_gr_x100.store(
                (self.high_comp.gain_reduction_db() * 100.0) as i32,
                Ordering::Relaxed,
            );
        }

        self.stats.samples_processed.fetch_add(
            frames as u64,
            Ordering::Relaxed,
        );

        // Record processing time
        let elapsed = start_time.elapsed();
        self.stats.process_time_us.store(elapsed.as_micros() as u64, Ordering::Relaxed);

        samples_read
    }

    /// Pre-fill is no longer needed with direct processing.
    /// Kept for API compatibility.
    pub fn prefill(&mut self) {
        // No-op - direct processing doesn't need prefill
    }

    /// Get statistics snapshot.
    pub fn get_stats(&self) -> ProcessorStats {
        ProcessorStats {
            samples_processed: self.stats.samples_processed.load(Ordering::Relaxed),
            input_peak: self.stats.input_peak_x1000.load(Ordering::Relaxed) as f32 / 1000.0,
            output_peak: self.stats.output_peak_x1000.load(Ordering::Relaxed) as f32 / 1000.0,
            low_band_gr_db: self.stats.low_gr_x100.load(Ordering::Relaxed) as f32 / 100.0,
            high_band_gr_db: self.stats.high_gr_x100.load(Ordering::Relaxed) as f32 / 100.0,
            underruns: self.stats.underruns.load(Ordering::Relaxed),
            process_time_us: self.stats.process_time_us.load(Ordering::Relaxed),
        }
    }

    /// Get the processor configuration.
    pub fn get_config(&self) -> &ProcessorConfig {
        &self.config
    }

    /// Reset all DSP state (filter history, envelope followers).
    pub fn reset(&mut self) {
        self.crossover.reset();
        self.low_comp.reset();
        self.high_comp.reset();
    }
}
