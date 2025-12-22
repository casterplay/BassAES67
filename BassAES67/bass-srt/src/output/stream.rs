//! SRT output stream implementation.
//! Pulls PCM from BASS and sends via SRT.

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::ffi::DWORD;
use crate::protocol::{PacketHeader, HEADER_SIZE, FORMAT_PCM_L16, FORMAT_OPUS, FORMAT_MP2, FORMAT_FLAC};
use crate::srt_bindings::{self, SockaddrIn};

// FFI import for BASS_ChannelGetData
#[link(name = "bass")]
extern "system" {
    fn BASS_ChannelGetData(handle: DWORD, buffer: *mut c_void, length: DWORD) -> DWORD;
}

/// BASS_DATA_FLOAT flag for BASS_ChannelGetData
const BASS_DATA_FLOAT: DWORD = 0x40000000;

use super::encoder::{create_encoder, AudioEncoder};

// Connection states (same as input for consistency)
pub const CONNECTION_STATE_DISCONNECTED: u32 = 0;
pub const CONNECTION_STATE_CONNECTING: u32 = 1;
pub const CONNECTION_STATE_CONNECTED: u32 = 2;
pub const CONNECTION_STATE_RECONNECTING: u32 = 3;

/// SRT connection mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ConnectionMode {
    /// Caller mode: connect to a remote SRT listener
    Caller = 0,
    /// Listener mode: wait for remote callers to connect
    Listener = 1,
}

impl Default for ConnectionMode {
    fn default() -> Self {
        ConnectionMode::Caller
    }
}

/// Output codec selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OutputCodec {
    /// Raw PCM L16 (no encoding)
    Pcm = 0,
    /// OPUS codec (low latency)
    Opus = 1,
    /// MP2 codec (broadcast standard)
    Mp2 = 2,
    /// FLAC codec (lossless)
    Flac = 3,
}

impl Default for OutputCodec {
    fn default() -> Self {
        OutputCodec::Opus
    }
}

impl OutputCodec {
    /// Get the protocol format byte for this codec
    pub fn format_byte(&self) -> u8 {
        match self {
            OutputCodec::Pcm => FORMAT_PCM_L16,
            OutputCodec::Opus => FORMAT_OPUS,
            OutputCodec::Mp2 => FORMAT_MP2,
            OutputCodec::Flac => FORMAT_FLAC,
        }
    }

    /// Get the default frame size in samples per channel
    pub fn default_frame_size(&self, sample_rate: u32) -> usize {
        match self {
            // OPUS: 5ms at 48kHz = 240 samples
            OutputCodec::Opus => (sample_rate as usize * 5) / 1000,
            // MP2/FLAC: 1152 samples (fixed by spec, ~24ms at 48kHz)
            OutputCodec::Mp2 | OutputCodec::Flac => 1152,
            // PCM: 5ms frames
            OutputCodec::Pcm => (sample_rate as usize * 5) / 1000,
        }
    }
}

/// Output stream configuration
#[derive(Clone)]
pub struct SrtOutputConfig {
    // Network settings
    /// Destination host IP address
    pub host: String,
    /// Destination port
    pub port: u16,
    /// Connection mode (Caller or Listener)
    pub mode: ConnectionMode,
    /// SRT latency in milliseconds
    pub latency_ms: u32,
    /// Encryption passphrase (None = no encryption)
    pub passphrase: Option<String>,
    /// SRT stream ID (optional)
    pub stream_id: Option<String>,

    // Audio settings
    /// Number of channels (1 = mono, 2 = stereo)
    pub channels: u16,
    /// Sample rate in Hz
    pub sample_rate: u32,

    // Codec settings
    /// Output codec
    pub codec: OutputCodec,
    /// Bitrate in kbps (for OPUS/MP2)
    pub bitrate_kbps: u32,
    /// FLAC compression level (0-8)
    pub flac_level: u32,
}

impl Default for SrtOutputConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 9000,
            mode: ConnectionMode::Caller,
            latency_ms: 120,
            passphrase: None,
            stream_id: None,
            channels: 2,
            sample_rate: 48000,
            codec: OutputCodec::Opus,
            bitrate_kbps: 192,
            flac_level: 5,
        }
    }
}

