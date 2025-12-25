//! Input stream: WebRTC -> BASS (receive audio from browser peers).
//!
//! Uses lock-free ring buffers to collect audio from all peers,
//! mixes them, and provides to BASS via STREAMPROC callback.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::ffi::c_void;

use ringbuf::traits::{Consumer, Observer};

use crate::ffi::bass::{DWORD, HSTREAM, BASS_STREAMPROC_END};
use crate::peer::MAX_PEERS;

/// Default buffer size in milliseconds
const DEFAULT_BUFFER_MS: u32 = 100;

/// Input stream statistics
#[derive(Default)]
pub struct InputStats {
    pub packets_received: AtomicU64,
    pub bytes_received: AtomicU64,
    pub decode_errors: AtomicU64,
    pub buffer_overruns: AtomicU64,
    pub buffer_underruns: AtomicU64,
}

/// Input stream: receives audio from WebRTC peers and provides to BASS.
pub struct WebRtcInputStream {
    /// Sample rate (48000)
    sample_rate: u32,
    /// Channels (2 for stereo)
    channels: u16,
    /// Ring buffer consumers for each peer slot
    peer_consumers: [Option<ringbuf::HeapCons<f32>>; MAX_PEERS],
    /// Buffering state
    buffering: AtomicBool,
    /// Target buffer level (samples)
    target_samples: usize,
    /// Recovery threshold (samples) - exit buffering when reached
    recovery_threshold: usize,
    /// Critical threshold (samples) - enter buffering when below
    critical_threshold: usize,
    /// Stream ended flag
    ended: AtomicBool,
    /// Statistics
    pub stats: Arc<InputStats>,
    /// Temp buffer for mixing
    mix_buffer: Vec<f32>,
    /// Active peer count for normalization
    active_peer_count: AtomicU32,
    /// Integral error for PI controller
    integral_error: f64,
    /// Resample position (fractional sample index)
    resample_pos: f64,
    /// Previous frame samples (for interpolation)
    prev_frame: Vec<f32>,
    /// Current frame samples (for interpolation)
    curr_frame: Vec<f32>,
}

impl WebRtcInputStream {
    /// Create a new input stream.
    ///
    /// # Arguments
    /// * `sample_rate` - Sample rate (should be 48000)
    /// * `channels` - Number of channels (1 or 2)
    /// * `buffer_ms` - Buffer size in milliseconds
    pub fn new(sample_rate: u32, channels: u16, buffer_ms: u32) -> Self {
        let samples_per_ms = (sample_rate as usize) / 1000;
        let buffer_samples = samples_per_ms * buffer_ms as usize * channels as usize;
        let target_samples = buffer_samples / 2; // Target 50% fill
        let recovery_threshold = (target_samples as f64 * 0.5) as usize;
        let critical_threshold = (target_samples as f64 * 0.1) as usize;

        // Frame size for mixing (20ms at 48kHz stereo = 1920 samples)
        let frame_samples = samples_per_ms * 20 * channels as usize;

        Self {
            sample_rate,
            channels,
            peer_consumers: Default::default(),
            buffering: AtomicBool::new(true),
            target_samples,
            recovery_threshold,
            critical_threshold,
            ended: AtomicBool::new(false),
            stats: Arc::new(InputStats::default()),
            mix_buffer: vec![0.0; frame_samples],
            active_peer_count: AtomicU32::new(0),
            integral_error: 0.0,
            resample_pos: 0.0,
            prev_frame: vec![0.0; channels as usize],
            curr_frame: vec![0.0; channels as usize],
        }
    }

