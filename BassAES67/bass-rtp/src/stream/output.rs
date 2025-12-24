//! RTP output stream implementation.
//!
//! Reads audio from BASS channel, encodes with selected codec, and transmits RTP packets.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::codec::{AudioFormat, AudioEncoder, Pcm16Encoder, Pcm24Encoder, twolame};
use crate::ffi::*;
use crate::rtp::{RtpPacketBuilder, RtpSocket, PayloadCodec};

/// MP2 RTP payload type (MPEG Audio)
const MP2_PAYLOAD_TYPE: u8 = 14;

/// Output stream statistics (lock-free atomic updates).
pub struct OutputStats {
    pub packets_sent: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub encode_errors: AtomicU64,
    pub underruns: AtomicU64,
}

impl OutputStats {
    pub fn new() -> Self {
        Self {
            packets_sent: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            encode_errors: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
        }
    }
}

impl Default for OutputStats {
    fn default() -> Self {
        Self::new()
    }
}

/// RTP output stream configuration.
#[derive(Clone)]
pub struct RtpOutputConfig {
    /// Sample rate (48000)
    pub sample_rate: u32,
    /// Number of channels (1 or 2)
    pub channels: u16,
    /// Codec to use for encoding
    pub codec: PayloadCodec,
    /// Bitrate for compressed codecs (kbps)
    pub bitrate: u32,
    /// Frame duration in milliseconds
    pub frame_duration_ms: u32,
}

impl Default for RtpOutputConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            channels: 2,
            codec: PayloadCodec::Pcm16,
            bitrate: 256,
            frame_duration_ms: 1,
        }
    }
}

/// Encoder type enum for codec switching.
#[allow(dead_code)]
enum EncoderType {
    /// No encoder (uninitialized)
    None,
    /// PCM 16-bit encoder
    Pcm16(Pcm16Encoder),
    /// PCM 24-bit encoder
    Pcm24(Pcm24Encoder),
    /// MP2 encoder (TwoLAME)
    Mp2(twolame::Encoder),
}

impl EncoderType {
    /// Encode samples and return number of bytes written.
    fn encode(&mut self, pcm: &[f32], output: &mut [u8]) -> Result<usize, String> {
        match self {
            EncoderType::None => Err("No encoder initialized".to_string()),
            EncoderType::Pcm16(enc) => enc.encode(pcm, output)
                .map_err(|e| format!("PCM16 encode error: {:?}", e)),
            EncoderType::Pcm24(enc) => enc.encode(pcm, output)
                .map_err(|e| format!("PCM24 encode error: {:?}", e)),
            EncoderType::Mp2(enc) => enc.encode_float(pcm, output)
                .map_err(|e| format!("MP2 encode error: {:?}", e)),
        }
    }

    /// Get total samples per frame (samples * channels).
    fn total_samples_per_frame(&self) -> usize {
        match self {
            EncoderType::None => 0,
            EncoderType::Pcm16(enc) => enc.total_samples_per_frame(),
            EncoderType::Pcm24(enc) => enc.total_samples_per_frame(),
            EncoderType::Mp2(enc) => enc.total_samples_per_frame(),
        }
    }

    /// Get payload type.
    fn payload_type(&self) -> u8 {
        match self {
            EncoderType::None => 0,
            EncoderType::Pcm16(enc) => enc.payload_type(),
            EncoderType::Pcm24(enc) => enc.payload_type(),
            EncoderType::Mp2(_) => MP2_PAYLOAD_TYPE,
        }
    }
}

/// RTP output stream.
///
/// Reads audio from a BASS channel and transmits as RTP packets.
pub struct RtpOutputStream {
    /// Flag to stop transmitter thread
    running: Arc<AtomicBool>,
    /// Transmitter thread handle
    transmitter_thread: Option<JoinHandle<()>>,
    /// Stream configuration
    config: RtpOutputConfig,
    /// Statistics (lock-free)
    stats: Arc<OutputStats>,
    /// BASS channel to read from
    source_channel: HSTREAM,
}

impl RtpOutputStream {
    /// Create a new RTP output stream.
    pub fn new(source_channel: HSTREAM, config: RtpOutputConfig) -> Result<Self, String> {
        Ok(Self {
            running: Arc::new(AtomicBool::new(false)),
            transmitter_thread: None,
            config,
            stats: Arc::new(OutputStats::new()),
            source_channel,
        })
    }