/// Atomic output statistics (lock-free)
struct AtomicOutputStats {
    packets_sent: AtomicU64,
    bytes_sent: AtomicU64,
    send_errors: AtomicU64,
    underruns: AtomicU64,
    connection_state: AtomicU32,
}

impl AtomicOutputStats {
    fn new() -> Self {
        Self {
            packets_sent: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            send_errors: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            connection_state: AtomicU32::new(CONNECTION_STATE_DISCONNECTED),
        }
    }

    fn snapshot(&self) -> OutputStats {
        OutputStats {
            packets_sent: self.packets_sent.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            send_errors: self.send_errors.load(Ordering::Relaxed),
            underruns: self.underruns.load(Ordering::Relaxed),
            connection_state: self.connection_state.load(Ordering::Relaxed),
        }
    }
}

/// Output statistics snapshot
#[derive(Debug, Default, Clone)]
pub struct OutputStats {
    pub packets_sent: u64,
    pub bytes_sent: u64,
    pub send_errors: u64,
    pub underruns: u64,
    pub connection_state: u32,
}

// ============================================================================
// Output Connection State Callback (separate from input)
// ============================================================================

/// Callback type for output connection state changes
pub type OutputConnectionStateCallback = extern "C" fn(state: u32, user: *mut c_void);

static OUTPUT_CONNECTION_STATE_CALLBACK: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static OUTPUT_CONNECTION_STATE_USER: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

/// Set the output connection state callback
pub fn set_output_connection_state_callback(callback: OutputConnectionStateCallback, user: *mut c_void) {
    OUTPUT_CONNECTION_STATE_CALLBACK.store(callback as *mut c_void, Ordering::Release);
    OUTPUT_CONNECTION_STATE_USER.store(user, Ordering::Release);
}

/// Clear the output connection state callback
pub fn clear_output_connection_state_callback() {
    OUTPUT_CONNECTION_STATE_CALLBACK.store(ptr::null_mut(), Ordering::Release);
    OUTPUT_CONNECTION_STATE_USER.store(ptr::null_mut(), Ordering::Release);
}

/// Notify the callback of a state change
fn notify_output_connection_state(state: u32) {
    let callback_ptr = OUTPUT_CONNECTION_STATE_CALLBACK.load(Ordering::Acquire);
    let user_ptr = OUTPUT_CONNECTION_STATE_USER.load(Ordering::Acquire);

    if !callback_ptr.is_null() {
        let callback: OutputConnectionStateCallback = unsafe {
            std::mem::transmute(callback_ptr)
        };
        callback(state, user_ptr);
    }
}

// ============================================================================
// SRT Output Stream
// ============================================================================

/// SRT output stream
pub struct SrtOutputStream {
    running: Arc<AtomicBool>,
    config: SrtOutputConfig,
    source_channel: DWORD,
    stats: Arc<AtomicOutputStats>,
    tx_thread: Option<JoinHandle<()>>,
}

impl SrtOutputStream {
    /// Create a new SRT output stream
    ///
    /// # Arguments
    /// * `source_channel` - BASS channel handle to read audio from
    /// * `config` - Output configuration
    pub fn new(source_channel: DWORD, config: SrtOutputConfig) -> Result<Self, String> {
        Ok(Self {
            running: Arc::new(AtomicBool::new(false)),
            config,
            source_channel,
            stats: Arc::new(AtomicOutputStats::new()),
            tx_thread: None,
        })
    }

    /// Start the output stream
    pub fn start(&mut self) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("Output stream already running".to_string());
        }

        self.running.store(true, Ordering::SeqCst);

        // Clone values for the thread
        let running = self.running.clone();
        let stats = self.stats.clone();
        let config = self.config.clone();
        let source_channel = self.source_channel;

        // Spawn transmitter thread
        let handle = thread::Builder::new()
            .name("srt-output".to_string())
            .spawn(move || {
                transmitter_loop(running, stats, config, source_channel);
            })
            .map_err(|e| format!("Failed to spawn transmitter thread: {}", e))?;

        self.tx_thread = Some(handle);
        Ok(())
    }

    /// Stop the output stream
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);

        if let Some(handle) = self.tx_thread.take() {
            let _ = handle.join();
        }
    }

    /// Check if the stream is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Get current statistics
    pub fn stats(&self) -> OutputStats {
        self.stats.snapshot()
    }
}

