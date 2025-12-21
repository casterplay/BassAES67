//! SRT stream implementation with lock-free audio transfer.
//! Manages SRT connection, L16 PCM reception, and BASS integration.
//! Uses a lock-free ring buffer between receiver thread and audio callback.
//!
//! Supports framed protocol with multiple codecs:
//! - PCM L16: Raw 16-bit signed little-endian
//! - OPUS: Low-latency codec
//! - MP2: MPEG Audio Layer 2 (broadcast standard)
//! - JSON: Metadata with callback

use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicPtr, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::ptr;

use ringbuf::{HeapRb, traits::{Producer, Consumer, Split, Observer}};

use super::url::SrtUrl;
use crate::ffi::*;
use crate::ffi::addon::AddonFunctions;
use crate::srt_bindings::{self, SockaddrIn, SrtSockStatus, SrtTranstype};
use crate::protocol::{self, PacketHeader, HEADER_SIZE, TYPE_AUDIO, TYPE_JSON};
use crate::protocol::{FORMAT_PCM_L16, FORMAT_OPUS, FORMAT_MP2};
use crate::codec::{opus, mpg123};

/// Callback type for JSON metadata
/// Called with: json string pointer, length, user data pointer
pub type MetadataCallback = extern "C" fn(json: *const c_char, len: u32, user: *mut c_void);

/// Global metadata callback (set via BASS_SRT_SetMetadataCallback)
static METADATA_CALLBACK: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static METADATA_USER: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

/// Set the metadata callback for JSON packets
pub fn set_metadata_callback(callback: MetadataCallback, user: *mut c_void) {
    METADATA_CALLBACK.store(callback as *mut c_void, Ordering::Release);
    METADATA_USER.store(user, Ordering::Release);
}

/// Clear the metadata callback
pub fn clear_metadata_callback() {
    METADATA_CALLBACK.store(ptr::null_mut(), Ordering::Release);
    METADATA_USER.store(ptr::null_mut(), Ordering::Release);
}

/// Audio decoder state for receiver thread
struct AudioDecoder {
    /// Decoder type
    decoder: DecoderType,
    /// Frames to discard at start (decoder warmup)
    warmup_frames: u32,
}

enum DecoderType {
    /// No decoder - raw PCM L16
    None,
    /// OPUS decoder
    Opus(opus::Decoder),
    /// MP2/MP3 decoder
    Mp2(mpg123::Decoder),
}

impl AudioDecoder {
    fn new() -> Self {
        Self {
            decoder: DecoderType::None,
            warmup_frames: 0,
        }
    }
}

impl AudioDecoder {
    /// Decode audio data based on format, returning total float samples (frames * channels)
    /// Returns 0 during warmup period (first few frames discarded for codec stabilization)
    fn decode(&mut self, format: u8, data: &[u8], output: &mut [f32]) -> Result<usize, String> {
        // Discard warmup frames for codec stabilization
        if self.warmup_frames > 0 {
            // Still decode to advance decoder state, but don't return samples
            match (&mut self.decoder, format) {
                (DecoderType::Opus(decoder), FORMAT_OPUS) => {
                    let _ = decoder.decode_float(data, output, false);
                }
                (DecoderType::Mp2(decoder), FORMAT_MP2) => {
                    let mut i16_buf = vec![0i16; output.len()];
                    let _ = decoder.decode(data, &mut i16_buf);
                }
                _ => {}
            }
            self.warmup_frames -= 1;
            return Ok(0);  // Signal no samples ready yet
        }

        match (&mut self.decoder, format) {
            (_, FORMAT_PCM_L16) => {
                // PCM L16 - convert directly (data is already interleaved samples)
                let sample_count = data.len() / 2;
                let count = sample_count.min(output.len());
                for i in 0..count {
                    let lo = data[i * 2] as i16;
                    let hi = data[i * 2 + 1] as i16;
                    let sample_i16 = lo | (hi << 8);
                    output[i] = sample_i16 as f32 / 32768.0;
                }
                Ok(count)
            }
            (DecoderType::Opus(decoder), FORMAT_OPUS) => {
                // OPUS decode_float returns samples per channel
                // For stereo, multiply by 2 to get total samples
                let samples_per_channel = decoder.decode_float(data, output, false)
                    .map_err(|e| format!("OPUS decode error: {}", e))?;
                // Total samples = samples_per_channel * channels (2 for stereo)
                Ok(samples_per_channel * 2)
            }
            (_, FORMAT_OPUS) => {
                Err("OPUS decoder not initialized".to_string())
            }
            (DecoderType::Mp2(decoder), FORMAT_MP2) => {
                // MP2 decode returns total i16 samples (already interleaved)
                let mut i16_buf = vec![0i16; output.len()];
                let total_samples = decoder.decode(data, &mut i16_buf)
                    .map_err(|e| format!("MP2 decode error: {}", e))?;

                // Convert i16 to float
                for i in 0..total_samples.min(output.len()) {
                    output[i] = i16_buf[i] as f32 / 32768.0;
                }
                Ok(total_samples)
            }
            (_, FORMAT_MP2) => {
                Err("MP2 decoder not initialized".to_string())
            }
            _ => {
                Err(format!("Unknown audio format: 0x{:02x}", format))
            }
        }
    }

