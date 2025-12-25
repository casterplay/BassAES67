//! RTP Input stream implementation.
//!
//! WE connect TO Z/IP ONE (or another RTP device), send our audio,
//! and receive return audio on the same socket.
//!
//! Lock-free architecture: no mutex in the audio path.

use std::ffi::c_void;
use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ringbuf::{traits::{Consumer, Observer, Producer, Split}, HeapRb};

use crate::clock_bindings::{clock_get_frequency_ppm, clock_is_locked, init_clock_bindings, ClockMode};
use crate::codec::g711::{G711UlawDecoder, G711UlawEncoder};
use crate::codec::g722::{G722Decoder, G722Encoder};
use crate::codec::ffmpeg_aac;
use crate::codec::mpg123;
use crate::codec::twolame;
use crate::codec::{
    AudioDecoder, AudioEncoder, AudioFormat, Pcm16Decoder, Pcm16Encoder, Pcm20Decoder,
    Pcm20Encoder, Pcm24Decoder, Pcm24Encoder,
};
use crate::ffi::*;
use crate::rtp::{PayloadCodec, RtpPacket, RtpPacketBuilder, RtpSocket};

/// FFI import for BASS_ChannelGetData
#[link(name = "bass")]
extern "system" {
    fn BASS_ChannelGetData(handle: DWORD, buffer: *mut c_void, length: DWORD) -> DWORD;
}

/// BASS_DATA_FLOAT flag
const BASS_DATA_FLOAT: DWORD = 0x40000000;

// ============================================================================
// Buffer Mode (shared with output module)
// ============================================================================

/// Buffer mode configuration.
#[derive(Clone, Copy, Debug)]
pub enum BufferMode {
    /// Simple mode: single buffer_ms value with automatic headroom
    Simple {
        /// Target buffer depth in milliseconds
        buffer_ms: u32,
    },
    /// Min/Max mode: separate min (target) and max (ceiling) values
    MinMax {
        /// Minimum buffer depth in milliseconds (target - system aims for this)
        min_ms: u32,
        /// Maximum buffer depth in milliseconds (ceiling - speed up if exceeded)
        max_ms: u32,
    },
}

impl Default for BufferMode {
    fn default() -> Self {
        BufferMode::Simple { buffer_ms: 100 }
    }
}

// ============================================================================
// Configuration
// ============================================================================

/// RTP Input stream configuration.
///
/// WE connect TO Z/IP ONE, send audio, receive return audio.
#[derive(Clone)]
pub struct RtpInputConfig {
    /// Remote IP address (Z/IP ONE) - we connect TO this
    pub remote_addr: Ipv4Addr,
    /// Remote port (9150-9153 for Z/IP ONE, or custom)
    pub remote_port: u16,
    /// Local port to bind (for receiving return audio, 0 = auto)
    pub local_port: u16,
    /// Network interface to bind to (0.0.0.0 = any)
    pub interface_addr: Ipv4Addr,
    /// Sample rate (48000)
    pub sample_rate: u32,
    /// Number of channels (1 or 2)
    pub channels: u16,
    /// Codec for sending audio
    pub send_codec: PayloadCodec,
    /// Bitrate for compressed codecs (kbps)
    pub send_bitrate: u32,
    /// Frame duration in milliseconds
    pub frame_duration_ms: u32,
    /// Clock mode (PTP/Livewire/System)
    pub clock_mode: ClockMode,
    /// PTP domain (0-127)
    pub ptp_domain: u8,
    /// Return audio buffer mode
    pub return_buffer_mode: BufferMode,
    /// Create return stream with BASS_STREAM_DECODE flag (for mixer compatibility)
    pub decode_stream: bool,
}

impl Default for RtpInputConfig {
    fn default() -> Self {
        Self {
            remote_addr: Ipv4Addr::new(192, 168, 1, 100),
            remote_port: 9152, // Z/IP ONE reciprocal - same codec reply
            local_port: 0,     // Auto-assign
            interface_addr: Ipv4Addr::UNSPECIFIED,
            sample_rate: 48000,
            channels: 2,
            send_codec: PayloadCodec::Pcm16,
            send_bitrate: 256,
            frame_duration_ms: 1,
            clock_mode: ClockMode::System,
            ptp_domain: 0,
            return_buffer_mode: BufferMode::Simple { buffer_ms: 100 },
            decode_stream: false,
        }
    }
}