impl Drop for SrtOutputStream {
    fn drop(&mut self) {
        self.stop();
    }
}

// ============================================================================
// Transmitter Thread
// ============================================================================

/// Main transmitter loop
fn transmitter_loop(
    running: Arc<AtomicBool>,
    stats: Arc<AtomicOutputStats>,
    config: SrtOutputConfig,
    source_channel: DWORD,
) {
    // Set thread priority high
    #[cfg(target_os = "linux")]
    unsafe {
        libc::nice(-20);
    }

    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
        };
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
    }

    // Ignore SIGPIPE on Unix
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }

    // Initialize SRT
    if let Err(e) = srt_bindings::startup() {
        eprintln!("[SRT Output] Failed to initialize SRT: {:?}", e);
        stats.connection_state.store(CONNECTION_STATE_DISCONNECTED, Ordering::SeqCst);
        notify_output_connection_state(CONNECTION_STATE_DISCONNECTED);
        return;
    }

    // Create encoder
    let mut encoder = match create_encoder(&config) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[SRT Output] Failed to create encoder: {}", e);
            srt_bindings::cleanup().ok();
            stats.connection_state.store(CONNECTION_STATE_DISCONNECTED, Ordering::SeqCst);
            notify_output_connection_state(CONNECTION_STATE_DISCONNECTED);
            return;
        }
    };

    // Allocate buffers
    let total_samples_per_frame = encoder.total_samples_per_frame();
    let mut audio_buffer = vec![0.0f32; total_samples_per_frame];
    let mut encoded_buffer = vec![0u8; 4096];
    let mut send_buffer = vec![0u8; 4096 + HEADER_SIZE];
    let bytes_needed = (total_samples_per_frame * 4) as DWORD; // float = 4 bytes

    // Calculate frame interval
    let frame_duration_us = (encoder.frame_size() as u64 * 1_000_000) / config.sample_rate as u64;
    let frame_interval = Duration::from_micros(frame_duration_us);

    // Run appropriate loop based on mode
    match config.mode {
        ConnectionMode::Caller => {
            caller_transmit_loop(
                running.clone(),
                &stats,
                &config,
                source_channel,
                &mut *encoder,
                &mut audio_buffer,
                &mut encoded_buffer,
                &mut send_buffer,
                bytes_needed,
                frame_interval,
            );
        }
        ConnectionMode::Listener => {
            listener_transmit_loop(
                running.clone(),
                &stats,
                &config,
                source_channel,
                &mut *encoder,
                &mut audio_buffer,
                &mut encoded_buffer,
                &mut send_buffer,
                bytes_needed,
                frame_interval,
            );
        }
    }

    srt_bindings::cleanup().ok();
    stats.connection_state.store(CONNECTION_STATE_DISCONNECTED, Ordering::SeqCst);
    notify_output_connection_state(CONNECTION_STATE_DISCONNECTED);
}

/// Create and configure an SRT socket
fn create_configured_socket(config: &SrtOutputConfig) -> Result<srt_bindings::SRTSOCKET, String> {
    let sock = srt_bindings::create_socket()
        .map_err(|e| format!("Failed to create socket: {:?}", e))?;

    // Set transtype to live mode
    srt_bindings::set_transtype(sock, srt_bindings::SrtTranstype::Live)
        .map_err(|e| format!("Failed to set transtype: {:?}", e))?;

    srt_bindings::set_latency(sock, config.latency_ms as i32)
        .map_err(|e| format!("Failed to set latency: {:?}", e))?;

    // Set passphrase if provided
    if let Some(ref passphrase) = config.passphrase {
        srt_bindings::set_passphrase(sock, passphrase)
            .map_err(|e| format!("Failed to set passphrase: {:?}", e))?;
    }

    // Set stream ID if provided
    if let Some(ref stream_id) = config.stream_id {
        srt_bindings::set_streamid(sock, stream_id)
            .map_err(|e| format!("Failed to set stream ID: {:?}", e))?;
    }

    Ok(sock)
}

