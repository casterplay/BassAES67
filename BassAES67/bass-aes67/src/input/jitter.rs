//! Jitter buffer for AES67 RTP streams.
//! Reorders packets and smooths out network timing variations.

use std::collections::VecDeque;
use super::rtp::{RtpPacket, sequence_diff, convert_24bit_be_to_float};

/// Single packet stored in the jitter buffer
struct BufferedPacket {
    sequence: u16,
    timestamp: u32,
    /// Audio data already converted to 32-bit float
    samples: Vec<f32>,
}

/// Statistics for monitoring jitter buffer health
#[derive(Debug, Default, Clone)]
pub struct JitterStats {
    pub packets_received: u64,
    pub packets_dropped_late: u64,
    pub packets_dropped_duplicate: u64,
    pub packets_reordered: u64,
    pub underruns: u64,
    pub overruns: u64,
}

/// Jitter buffer for RTP audio packets.
/// Stores packets indexed by sequence number and releases them in order.
pub struct JitterBuffer {
    /// Buffered packets sorted by sequence number
    packets: VecDeque<BufferedPacket>,
    /// Maximum buffer size in packets
    max_packets: usize,
    /// Target buffer level in packets (for latency control)
    target_level: usize,
    /// Next expected sequence number for playout
    playout_seq: Option<u16>,
    /// Number of channels
    channels: u16,
    /// Sample rate
    sample_rate: u32,
    /// Statistics
    stats: JitterStats,
    /// Whether we're in initial buffering phase
    buffering: bool,
    /// Fractional sample position for PTP-based resampling
    resample_position: f64,
    /// Current samples for interpolation (one per channel)
    curr_samples: Vec<f32>,
    /// Previous samples for interpolation (one per channel)
    prev_samples: Vec<f32>,
    /// Position within current packet's samples
    packet_sample_index: usize,
    /// Current packet being read (for resampling)
    current_packet_samples: Vec<f32>,
}

impl JitterBuffer {
    /// Create a new jitter buffer.
    ///
    /// # Arguments
    /// * `jitter_ms` - Target jitter buffer depth in milliseconds
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `channels` - Number of audio channels
    /// * `samples_per_packet` - Expected samples per RTP packet (e.g., 48 for 1ms at 48kHz)
    pub fn new(jitter_ms: u32, sample_rate: u32, channels: u16, samples_per_packet: usize) -> Self {
        // Calculate buffer size in packets
        let samples_per_ms = sample_rate / 1000;
        let target_samples = jitter_ms * samples_per_ms;
        let target_packets = (target_samples as usize / samples_per_packet).max(4);
        let max_packets = target_packets * 3; // Allow 3x target for burst handling

        Self {
            packets: VecDeque::with_capacity(max_packets),
            max_packets,
            target_level: target_packets,
            playout_seq: None,
            channels,
            sample_rate,
            stats: JitterStats::default(),
            buffering: true,
            resample_position: 0.0,
            curr_samples: vec![0.0; channels as usize],
            prev_samples: vec![0.0; channels as usize],
            packet_sample_index: 0,
            current_packet_samples: Vec::new(),
        }
    }

    /// Push an RTP packet into the buffer.
    /// Returns true if packet was accepted, false if dropped.
    pub fn push(&mut self, packet: &RtpPacket) -> bool {
        self.stats.packets_received += 1;

        let seq = packet.header.sequence;

        // Convert audio to float
        let sample_count = packet.sample_count(self.channels);
        let mut samples = vec![0.0f32; sample_count * self.channels as usize];
        convert_24bit_be_to_float(packet.payload, &mut samples, self.channels);

        let new_packet = BufferedPacket {
            sequence: seq,
            timestamp: packet.header.timestamp,
            samples,
        };

        // Check if this is a late packet (already played out)
        if let Some(playout) = self.playout_seq {
            let diff = sequence_diff(playout, seq);
            if diff < 0 {
                // Packet arrived too late
                self.stats.packets_dropped_late += 1;
                return false;
            }
        }

        // Find insertion point (maintain sequence order)
        let insert_pos = self.find_insert_position(seq);

        // Check for duplicate
        if let Some(pos) = insert_pos {
            if pos < self.packets.len() && self.packets[pos].sequence == seq {
                self.stats.packets_dropped_duplicate += 1;
                return false;
            }

            // Check if this is a reordered packet
            if pos < self.packets.len() {
                self.stats.packets_reordered += 1;
            }

            self.packets.insert(pos, new_packet);
        } else {
            self.packets.push_back(new_packet);
        }

        // Handle overflow
        while self.packets.len() > self.max_packets {
            self.packets.pop_front();
            self.stats.overruns += 1;
        }

        true
    }