// ============================================================================
// Statistics
// ============================================================================

/// Atomic statistics for lock-free access.
struct AtomicStats {
    // Send (TX) stats
    tx_packets: AtomicU64,
    tx_bytes: AtomicU64,
    tx_encode_errors: AtomicU64,
    tx_underruns: AtomicU64,
    // Receive (RX) stats - return audio
    rx_packets: AtomicU64,
    rx_bytes: AtomicU64,
    rx_decode_errors: AtomicU64,
    rx_dropped: AtomicU64,
    // Return audio buffer level (samples)
    buffer_level: AtomicU32,
    // Detected return codec PT
    detected_return_pt: AtomicU32,
}

impl AtomicStats {
    fn new() -> Self {
        Self {
            tx_packets: AtomicU64::new(0),
            tx_bytes: AtomicU64::new(0),
            tx_encode_errors: AtomicU64::new(0),
            tx_underruns: AtomicU64::new(0),
            rx_packets: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
            rx_decode_errors: AtomicU64::new(0),
            rx_dropped: AtomicU64::new(0),
            buffer_level: AtomicU32::new(0),
            detected_return_pt: AtomicU32::new(0),
        }
    }
}

/// Statistics snapshot for external access.
#[derive(Debug, Default, Clone)]
pub struct RtpInputStats {
    /// TX packets sent
    pub tx_packets: u64,
    /// TX bytes sent
    pub tx_bytes: u64,
    /// TX encode errors
    pub tx_encode_errors: u64,
    /// TX buffer underruns
    pub tx_underruns: u64,
    /// RX packets received (return audio)
    pub rx_packets: u64,
    /// RX bytes received
    pub rx_bytes: u64,
    /// RX decode errors
    pub rx_decode_errors: u64,
    /// RX packets dropped (buffer full)
    pub rx_dropped: u64,
    /// Current return buffer level (samples)
    pub buffer_level: u32,
    /// Detected return audio payload type
    pub detected_return_pt: u8,
    /// Current PPM adjustment
    pub current_ppm: f64,
}

// ============================================================================
// Encoder type for TX
// ============================================================================

/// Encoder type enum for sending audio.
enum SendEncoderType {
    None,
    Pcm16(Pcm16Encoder),
    Pcm20(Pcm20Encoder),
    Pcm24(Pcm24Encoder),
    Mp2(twolame::Encoder),
    G711Ulaw(G711UlawEncoder),
    G722(G722Encoder),
}

impl SendEncoderType {
    fn encode(&mut self, pcm: &[f32], output: &mut [u8]) -> Result<usize, String> {
        match self {
            SendEncoderType::None => Err("No encoder".to_string()),
            SendEncoderType::Pcm16(enc) => {
                enc.encode(pcm, output).map_err(|e| format!("{:?}", e))
            }
            SendEncoderType::Pcm20(enc) => {
                enc.encode(pcm, output).map_err(|e| format!("{:?}", e))
            }
            SendEncoderType::Pcm24(enc) => {
                enc.encode(pcm, output).map_err(|e| format!("{:?}", e))
            }
            SendEncoderType::Mp2(enc) => {
                enc.encode_float(pcm, output).map_err(|e| format!("{:?}", e))
            }
            SendEncoderType::G711Ulaw(enc) => {
                enc.encode(pcm, output).map_err(|e| format!("{:?}", e))
            }
            SendEncoderType::G722(enc) => {
                enc.encode(pcm, output).map_err(|e| format!("{:?}", e))
            }
        }
    }

    fn total_samples_per_frame(&self) -> usize {
        match self {
            SendEncoderType::None => 0,
            SendEncoderType::Pcm16(enc) => enc.total_samples_per_frame(),
            SendEncoderType::Pcm20(enc) => enc.total_samples_per_frame(),
            SendEncoderType::Pcm24(enc) => enc.total_samples_per_frame(),
            SendEncoderType::Mp2(enc) => enc.total_samples_per_frame(),
            SendEncoderType::G711Ulaw(enc) => enc.total_samples_per_frame(),
            SendEncoderType::G722(enc) => enc.total_samples_per_frame(),
        }
    }

