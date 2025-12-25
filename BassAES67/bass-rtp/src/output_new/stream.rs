//! RTP Output stream implementation.
//!
//! Z/IP ONE (or another RTP device) connects TO us. We receive their audio
//! and send backfeed audio on the same socket.
//!
//! The remote address is auto-detected from the first incoming RTP packet.
//!
//! Lock-free architecture: no mutex in the audio path.

use std::ffi::c_void;
use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Connection state for callbacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum ConnectionState {
    /// No connection / disconnected
    Disconnected = 0,
    /// Connected and receiving audio
    Connected = 1,
}

/// Callback type for connection state changes.
/// Called when connection is established or lost.
pub type ConnectionCallback = extern "C" fn(state: ConnectionState, user_data: *mut c_void);

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
use crate::input::BufferMode;

/// FFI import for BASS_ChannelGetData
#[link(name = "bass")]
extern "system" {
    fn BASS_ChannelGetData(handle: DWORD, buffer: *mut c_void, length: DWORD) -> DWORD;
}

/// BASS_DATA_FLOAT flag
const BASS_DATA_FLOAT: DWORD = 0x40000000;

// ============================================================================
// Configuration
// ============================================================================

/// RTP Output stream configuration.
///
/// Z/IP ONE connects TO us. We listen on local_port.
#[derive(Clone)]
pub struct RtpOutputConfig {
    /// Local port to listen on (Z/IP ONE connects here)
    pub local_port: u16,
    /// Network interface to bind to (0.0.0.0 = any)
    pub interface_addr: Ipv4Addr,
    /// Sample rate (48000)
    pub sample_rate: u32,
    /// Number of channels (1 or 2)
    pub channels: u16,
    /// Codec for backfeed audio
    pub backfeed_codec: PayloadCodec,
    /// Bitrate for compressed codecs (kbps)
    pub backfeed_bitrate: u32,
    /// Frame duration in milliseconds
    pub frame_duration_ms: u32,
    /// Clock mode (PTP/Livewire/System)
    pub clock_mode: ClockMode,
    /// PTP domain (0-127)
    pub ptp_domain: u8,
    /// Incoming audio buffer mode
    pub buffer_mode: BufferMode,
    /// Connection state callback (optional)
    pub connection_callback: Option<ConnectionCallback>,
    /// User data for callback
    pub callback_user_data: *mut c_void,
}

impl Default for RtpOutputConfig {
    fn default() -> Self {
        Self {
            local_port: 5004,  // Default RTP port
            interface_addr: Ipv4Addr::UNSPECIFIED,
            sample_rate: 48000,
            channels: 2,
            backfeed_codec: PayloadCodec::Pcm16,
            backfeed_bitrate: 256,
            frame_duration_ms: 1,
            clock_mode: ClockMode::System,
            ptp_domain: 0,
            buffer_mode: BufferMode::Simple { buffer_ms: 100 },
            connection_callback: None,
            callback_user_data: std::ptr::null_mut(),
        }
    }
}

/// TX thread configuration (Send-safe, no raw pointers)
#[derive(Clone)]
struct TxConfig {
    sample_rate: u32,
    channels: u16,
    backfeed_codec: PayloadCodec,
    backfeed_bitrate: u32,
    frame_duration_ms: u32,
}

// ============================================================================
// Statistics
// ============================================================================

/// Atomic statistics for lock-free access.
struct AtomicStats {
    // Receive (RX) stats - incoming audio
    rx_packets: AtomicU64,
    rx_bytes: AtomicU64,
    rx_decode_errors: AtomicU64,
    rx_dropped: AtomicU64,
    // Send (TX) stats - backfeed
    tx_packets: AtomicU64,
    tx_bytes: AtomicU64,
    tx_encode_errors: AtomicU64,
    tx_underruns: AtomicU64,
    // Incoming audio buffer level (samples)
    buffer_level: AtomicU32,
    // Detected incoming codec PT
    detected_incoming_pt: AtomicU32,
}