    /// Find the position to insert a packet with given sequence number
    fn find_insert_position(&self, seq: u16) -> Option<usize> {
        if self.packets.is_empty() {
            return None;
        }

        // Binary search for insert position
        let mut left = 0;
        let mut right = self.packets.len();

        while left < right {
            let mid = (left + right) / 2;
            let diff = sequence_diff(self.packets[mid].sequence, seq);

            if diff < 0 {
                right = mid;
            } else {
                left = mid + 1;
            }
        }

        Some(left)
    }

    /// Read samples from the buffer into the output slice.
    /// Returns the number of samples written.
    pub fn read(&mut self, output: &mut [f32]) -> usize {
        // Check if we need to start buffering
        if self.buffering {
            if self.packets.len() >= self.target_level {
                self.buffering = false;
                // Initialize playout sequence from first packet
                if let Some(first) = self.packets.front() {
                    self.playout_seq = Some(first.sequence);
                }
            } else {
                // Still buffering - output silence
                for sample in output.iter_mut() {
                    *sample = 0.0;
                }
                return output.len();
            }
        }

        let mut written = 0;
        let mut remaining = output.len();

        while remaining > 0 {
            // Get next packet in sequence
            if let Some(playout_seq) = self.playout_seq {
                // Check if front packet matches expected sequence
                let front_matches = self.packets.front()
                    .map(|p| p.sequence == playout_seq)
                    .unwrap_or(false);

                if front_matches {
                    let packet = self.packets.pop_front().unwrap();
                    let to_copy = packet.samples.len().min(remaining);

                    output[written..written + to_copy]
                        .copy_from_slice(&packet.samples[..to_copy]);

                    written += to_copy;
                    remaining -= to_copy;

                    // Advance playout sequence
                    self.playout_seq = Some(playout_seq.wrapping_add(1));
                } else {
                    // Missing packet - output silence for one packet worth
                    // and advance sequence
                    self.stats.underruns += 1;

                    // Estimate samples per packet from existing packets
                    let silence_samples = self.packets.front()
                        .map(|p| p.samples.len())
                        .unwrap_or(48 * self.channels as usize); // Default 1ms

                    let to_silence = silence_samples.min(remaining);
                    for i in 0..to_silence {
                        output[written + i] = 0.0;
                    }

                    written += to_silence;
                    remaining -= to_silence;

                    // Advance playout sequence
                    self.playout_seq = Some(playout_seq.wrapping_add(1));

                    // If buffer is empty, go back to buffering mode
                    if self.packets.is_empty() {
                        self.buffering = true;
                        // Fill rest with silence
                        for sample in output[written..].iter_mut() {
                            *sample = 0.0;
                        }
                        return output.len();
                    }
                }
            } else {
                // No playout sequence yet - output silence
                for sample in output[written..].iter_mut() {
                    *sample = 0.0;
                }
                return output.len();
            }
        }

        written
    }