    fn payload_type(&self) -> u8 {
        match self {
            SendEncoderType::None => 0,
            SendEncoderType::Pcm16(enc) => enc.payload_type(),
            SendEncoderType::Pcm20(enc) => enc.payload_type(),
            SendEncoderType::Pcm24(enc) => enc.payload_type(),
            SendEncoderType::Mp2(_) => 14, // MPEG Audio
            SendEncoderType::G711Ulaw(enc) => enc.payload_type(),
            SendEncoderType::G722(enc) => enc.payload_type(),
        }
    }
}

// ============================================================================
// Decoder type for RX (return audio)
// ============================================================================

/// Decoder type for return audio.
enum ReturnDecoderType {
    None,
    Pcm16(Pcm16Decoder),
    Pcm20(Pcm20Decoder),
    Pcm24(Pcm24Decoder),
    Mp2(mpg123::Decoder),
    G711Ulaw(G711UlawDecoder),
    G722(G722Decoder),
    Aac(ffmpeg_aac::Decoder),
}

impl ReturnDecoderType {
    fn decode(&mut self, data: &[u8], output: &mut [f32]) -> Result<usize, String> {
        match self {
            ReturnDecoderType::None => Err("No decoder".to_string()),
            ReturnDecoderType::Pcm16(dec) => {
                dec.decode(data, output).map_err(|e| format!("{:?}", e))
            }
            ReturnDecoderType::Pcm20(dec) => {
                dec.decode(data, output).map_err(|e| format!("{:?}", e))
            }
            ReturnDecoderType::Pcm24(dec) => {
                dec.decode(data, output).map_err(|e| format!("{:?}", e))
            }
            ReturnDecoderType::Mp2(dec) => {
                // MP2 handling with RFC 2250 header
                let mut i16_buf = vec![0i16; output.len()];
                let mp2_data =
                    if data.len() > 4 && data[4] == 0xFF && (data[5] & 0xE0) == 0xE0 {
                        &data[4..]
                    } else if !data.is_empty() && data[0] == 0xFF && (data[1] & 0xE0) == 0xE0 {
                        data
                    } else {
                        return Ok(0);
                    };

                if let Err(e) = dec.feed(mp2_data) {
                    return Err(format!("MP2 feed: {:?}", e));
                }

                let mut total = 0;
                loop {
                    match dec.read_samples(&mut i16_buf[total..]) {
                        Ok(n) if n > 0 => {
                            total += n;
                            if total + 2304 > output.len() {
                                break;
                            }
                        }
                        Ok(_) => break,
                        Err(e) => return Err(format!("MP2 decode: {:?}", e)),
                    }
                }

                for i in 0..total {
                    output[i] = i16_buf[i] as f32 / 32768.0;
                }
                Ok(total)
            }
            ReturnDecoderType::G711Ulaw(dec) => {
                dec.decode(data, output).map_err(|e| format!("{:?}", e))
            }
            ReturnDecoderType::G722(dec) => {
                dec.decode(data, output).map_err(|e| format!("{:?}", e))
            }
            ReturnDecoderType::Aac(dec) => {
                dec.decode(data, output).map_err(|e| format!("{:?}", e))
            }
        }
    }
}

/// Create decoder for payload type.
fn create_decoder_for_pt(pt: u8) -> Option<ReturnDecoderType> {
    match PayloadCodec::from_pt(pt) {
        PayloadCodec::Pcm16 => Some(ReturnDecoderType::Pcm16(Pcm16Decoder::new_auto(2))),
        PayloadCodec::Pcm20 => Some(ReturnDecoderType::Pcm20(Pcm20Decoder::new_auto(2))),
        PayloadCodec::Pcm24 => Some(ReturnDecoderType::Pcm24(Pcm24Decoder::new_auto(2))),
        PayloadCodec::Mp2 => {
            mpg123::Decoder::new().ok().map(ReturnDecoderType::Mp2)
        }
        PayloadCodec::G711Ulaw => Some(ReturnDecoderType::G711Ulaw(G711UlawDecoder::new())),
        PayloadCodec::G722 => Some(ReturnDecoderType::G722(G722Decoder::new())),
        PayloadCodec::Aac => {
            ffmpeg_aac::Decoder::new().ok().map(ReturnDecoderType::Aac)
        }
        PayloadCodec::Unknown(_) => None,
        _ => None,
    }
}

// ============================================================================
// Main stream struct
// ============================================================================

