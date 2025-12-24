//! RTP input stream implementation with lock-free audio transfer.
//!
//! Receives RTP packets from UDP, decodes audio, and provides samples to BASS
//! through a lock-free ring buffer.

use std::ffi::c_void;
use std::io::ErrorKind;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use ringbuf::{HeapRb, traits::{Producer, Consumer, Split, Observer}};

use crate::codec::{AudioDecoder, Pcm16Decoder, Pcm24Decoder, mpg123};
use crate::ffi::*;
use crate::rtp::{RtpPacket, RtpSocket, PayloadCodec};

/// Input stream statistics (lock-free atomic updates).
pub struct InputStats {
    pub packets_received: AtomicU64,
    pub packets_dropped: AtomicU64,
    pub decode_errors: AtomicU64,
    pub underruns: AtomicU64,
    /// Detected payload type from incoming stream
    pub detected_pt: AtomicU32,
}

impl InputStats {
    pub fn new() -> Self {
        Self {
            packets_received: AtomicU64::new(0),
            packets_dropped: AtomicU64::new(0),
            decode_errors: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            detected_pt: AtomicU32::new(0),
        }
    }
}

impl Default for InputStats {
    fn default() -> Self {
        Self::new()
    }
}

/// RTP input stream configuration.
#[derive(Clone)]
pub struct RtpInputConfig {
    /// Sample rate (48000)
    pub sample_rate: u32,
    /// Number of channels (1 or 2)
    pub channels: u16,
    /// Jitter buffer depth in milliseconds
    pub jitter_ms: u32,
}

impl Default for RtpInputConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            channels: 2,
            jitter_ms: 20,
        }
    }
}

/// Decoder type enum for codec switching.
enum DecoderType {
    /// No decoder (uninitialized)
    None,
    /// PCM 16-bit decoder
    Pcm16(Pcm16Decoder),
    /// PCM 24-bit decoder
    Pcm24(Pcm24Decoder),
    /// MP2 decoder (mpg123)
    Mp2(mpg123::Decoder),
}

impl DecoderType {
    /// Decode data and return number of samples written.
    fn decode(&mut self, data: &[u8], output: &mut [f32]) -> Result<usize, String> {
        match self {
            DecoderType::None => Err("No decoder initialized".to_string()),
            DecoderType::Pcm16(dec) => dec.decode(data, output)
                .map_err(|e| format!("PCM16 decode error: {}", e)),
            DecoderType::Pcm24(dec) => dec.decode(data, output)
                .map_err(|e| format!("PCM24 decode error: {}", e)),
            DecoderType::Mp2(dec) => {
                // MP2 decoder outputs i16, need to convert to f32
                // mpg123 may need multiple packets before producing output,
                // and RTP packets may contain multiple MP2 frames.
                let mut i16_buf = vec![0i16; output.len()];

                // RFC 2250: MPEG Audio RTP payload has a 4-byte header
                // Bytes 0-1: MBZ (must be zero)
                // Bytes 2-3: Fragment offset
                // The actual MPEG audio frame starts at byte 4
                let mp2_data = if data.len() > 4 && data[4] == 0xFF && (data[5] & 0xE0) == 0xE0 {
                    // Skip RFC 2250 header - sync word found at offset 4
                    &data[4..]
                } else if data.len() > 0 && data[0] == 0xFF && (data[1] & 0xE0) == 0xE0 {
                    // No RFC 2250 header - raw MPEG audio
                    data
                } else {
                    // Can't find sync word
                    return Ok(0);
                };

                // Feed data to the decoder
                if let Err(e) = dec.feed(mp2_data) {
                    return Err(format!("MP2 feed error: {:?}", e));
                }

                // Read all available samples (may be multiple frames)
                let mut total_samples = 0;
                loop {
                    match dec.read_samples(&mut i16_buf[total_samples..]) {
                        Ok(samples) if samples > 0 => {
                            total_samples += samples;
                            // Check if we have room for more
                            if total_samples + 2304 > output.len() {
                                break; // Output buffer nearly full
                            }
                        }
                        Ok(_) => break, // No more samples available
                        Err(e) => return Err(format!("MP2 decode error: {:?}", e)),
                    }
                }

                // Convert i16 to f32
                for i in 0..total_samples {
                    output[i] = i16_buf[i] as f32 / 32768.0;
                }
                Ok(total_samples)
            }
        }
    }
}