impl AtomicStats {
    fn new() -> Self {
        Self {
            rx_packets: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
            rx_decode_errors: AtomicU64::new(0),
            rx_dropped: AtomicU64::new(0),
            tx_packets: AtomicU64::new(0),
            tx_bytes: AtomicU64::new(0),
            tx_encode_errors: AtomicU64::new(0),
            tx_underruns: AtomicU64::new(0),
            buffer_level: AtomicU32::new(0),
            detected_incoming_pt: AtomicU32::new(0),
        }
    }
}

/// Statistics snapshot for external access.
#[derive(Debug, Default, Clone)]
pub struct RtpOutputStats {
    /// RX packets received (incoming audio)
    pub rx_packets: u64,
    /// RX bytes received
    pub rx_bytes: u64,
    /// RX decode errors
    pub rx_decode_errors: u64,
    /// RX packets dropped (buffer full)
    pub rx_dropped: u64,
    /// TX packets sent (backfeed)
    pub tx_packets: u64,
    /// TX bytes sent
    pub tx_bytes: u64,
    /// TX encode errors
    pub tx_encode_errors: u64,
    /// TX buffer underruns
    pub tx_underruns: u64,
    /// Current incoming buffer level (samples)
    pub buffer_level: u32,
    /// Detected incoming audio payload type
    pub detected_incoming_pt: u8,
    /// Current PPM adjustment
    pub current_ppm: f64,
}

// ============================================================================
// Encoder type for TX (backfeed)
// ============================================================================

/// Encoder type enum for backfeed audio.
enum BackfeedEncoderType {
    None,
    Pcm16(Pcm16Encoder),
    Pcm20(Pcm20Encoder),
    Pcm24(Pcm24Encoder),
    Mp2(twolame::Encoder),
    G711Ulaw(G711UlawEncoder),
    G722(G722Encoder),
}

impl BackfeedEncoderType {
    fn encode(&mut self, pcm: &[f32], output: &mut [u8]) -> Result<usize, String> {
        match self {
            BackfeedEncoderType::None => Err("No encoder".to_string()),
            BackfeedEncoderType::Pcm16(enc) => {
                enc.encode(pcm, output).map_err(|e| format!("{:?}", e))
            }
            BackfeedEncoderType::Pcm20(enc) => {
                enc.encode(pcm, output).map_err(|e| format!("{:?}", e))
            }
            BackfeedEncoderType::Pcm24(enc) => {
                enc.encode(pcm, output).map_err(|e| format!("{:?}", e))
            }
            BackfeedEncoderType::Mp2(enc) => {
                enc.encode_float(pcm, output).map_err(|e| format!("{:?}", e))
            }
            BackfeedEncoderType::G711Ulaw(enc) => {
                enc.encode(pcm, output).map_err(|e| format!("{:?}", e))
            }
            BackfeedEncoderType::G722(enc) => {
                enc.encode(pcm, output).map_err(|e| format!("{:?}", e))
            }
        }
    }

    fn total_samples_per_frame(&self) -> usize {
        match self {
            BackfeedEncoderType::None => 0,
            BackfeedEncoderType::Pcm16(enc) => enc.total_samples_per_frame(),
            BackfeedEncoderType::Pcm20(enc) => enc.total_samples_per_frame(),
            BackfeedEncoderType::Pcm24(enc) => enc.total_samples_per_frame(),
            BackfeedEncoderType::Mp2(enc) => enc.total_samples_per_frame(),
            BackfeedEncoderType::G711Ulaw(enc) => enc.total_samples_per_frame(),
            BackfeedEncoderType::G722(enc) => enc.total_samples_per_frame(),
        }
    }

    fn payload_type(&self) -> u8 {
        match self {
            BackfeedEncoderType::None => 0,
            BackfeedEncoderType::Pcm16(enc) => enc.payload_type(),
            BackfeedEncoderType::Pcm20(enc) => enc.payload_type(),
            BackfeedEncoderType::Pcm24(enc) => enc.payload_type(),
            BackfeedEncoderType::Mp2(_) => 14, // MPEG Audio
            BackfeedEncoderType::G711Ulaw(enc) => enc.payload_type(),
            BackfeedEncoderType::G722(enc) => enc.payload_type(),
        }
    }
}