/// RTP Input stream.
///
/// WE connect TO the remote device, send audio, and receive return audio.
pub struct RtpInput {
    /// Running flag
    running: Arc<AtomicBool>,
    /// Statistics
    stats: Arc<AtomicStats>,
    /// Current PPM (scaled by 1000)
    current_ppm_x1000: Arc<AtomicI64>,
    /// TX thread
    tx_thread: Option<JoinHandle<()>>,
    /// RX thread
    rx_thread: Option<JoinHandle<()>>,
    /// Configuration
    pub config: RtpInputConfig,
    /// BASS source channel (we read audio from this to send)
    source_channel: HSTREAM,
    /// Return audio ring buffer consumer
    return_consumer: Option<ringbuf::HeapCons<f32>>,
    /// Return audio BASS handle
    pub return_handle: HSTREAM,
    /// Target buffer level (samples)
    target_samples: usize,
    /// Max buffer level (samples)
    max_samples: usize,
    /// Channels
    channels: usize,
    /// Adaptive resampling state
    resample_pos: f64,
    prev_samples: Vec<f32>,
    curr_samples: Vec<f32>,
    resample_init: bool,
    integral_error: f64,
    /// Initial buffering flag
    buffering: AtomicBool,
}

impl RtpInput {
    /// Create a new RTP Input stream.
    ///
    /// # Arguments
    /// * `source_channel` - BASS channel to read audio FROM to send to Z/IP ONE
    /// * `config` - Stream configuration
    pub fn new(source_channel: HSTREAM, config: RtpInputConfig) -> Result<Self, String> {
        init_clock_bindings();

        let channels = config.channels as usize;
        let samples_per_ms = (config.sample_rate / 1000) as usize;

        // Calculate buffer sizes
        let (target_samples, max_samples, buffer_size) = match config.return_buffer_mode {
            BufferMode::Simple { buffer_ms } => {
                let target = buffer_ms as usize * samples_per_ms * channels;
                let max = target * 3;
                (target, max, max)
            }
            BufferMode::MinMax { min_ms, max_ms } => {
                let target = min_ms as usize * samples_per_ms * channels;
                let max = max_ms as usize * samples_per_ms * channels;
                (target, max, max * 2)
            }
        };

        // Create ring buffer for return audio
        let rb = HeapRb::<f32>::new(buffer_size);
        let (_producer, consumer) = rb.split();

        Ok(Self {
            running: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(AtomicStats::new()),
            current_ppm_x1000: Arc::new(AtomicI64::new(0)),
            tx_thread: None,
            rx_thread: None,
            config,
            source_channel,
            return_consumer: Some(consumer),
            return_handle: 0,
            target_samples,
            max_samples,
            channels,
            resample_pos: 0.0,
            prev_samples: vec![0.0; channels],
            curr_samples: vec![0.0; channels],
            resample_init: false,
            integral_error: 0.0,
            buffering: AtomicBool::new(true),
        })
    }