/// RTP input stream with lock-free architecture.
///
/// Each instance is independent and can be used with its own socket.
pub struct RtpInputStream {
    /// Ring buffer consumer (audio callback reads from here)
    consumer: ringbuf::HeapCons<f32>,
    /// Flag to stop receiver thread
    running: Arc<AtomicBool>,
    /// Stream ended flag
    ended: Arc<AtomicBool>,
    /// Receiver thread handle
    receiver_thread: Option<JoinHandle<()>>,
    /// BASS stream handle (set after creation)
    pub handle: HSTREAM,
    /// Stream configuration
    pub config: RtpInputConfig,
    /// Statistics (lock-free)
    stats: Arc<InputStats>,
    /// Target buffer level in samples
    target_samples: usize,
    /// Whether we're in initial buffering phase
    buffering: AtomicBool,
    /// Number of channels
    channels: usize,
    /// Adaptive resampling: fractional position
    resample_pos: f64,
    /// Previous frame samples (one per channel)
    prev_samples: Vec<f32>,
    /// Current frame samples (one per channel)
    curr_samples: Vec<f32>,
    /// Whether resampling is initialized
    resample_init: bool,
    /// PI controller integral error
    integral_error: f64,
}

impl RtpInputStream {
    /// Create a new RTP input stream.
    pub fn new(config: RtpInputConfig) -> Result<Self, String> {
        let channels = config.channels as usize;

        // Calculate buffer size
        let samples_per_ms = config.sample_rate / 1000;
        let target_samples = (config.jitter_ms * samples_per_ms) as usize * channels;
        let buffer_size = target_samples * 3; // 3x target for headroom

        // Create lock-free ring buffer
        let rb = HeapRb::<f32>::new(buffer_size);
        let (_producer, consumer) = rb.split();

        Ok(Self {
            consumer,
            running: Arc::new(AtomicBool::new(false)),
            ended: Arc::new(AtomicBool::new(false)),
            receiver_thread: None,
            handle: 0,
            config,
            stats: Arc::new(InputStats::new()),
            target_samples,
            buffering: AtomicBool::new(true),
            channels,
            resample_pos: 0.0,
            prev_samples: vec![0.0; channels],
            curr_samples: vec![0.0; channels],
            resample_init: false,
            integral_error: 0.0,
        })
    }