// ============================================================================
// Decoder type for RX (incoming audio)
// ============================================================================

/// Decoder type for incoming audio.
enum IncomingDecoderType {
    None,
    Pcm16(Pcm16Decoder),
    Pcm20(Pcm20Decoder),
    Pcm24(Pcm24Decoder),
    Mp2(mpg123::Decoder),
    G711Ulaw(G711UlawDecoder),
    G722(G722Decoder),
    Aac(ffmpeg_aac::Decoder),
}

impl IncomingDecoderType {
    fn decode(&mut self, data: &[u8], output: &mut [f32]) -> Result<usize, String> {
        match self {
            IncomingDecoderType::None => Err("No decoder".to_string()),
            IncomingDecoderType::Pcm16(dec) => {
                dec.decode(data, output).map_err(|e| format!("{:?}", e))
            }
            IncomingDecoderType::Pcm20(dec) => {
                dec.decode(data, output).map_err(|e| format!("{:?}", e))
            }
            IncomingDecoderType::Pcm24(dec) => {
                dec.decode(data, output).map_err(|e| format!("{:?}", e))
            }
            IncomingDecoderType::Mp2(dec) => {
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
            IncomingDecoderType::G711Ulaw(dec) => {
                dec.decode(data, output).map_err(|e| format!("{:?}", e))
            }
            IncomingDecoderType::G722(dec) => {
                dec.decode(data, output).map_err(|e| format!("{:?}", e))
            }
            IncomingDecoderType::Aac(dec) => {
                dec.decode(data, output).map_err(|e| format!("{:?}", e))
            }
        }
    }
}

/// Create decoder for payload type.
fn create_decoder_for_pt(pt: u8) -> Option<IncomingDecoderType> {
    match PayloadCodec::from_pt(pt) {
        PayloadCodec::Pcm16 => Some(IncomingDecoderType::Pcm16(Pcm16Decoder::new_auto(2))),
        PayloadCodec::Pcm20 => Some(IncomingDecoderType::Pcm20(Pcm20Decoder::new_auto(2))),
        PayloadCodec::Pcm24 => Some(IncomingDecoderType::Pcm24(Pcm24Decoder::new_auto(2))),
        PayloadCodec::Mp2 => {
            mpg123::Decoder::new().ok().map(IncomingDecoderType::Mp2)
        }
        PayloadCodec::G711Ulaw => Some(IncomingDecoderType::G711Ulaw(G711UlawDecoder::new())),
        PayloadCodec::G722 => Some(IncomingDecoderType::G722(G722Decoder::new())),
        PayloadCodec::Aac => {
            ffmpeg_aac::Decoder::new().ok().map(IncomingDecoderType::Aac)
        }
        PayloadCodec::Unknown(_) => None,
        _ => None,
    }
}

// ============================================================================
// Shared state for remote address (auto-detected)
// ============================================================================

use std::sync::RwLock;

/// Timeout for considering connection lost (in milliseconds)
const CONNECTION_TIMEOUT_MS: u64 = 3000;

/// Shared remote address - auto-detected from first incoming packet.
/// Also tracks last packet time to detect disconnection.
struct SharedRemoteAddr {
    addr: RwLock<Option<SocketAddr>>,
    last_packet_time: AtomicU64,
    /// Connection generation - increments on each new connection
    generation: AtomicU64,
    /// Connection state (0 = disconnected, 1 = connected)
    connected: AtomicBool,
}

impl SharedRemoteAddr {
    fn new() -> Self {
        Self {
            addr: RwLock::new(None),
            last_packet_time: AtomicU64::new(0),
            generation: AtomicU64::new(0),
            connected: AtomicBool::new(false),
        }
    }

    fn get(&self) -> Option<SocketAddr> {
        *self.addr.read().unwrap()
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    /// Set new connection. Returns true if this is a NEW connection (state changed).
    fn set(&self, addr: SocketAddr) -> bool {
        let mut guard = self.addr.write().unwrap();
        *guard = Some(addr);
        // Update last packet time
        self.last_packet_time.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            Ordering::Release,
        );
        // Increment generation for new connection
        self.generation.fetch_add(1, Ordering::Release);
        // Set connected and return if state changed
        !self.connected.swap(true, Ordering::Release)
    }