    /// Start transmitting to the given socket and remote address.
    pub fn start(&mut self, socket: RtpSocket, remote_addr: std::net::SocketAddr) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("Stream already running".to_string());
        }

        self.running.store(true, Ordering::SeqCst);

        let running = self.running.clone();
        let stats = self.stats.clone();
        let config = self.config.clone();
        let source_channel = self.source_channel;

        self.transmitter_thread = Some(thread::spawn(move || {
            Self::transmitter_loop(socket, remote_addr, running, stats, config, source_channel);
        }));

        Ok(())
    }

    /// Transmitter thread loop - reads from BASS channel, encodes, and sends RTP packets.
    fn transmitter_loop(
        socket: RtpSocket,
        remote_addr: std::net::SocketAddr,
        running: Arc<AtomicBool>,
        stats: Arc<OutputStats>,
        config: RtpOutputConfig,
        source_channel: HSTREAM,
    ) {
        let format = AudioFormat::new(config.sample_rate, config.channels as u8);

        // Create encoder based on config
        let mut encoder = match config.codec {
            PayloadCodec::Pcm16 => EncoderType::Pcm16(Pcm16Encoder::new(format, config.frame_duration_ms as usize)),
            PayloadCodec::Pcm24 => EncoderType::Pcm24(Pcm24Encoder::new(format, config.frame_duration_ms as usize)),
            PayloadCodec::Mp2 => {
                match twolame::Encoder::new(format, config.bitrate) {
                    Ok(enc) => EncoderType::Mp2(enc),
                    Err(e) => {
                        eprintln!("Failed to create MP2 encoder: {:?}", e);
                        return;
                    }
                }
            }
            _ => {
                eprintln!("Unsupported codec: {:?}", config.codec);
                return;
            }
        };

        let samples_per_frame = encoder.total_samples_per_frame();
        let samples_per_channel = samples_per_frame / config.channels as usize;

        // Calculate frame duration from actual samples (important for MP2 which has fixed 1152 samples)
        let frame_duration_us = (samples_per_channel as u64 * 1_000_000) / config.sample_rate as u64;
        let frame_duration = Duration::from_micros(frame_duration_us);

        // Allocate buffers - MP2 needs larger encode buffer for compressed output
        let mut pcm_buffer = vec![0.0f32; samples_per_frame];
        let encode_buffer_size = match config.codec {
            PayloadCodec::Mp2 => 4608, // Max MP2 frame size
            _ => samples_per_frame * 3, // Max 3 bytes per sample (24-bit PCM)
        };
        let mut encode_buffer = vec![0u8; encode_buffer_size];

        // Create packet builder
        let mut packet_builder = RtpPacketBuilder::new(encoder.payload_type());

        let mut next_send_time = Instant::now();

        while running.load(Ordering::SeqCst) {
            // Wait until next send time
            let now = Instant::now();
            if now < next_send_time {
                thread::sleep(next_send_time - now);
            }
            next_send_time += frame_duration;

            // Read samples from BASS channel
            let bytes_needed = (samples_per_frame * 4) as u32; // 4 bytes per f32
            let bytes_read = unsafe {
                BASS_ChannelGetData(
                    source_channel,
                    pcm_buffer.as_mut_ptr() as *mut std::ffi::c_void,
                    bytes_needed | BASS_DATA_FLOAT,
                )
            };

            if bytes_read == u32::MAX {
                // Error or end of stream
                stats.underruns.fetch_add(1, Ordering::Relaxed);
                pcm_buffer.fill(0.0); // Send silence
            } else if (bytes_read as usize) < samples_per_frame * 4 {
                // Partial read - fill rest with silence
                let samples_read = bytes_read as usize / 4;
                pcm_buffer[samples_read..].fill(0.0);
                stats.underruns.fetch_add(1, Ordering::Relaxed);
            }

            // Encode
            match encoder.encode(&pcm_buffer, &mut encode_buffer) {
                Ok(encoded_bytes) => {
                    // Only send if we have encoded data (MP2 buffers until full frame)
                    if encoded_bytes > 0 {
                        // Build and send RTP packet
                        let packet = packet_builder.build_packet(
                            &encode_buffer[..encoded_bytes],
                            samples_per_channel as u32,
                        );

                        if let Err(_) = socket.send_to(packet, remote_addr) {
                            // Network error - continue anyway
                        } else {
                            stats.packets_sent.fetch_add(1, Ordering::Relaxed);
                            stats.bytes_sent.fetch_add(packet.len() as u64, Ordering::Relaxed);
                        }
                    }
                }
                Err(_) => {
                    stats.encode_errors.fetch_add(1, Ordering::Relaxed);
                }
            }

            // Apply clock correction if available
            if crate::clock_bindings::clock_is_locked() {
                let ppm = crate::clock_bindings::clock_get_frequency_ppm();
                // Adjust next_send_time based on clock offset
                let adjustment_ns = (frame_duration.as_nanos() as f64 * ppm / 1_000_000.0) as i64;
                if adjustment_ns > 0 {
                    next_send_time += Duration::from_nanos(adjustment_ns as u64);
                } else if adjustment_ns < 0 {
                    next_send_time = next_send_time.checked_sub(Duration::from_nanos((-adjustment_ns) as u64))
                        .unwrap_or(next_send_time);
                }
            }
        }
    }

    /// Stop the stream.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);

        if let Some(thread) = self.transmitter_thread.take() {
            let _ = thread.join();
        }
    }

    /// Check if stream is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Get statistics reference.
    pub fn stats(&self) -> &Arc<OutputStats> {
        &self.stats
    }
}

impl Drop for RtpOutputStream {
    fn drop(&mut self) {
        self.stop();
    }
}
