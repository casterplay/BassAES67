//! AES67 stream implementation with lock-free audio transfer.
//! Manages UDP multicast reception, RTP parsing, and BASS integration.
//! Uses a lock-free ring buffer between receiver thread and audio callback.

use std::ffi::c_void;
use std::net::{UdpSocket, Ipv4Addr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use ringbuf::{HeapRb, traits::{Producer, Consumer, Split, Observer}};

use super::url::Aes67Url;
use super::rtp::{RtpPacket, convert_24bit_be_to_float};
use crate::ffi::*;
use crate::ffi::addon::AddonFunctions;

/// Statistics tracked with atomics (no locking needed)
struct StreamStats {
    packets_received: AtomicU64,
    packets_dropped: AtomicU64,
    underruns: AtomicU64,
    /// Detected packet time in microseconds (from first packet payload size)
    detected_packet_time_us: AtomicU64,
}

impl StreamStats {
    fn new() -> Self {
        Self {
            packets_received: AtomicU64::new(0),
            packets_dropped: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            detected_packet_time_us: AtomicU64::new(0),
        }
    }
}

/// AES67 input stream with lock-free architecture
pub struct Aes67Stream {
    /// Ring buffer consumer (audio callback reads from here)
    consumer: ringbuf::HeapCons<f32>,
    /// Flag to stop receiver thread
    running: Arc<AtomicBool>,
    /// Stream ended flag (set by receiver on socket error)
    ended: Arc<AtomicBool>,
    /// Receiver thread handle
    receiver_thread: Option<JoinHandle<()>>,
    /// BASS stream handle (set after creation)
    pub handle: HSTREAM,
    /// Stream configuration
    config: Aes67Url,
    /// Statistics (lock-free)
    stats: Arc<StreamStats>,
    /// Target buffer level in samples
    target_samples: usize,
    /// Whether we're in initial buffering phase
    buffering: AtomicBool,
    /// Number of channels
    channels: usize,
    /// Adaptive resampling: fractional position for interpolation
    resample_pos: f64,
    /// Adaptive resampling: previous frame samples (one per channel)
    prev_samples: Vec<f32>,
    /// Adaptive resampling: current frame samples (one per channel)
    curr_samples: Vec<f32>,
    /// Whether resampling state is initialized
    resample_init: bool,
    /// Integral term for PI controller (accumulated error)
    integral_error: f64,
    /// Smoothed resampling ratio (exponential moving average)
    smoothed_ratio: f64,
    /// Smoothed PTP ppm value (for gradual correction)
    last_ptp_ppm: f64,
}

impl Aes67Stream {
    /// Create a new AES67 stream from URL parameters.
    pub fn new(config: Aes67Url) -> Result<Self, String> {
        let channels = config.channels as usize;

        // Calculate buffer size based on jitter_ms setting
        // Use 3x target for headroom
        let samples_per_ms = config.sample_rate / 1000;
        let target_samples = (config.jitter_ms * samples_per_ms) as usize * channels;
        let buffer_size = target_samples * 3;

        // Create lock-free ring buffer
        // Producer will be created fresh in start() and given to receiver thread
        let rb = HeapRb::<f32>::new(buffer_size);
        let (_producer, consumer) = rb.split();

        Ok(Self {
            consumer,
            running: Arc::new(AtomicBool::new(false)),
            ended: Arc::new(AtomicBool::new(false)),
            receiver_thread: None,
            handle: 0,
            config,
            stats: Arc::new(StreamStats::new()),
            target_samples,
            buffering: AtomicBool::new(true),
            channels,
            resample_pos: 0.0,
            prev_samples: vec![0.0; channels],
            curr_samples: vec![0.0; channels],
            resample_init: false,
            integral_error: 0.0,
            smoothed_ratio: 1.0,
            last_ptp_ppm: 0.0,
        })
    }

    /// Start the stream - creates UDP socket and begins receiving packets.
    pub fn start(&mut self) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("Stream already running".to_string());
        }

        // Create UDP socket
        let socket = self.create_multicast_socket()?;

        // Create a new ring buffer and swap out consumer
        let samples_per_ms = self.config.sample_rate / 1000;
        let target_samples = (self.config.jitter_ms * samples_per_ms) as usize * self.channels;
        let buffer_size = target_samples * 3;

        let rb = HeapRb::<f32>::new(buffer_size);
        let (producer, consumer) = rb.split();
        self.consumer = consumer;
        self.target_samples = target_samples;

        // Reset resampling state
        self.resample_pos = 0.0;
        self.prev_samples.fill(0.0);
        self.curr_samples.fill(0.0);
        self.resample_init = false;
        self.integral_error = 0.0;
        self.last_ptp_ppm = 0.0;

        // Start receiver thread
        self.running.store(true, Ordering::SeqCst);
        self.ended.store(false, Ordering::SeqCst);
        self.buffering.store(true, Ordering::Relaxed);

        let running = self.running.clone();
        let ended = self.ended.clone();
        let stats = self.stats.clone();
        let payload_type = self.config.payload_type;
        let channels = self.config.channels;
        let sample_rate = self.config.sample_rate;

        self.receiver_thread = Some(thread::spawn(move || {
            Self::receiver_loop(socket, running, ended, stats, producer, payload_type, channels, sample_rate);
        }));

        Ok(())
    }

    /// Create and configure the multicast UDP socket.
    fn create_multicast_socket(&self) -> Result<UdpSocket, String> {
        let socket_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, self.config.port);

        let socket = UdpSocket::bind(socket_addr)
            .map_err(|e| format!("Failed to bind socket to {}: {}", socket_addr, e))?;

        let interface = self.config.interface.unwrap_or(Ipv4Addr::UNSPECIFIED);

        socket
            .join_multicast_v4(&self.config.multicast_addr, &interface)
            .map_err(|e| {
                format!(
                    "Failed to join multicast group {} on interface {}: {}",
                    self.config.multicast_addr, interface, e
                )
            })?;

        // Set read timeout for clean shutdown
        socket
            .set_read_timeout(Some(std::time::Duration::from_millis(100)))
            .map_err(|e| format!("Failed to set read timeout: {}", e))?;

        Ok(socket)
    }

    /// Receiver thread loop - reads packets and pushes samples to ring buffer.
    /// This is the ONLY thread that writes to the ring buffer (single producer).
    fn receiver_loop(
        socket: UdpSocket,
        running: Arc<AtomicBool>,
        ended: Arc<AtomicBool>,
        stats: Arc<StreamStats>,
        mut producer: ringbuf::HeapProd<f32>,
        expected_pt: u8,
        channels: u16,
        sample_rate: u32,
    ) {
        let mut buf = [0u8; 2048];
        let mut sample_buf = vec![0.0f32; 480 * channels as usize]; // Max samples per packet

        while running.load(Ordering::SeqCst) {
            match socket.recv(&mut buf) {
                Ok(len) => {
                    if len < 12 {
                        continue;
                    }

                    if let Some(packet) = RtpPacket::parse(&buf[..len]) {
                        if packet.header.payload_type != expected_pt {
                            continue;
                        }

                        stats.packets_received.fetch_add(1, Ordering::Relaxed);

                        // Convert to float samples
                        let sample_count = packet.sample_count(channels);
                        let total_samples = sample_count * channels as usize;

                        // Detect packet time from first packet (only once)
                        if stats.detected_packet_time_us.load(Ordering::Relaxed) == 0 && sample_count > 0 {
                            // packet_time_us = (samples_per_channel * 1_000_000) / sample_rate
                            let packet_time_us = (sample_count as u64 * 1_000_000) / sample_rate as u64;
                            stats.detected_packet_time_us.store(packet_time_us, Ordering::Relaxed);
                        }

                        if total_samples > sample_buf.len() {
                            sample_buf.resize(total_samples, 0.0);
                        }

                        convert_24bit_be_to_float(packet.payload, &mut sample_buf[..total_samples], channels);

                        // Push to ring buffer (lock-free)
                        // IMPORTANT: Only push if we have room for the ENTIRE packet
                        // Partial pushes corrupt frame alignment (L/R channels get swapped)
                        if producer.vacant_len() >= total_samples {
                            producer.push_slice(&sample_buf[..total_samples]);
                        } else {
                            // Buffer full - drop this entire packet
                            stats.packets_dropped.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    continue;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    continue;
                }
                Err(_) => {
                    break;
                }
            }
        }

        ended.store(true, Ordering::SeqCst);
    }

    /// Stop the stream.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);

        if let Some(thread) = self.receiver_thread.take() {
            let _ = thread.join();
        }
    }

    /// Load one frame (all channels) from ring buffer into curr_samples.
    /// Returns false if not enough data available.
    fn load_next_frame(&mut self) -> bool {
        if self.consumer.occupied_len() < self.channels {
            return false;
        }

        // Shift: curr becomes prev
        std::mem::swap(&mut self.prev_samples, &mut self.curr_samples);

        // Pop new frame into curr
        let read = self.consumer.pop_slice(&mut self.curr_samples);
        read == self.channels
    }

    /// Get samples from ring buffer with adaptive resampling.
    /// Uses buffer level feedback to adjust consumption rate.
    /// When buffer is above target: consume faster (ratio > 1.0)
    /// When buffer is below target: consume slower (ratio < 1.0)
    pub fn read_samples(&mut self, buffer: &mut [f32]) -> usize {
        let available = self.consumer.occupied_len();
        let is_buffering = self.buffering.load(Ordering::Relaxed);

        // Critical buffer protection thresholds
        let critical_threshold = self.target_samples / 20;  // 5% - enter recovery mode
        let recovery_threshold = self.target_samples / 2;   // 50% - exit recovery mode

        // Buffering/recovery mode - wait until we have enough samples
        if is_buffering {
            if available >= recovery_threshold {
                self.buffering.store(false, Ordering::Relaxed);
                // DON'T reset integral_error - preserve accumulated correction to prevent oscillation
            } else {
                // Output silence while buffering
                for sample in buffer.iter_mut() {
                    *sample = 0.0;
                }
                return buffer.len();
            }
        }

        // If buffer falls critically low, enter recovery mode
        if available < critical_threshold {
            self.buffering.store(true, Ordering::Relaxed);
            // DON'T reset integral_error - preserve the accumulated correction
            // This helps prevent repeated underruns after recovery
            for sample in buffer.iter_mut() {
                *sample = 0.0;
            }
            self.stats.underruns.fetch_add(1, Ordering::Relaxed);
            return buffer.len();
        }

        // PI controller matches BASS consumption rate to actual packet arrival rate.
        // Output pulls at PTP-corrected rate, so we need to match that.
        let target = self.target_samples as f64;
        let error = (available as f64 - target) / target;  // Normalized: -1 to +1

        // PI controller: P for immediate response, I for steady-state tracking
        const KP: f64 = 0.0001;   // P: proportional response to buffer level
        const KI: f64 = 0.00005;  // I: moderate integral for fine-tuning
        const MAX_TRIM_PPM: f64 = 20.0;  // ±20 ppm for PI fine-tuning

        self.integral_error += error;
        let max_integral = MAX_TRIM_PPM / KI / 1e6;
        self.integral_error = self.integral_error.clamp(-max_integral, max_integral);

        let trim = KP * error + KI * self.integral_error;
        let trim_clamped = trim.clamp(-MAX_TRIM_PPM / 1e6, MAX_TRIM_PPM / 1e6);

        // Clock feedforward: match output's consumption rate when clock is locked
        // Output uses interval_factor = 1.0 - (ppm / 1e6), so it sends FASTER when ppm > 0
        // We need to consume FASTER too, so add ppm to our ratio
        let clock_feedforward = if crate::clock_bindings::clock_is_locked() {
            let ppm = crate::clock_bindings::clock_get_frequency_ppm();
            ppm / 1_000_000.0
        } else {
            0.0  // No feedforward during clock calibration
        };

        let resample_ratio = 1.0 + clock_feedforward + trim_clamped;

        // Initialize resampling state if needed
        if !self.resample_init {
            if self.load_next_frame() && self.load_next_frame() {
                self.resample_init = true;
            } else {
                // Not enough data - output silence
                for sample in buffer.iter_mut() {
                    *sample = 0.0;
                }
                return buffer.len();
            }
        }

        // Generate output samples using linear interpolation
        let frames_requested = buffer.len() / self.channels;
        let mut out_idx = 0;

        for _ in 0..frames_requested {
            // Linear interpolation between prev and curr frames
            let t = self.resample_pos;
            for ch in 0..self.channels {
                let prev = self.prev_samples[ch];
                let curr = self.curr_samples[ch];
                buffer[out_idx + ch] = prev + (curr - prev) * t as f32;
            }
            out_idx += self.channels;

            // Advance position by resample ratio
            self.resample_pos += resample_ratio;

            // Load new frames as needed
            while self.resample_pos >= 1.0 {
                self.resample_pos -= 1.0;
                if !self.load_next_frame() {
                    // Underrun - fill rest with silence
                    for sample in buffer[out_idx..].iter_mut() {
                        *sample = 0.0;
                    }
                    self.stats.underruns.fetch_add(1, Ordering::Relaxed);
                    return buffer.len();
                }
            }
        }

        buffer.len()
    }

    /// Check if stream has ended.
    pub fn is_ended(&self) -> bool {
        self.ended.load(Ordering::SeqCst) && self.consumer.occupied_len() == 0
    }

    /// Get stream configuration.
    pub fn config(&self) -> &Aes67Url {
        &self.config
    }

    /// Get buffer fill percentage (0-200, where 100 = at target level).
    pub fn buffer_fill_percent(&self) -> u32 {
        let level = self.consumer.occupied_len();
        if self.target_samples > 0 {
            ((level as f64 / self.target_samples as f64) * 100.0) as u32
        } else {
            100
        }
    }

    /// Get underrun count.
    pub fn jitter_underruns(&self) -> u64 {
        self.stats.underruns.load(Ordering::Relaxed)
    }

    /// Get total packets received.
    pub fn packets_received(&self) -> u64 {
        self.stats.packets_received.load(Ordering::Relaxed)
    }

    /// Get count of packets dropped (buffer overflow).
    pub fn packets_late(&self) -> u64 {
        self.stats.packets_dropped.load(Ordering::Relaxed)
    }

    /// Get current buffer level in samples.
    pub fn buffer_packets(&self) -> usize {
        let samples = self.consumer.occupied_len();
        let samples_per_packet = 48 * self.channels; // Assume 1ms packets
        samples / samples_per_packet.max(1)
    }

    /// Get target buffer level in packets.
    pub fn target_packets(&self) -> usize {
        let samples_per_packet = 48 * self.channels;
        self.target_samples / samples_per_packet.max(1)
    }

    /// Get detected packet time in microseconds (from first received packet).
    /// Returns 0 if no packets received yet.
    pub fn detected_packet_time_us(&self) -> u64 {
        self.stats.detected_packet_time_us.load(Ordering::Relaxed)
    }
}