/// Transmit loop for Caller mode (connect to remote listener)
fn caller_transmit_loop(
    running: Arc<AtomicBool>,
    stats: &Arc<AtomicOutputStats>,
    config: &SrtOutputConfig,
    source_channel: DWORD,
    encoder: &mut dyn AudioEncoder,
    audio_buffer: &mut [f32],
    encoded_buffer: &mut [u8],
    send_buffer: &mut [u8],
    bytes_needed: DWORD,
    frame_interval: Duration,
) {
    while running.load(Ordering::SeqCst) {
        stats.connection_state.store(CONNECTION_STATE_CONNECTING, Ordering::SeqCst);
        notify_output_connection_state(CONNECTION_STATE_CONNECTING);

        // Create socket
        let sock = match create_configured_socket(config) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[SRT Output] {}", e);
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };

        // Parse address and connect
        let parts: Vec<&str> = config.host.split('.').collect();
        if parts.len() != 4 {
            eprintln!("[SRT Output] Invalid host address: {}", config.host);
            srt_bindings::close(sock).ok();
            thread::sleep(Duration::from_secs(1));
            continue;
        }

        let addr = match (
            parts[0].parse::<u8>(),
            parts[1].parse::<u8>(),
            parts[2].parse::<u8>(),
            parts[3].parse::<u8>(),
        ) {
            (Ok(a), Ok(b), Ok(c), Ok(d)) => SockaddrIn::from_parts(a, b, c, d, config.port),
            _ => {
                eprintln!("[SRT Output] Invalid host address: {}", config.host);
                srt_bindings::close(sock).ok();
                thread::sleep(Duration::from_secs(1));
                continue;
            }
        };

        if let Err(e) = srt_bindings::connect(sock, &addr) {
            eprintln!("[SRT Output] Failed to connect to {}:{}: {:?}", config.host, config.port, e);
            srt_bindings::close(sock).ok();
            thread::sleep(Duration::from_secs(1));
            continue;
        }

        stats.connection_state.store(CONNECTION_STATE_CONNECTED, Ordering::SeqCst);
        notify_output_connection_state(CONNECTION_STATE_CONNECTED);
        println!("[SRT Output] Connected to {}:{}", config.host, config.port);

        // Transmit to this socket
        transmit_to_socket(
            sock,
            &running,
            stats,
            config,
            source_channel,
            encoder,
            audio_buffer,
            encoded_buffer,
            send_buffer,
            bytes_needed,
            frame_interval,
        );

        srt_bindings::close(sock).ok();

        if running.load(Ordering::SeqCst) {
            stats.connection_state.store(CONNECTION_STATE_RECONNECTING, Ordering::SeqCst);
            notify_output_connection_state(CONNECTION_STATE_RECONNECTING);
            println!("[SRT Output] Disconnected, reconnecting...");
            thread::sleep(Duration::from_millis(500));
        }
    }
}

/// Transmit loop for Listener mode (wait for remote callers)
fn listener_transmit_loop(
    running: Arc<AtomicBool>,
    stats: &Arc<AtomicOutputStats>,
    config: &SrtOutputConfig,
    source_channel: DWORD,
    encoder: &mut dyn AudioEncoder,
    audio_buffer: &mut [f32],
    encoded_buffer: &mut [u8],
    send_buffer: &mut [u8],
    bytes_needed: DWORD,
    frame_interval: Duration,
) {
    // Create and bind listen socket
    let listen_sock = match create_configured_socket(config) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[SRT Output] {}", e);
            return;
        }
    };

    let bind_addr = SockaddrIn::from_parts(0, 0, 0, 0, config.port);
    if let Err(e) = srt_bindings::bind(listen_sock, &bind_addr) {
        eprintln!("[SRT Output] Failed to bind to port {}: {:?}", config.port, e);
        srt_bindings::close(listen_sock).ok();
        return;
    }

    if let Err(e) = srt_bindings::listen(listen_sock, 1) {
        eprintln!("[SRT Output] Failed to listen: {:?}", e);
        srt_bindings::close(listen_sock).ok();
        return;
    }

    println!("[SRT Output] Listening on port {}", config.port);

    while running.load(Ordering::SeqCst) {
        stats.connection_state.store(CONNECTION_STATE_CONNECTING, Ordering::SeqCst);
        notify_output_connection_state(CONNECTION_STATE_CONNECTING);

        // Accept connection (with timeout for checking running flag)
        match srt_bindings::accept(listen_sock) {
            Ok(client_sock) => {
                stats.connection_state.store(CONNECTION_STATE_CONNECTED, Ordering::SeqCst);
                notify_output_connection_state(CONNECTION_STATE_CONNECTED);
                println!("[SRT Output] Client connected");

                // Transmit to this client
                transmit_to_socket(
                    client_sock,
                    &running,
                    stats,
                    config,
                    source_channel,
                    encoder,
                    audio_buffer,
                    encoded_buffer,
                    send_buffer,
                    bytes_needed,
                    frame_interval,
                );

                srt_bindings::close(client_sock).ok();

                if running.load(Ordering::SeqCst) {
                    stats.connection_state.store(CONNECTION_STATE_RECONNECTING, Ordering::SeqCst);
                    notify_output_connection_state(CONNECTION_STATE_RECONNECTING);
                    println!("[SRT Output] Client disconnected, waiting for new connection...");
                }
            }
            Err(_) => {
                // Accept failed or timed out, check if still running
                thread::sleep(Duration::from_millis(100));
            }
        }
    }

    srt_bindings::close(listen_sock).ok();
}