    /// Start receiving from the given socket.
    ///
    /// The socket should already be bound and configured for receiving.
    pub fn start(&mut self, socket: RtpSocket) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("Stream already running".to_string());
        }

        // Create new ring buffer
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

        // Start receiver thread
        self.running.store(true, Ordering::SeqCst);
        self.ended.store(false, Ordering::SeqCst);
        self.buffering.store(true, Ordering::Relaxed);

        let running = self.running.clone();
        let ended = self.ended.clone();
        let stats = self.stats.clone();
        let channels = self.config.channels;

        self.receiver_thread = Some(thread::spawn(move || {
            Self::receiver_loop(socket, running, ended, stats, producer, channels);
        }));

        Ok(())
    }

    /// Receiver thread loop - reads RTP packets and decodes to ring buffer.
    fn receiver_loop(
        socket: RtpSocket,
        running: Arc<AtomicBool>,
        ended: Arc<AtomicBool>,
        stats: Arc<InputStats>,
        mut producer: ringbuf::HeapProd<f32>,
        channels: u16,
    ) {
        let mut buf = [0u8; 2048];
        // MP2 frame = 1152 samples * 2 channels = 2304 samples, need extra headroom
        let mut sample_buf = vec![0.0f32; 4608];
        let mut decoder: DecoderType = DecoderType::None;
        let mut last_pt: Option<u8> = None;

        while running.load(Ordering::SeqCst) {
            match socket.recv(&mut buf) {
                Ok(len) if len >= 12 => {
                    if let Some(packet) = RtpPacket::parse(&buf[..len]) {
                        let pt = packet.header.payload_type;

                        // Switch decoder if payload type changed
                        if last_pt != Some(pt) {
                            decoder = create_decoder_for_pt(pt, channels as u8);
                            last_pt = Some(pt);
                            stats.detected_pt.store(pt as u32, Ordering::Relaxed);
                        }

                        stats.packets_received.fetch_add(1, Ordering::Relaxed);

                        // Decode payload
                        match decoder.decode(packet.payload, &mut sample_buf) {
                            Ok(samples) if samples > 0 => {
                                // Push to ring buffer (only if room for entire packet)
                                if producer.vacant_len() >= samples {
                                    producer.push_slice(&sample_buf[..samples]);
                                } else {
                                    stats.packets_dropped.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                            Ok(_) => {} // No samples decoded
                            Err(_) => {
                                stats.decode_errors.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
                Ok(_) => continue, // Packet too small
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => continue,
                Err(ref e) if e.kind() == ErrorKind::TimedOut => continue,
                Err(_) => break, // Socket error
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

    /// Load one frame from ring buffer.
    fn load_next_frame(&mut self) -> bool {
        if self.consumer.occupied_len() < self.channels {
            return false;
        }

        std::mem::swap(&mut self.prev_samples, &mut self.curr_samples);
        let read = self.consumer.pop_slice(&mut self.curr_samples);
        read == self.channels
    }

    /// Read samples with adaptive resampling.
    pub fn read_samples(&mut self, buffer: &mut [f32]) -> usize {
        let available = self.consumer.occupied_len();
        let is_buffering = self.buffering.load(Ordering::Relaxed);

        // Thresholds - more conservative to handle codec frame sizes
        // MP2 frames are 1152 samples * 2 channels = 2304 samples (~24ms at 48kHz)
        // Critical should be at least 2 frames worth to handle jitter
        let min_critical = 4608; // ~2 MP2 frames (48ms)
        let critical_threshold = (self.target_samples / 4).max(min_critical);  // 25% or 48ms minimum
        let recovery_threshold = self.target_samples;   // 100% of target to exit buffering

        // Buffering mode
        if is_buffering {
            if available >= recovery_threshold {
                self.buffering.store(false, Ordering::Relaxed);
            } else {
                buffer.fill(0.0);
                return buffer.len();
            }
        }

        // Enter recovery if critically low
        if available < critical_threshold {
            self.buffering.store(true, Ordering::Relaxed);
            buffer.fill(0.0);
            self.stats.underruns.fetch_add(1, Ordering::Relaxed);
            return buffer.len();
        }

        // PI controller for adaptive resampling
        let target = self.target_samples as f64;
        let error = (available as f64 - target) / target;

        const KP: f64 = 0.0001;
        const KI: f64 = 0.00005;
        const MAX_TRIM_PPM: f64 = 20.0;

        self.integral_error += error;
        let max_integral = MAX_TRIM_PPM / KI / 1e6;
        self.integral_error = self.integral_error.clamp(-max_integral, max_integral);

        let trim = KP * error + KI * self.integral_error;
        let trim_clamped = trim.clamp(-MAX_TRIM_PPM / 1e6, MAX_TRIM_PPM / 1e6);

        // Clock feedforward (when clock is available)
        let clock_feedforward = if crate::clock_bindings::clock_is_locked() {
            crate::clock_bindings::clock_get_frequency_ppm() / 1_000_000.0
        } else {
            0.0
        };

        let resample_ratio = 1.0 + clock_feedforward + trim_clamped;

        // Initialize resampling
        if !self.resample_init {
            if self.load_next_frame() && self.load_next_frame() {
                self.resample_init = true;
            } else {
                buffer.fill(0.0);
                return buffer.len();
            }
        }

        // Generate output with linear interpolation
        let frames_requested = buffer.len() / self.channels;
        let mut out_idx = 0;

        for _ in 0..frames_requested {
            let t = self.resample_pos;
            for ch in 0..self.channels {
                let prev = self.prev_samples[ch];
                let curr = self.curr_samples[ch];
                buffer[out_idx + ch] = prev + (curr - prev) * t as f32;
            }
            out_idx += self.channels;

            self.resample_pos += resample_ratio;

            while self.resample_pos >= 1.0 {
                self.resample_pos -= 1.0;
                if !self.load_next_frame() {
                    buffer[out_idx..].fill(0.0);
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

    /// Get buffer fill percentage.
    pub fn buffer_fill_percent(&self) -> u32 {
        let level = self.consumer.occupied_len();
        if self.target_samples > 0 {
            ((level as f64 / self.target_samples as f64) * 100.0) as u32
        } else {
            100
        }
    }

    /// Get statistics reference.
    pub fn stats(&self) -> &Arc<InputStats> {
        &self.stats
    }

    /// Get detected payload type.
    pub fn detected_payload_type(&self) -> u8 {
        self.stats.detected_pt.load(Ordering::Relaxed) as u8
    }
}

impl Drop for RtpInputStream {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Create decoder for a given payload type.
fn create_decoder_for_pt(pt: u8, _channels: u8) -> DecoderType {
    let codec = PayloadCodec::from_pt(pt);

    match codec {
        PayloadCodec::Pcm16 => DecoderType::Pcm16(Pcm16Decoder::new_auto(_channels)),
        PayloadCodec::Pcm24 => DecoderType::Pcm24(Pcm24Decoder::new_auto(_channels)),
        PayloadCodec::Mp2 => {
            match mpg123::Decoder::new() {
                Ok(dec) => DecoderType::Mp2(dec),
                Err(e) => {
                    eprintln!("Failed to create MP2 decoder: {:?}", e);
                    DecoderType::None
                }
            }
        }
        _ => DecoderType::None,
    }
}

// ============================================================================
// BASS STREAMPROC callback
// ============================================================================

/// BASS STREAMPROC callback for RTP input streams.
pub unsafe extern "system" fn input_stream_proc(
    _handle: HSTREAM,
    buffer: *mut c_void,
    length: DWORD,
    user: *mut c_void,
) -> DWORD {
    if user.is_null() {
        return 0;
    }

    let stream = &mut *(user as *mut RtpInputStream);

    let samples = length as usize / 4;
    let float_buffer = std::slice::from_raw_parts_mut(buffer as *mut f32, samples);

    let written = stream.read_samples(float_buffer);

    if stream.is_ended() {
        (written * 4) as DWORD | BASS_STREAMPROC_END
    } else {
        (written * 4) as DWORD
    }
}