impl Drop for Aes67Stream {
    fn drop(&mut self) {
        self.stop();
    }
}

/// BASS STREAMPROC callback for AES67 streams.
/// This function is called by BASS to fill the playback buffer.
pub unsafe extern "system" fn stream_proc(
    _handle: HSTREAM,
    buffer: *mut c_void,
    length: DWORD,
    user: *mut c_void,
) -> DWORD {
    if user.is_null() {
        return 0;
    }

    // Need mutable reference for read_samples
    let stream = &mut *(user as *mut Aes67Stream);

    let samples = length as usize / 4;
    let float_buffer = std::slice::from_raw_parts_mut(buffer as *mut f32, samples);

    let written = stream.read_samples(float_buffer);

    if stream.is_ended() {
        (written * 4) as DWORD | BASS_STREAMPROC_END
    } else {
        (written * 4) as DWORD
    }
}

// ============================================================================
// Addon functions for BASS integration
// ============================================================================

/// Free the stream instance
unsafe extern "system" fn addon_free(inst: *mut c_void) {
    if !inst.is_null() {
        crate::clear_active_stream(inst as *mut Aes67Stream);
        let _ = Box::from_raw(inst as *mut Aes67Stream);
    }
}

/// Get stream length (return -1 for unknown/infinite live stream)
unsafe extern "system" fn addon_get_length(_inst: *mut c_void, _mode: DWORD) -> QWORD {
    u64::MAX
}