    /// Set the ring buffer consumer for a peer slot.
    ///
    /// # Arguments
    /// * `peer_id` - Peer slot index (0-4)
    /// * `consumer` - Ring buffer consumer
    pub fn set_peer_consumer(&mut self, peer_id: u32, consumer: ringbuf::HeapCons<f32>) {
        if (peer_id as usize) < MAX_PEERS {
            self.peer_consumers[peer_id as usize] = Some(consumer);
            self.active_peer_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Remove a peer's ring buffer consumer.
    pub fn remove_peer_consumer(&mut self, peer_id: u32) {
        if (peer_id as usize) < MAX_PEERS {
            if self.peer_consumers[peer_id as usize].take().is_some() {
                self.active_peer_count.fetch_sub(1, Ordering::SeqCst);
            }
        }
    }

    /// Get total available samples across all peers (minimum of all buffers)
    fn min_available_samples(&self) -> usize {
        let mut min_available = usize::MAX;
        let mut has_any = false;

        for consumer in &self.peer_consumers {
            if let Some(c) = consumer {
                let available = c.occupied_len();
                if available < min_available {
                    min_available = available;
                }
                has_any = true;
            }
        }

        if has_any {
            min_available
        } else {
            0
        }
    }

    /// Read and mix samples from all peers.
    ///
    /// # Arguments
    /// * `output` - Output buffer for mixed samples
    ///
    /// # Returns
    /// Number of samples written
    pub fn read_samples(&mut self, output: &mut [f32]) -> usize {
        let available = self.min_available_samples();

        // Check buffering state
        if self.buffering.load(Ordering::Relaxed) {
            if available >= self.recovery_threshold {
                self.buffering.store(false, Ordering::Relaxed);
            } else {
                // Output silence while buffering
                output.fill(0.0);
                return output.len();
            }
        } else if available < self.critical_threshold {
            // Enter buffering mode
            self.buffering.store(true, Ordering::Relaxed);
            self.stats.buffer_underruns.fetch_add(1, Ordering::Relaxed);
            output.fill(0.0);
            return output.len();
        }

        // Calculate how many samples to read
        let samples_to_read = output.len().min(available);
        if samples_to_read == 0 {
            output.fill(0.0);
            return output.len();
        }

        // Clear output buffer
        output[..samples_to_read].fill(0.0);

        // Mix all peer buffers
        let mut peer_count = 0u32;
        let mut temp_buffer = vec![0.0f32; samples_to_read];

        for consumer in &mut self.peer_consumers {
            if let Some(c) = consumer {
                let read = c.pop_slice(&mut temp_buffer[..samples_to_read]);
                if read > 0 {
                    // Add to output (mixing)
                    for i in 0..read {
                        output[i] += temp_buffer[i];
                    }
                    peer_count += 1;
                }
            }
        }

        // Normalize if multiple peers (optional - can clip if needed)
        // For now, just clamp to prevent clipping
        if peer_count > 1 {
            for sample in output[..samples_to_read].iter_mut() {
                *sample = sample.clamp(-1.0, 1.0);
            }
        }

        samples_to_read
    }

    /// Mark stream as ended
    pub fn set_ended(&mut self) {
        self.ended.store(true, Ordering::SeqCst);
    }

    /// Check if stream has ended
    pub fn is_ended(&self) -> bool {
        self.ended.load(Ordering::SeqCst)
    }

    /// Get statistics snapshot
    pub fn get_stats(&self) -> InputStatsSnapshot {
        InputStatsSnapshot {
            packets_received: self.stats.packets_received.load(Ordering::Relaxed),
            bytes_received: self.stats.bytes_received.load(Ordering::Relaxed),
            decode_errors: self.stats.decode_errors.load(Ordering::Relaxed),
            buffer_overruns: self.stats.buffer_overruns.load(Ordering::Relaxed),
            buffer_underruns: self.stats.buffer_underruns.load(Ordering::Relaxed),
            buffer_level: self.min_available_samples() as u32,
            is_buffering: self.buffering.load(Ordering::Relaxed),
        }
    }
}

/// Statistics snapshot
#[derive(Debug, Clone, Default)]
pub struct InputStatsSnapshot {
    pub packets_received: u64,
    pub bytes_received: u64,
    pub decode_errors: u64,
    pub buffer_overruns: u64,
    pub buffer_underruns: u64,
    pub buffer_level: u32,
    pub is_buffering: bool,
}

/// BASS STREAMPROC callback for input stream.
///
/// # Safety
/// This function is called from BASS's audio thread. The user pointer
/// must point to a valid WebRtcInputStream.
pub unsafe extern "system" fn input_stream_proc(
    _handle: HSTREAM,
    buffer: *mut c_void,
    length: DWORD,
    user: *mut c_void,
) -> DWORD {
    let stream = &mut *(user as *mut WebRtcInputStream);
    let samples = length as usize / 4; // 4 bytes per float
    let float_buffer = std::slice::from_raw_parts_mut(buffer as *mut f32, samples);

    let written = stream.read_samples(float_buffer);

    if stream.is_ended() {
        (written * 4) as DWORD | BASS_STREAMPROC_END
    } else {
        (written * 4) as DWORD
    }
}