    fn update_packet_time(&self) {
        self.last_packet_time.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            Ordering::Release,
        );
    }

    /// Clear connection. Returns true if state changed from connected to disconnected.
    fn clear(&self) -> bool {
        let mut guard = self.addr.write().unwrap();
        *guard = None;
        self.connected.swap(false, Ordering::Release)
    }

    fn is_connection_active(&self) -> bool {
        let last = self.last_packet_time.load(Ordering::Acquire);
        if last == 0 {
            return false;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        now - last < CONNECTION_TIMEOUT_MS
    }
}

// ============================================================================
// Main stream struct
// ============================================================================

/// RTP Output stream.
///
/// Z/IP ONE connects TO us. We receive their audio and send backfeed.
pub struct RtpOutput {
    /// Running flag
    running: Arc<AtomicBool>,
    /// Statistics
    stats: Arc<AtomicStats>,
    /// Current PPM (scaled by 1000)
    current_ppm_x1000: Arc<AtomicI64>,
    /// RX thread (receiver - handles both RX and TX after remote is known)
    rx_thread: Option<JoinHandle<()>>,
    /// TX thread (backfeed transmitter)
    tx_thread: Option<JoinHandle<()>>,
    /// Configuration
    pub config: RtpOutputConfig,
    /// BASS backfeed channel (we read audio from this to send as backfeed)
    backfeed_channel: HSTREAM,
    /// Incoming audio ring buffer consumer
    incoming_consumer: Option<ringbuf::HeapCons<f32>>,
    /// Incoming audio BASS handle
    pub incoming_handle: HSTREAM,
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
    /// Shared remote address (auto-detected)
    remote_addr: Arc<SharedRemoteAddr>,
    /// Last seen connection generation (for detecting reconnection)
    last_generation: AtomicU64,
}

impl RtpOutput {
    /// Create a new RTP Output stream.
    ///
    /// # Arguments
    /// * `backfeed_channel` - BASS channel to read audio FROM to send as backfeed
    /// * `config` - Stream configuration
    pub fn new(backfeed_channel: HSTREAM, config: RtpOutputConfig) -> Result<Self, String> {
        init_clock_bindings();

        let channels = config.channels as usize;
        let samples_per_ms = (config.sample_rate / 1000) as usize;

        // Calculate buffer sizes
        let (target_samples, max_samples, buffer_size) = match config.buffer_mode {
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

        // Create ring buffer for incoming audio
        let rb = HeapRb::<f32>::new(buffer_size);
        let (_producer, consumer) = rb.split();

        Ok(Self {
            running: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(AtomicStats::new()),
            current_ppm_x1000: Arc::new(AtomicI64::new(0)),
            rx_thread: None,
            tx_thread: None,
            config,
            backfeed_channel,
            incoming_consumer: Some(consumer),
            incoming_handle: 0,
            target_samples,
            max_samples,
            channels,
            resample_pos: 0.0,
            prev_samples: vec![0.0; channels],
            curr_samples: vec![0.0; channels],
            resample_init: false,
            integral_error: 0.0,
            buffering: AtomicBool::new(true),
            remote_addr: Arc::new(SharedRemoteAddr::new()),
            last_generation: AtomicU64::new(0),
        })
    }

    /// Start the stream (listening for connections).
    pub fn start(&mut self) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("Already running".to_string());
        }

        // Create socket and bind to local port
        let local_addr = SocketAddrV4::new(self.config.interface_addr, self.config.local_port);
        let socket = RtpSocket::bind(SocketAddr::V4(local_addr))?;
        socket.set_read_timeout(Some(Duration::from_millis(100)))
            .map_err(|e| e.to_string())?;

        // Clone socket for TX thread
        let socket_for_tx = socket.try_clone()?;

        self.running.store(true, Ordering::SeqCst);