/// Get stream info - fill BASS_CHANNELINFO
unsafe extern "system" fn addon_get_info(inst: *mut c_void, info: *mut BassChannelInfo) {
    if inst.is_null() || info.is_null() {
        return;
    }

    let stream = &*(inst as *const Aes67Stream);
    let cfg = &stream.config;

    (*info).freq = cfg.sample_rate;
    (*info).chans = cfg.channels as DWORD;
    (*info).flags = BASS_SAMPLE_FLOAT;
    (*info).ctype = BASS_CTYPE_STREAM_AES67;
    (*info).origres = 24;
    (*info).plugin = 0;
    (*info).sample = 0;
    (*info).filename = std::ptr::null();
}

/// Check if position can be set - NO for live streams
unsafe extern "system" fn addon_can_set_position(_inst: *mut c_void, _pos: QWORD, _mode: DWORD) -> BOOL {
    FALSE
}

/// Set position - not supported for live streams
unsafe extern "system" fn addon_set_position(_inst: *mut c_void, _pos: QWORD, _mode: DWORD) -> QWORD {
    0
}

/// Static addon functions structure for BASS
pub static ADDON_FUNCS: AddonFunctions = AddonFunctions {
    flags: 0,
    free: Some(addon_free),
    get_length: Some(addon_get_length),
    get_tags: None,
    get_file_position: None,
    get_info: Some(addon_get_info),
    can_set_position: Some(addon_can_set_position),
    set_position: Some(addon_set_position),
    get_position: None,
    set_sync: None,
    remove_sync: None,
    can_resume: None,
    set_flags: None,
    attribute: None,
    attribute_ex: None,
};