    /// Switch decoder type based on format byte
    fn ensure_decoder(&mut self, format: u8) -> Result<(), String> {
        match format {
            FORMAT_PCM_L16 => {
                // No decoder needed for PCM
                Ok(())
            }
            FORMAT_OPUS => {
                if !matches!(self.decoder, DecoderType::Opus(_)) {
                    self.decoder = DecoderType::Opus(
                        opus::Decoder::new_48k_stereo_5ms()
                            .map_err(|e| format!("Failed to create OPUS decoder: {}", e))?
                    );
                    // OPUS needs ~2 frames to stabilize
                    self.warmup_frames = 3;
                }
                Ok(())
            }
            FORMAT_MP2 => {
                if !matches!(self.decoder, DecoderType::Mp2(_)) {
                    self.decoder = DecoderType::Mp2(
                        mpg123::Decoder::new()
                            .map_err(|e| format!("Failed to create MP2 decoder: {}", e))?
                    );
                    // MP2 needs a few frames for sync/stabilization
                    self.warmup_frames = 3;
                }
                Ok(())
            }
            _ => Err(format!("Unknown format: 0x{:02x}", format))
        }
    }
}

// Codec type for reporting (matches BASS_CONFIG_SRT_CODEC values)
pub const CODEC_UNKNOWN: u32 = 0;
pub const CODEC_PCM: u32 = 1;
pub const CODEC_OPUS: u32 = 2;
pub const CODEC_MP2: u32 = 3;

use std::sync::atomic::AtomicU32;

use std::sync::atomic::AtomicBool as AtomicBoolStats;

// Statistics tracked with atomics (no locking needed)
struct StreamStats {
    packets_received: AtomicU64,
    packets_dropped: AtomicU64,
    underruns: AtomicU64,
    bytes_received: AtomicU64,
    detected_codec: AtomicU32,
    detected_bitrate: AtomicU32,  // kbps
    encrypted: AtomicBoolStats,   // true if passphrase was set
    connection_mode: AtomicU32,   // 0=caller, 1=listener, 2=rendezvous
}

impl StreamStats {
    fn new() -> Self {
        Self {
            packets_received: AtomicU64::new(0),
            packets_dropped: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            detected_codec: AtomicU32::new(CODEC_UNKNOWN),
            detected_bitrate: AtomicU32::new(0),
            encrypted: AtomicBoolStats::new(false),
            connection_mode: AtomicU32::new(0),
        }
    }
}

// SRT input stream with lock-free architecture
pub struct SrtStream {
    // Ring buffer consumer (audio callback reads from here)
    consumer: ringbuf::HeapCons<f32>,
    // Flag to stop receiver thread
    running: Arc<AtomicBool>,
    // Stream ended flag (set by receiver on socket error)
    ended: Arc<AtomicBool>,
    // Receiver thread handle
    receiver_thread: Option<JoinHandle<()>>,
    // BASS stream handle (set after creation)
    pub handle: HSTREAM,
    // Stream configuration
    config: SrtUrl,
    // Statistics (lock-free)
    stats: Arc<StreamStats>,
    // Target buffer level in samples
    target_samples: usize,
    // Whether we're in initial buffering phase
    buffering: AtomicBool,
    // Number of channels
    channels: usize,
    // Adaptive resampling: fractional position for interpolation
    resample_pos: f64,
    // Adaptive resampling: previous frame samples (one per channel)
    prev_samples: Vec<f32>,
    // Adaptive resampling: current frame samples (one per channel)
    curr_samples: Vec<f32>,
    // Whether resampling state is initialized
    resample_init: bool,
    // Integral term for PI controller (accumulated error)
    integral_error: f64,
    // BASS stream flags (BASS_STREAM_DECODE, etc.) - stored for get_info
    pub stream_flags: DWORD,
}