        // Create ring buffer
        let channels = self.config.channels as usize;
        let samples_per_ms = (self.config.sample_rate / 1000) as usize;
        let (target_samples, max_samples, buffer_size) = match self.config.buffer_mode {
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
        self.incoming_consumer = Some(consumer);
        self.target_samples = target_samples;
        self.max_samples = max_samples;

        // Start RX thread (receives incoming audio, detects remote address)
        let running_rx = self.running.clone();
        let stats_rx = self.stats.clone();
        let remote_addr_rx = self.remote_addr.clone();
        let callback_rx = self.config.connection_callback;
        let callback_user_data_rx = self.config.callback_user_data as usize; // Cast to usize for Send

        self.rx_thread = Some(thread::spawn(move || {
            Self::receiver_loop(running_rx, stats_rx, socket, producer, remote_addr_rx, callback_rx, callback_user_data_rx);
        }));

        // Start TX thread (sends backfeed once remote is known)
        let running_tx = self.running.clone();
        let stats_tx = self.stats.clone();
        let ppm_tx = self.current_ppm_x1000.clone();
        let tx_config = TxConfig {
            sample_rate: self.config.sample_rate,
            channels: self.config.channels,
            backfeed_codec: self.config.backfeed_codec.clone(),
            backfeed_bitrate: self.config.backfeed_bitrate,
            frame_duration_ms: self.config.frame_duration_ms,
        };
        let backfeed = self.backfeed_channel;
        let remote_addr_tx = self.remote_addr.clone();
        let callback_tx = self.config.connection_callback;
        let callback_user_data_tx = self.config.callback_user_data as usize;

        self.tx_thread = Some(thread::spawn(move || {
            Self::transmitter_loop(running_tx, stats_tx, ppm_tx, socket_for_tx, tx_config, backfeed, remote_addr_tx, callback_tx, callback_user_data_tx);
        }));

        Ok(())
    }

    /// Stop the stream.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);

        if let Some(t) = self.rx_thread.take() {
            let _ = t.join();
        }
        if let Some(t) = self.tx_thread.take() {
            let _ = t.join();
        }
    }