/// Transmit audio to a connected SRT socket
fn transmit_to_socket(
    sock: srt_bindings::SRTSOCKET,
    running: &Arc<AtomicBool>,
    stats: &Arc<AtomicOutputStats>,
    config: &SrtOutputConfig,
    source_channel: DWORD,
    encoder: &mut dyn AudioEncoder,
    audio_buffer: &mut [f32],
    encoded_buffer: &mut [u8],
    send_buffer: &mut [u8],
    bytes_needed: DWORD,
    frame_interval: Duration,
) {
    let format_byte = config.codec.format_byte();
    let mut next_tx = Instant::now() + frame_interval;

    while running.load(Ordering::SeqCst) {
        // Wait for next packet time with high precision
        let now = Instant::now();
        if next_tx > now {
            let sleep_time = next_tx - now;
            if sleep_time > Duration::from_millis(2) {
                thread::sleep(sleep_time - Duration::from_millis(1));
            }
            // Spin for precise timing
            while Instant::now() < next_tx {
                std::hint::spin_loop();
            }
        }

        // Read audio from BASS
        let bytes_read = unsafe {
            BASS_ChannelGetData(
                source_channel,
                audio_buffer.as_mut_ptr() as *mut c_void,
                bytes_needed | BASS_DATA_FLOAT,
            )
        };

        if bytes_read == 0xFFFFFFFF {
            // Underrun - fill with silence
            audio_buffer.fill(0.0);
            stats.underruns.fetch_add(1, Ordering::Relaxed);
        }

        // Encode audio
        let (encoded_len, _format) = match encoder.encode(audio_buffer, encoded_buffer) {
            Ok((len, fmt)) if len > 0 => (len, fmt),
            Ok(_) => {
                // No output yet (encoder buffering)
                next_tx += frame_interval;
                continue;
            }
            Err(e) => {
                eprintln!("[SRT Output] Encode error: {}", e);
                next_tx += frame_interval;
                continue;
            }
        };

        // Frame the packet with protocol header
        let header = PacketHeader::audio(format_byte, encoded_len as u16);
        send_buffer[..HEADER_SIZE].copy_from_slice(&header.encode());
        send_buffer[HEADER_SIZE..HEADER_SIZE + encoded_len]
            .copy_from_slice(&encoded_buffer[..encoded_len]);

        // Send via SRT
        let total_len = HEADER_SIZE + encoded_len;
        match srt_bindings::send(sock, &send_buffer[..total_len]) {
            Ok(_) => {
                stats.packets_sent.fetch_add(1, Ordering::Relaxed);
                stats.bytes_sent.fetch_add(total_len as u64, Ordering::Relaxed);
            }
            Err(e) => {
                stats.send_errors.fetch_add(1, Ordering::Relaxed);
                eprintln!("[SRT Output] Send error: {:?}", e);
                // Connection likely lost
                break;
            }
        }

        next_tx += frame_interval;

        // If we've fallen behind, catch up
        if Instant::now() > next_tx + frame_interval {
            next_tx = Instant::now() + frame_interval;
        }
    }
}
