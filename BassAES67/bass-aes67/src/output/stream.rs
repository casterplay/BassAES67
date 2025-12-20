//! AES67 output stream implementation.
//! Extracts PCM from a BASS channel and transmits via RTP over UDP multicast.
//!
//! Single-thread design: transmitter thread reads from BASS and sends packets
//! at precise PTP-synchronized intervals. No Mutex in the audio path.

use std::ffi::c_void;
use std::net::{UdpSocket, Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicI64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use socket2::{Socket, Domain, Type, Protocol, SockAddr};

use super::rtp::RtpPacketBuilder;
use crate::ffi::DWORD;
use crate::clock_bindings::{init_clock_bindings, clock_get_frequency_ppm};

// FFI import for BASS_ChannelGetData
#[link(name = "bass")]
extern "system" {
    fn BASS_ChannelGetData(handle: DWORD, buffer: *mut c_void, length: DWORD) -> DWORD;
}

/// BASS_DATA_FLOAT flag for BASS_ChannelGetData
const BASS_DATA_FLOAT: DWORD = 0x40000000;

/// Configuration for AES67 output stream
#[derive(Clone)]
pub struct Aes67OutputConfig {
    /// Multicast destination address
    pub multicast_addr: Ipv4Addr,
    /// UDP port
    pub port: u16,
    /// Network interface to send from (None = default)
    pub interface: Option<Ipv4Addr>,
    /// RTP payload type (typically 96 for L24/48000)
    pub payload_type: u8,
    /// Number of audio channels
    pub channels: u16,
    /// Sample rate in Hz
    pub sample_rate: u32,
    /// Packet time in microseconds (250, 1000, or 5000)
    pub packet_time_us: u32,
}

impl Default for Aes67OutputConfig {
    fn default() -> Self {
        Self {
            multicast_addr: Ipv4Addr::new(239, 192, 76, 52),
            port: 5004,
            interface: None,
            payload_type: 96,
            channels: 2,
            sample_rate: 48000,
            packet_time_us: 1000, // 1ms default (AES67 standard)
        }
    }
}

/// Statistics for the output stream (atomic for lock-free access)
struct AtomicStats {
    packets_sent: AtomicU64,
    samples_sent: AtomicU64,
    send_errors: AtomicU64,
    underruns: AtomicU64,
}

impl AtomicStats {
    fn new() -> Self {
        Self {
            packets_sent: AtomicU64::new(0),
            samples_sent: AtomicU64::new(0),
            send_errors: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
        }
    }
}

/// Statistics snapshot for external access
#[derive(Debug, Default, Clone)]
pub struct OutputStats {
    /// Total packets transmitted
    pub packets_sent: u64,
    /// Total samples transmitted
    pub samples_sent: u64,
    /// Transmission errors
    pub send_errors: u64,
    /// Buffer underruns (not enough samples from source)
    pub underruns: u64,
}

/// AES67 output stream - lock-free design
/// Reads PCM from a BASS channel and transmits via RTP multicast
pub struct Aes67OutputStream {
    /// Running flag (shared with thread)
    running: Arc<AtomicBool>,
    /// Statistics (atomic for lock-free access)
    stats: Arc<AtomicStats>,
    /// Current applied PPM adjustment (scaled by 1000 for precision)
    current_ppm_x1000: Arc<AtomicI64>,
    /// Transmitter thread handle
    tx_thread: Option<JoinHandle<()>>,
    /// Configuration (saved for reference)
    config: Aes67OutputConfig,
    /// BASS channel handle
    source_channel: DWORD,
    /// Samples per packet
    samples_per_packet: usize,
}

impl Aes67OutputStream {
    /// Create a new AES67 output stream.
    pub fn new(source_channel: DWORD, config: Aes67OutputConfig) -> Result<Self, String> {
        // Initialize clock bindings for frequency adjustment
        init_clock_bindings();

        // Calculate samples per packet based on packet time
        let samples_per_packet = (config.sample_rate as u64 * config.packet_time_us as u64 / 1_000_000) as usize;
        if samples_per_packet == 0 {
            return Err("Invalid packet time configuration".to_string());
        }

        Ok(Self {
            running: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(AtomicStats::new()),
            current_ppm_x1000: Arc::new(AtomicI64::new(0)),
            tx_thread: None,
            config,
            source_channel,
            samples_per_packet,
        })
    }

    /// Create and configure the multicast UDP socket
    fn create_multicast_socket(config: &Aes67OutputConfig) -> Result<UdpSocket, String> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
            .map_err(|e| format!("Failed to create socket: {}", e))?;

        let bind_addr = config.interface.unwrap_or(Ipv4Addr::UNSPECIFIED);
        let socket_addr = SocketAddrV4::new(bind_addr, 0);
        socket
            .bind(&SockAddr::from(socket_addr))
            .map_err(|e| format!("Failed to bind socket: {}", e))?;

        socket
            .set_multicast_ttl_v4(8)
            .map_err(|e| format!("Failed to set multicast TTL: {}", e))?;

        if let Some(iface) = config.interface {
            socket
                .set_multicast_if_v4(&iface)
                .map_err(|e| format!("Failed to set multicast interface: {}", e))?;
        }

        socket
            .set_nonblocking(true)
            .map_err(|e| format!("Failed to set non-blocking: {}", e))?;

        Ok(socket.into())
    }

    /// Generate a random SSRC
    fn generate_ssrc() -> u32 {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let seed = now.as_nanos() as u32;
        let mut x = seed ^ 0xDEADBEEF;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x
    }

    /// Start the output stream.
    pub fn start(&mut self) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("Stream already running".to_string());
        }

        // Create socket
        let socket = Self::create_multicast_socket(&self.config)?;
        let dest_addr = SocketAddrV4::new(self.config.multicast_addr, self.config.port);

        self.running.store(true, Ordering::SeqCst);

        // Clone shared state for thread
        let running = self.running.clone();
        let stats = self.stats.clone();
        let current_ppm_x1000 = self.current_ppm_x1000.clone();
        let source_channel = self.source_channel;
        let samples_per_packet = self.samples_per_packet;
        let channels = self.config.channels;
        let interval_us = self.config.packet_time_us as u64;
        let payload_type = self.config.payload_type;

        // Spawn transmitter thread
        let tx = thread::spawn(move || {
            Self::transmitter_loop(
                running,
                stats,
                current_ppm_x1000,
                socket,
                dest_addr,
                source_channel,
                samples_per_packet,
                channels,
                interval_us,
                payload_type,
            );
        });

        self.tx_thread = Some(tx);
        Ok(())
    }

    /// Stop the output stream
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);

        if let Some(thread) = self.tx_thread.take() {
            let _ = thread.join();
        }
    }

    /// Transmitter thread - reads from BASS and sends packets at precise intervals
    fn transmitter_loop(
        running: Arc<AtomicBool>,
        stats: Arc<AtomicStats>,
        current_ppm_x1000: Arc<AtomicI64>,
        socket: UdpSocket,
        dest_addr: SocketAddrV4,
        source_channel: DWORD,
        samples_per_packet: usize,
        channels: u16,
        interval_us: u64,
        payload_type: u8,
    ) {
        // Set thread priority high for better timing (Windows)
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Threading::{
                GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
            };
            unsafe {
                SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
            }
        }

        let ssrc = Self::generate_ssrc();
        let mut rtp = RtpPacketBuilder::new(ssrc, payload_type);
        let buffer_size = samples_per_packet * channels as usize;
        let mut audio_buffer = vec![0.0f32; buffer_size];
        let bytes_needed = (buffer_size * 4) as DWORD;

        let base_interval_us = interval_us as f64;
        let mut next_tx = Instant::now() + Duration::from_micros(interval_us);
        let mut ppm_update_counter = 0u32;
        let mut current_ppm = 0.0f64;


        while running.load(Ordering::SeqCst) {
            // Update PPM every 100 packets to avoid overhead
            ppm_update_counter += 1;
            if ppm_update_counter >= 100 {
                ppm_update_counter = 0;
                current_ppm = clock_get_frequency_ppm();
                current_ppm_x1000.store((current_ppm * 1000.0) as i64, Ordering::Relaxed);
            }

            // Apply PTP frequency correction to send at PTP-synchronized rate
            let interval_factor = 1.0 - (current_ppm / 1_000_000.0);
            let adjusted_interval_us = (base_interval_us * interval_factor) as u64;
            let interval = Duration::from_micros(adjusted_interval_us);

            // Wait until next packet time
            let now = Instant::now();
            if next_tx > now {
                let sleep_time = next_tx - now;
                if sleep_time > Duration::from_millis(2) {
                    thread::sleep(sleep_time - Duration::from_millis(1));
                }
                while Instant::now() < next_tx {
                    std::hint::spin_loop();
                }
            }

            let target_time = next_tx;

            // Read samples directly from BASS (no mutex, no intermediate buffer)
            let bytes_read = unsafe {
                BASS_ChannelGetData(
                    source_channel,
                    audio_buffer.as_mut_ptr() as *mut c_void,
                    bytes_needed | BASS_DATA_FLOAT,
                )
            };

            if bytes_read == 0xFFFFFFFF {
                // Error or end of stream - send silence
                audio_buffer.fill(0.0);
                stats.underruns.fetch_add(1, Ordering::Relaxed);
            } else {
                let samples_read = bytes_read as usize / 4;
                if samples_read < buffer_size {
                    // Partial read - fill rest with silence
                    for i in samples_read..buffer_size {
                        audio_buffer[i] = 0.0;
                    }
                    if samples_read == 0 {
                        stats.underruns.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }

            // Build and send packet
            let packet = rtp.build_packet(&audio_buffer, channels);
            match socket.send_to(packet, dest_addr) {
                Ok(_) => {
                    stats.packets_sent.fetch_add(1, Ordering::Relaxed);
                    stats.samples_sent.fetch_add(samples_per_packet as u64, Ordering::Relaxed);
                }
                Err(_) => {
                    stats.send_errors.fetch_add(1, Ordering::Relaxed);
                }
            }

            // Schedule next packet
            next_tx = target_time + interval;

            // Reset if fallen too far behind
            if Instant::now() > next_tx + interval {
                next_tx = Instant::now() + interval;
            }
        }
    }

    /// Get current statistics (lock-free snapshot)
    pub fn stats(&self) -> OutputStats {
        OutputStats {
            packets_sent: self.stats.packets_sent.load(Ordering::Relaxed),
            samples_sent: self.stats.samples_sent.load(Ordering::Relaxed),
            send_errors: self.stats.send_errors.load(Ordering::Relaxed),
            underruns: self.stats.underruns.load(Ordering::Relaxed),
        }
    }

    /// Get packets sent count (lock-free)
    pub fn packets_sent(&self) -> u64 {
        self.stats.packets_sent.load(Ordering::Relaxed)
    }

    /// Get send errors count (lock-free)
    pub fn send_errors(&self) -> u64 {
        self.stats.send_errors.load(Ordering::Relaxed)
    }

    /// Check if stream is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Get the current applied PPM adjustment (lock-free)
    pub fn applied_ppm(&self) -> f64 {
        self.current_ppm_x1000.load(Ordering::Relaxed) as f64 / 1000.0
    }
}

impl Drop for Aes67OutputStream {
    fn drop(&mut self) {
        self.stop();
    }
}

unsafe impl Send for Aes67OutputStream {}