    /// Receiver loop - receives RTP, decodes, pushes to ring buffer, auto-detects remote.
    fn receiver_loop(
        running: Arc<AtomicBool>,
        stats: Arc<AtomicStats>,
        socket: RtpSocket,
        mut producer: ringbuf::HeapProd<f32>,
        remote_addr: Arc<SharedRemoteAddr>,
        callback: Option<ConnectionCallback>,
        callback_user_data: usize,
    ) {
        let mut recv_buf = vec![0u8; 4096];
        let mut decode_buf = vec![0.0f32; 8192];
        let mut decoder: IncomingDecoderType = IncomingDecoderType::None;
        let mut current_pt: Option<u8> = None;

        while running.load(Ordering::SeqCst) {
            match socket.recv_from(&mut recv_buf) {
                Ok((len, src_addr)) if len >= 12 => {
                    // Auto-detect remote address from first packet
                    let is_new_connection = remote_addr.get().is_none();
                    if is_new_connection {
                        // New connection - reset decoder
                        decoder = IncomingDecoderType::None;
                        current_pt = None;
                        stats.detected_incoming_pt.store(0, Ordering::Relaxed);

                        // Set address and check if state changed
                        if remote_addr.set(src_addr) {
                            // State changed to connected - fire callback
                            if let Some(cb) = callback {
                                cb(ConnectionState::Connected, callback_user_data as *mut c_void);
                            }
                        }
                    } else {
                        remote_addr.update_packet_time();
                    }

                    if let Some(packet) = RtpPacket::parse(&recv_buf[..len]) {
                        let pt = packet.header.payload_type;

                        // Switch decoder if PT changed
                        if current_pt != Some(pt) {
                            if let Some(new_dec) = create_decoder_for_pt(pt) {
                                decoder = new_dec;
                                current_pt = Some(pt);
                                stats.detected_incoming_pt.store(pt as u32, Ordering::Relaxed);
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
                            Ok(_) => {
                                // Zero output, need more data
                            }
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

    /// Transmitter loop - reads from BASS, encodes, sends backfeed RTP.
    /// Automatically stops sending when connection is lost and resumes when reconnected.
    fn transmitter_loop(
        running: Arc<AtomicBool>,
        stats: Arc<AtomicStats>,
        current_ppm_x1000: Arc<AtomicI64>,
        socket: RtpSocket,
        config: TxConfig,
        backfeed_channel: HSTREAM,
        remote_addr: Arc<SharedRemoteAddr>,
        callback: Option<ConnectionCallback>,
        callback_user_data: usize,
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
        let mut encoder = match config.backfeed_codec {
            PayloadCodec::Pcm16 => BackfeedEncoderType::Pcm16(Pcm16Encoder::new(
                format,
                config.frame_duration_ms as usize,
            )),
            PayloadCodec::Pcm20 => BackfeedEncoderType::Pcm20(Pcm20Encoder::new(
                format,
                config.frame_duration_ms as usize,
            )),
            PayloadCodec::Pcm24 => BackfeedEncoderType::Pcm24(Pcm24Encoder::new(
                format,
                config.frame_duration_ms as usize,
            )),
            PayloadCodec::Mp2 => match twolame::Encoder::new(format, config.backfeed_bitrate) {
                Ok(e) => BackfeedEncoderType::Mp2(e),
                Err(_) => return,
            },
            PayloadCodec::G711Ulaw => BackfeedEncoderType::G711Ulaw(G711UlawEncoder::new()),
            PayloadCodec::G722 => BackfeedEncoderType::G722(G722Encoder::new()),
            _ => {
                eprintln!("Unsupported backfeed codec: {:?}", config.backfeed_codec);
                return;
            }
        };

        let samples_per_frame = encoder.total_samples_per_frame();
        let samples_per_channel = samples_per_frame / config.channels as usize;
        let frame_duration_us = (samples_per_channel as u64 * 1_000_000) / config.sample_rate as u64;

        let mut pcm_buffer = vec![0.0f32; samples_per_frame];
        let encode_buffer_size = match config.backfeed_codec {
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

        // Main loop - handles connection/disconnection cycles
        while running.load(Ordering::SeqCst) {
            // Wait for remote address to be detected (or reconnected)
            while running.load(Ordering::SeqCst) && remote_addr.get().is_none() {
                thread::sleep(Duration::from_millis(10));
            }

            if !running.load(Ordering::SeqCst) {
                break;
            }

            let target_addr = match remote_addr.get() {
                Some(addr) => addr,
                None => continue,
            };

            // Reset timing when (re)connecting
            next_tx = Instant::now();

            // Transmit loop - runs while connection is active
            while running.load(Ordering::SeqCst) && remote_addr.is_connection_active() {
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

                // Read from BASS backfeed channel
                let bytes_needed = (samples_per_frame * 4) as u32;
                let bytes_read = unsafe {
                    BASS_ChannelGetData(
                        backfeed_channel,
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
                        if socket.send_to(packet, target_addr).is_ok() {
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

            // Connection lost - clear remote address so we can detect a new connection
            if running.load(Ordering::SeqCst) {
                if remote_addr.clear() {
                    // State changed to disconnected - fire callback
                    if let Some(cb) = callback {
                        cb(ConnectionState::Disconnected, callback_user_data as *mut c_void);
                    }
                }
            }
        }
    }

    /// Check if running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Check if connected (receiving RTP packets).
    pub fn is_connected(&self) -> bool {
        self.remote_addr.is_connected()
    }

    /// Get statistics snapshot.
    pub fn stats(&self) -> RtpOutputStats {
        RtpOutputStats {
            rx_packets: self.stats.rx_packets.load(Ordering::Relaxed),
            rx_bytes: self.stats.rx_bytes.load(Ordering::Relaxed),
            rx_decode_errors: self.stats.rx_decode_errors.load(Ordering::Relaxed),
            rx_dropped: self.stats.rx_dropped.load(Ordering::Relaxed),
            tx_packets: self.stats.tx_packets.load(Ordering::Relaxed),
            tx_bytes: self.stats.tx_bytes.load(Ordering::Relaxed),
            tx_encode_errors: self.stats.tx_encode_errors.load(Ordering::Relaxed),
            tx_underruns: self.stats.tx_underruns.load(Ordering::Relaxed),
            buffer_level: self.stats.buffer_level.load(Ordering::Relaxed),
            detected_incoming_pt: self.stats.detected_incoming_pt.load(Ordering::Relaxed) as u8,
            current_ppm: self.current_ppm_x1000.load(Ordering::Relaxed) as f64 / 1000.0,
        }
    }

    /// Get current PPM.
    pub fn applied_ppm(&self) -> f64 {
        self.current_ppm_x1000.load(Ordering::Relaxed) as f64 / 1000.0
    }

    /// Read samples from incoming audio buffer (for STREAMPROC callback).
    ///
    /// Uses adaptive resampling to handle clock drift.
    /// Mutes output when disconnected (user notified via callback).
    pub fn read_samples(&mut self, output: &mut [f32]) -> usize {
        let consumer = match &mut self.incoming_consumer {
            Some(c) => c,
            None => {
                output.fill(0.0);
                return output.len();
            }
        };

        // Check if disconnected - just mute
        if !self.remote_addr.is_connected() {
            output.fill(0.0);
            return output.len();
        }

        // Check if a new connection has been established (generation changed)
        let current_gen = self.remote_addr.generation();
        let last_gen = self.last_generation.load(Ordering::Relaxed);
        if current_gen != last_gen {
            // New connection - reset all state
            self.last_generation.store(current_gen, Ordering::Relaxed);
            self.buffering.store(true, Ordering::Relaxed);
            self.resample_pos = 0.0;
            self.resample_init = false;
            self.integral_error = 0.0;
            self.prev_samples.fill(0.0);
            self.curr_samples.fill(0.0);
            // Note: Don't drain buffer here - it races with producer thread
            // The buffering logic will naturally consume stale data while fresh data fills
        }

        let available = consumer.occupied_len();

        // Initial buffering - wait for buffer to fill with margin
        if self.buffering.load(Ordering::Relaxed) {
            // Wait for 150% of target to absorb BASS's initial buffer demand
            let buffering_threshold = self.target_samples + self.target_samples / 2;
            if available < buffering_threshold {
                output.fill(0.0);
                return output.len();
            }
            self.buffering.store(false, Ordering::Relaxed);
        }

        // Calculate fill level and error for PI controller
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
            while self.resample_pos >= 1.0 {
                std::mem::swap(&mut self.prev_samples, &mut self.curr_samples);
                if consumer.occupied_len() >= channels {
                    // Got real samples from buffer
                    for i in 0..channels {
                        self.curr_samples[i] = consumer.try_pop().unwrap_or(0.0);
                    }
                    self.resample_init = true;
                } else {
                    // Buffer underrun - fade to zeros gracefully (no pop)
                    self.curr_samples.fill(0.0);
                }
                self.resample_pos -= 1.0;
            }

            if !self.resample_init {
                // Not enough data yet - output zeros
                for c in 0..channels {
                    output[out_idx * channels + c] = 0.0;
                }
            } else {
                // Linear interpolation
                let t = self.resample_pos as f32;
                for c in 0..channels {
                    let sample = self.prev_samples[c] * (1.0 - t) + self.curr_samples[c] * t;
                    output[out_idx * channels + c] = sample;
                }
            }

            self.resample_pos += ratio;
            out_idx += 1;
        }

        output.len()
    }

    /// Get incoming audio buffer level in milliseconds.
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

impl Drop for RtpOutput {
    fn drop(&mut self) {
        self.stop();
    }
}

unsafe impl Send for RtpOutput {}

// ============================================================================
// BASS STREAMPROC callback for incoming audio
// ============================================================================

/// BASS STREAMPROC callback for RTP output incoming audio streams.
pub unsafe extern "system" fn output_incoming_stream_proc(
    _handle: HSTREAM,
    buffer: *mut c_void,
    length: DWORD,
    user: *mut c_void,
) -> DWORD {
    if user.is_null() {
        return 0;
    }

    let stream = &mut *(user as *mut RtpOutput);

    let samples = length as usize / 4;
    let float_buffer = std::slice::from_raw_parts_mut(buffer as *mut f32, samples);

    let written = stream.read_samples(float_buffer);

    (written * 4) as DWORD
}