impl SrtStream {
    // Create a new SRT stream from URL parameters.
    pub fn new(config: SrtUrl) -> Result<Self, String> {
        let channels = config.channels as usize;

        // Calculate buffer size based on latency setting
        // Use 3x target for headroom
        let target_samples = config.target_buffer_samples();
        let buffer_size = target_samples * 3;

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
            stats: Arc::new(StreamStats::new()),
            target_samples,
            buffering: AtomicBool::new(true),
            channels,
            resample_pos: 0.0,
            prev_samples: vec![0.0; channels],
            curr_samples: vec![0.0; channels],
            resample_init: false,
            integral_error: 0.0,
            stream_flags: 0,
        })
    }

    // Start the stream - creates SRT socket and begins receiving packets.
    pub fn start(&mut self) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("Stream already running".to_string());
        }

        // Create a new ring buffer and swap out consumer
        let target_samples = self.config.target_buffer_samples();
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
        let config = self.config.clone();

        self.receiver_thread = Some(thread::spawn(move || {
            Self::receiver_loop(running, ended, stats, producer, config);
        }));

        Ok(())
    }

    // Receiver thread loop - connects/listens for SRT and pushes samples to ring buffer.
    // Supports both framed protocol (with header) and legacy unframed PCM.
    // Handles caller, listener (with reconnect loop), and rendezvous modes.
    fn receiver_loop(
        running: Arc<AtomicBool>,
        ended: Arc<AtomicBool>,
        stats: Arc<StreamStats>,
        mut producer: ringbuf::HeapProd<f32>,
        config: SrtUrl,
    ) {
        use super::url::ConnectionMode;

        // Initialize SRT library
        if srt_bindings::startup().is_err() {
            ended.store(true, Ordering::SeqCst);
            return;
        }

        // Store connection mode for stats
        stats.connection_mode.store(config.mode.as_u32(), Ordering::Relaxed);

        // Parse address once
        let ip = match config.host.parse::<std::net::Ipv4Addr>() {
            Ok(ip) => ip,
            Err(_) => {
                srt_bindings::cleanup().ok();
                ended.store(true, Ordering::SeqCst);
                return;
            }
        };
        let octets = ip.octets();
        let addr = SockaddrIn::from_parts(octets[0], octets[1], octets[2], octets[3], config.port);

        // Helper to configure a socket with common options
        let configure_socket = |sock: srt_bindings::SRTSOCKET| -> Result<(), ()> {
            // Live mode
            if srt_bindings::set_transtype(sock, SrtTranstype::Live).is_err() {
                return Err(());
            }
            // Latency
            if srt_bindings::set_latency(sock, config.latency_ms as i32).is_err() {
                return Err(());
            }
            // Passphrase (encryption)
            if let Some(ref passphrase) = config.passphrase {
                if srt_bindings::set_passphrase(sock, passphrase).is_err() {
                    return Err(());
                }
                stats.encrypted.store(true, Ordering::Relaxed);
            }
            // Stream ID
            if let Some(ref stream_id) = config.stream_id {
                if srt_bindings::set_streamid(sock, stream_id).is_err() {
                    return Err(());
                }
            }
            // Receive buffer
            if config.rcvbuf > 0 {
                let _ = srt_bindings::set_rcvbuf(sock, config.rcvbuf as i32);
            }
            // Send buffer
            if config.sndbuf > 0 {
                let _ = srt_bindings::set_sndbuf(sock, config.sndbuf as i32);
            }
            Ok(())
        };

        match config.mode {
            ConnectionMode::Caller => {
                // Caller mode: connect to remote listener
                let sock = match srt_bindings::create_socket() {
                    Ok(s) => s,
                    Err(_) => {
                        srt_bindings::cleanup().ok();
                        ended.store(true, Ordering::SeqCst);
                        return;
                    }
                };

                if configure_socket(sock).is_err() {
                    srt_bindings::close(sock).ok();
                    srt_bindings::cleanup().ok();
                    ended.store(true, Ordering::SeqCst);
                    return;
                }

                if srt_bindings::connect(sock, &addr).is_err() {
                    srt_bindings::close(sock).ok();
                    srt_bindings::cleanup().ok();
                    ended.store(true, Ordering::SeqCst);
                    return;
                }

                Self::receive_from_socket(sock, &running, &ended, &stats, &mut producer, &config);
                srt_bindings::close(sock).ok();
            }

            ConnectionMode::Listener => {
                // Listener mode: accept connections in a loop
                let listen_sock = match srt_bindings::create_socket() {
                    Ok(s) => s,
                    Err(_) => {
                        srt_bindings::cleanup().ok();
                        ended.store(true, Ordering::SeqCst);
                        return;
                    }
                };

                if configure_socket(listen_sock).is_err() {
                    srt_bindings::close(listen_sock).ok();
                    srt_bindings::cleanup().ok();
                    ended.store(true, Ordering::SeqCst);
                    return;
                }

                if srt_bindings::bind(listen_sock, &addr).is_err() {
                    srt_bindings::close(listen_sock).ok();
                    srt_bindings::cleanup().ok();
                    ended.store(true, Ordering::SeqCst);
                    return;
                }

                if srt_bindings::listen(listen_sock, 1).is_err() {
                    srt_bindings::close(listen_sock).ok();
                    srt_bindings::cleanup().ok();
                    ended.store(true, Ordering::SeqCst);
                    return;
                }

                // Accept loop - reconnect when client disconnects
                while running.load(Ordering::SeqCst) {
                    match srt_bindings::accept(listen_sock) {
                        Ok(client_sock) => {
                            // Receive from this client until disconnect
                            Self::receive_from_socket(client_sock, &running, &ended, &stats, &mut producer, &config);
                            srt_bindings::close(client_sock).ok();
                            // Continue to accept next client
                        }
                        Err(_) => {
                            // Accept failed - check if we should keep running
                            if !running.load(Ordering::SeqCst) {
                                break;
                            }
                            // Small delay before retry
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                    }
                }

                srt_bindings::close(listen_sock).ok();
            }

            ConnectionMode::Rendezvous => {
                // Rendezvous mode: both sides connect simultaneously
                let sock = match srt_bindings::create_socket() {
                    Ok(s) => s,
                    Err(_) => {
                        srt_bindings::cleanup().ok();
                        ended.store(true, Ordering::SeqCst);
                        return;
                    }
                };

                if configure_socket(sock).is_err() {
                    srt_bindings::close(sock).ok();
                    srt_bindings::cleanup().ok();
                    ended.store(true, Ordering::SeqCst);
                    return;
                }

                // Enable rendezvous mode
                if srt_bindings::set_rendezvous(sock, true).is_err() {
                    srt_bindings::close(sock).ok();
                    srt_bindings::cleanup().ok();
                    ended.store(true, Ordering::SeqCst);
                    return;
                }

                // In rendezvous, we bind to local address then connect
                // Using port 0 for local to let OS assign
                let local_addr = SockaddrIn::from_parts(0, 0, 0, 0, config.port);
                if srt_bindings::bind(sock, &local_addr).is_err() {
                    srt_bindings::close(sock).ok();
                    srt_bindings::cleanup().ok();
                    ended.store(true, Ordering::SeqCst);
                    return;
                }

                if srt_bindings::connect(sock, &addr).is_err() {
                    srt_bindings::close(sock).ok();
                    srt_bindings::cleanup().ok();
                    ended.store(true, Ordering::SeqCst);
                    return;
                }

                Self::receive_from_socket(sock, &running, &ended, &stats, &mut producer, &config);
                srt_bindings::close(sock).ok();
            }
        }

        srt_bindings::cleanup().ok();
        ended.store(true, Ordering::SeqCst);
    }

    // Helper: receive data from a connected socket until disconnection
    fn receive_from_socket(
        sock: srt_bindings::SRTSOCKET,
        running: &Arc<AtomicBool>,
        _ended: &Arc<AtomicBool>,
        stats: &Arc<StreamStats>,
        producer: &mut ringbuf::HeapProd<f32>,
        config: &SrtUrl,
    ) {
        let bytes_per_packet = config.bytes_per_packet();

        // Receive buffer - max packet size
        let mut recv_buf = vec![0u8; bytes_per_packet.max(8192)];
        // Float sample buffer (large enough for decoded audio)
        // MP2 frames are 1152 samples/ch * 2 ch = 2304 samples, but decoder may buffer
        // multiple frames, so use larger buffer
        let mut sample_buf = vec![0.0f32; 16384];

        // Audio decoder (initialized on first framed packet)
        let mut decoder = AudioDecoder::new();

        // Track whether we're receiving framed or unframed data
        let mut framed_mode: Option<bool> = None;

        while running.load(Ordering::SeqCst) {
            // Check socket state
            let state = srt_bindings::get_sock_state(sock);
            if state != SrtSockStatus::Connected {
                break;
            }

            // Receive data
            match srt_bindings::recv(sock, &mut recv_buf) {
                Ok(len) if len > 0 => {
                    stats.packets_received.fetch_add(1, Ordering::Relaxed);
                    stats.bytes_received.fetch_add(len as u64, Ordering::Relaxed);

                    let data = &recv_buf[..len];

                    // Determine if this is framed or unframed data
                    // Framed packets start with Type byte (0x01 or 0x02)
                    // Unframed L16 PCM has arbitrary first bytes
                    let is_framed = if framed_mode.is_none() {
                        // First packet - check if it looks like a framed header
                        len >= HEADER_SIZE && (data[0] == TYPE_AUDIO || data[0] == TYPE_JSON)
                    } else {
                        framed_mode.unwrap()
                    };

                    if is_framed {
                        framed_mode = Some(true);

                        // Parse framed packet
                        if let Some(header) = PacketHeader::decode(data) {
                            let payload_start = HEADER_SIZE;
                            let payload_end = payload_start + header.length as usize;

                            if payload_end <= len {
                                let payload = &data[payload_start..payload_end];

                                match header.ptype {
                                    TYPE_AUDIO => {
                                        // Update detected codec
                                        let codec = match header.format {
                                            FORMAT_PCM_L16 => CODEC_PCM,
                                            FORMAT_OPUS => CODEC_OPUS,
                                            FORMAT_MP2 => CODEC_MP2,
                                            _ => CODEC_UNKNOWN,
                                        };
                                        stats.detected_codec.store(codec, Ordering::Relaxed);

                                        // Calculate bitrate for encoded codecs
                                        // Bitrate = (payload_bytes * 8) / frame_duration_seconds
                                        let bitrate_kbps = match header.format {
                                            FORMAT_PCM_L16 => 0,  // PCM doesn't report bitrate
                                            FORMAT_OPUS => {
                                                // OPUS: 5ms frames (240 samples at 48kHz)
                                                let frame_duration_ms = 5.0;
                                                let bits = header.length as f32 * 8.0;
                                                (bits / frame_duration_ms) as u32  // kbps
                                            }
                                            FORMAT_MP2 => {
                                                // MP2: 1152 samples at 48kHz = 24ms
                                                let frame_duration_ms = 24.0;
                                                let bits = header.length as f32 * 8.0;
                                                (bits / frame_duration_ms) as u32  // kbps
                                            }
                                            _ => 0,
                                        };
                                        stats.detected_bitrate.store(bitrate_kbps, Ordering::Relaxed);

                                        // Ensure decoder matches format
                                        if decoder.ensure_decoder(header.format).is_ok() {
                                            // Decode audio
                                            match decoder.decode(header.format, payload, &mut sample_buf) {
                                                Ok(samples) if samples > 0 => {
                                                    // Push to ring buffer
                                                    if producer.vacant_len() >= samples {
                                                        producer.push_slice(&sample_buf[..samples]);
                                                    } else {
                                                        stats.packets_dropped.fetch_add(1, Ordering::Relaxed);
                                                    }
                                                }
                                                Ok(_) => {
                                                    // No samples decoded (buffering)
                                                }
                                                Err(_) => {
                                                    stats.packets_dropped.fetch_add(1, Ordering::Relaxed);
                                                }
                                            }
                                        }
                                    }
                                    TYPE_JSON => {
                                        // Call metadata callback if set
                                        let callback_ptr = METADATA_CALLBACK.load(Ordering::Acquire);
                                        let user_ptr = METADATA_USER.load(Ordering::Acquire);

                                        if !callback_ptr.is_null() {
                                            let callback: MetadataCallback = unsafe {
                                                std::mem::transmute(callback_ptr)
                                            };
                                            callback(
                                                payload.as_ptr() as *const c_char,
                                                payload.len() as u32,
                                                user_ptr,
                                            );
                                        }
                                    }
                                    _ => {
                                        // Unknown packet type - ignore
                                    }
                                }
                            }
                        }
                    } else {
                        // Unframed mode - legacy raw PCM L16
                        framed_mode = Some(false);
                        stats.detected_codec.store(CODEC_PCM, Ordering::Relaxed);

                        let sample_count = len / 2;
                        let total_samples = sample_count.min(sample_buf.len());

                        convert_l16_to_float(data, &mut sample_buf[..total_samples]);

                        if producer.vacant_len() >= total_samples {
                            producer.push_slice(&sample_buf[..total_samples]);
                        } else {
                            stats.packets_dropped.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                Ok(_) => {
                    // Zero bytes received - connection may be closing
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(_) => {
                    // Error receiving
                    break;
                }
            }
        }
        // Socket cleanup is done by the caller
    }

    // Stop the stream.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);

        if let Some(thread) = self.receiver_thread.take() {
            let _ = thread.join();
        }
    }

    // Load one frame (all channels) from ring buffer into curr_samples.
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

    // Get samples from ring buffer with adaptive resampling.
    // Uses buffer level feedback to adjust consumption rate.
    pub fn read_samples(&mut self, buffer: &mut [f32]) -> usize {
        let available = self.consumer.occupied_len();
        let is_buffering = self.buffering.load(Ordering::Relaxed);

        // Buffer protection thresholds
        let critical_threshold = self.target_samples / 10;  // 10% - enter recovery
        let recovery_threshold = (self.target_samples * 3) / 4;  // 75% - exit recovery/initial buffering

        // Buffering/recovery mode - wait until we have enough samples
        if is_buffering {
            if available >= recovery_threshold {
                self.buffering.store(false, Ordering::Relaxed);
                // Reset resampling state on exit from buffering for clean start
                self.resample_init = false;
                self.integral_error = 0.0;
                // Return silence this call, start playing on next call with fresh state
                buffer.fill(0.0);
                return buffer.len();
            } else {
                buffer.fill(0.0);
                return buffer.len();
            }
        }

        // If buffer falls critically low, enter recovery mode
        if available < critical_threshold {
            self.buffering.store(true, Ordering::Relaxed);
            buffer.fill(0.0);
            self.stats.underruns.fetch_add(1, Ordering::Relaxed);
            return buffer.len();
        }

        // PI controller to match consumption rate to arrival rate
        let target = self.target_samples as f64;
        let error = (available as f64 - target) / target;

        // PI controller gains
        const KP: f64 = 0.0001;
        const KI: f64 = 0.00005;
        const MAX_TRIM_PPM: f64 = 50.0;  // SRT may have more variance than AES67

        self.integral_error += error;
        let max_integral = MAX_TRIM_PPM / KI / 1e6;
        self.integral_error = self.integral_error.clamp(-max_integral, max_integral);

        let trim = KP * error + KI * self.integral_error;
        let trim_clamped = trim.clamp(-MAX_TRIM_PPM / 1e6, MAX_TRIM_PPM / 1e6);

        // No clock feedforward for SRT - just use buffer level feedback
        let resample_ratio = 1.0 + trim_clamped;

        // Initialize resampling state if needed
        if !self.resample_init {
            if self.load_next_frame() && self.load_next_frame() {
                self.resample_init = true;
                self.resample_pos = 0.0;
            } else {
                buffer.fill(0.0);
                return buffer.len();
            }
        }

        // Process samples with linear interpolation
        let frames_requested = buffer.len() / self.channels;
        let mut out_idx = 0;

        for _ in 0..frames_requested {
            // Linear interpolation between prev and curr
            let t = self.resample_pos as f32;
            for ch in 0..self.channels {
                let prev = self.prev_samples[ch];
                let curr = self.curr_samples[ch];
                buffer[out_idx + ch] = prev + (curr - prev) * t;
            }
            out_idx += self.channels;

            // Advance position by resample ratio
            self.resample_pos += resample_ratio;

            // Load new frames as needed
            while self.resample_pos >= 1.0 {
                self.resample_pos -= 1.0;
                if !self.load_next_frame() {
                    // Underrun - fill rest with silence
                    for i in out_idx..buffer.len() {
                        buffer[i] = 0.0;
                    }
                    self.stats.underruns.fetch_add(1, Ordering::Relaxed);
                    return buffer.len();
                }
            }
        }

        buffer.len()
    }

    // Check if stream has ended
    pub fn is_ended(&self) -> bool {
        self.ended.load(Ordering::SeqCst)
    }

    // Get buffer fill percentage (0-200, 100 = target)
    pub fn buffer_fill_percent(&self) -> u32 {
        let available = self.consumer.occupied_len();
        if self.target_samples == 0 {
            return 0;
        }
        ((available * 100) / self.target_samples) as u32
    }

    // Get statistics
    pub fn packets_received(&self) -> u64 {
        self.stats.packets_received.load(Ordering::Relaxed)
    }

    pub fn packets_dropped(&self) -> u64 {
        self.stats.packets_dropped.load(Ordering::Relaxed)
    }

    pub fn underruns(&self) -> u64 {
        self.stats.underruns.load(Ordering::Relaxed)
    }

    pub fn detected_codec(&self) -> u32 {
        self.stats.detected_codec.load(Ordering::Relaxed)
    }

    pub fn detected_bitrate(&self) -> u32 {
        self.stats.detected_bitrate.load(Ordering::Relaxed)
    }

    pub fn is_encrypted(&self) -> bool {
        self.stats.encrypted.load(Ordering::Relaxed)
    }

    pub fn connection_mode(&self) -> u32 {
        self.stats.connection_mode.load(Ordering::Relaxed)
    }
}

impl Drop for SrtStream {
    fn drop(&mut self) {
        self.stop();
    }
}

// Convert L16 (16-bit signed little-endian) to float
fn convert_l16_to_float(input: &[u8], output: &mut [f32]) {
    let samples = input.len() / 2;
    for i in 0..samples.min(output.len()) {
        let lo = input[i * 2] as i16;
        let hi = input[i * 2 + 1] as i16;
        let sample_i16 = lo | (hi << 8);
        output[i] = sample_i16 as f32 / 32768.0;
    }
}

// BASS STREAMPROC callback - called by BASS to get audio samples
pub unsafe extern "system" fn stream_proc(
    _handle: HSTREAM,
    buffer: *mut c_void,
    length: DWORD,
    user: *mut c_void,
) -> DWORD {
    if user.is_null() {
        return 0;
    }

    let stream = &mut *(user as *mut SrtStream);
    let samples = length as usize / 4;  // 4 bytes per float
    let float_buffer = std::slice::from_raw_parts_mut(buffer as *mut f32, samples);

    let written = stream.read_samples(float_buffer);

    if stream.is_ended() {
        (written * 4) as DWORD | BASS_STREAMPROC_END
    } else {
        (written * 4) as DWORD
    }
}

// Add-on free callback
unsafe extern "system" fn addon_free(inst: *mut c_void) {
    if !inst.is_null() {
        let stream = Box::from_raw(inst as *mut SrtStream);
        drop(stream);
    }
}

// Add-on get_info callback
unsafe extern "system" fn addon_get_info(inst: *mut c_void, info: *mut BassChannelInfo) {
    if inst.is_null() || info.is_null() {
        return;
    }

    let stream = &*(inst as *const SrtStream);
    let info = &mut *info;

    info.freq = stream.config.sample_rate;
    info.chans = stream.config.channels as DWORD;
    info.flags = BASS_SAMPLE_FLOAT | stream.stream_flags;
    info.ctype = BASS_CTYPE_STREAM_SRT;
    info.origres = 16;  // L16 = 16-bit resolution
    info.plugin = 0;
    info.sample = 0;
    info.filename = ptr::null();
}

// Add-on functions table for BASS
pub static ADDON_FUNCS: AddonFunctions = AddonFunctions {
    flags: 0,
    free: Some(addon_free),
    get_length: None,
    get_tags: None,
    get_file_position: None,
    get_info: Some(addon_get_info),
    can_set_position: None,
    set_position: None,
    get_position: None,
    set_sync: None,
    remove_sync: None,
    can_resume: None,
    set_flags: None,
    attribute: None,
    attribute_ex: None,
};

// Global stream reference for config queries (atomic, no mutex)
static ACTIVE_STREAM: std::sync::atomic::AtomicPtr<SrtStream> =
    std::sync::atomic::AtomicPtr::new(ptr::null_mut());

// Set the active stream for config queries
pub fn set_active_stream(stream: *mut SrtStream) {
    ACTIVE_STREAM.store(stream, Ordering::Release);
}

// Get the active stream for config queries
pub fn get_active_stream() -> *mut SrtStream {
    ACTIVE_STREAM.load(Ordering::Acquire)
}