    /// Read samples with PTP-based resampling.
    /// Uses linear interpolation to adjust sample rate based on PTP frequency correction.
    /// frequency_ppm: PTP frequency correction (positive = local clock slow, need more output)
    pub fn read_resampled(&mut self, output: &mut [f32], frequency_ppm: f64) -> usize {
        let channels = self.channels as usize;

        // Check if we need to start buffering
        if self.buffering {
            if self.packets.len() >= self.target_level {
                self.buffering = false;
                // Initialize playout sequence from first packet
                if let Some(first) = self.packets.front() {
                    self.playout_seq = Some(first.sequence);
                }
                // Load first packet for resampling
                if let Some(packet) = self.packets.pop_front() {
                    self.current_packet_samples = packet.samples;
                    self.packet_sample_index = channels; // Start at second frame
                    self.resample_position = 0.0;
                    // Initialize: prev = first frame, curr = second frame
                    if self.current_packet_samples.len() >= channels * 2 {
                        for ch in 0..channels {
                            self.prev_samples[ch] = self.current_packet_samples[ch];
                            self.curr_samples[ch] = self.current_packet_samples[channels + ch];
                        }
                    }
                    if let Some(seq) = self.playout_seq {
                        self.playout_seq = Some(seq.wrapping_add(1));
                    }
                }
            } else {
                // Still buffering - output silence
                for sample in output.iter_mut() {
                    *sample = 0.0;
                }
                return output.len();
            }
        }

        // Clamp ppm to reasonable bounds (±1000ppm max)
        let ppm = frequency_ppm.clamp(-1000.0, 1000.0);

        // Calculate resampling step
        // When ppm > 0 (local slow): output consumes faster, step > 1.0 (consume more input)
        // When ppm < 0 (local fast): output consumes slower, step < 1.0 (consume less input)
        // Match what the output does: step = 1.0 - (ppm / 1_000_000.0) seems inverted
        // Try: step = 1.0 + (ppm / 1_000_000.0) to consume MORE when local is slow
        let step = 1.0 + (ppm / 1_000_000.0);

        let mut written = 0;
        let output_frames = output.len() / channels;

        for _frame in 0..output_frames {
            // Linear interpolation between prev and curr samples
            let frac = self.resample_position as f32;
            for ch in 0..channels {
                output[written + ch] = self.prev_samples[ch] + frac * (self.curr_samples[ch] - self.prev_samples[ch]);
            }
            written += channels;

            // Advance position by step
            self.resample_position += step;

            // Consume input samples as needed (when position crosses 1.0)
            while self.resample_position >= 1.0 {
                self.resample_position -= 1.0;

                // Advance to next input frame
                self.packet_sample_index += channels;

                // Check if we need a new packet
                if self.packet_sample_index >= self.current_packet_samples.len() {
                    if !self.load_next_packet() {
                        // No more packets - fill rest with silence
                        for sample in output[written..].iter_mut() {
                            *sample = 0.0;
                        }
                        return output.len();
                    }
                }

                // Shift samples: curr becomes prev, load new curr
                for ch in 0..channels {
                    self.prev_samples[ch] = self.curr_samples[ch];
                    self.curr_samples[ch] = self.current_packet_samples[self.packet_sample_index + ch];
                }
            }
        }

        written
    }

    /// Load next packet for resampling. Returns false if no packet available.
    fn load_next_packet(&mut self) -> bool {
        if let Some(playout_seq) = self.playout_seq {
            // Check if front packet matches expected sequence
            let front_matches = self.packets.front()
                .map(|p| p.sequence == playout_seq)
                .unwrap_or(false);

            if front_matches {
                let packet = self.packets.pop_front().unwrap();
                self.current_packet_samples = packet.samples;
                self.packet_sample_index = 0;
                self.playout_seq = Some(playout_seq.wrapping_add(1));
                return true;
            } else if !self.packets.is_empty() {
                // Missing packet - count as sequence gap and skip to next available
                self.stats.underruns += 1;
                // Skip to the sequence of the front packet
                if let Some(front) = self.packets.front() {
                    self.playout_seq = Some(front.sequence);
                }
                // Load the available packet
                let packet = self.packets.pop_front().unwrap();
                self.current_packet_samples = packet.samples;
                self.packet_sample_index = 0;
                if let Some(seq) = self.playout_seq {
                    self.playout_seq = Some(seq.wrapping_add(1));
                }
                return true;
            } else {
                // Buffer empty - go back to buffering
                self.buffering = true;
                return false;
            }
        }
        false
    }

    /// Get current buffer level in packets
    pub fn level(&self) -> usize {
        self.packets.len()
    }

    /// Get target buffer level in packets
    pub fn target_level(&self) -> usize {
        self.target_level
    }

    /// Check if buffer is in initial buffering phase
    pub fn is_buffering(&self) -> bool {
        self.buffering
    }

    /// Get buffer statistics
    pub fn stats(&self) -> &JitterStats {
        &self.stats
    }

    /// Reset the buffer (e.g., on stream restart)
    pub fn reset(&mut self) {
        self.packets.clear();
        self.playout_seq = None;
        self.buffering = true;
    }
}