    /// Start the stream.
    pub fn start(&mut self) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("Already running".to_string());
        }

        // Create socket
        let local_addr = SocketAddrV4::new(self.config.interface_addr, self.config.local_port);
        let socket = RtpSocket::bind(SocketAddr::V4(local_addr))?;
        socket.set_read_timeout(Some(Duration::from_millis(100)))
            .map_err(|e| e.to_string())?;

        let remote_addr = SocketAddr::V4(SocketAddrV4::new(
            self.config.remote_addr,
            self.config.remote_port,
        ));

        // Clone socket for RX thread
        let socket_for_rx = socket.try_clone()?;

        self.running.store(true, Ordering::SeqCst);

        // Create ring buffer
        let channels = self.config.channels as usize;
        let samples_per_ms = (self.config.sample_rate / 1000) as usize;
        let (target_samples, max_samples, buffer_size) = match self.config.return_buffer_mode {
            BufferMode::Simple { buffer_ms } => {
                let target = buffer_ms as usize * samples_per_ms * channels;
                (target, target * 3, target * 3)
            }
            BufferMode::MinMax { min_ms, max_ms } => {
                let target = min_ms as usize * samples_per_ms * channels;
                let max = max_ms as usize * samples_per_ms * channels;
                (target, max, max * 2)
            }
        };

        let rb = HeapRb::<f32>::new(buffer_size);
        let (producer, consumer) = rb.split();
        self.return_consumer = Some(consumer);
        self.target_samples = target_samples;
        self.max_samples = max_samples;

        // Start TX thread
        let running_tx = self.running.clone();
        let stats_tx = self.stats.clone();
        let ppm_tx = self.current_ppm_x1000.clone();
        let config_tx = self.config.clone();
        let source = self.source_channel;

        self.tx_thread = Some(thread::spawn(move || {
            Self::transmitter_loop(running_tx, stats_tx, ppm_tx, socket, remote_addr, config_tx, source);
        }));

        // Start RX thread
        let running_rx = self.running.clone();
        let stats_rx = self.stats.clone();

        self.rx_thread = Some(thread::spawn(move || {
            Self::receiver_loop(running_rx, stats_rx, socket_for_rx, producer);
        }));

        Ok(())
    }

    /// Stop the stream.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);

        if let Some(t) = self.tx_thread.take() {
            let _ = t.join();
        }
        if let Some(t) = self.rx_thread.take() {
            let _ = t.join();
        }
    }

    /// Transmitter loop - reads from BASS, encodes, sends RTP.
    fn transmitter_loop(
        running: Arc<AtomicBool>,
        stats: Arc<AtomicStats>,
        current_ppm_x1000: Arc<AtomicI64>,
        socket: RtpSocket,
        remote_addr: SocketAddr,
        config: RtpInputConfig,
        source_channel: HSTREAM,
    ) {
        // Set thread priority
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Threading::{
                GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
            };
            unsafe {
                SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
            }
        }

        let format = AudioFormat::new(config.sample_rate, config.channels as u8);

        // Create encoder
        let mut encoder = match config.send_codec {
            PayloadCodec::Pcm16 => SendEncoderType::Pcm16(Pcm16Encoder::new(
                format,
                config.frame_duration_ms as usize,
            )),
            PayloadCodec::Pcm20 => SendEncoderType::Pcm20(Pcm20Encoder::new(
                format,
                config.frame_duration_ms as usize,
            )),
            PayloadCodec::Pcm24 => SendEncoderType::Pcm24(Pcm24Encoder::new(
                format,
                config.frame_duration_ms as usize,
            )),
            PayloadCodec::Mp2 => match twolame::Encoder::new(format, config.send_bitrate) {
                Ok(e) => SendEncoderType::Mp2(e),
                Err(_) => return,
            },
            PayloadCodec::G711Ulaw => SendEncoderType::G711Ulaw(G711UlawEncoder::new()),
            PayloadCodec::G722 => SendEncoderType::G722(G722Encoder::new()),
            _ => {
                eprintln!("Unsupported send codec: {:?}", config.send_codec);
                return;
            }
        };

        let samples_per_frame = encoder.total_samples_per_frame();
        let samples_per_channel = samples_per_frame / config.channels as usize;
        let frame_duration_us = (samples_per_channel as u64 * 1_000_000) / config.sample_rate as u64;
        let base_interval = Duration::from_micros(frame_duration_us);

        let mut pcm_buffer = vec![0.0f32; samples_per_frame];
        let encode_buffer_size = match config.send_codec {
            PayloadCodec::Mp2 => 4608,
            PayloadCodec::G711Ulaw => samples_per_channel,
            PayloadCodec::G722 => samples_per_channel,
            _ => samples_per_frame * 3,
        };
        let mut encode_buffer = vec![0u8; encode_buffer_size];

        let mut packet_builder = RtpPacketBuilder::new(encoder.payload_type());
        let mut next_tx = Instant::now();
        let mut ppm_counter = 0u32;
        let mut current_ppm = 0.0f64;

        let _ = base_interval; // Suppress unused warning

        while running.load(Ordering::SeqCst) {
            // Update PPM every 100 packets
            ppm_counter += 1;
            if ppm_counter >= 100 {
                ppm_counter = 0;
                if clock_is_locked() {
                    current_ppm = clock_get_frequency_ppm();
                    current_ppm_x1000.store((current_ppm * 1000.0) as i64, Ordering::Relaxed);
                }
            }

            // Apply clock correction
            let factor = 1.0 - (current_ppm / 1_000_000.0);
            let adjusted_us = (frame_duration_us as f64 * factor) as u64;
            let interval = Duration::from_micros(adjusted_us);

            // Hybrid sleep-spin timing
            let now = Instant::now();
            if next_tx > now {
                let wait = next_tx - now;
                if wait > Duration::from_millis(2) {
                    thread::sleep(wait - Duration::from_millis(1));
                }
                while Instant::now() < next_tx {
                    std::hint::spin_loop();
                }
            }

            // Read from BASS
            let bytes_needed = (samples_per_frame * 4) as u32;
            let bytes_read = unsafe {
                BASS_ChannelGetData(
                    source_channel,
                    pcm_buffer.as_mut_ptr() as *mut c_void,
                    bytes_needed | BASS_DATA_FLOAT,
                )
            };

            if bytes_read == u32::MAX {
                pcm_buffer.fill(0.0);
                stats.tx_underruns.fetch_add(1, Ordering::Relaxed);
            } else if (bytes_read as usize) < samples_per_frame * 4 {
                let read = bytes_read as usize / 4;
                pcm_buffer[read..].fill(0.0);
                stats.tx_underruns.fetch_add(1, Ordering::Relaxed);
            }

            // Encode and send
            match encoder.encode(&pcm_buffer, &mut encode_buffer) {
                Ok(len) if len > 0 => {
                    let packet =
                        packet_builder.build_packet(&encode_buffer[..len], samples_per_channel as u32);
                    if socket.send_to(packet, remote_addr).is_ok() {
                        stats.tx_packets.fetch_add(1, Ordering::Relaxed);
                        stats.tx_bytes.fetch_add(packet.len() as u64, Ordering::Relaxed);
                    }
                }
                Ok(_) => {}
                Err(_) => {
                    stats.tx_encode_errors.fetch_add(1, Ordering::Relaxed);
                }
            }

            next_tx += interval;

            // Reset if behind
            if Instant::now() > next_tx + interval {
                next_tx = Instant::now() + interval;
            }
        }
    }

    /// Receiver loop - receives RTP, decodes, pushes to ring buffer.
    fn receiver_loop(
        running: Arc<AtomicBool>,
        stats: Arc<AtomicStats>,
        socket: RtpSocket,
        mut producer: ringbuf::HeapProd<f32>,
    ) {
        let mut recv_buf = vec![0u8; 4096];
        let mut decode_buf = vec![0.0f32; 8192];
        let mut decoder: ReturnDecoderType = ReturnDecoderType::None;
        let mut current_pt: Option<u8> = None;

        while running.load(Ordering::SeqCst) {
            match socket.recv(&mut recv_buf) {
                Ok(len) if len >= 12 => {
                    if let Some(packet) = RtpPacket::parse(&recv_buf[..len]) {
                        let pt = packet.header.payload_type;

                        // Switch decoder if PT changed
                        if current_pt != Some(pt) {
                            if let Some(new_dec) = create_decoder_for_pt(pt) {
                                decoder = new_dec;
                                current_pt = Some(pt);
                                stats.detected_return_pt.store(pt as u32, Ordering::Relaxed);
                            }
                        }

                        // Decode
                        match decoder.decode(packet.payload, &mut decode_buf) {
                            Ok(samples) if samples > 0 => {
                                stats.rx_packets.fetch_add(1, Ordering::Relaxed);
                                stats.rx_bytes.fetch_add(len as u64, Ordering::Relaxed);

                                // Push to ring buffer
                                let written = producer.push_slice(&decode_buf[..samples]);
                                if written < samples {
                                    stats.rx_dropped.fetch_add(1, Ordering::Relaxed);
                                }
                                stats.buffer_level.store(producer.occupied_len() as u32, Ordering::Relaxed);
                            }
                            Ok(_) => {}
                            Err(_) => {
                                stats.rx_decode_errors.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {}
                Err(_) => {}
            }
        }
    }

    /// Check if running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Get statistics snapshot.
    pub fn stats(&self) -> RtpInputStats {
        RtpInputStats {
            tx_packets: self.stats.tx_packets.load(Ordering::Relaxed),
            tx_bytes: self.stats.tx_bytes.load(Ordering::Relaxed),
            tx_encode_errors: self.stats.tx_encode_errors.load(Ordering::Relaxed),
            tx_underruns: self.stats.tx_underruns.load(Ordering::Relaxed),
            rx_packets: self.stats.rx_packets.load(Ordering::Relaxed),
            rx_bytes: self.stats.rx_bytes.load(Ordering::Relaxed),
            rx_decode_errors: self.stats.rx_decode_errors.load(Ordering::Relaxed),
            rx_dropped: self.stats.rx_dropped.load(Ordering::Relaxed),
            buffer_level: self.stats.buffer_level.load(Ordering::Relaxed),
            detected_return_pt: self.stats.detected_return_pt.load(Ordering::Relaxed) as u8,
            current_ppm: self.current_ppm_x1000.load(Ordering::Relaxed) as f64 / 1000.0,
        }
    }

    /// Get current PPM.
    pub fn applied_ppm(&self) -> f64 {
        self.current_ppm_x1000.load(Ordering::Relaxed) as f64 / 1000.0
    }

    /// Read samples from return audio buffer (for STREAMPROC callback).
    ///
    /// Uses adaptive resampling to handle clock drift.
    pub fn read_samples(&mut self, output: &mut [f32]) -> usize {
        let consumer = match &mut self.return_consumer {
            Some(c) => c,
            None => return 0,
        };

        let available = consumer.occupied_len();

        // Initial buffering
        if self.buffering.load(Ordering::Relaxed) {
            if available < self.target_samples {
                output.fill(0.0);
                return output.len();
            }
            self.buffering.store(false, Ordering::Relaxed);
        }

        // Calculate fill level and error
        let fill_level = available as f64 / self.target_samples as f64;
        let error = fill_level - 1.0;

        // PI controller
        const KP: f64 = 0.00002;
        const KI: f64 = 0.000001;
        const MAX_INTEGRAL: f64 = 50.0;

        self.integral_error = (self.integral_error + error).clamp(-MAX_INTEGRAL, MAX_INTEGRAL);
        let ppm_adjustment = error * KP * 1_000_000.0 + self.integral_error * KI * 1_000_000.0;
        let ppm_adjustment = ppm_adjustment.clamp(-100.0, 100.0);

        // Calculate resampling ratio
        let ratio = 1.0 + ppm_adjustment / 1_000_000.0;

        // Adaptive resampling output
        let channels = self.channels;
        let mut out_idx = 0;
        let out_frames = output.len() / channels;

        while out_idx < out_frames {
            // Load next frame if needed
            while self.resample_pos >= 1.0 && consumer.occupied_len() >= channels {
                std::mem::swap(&mut self.prev_samples, &mut self.curr_samples);
                for i in 0..channels {
                    self.curr_samples[i] = consumer.try_pop().unwrap_or(0.0);
                }
                self.resample_pos -= 1.0;
                self.resample_init = true;
            }

            if !self.resample_init {
                // Not enough data yet
                for c in 0..channels {
                    output[out_idx * channels + c] = 0.0;
                }
            } else {
                // Linear interpolation
                let t = self.resample_pos as f32;
                for c in 0..channels {
                    output[out_idx * channels + c] =
                        self.prev_samples[c] * (1.0 - t) + self.curr_samples[c] * t;
                }
            }

            self.resample_pos += ratio;
            out_idx += 1;
        }

        output.len()
    }

    /// Get return audio buffer level in milliseconds.
    pub fn buffer_level_ms(&self) -> u32 {
        let samples = self.stats.buffer_level.load(Ordering::Relaxed) as usize;
        let samples_per_ms = (self.config.sample_rate / 1000) as usize * self.channels;
        if samples_per_ms > 0 {
            (samples / samples_per_ms) as u32
        } else {
            0
        }
    }
}

impl Drop for RtpInput {
    fn drop(&mut self) {
        self.stop();
    }
}

unsafe impl Send for RtpInput {}

// ============================================================================
// BASS STREAMPROC callback for return audio
// ============================================================================

/// BASS STREAMPROC callback for RTP input return audio streams.
pub unsafe extern "system" fn input_return_stream_proc(
    _handle: HSTREAM,
    buffer: *mut c_void,
    length: DWORD,
    user: *mut c_void,
) -> DWORD {
    if user.is_null() {
        return 0;
    }

    let stream = &mut *(user as *mut RtpInput);

    let samples = length as usize / 4;
    let float_buffer = std::slice::from_raw_parts_mut(buffer as *mut f32, samples);

    let written = stream.read_samples(float_buffer);

    (written * 4) as DWORD
}
