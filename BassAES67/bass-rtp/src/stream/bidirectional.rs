//! Bidirectional RTP stream implementation.
//!
//! Combines input and output streams on a single UDP socket for
//! full-duplex communication with Telos Z/IP ONE.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::ffi::*;
use crate::rtp::{RtpSocket, PayloadCodec};
use crate::stream::input::{RtpInputStream, RtpInputConfig};
use crate::stream::output::{RtpOutputStream, RtpOutputConfig};

/// Bidirectional RTP stream configuration.
#[derive(Clone)]
pub struct BidirectionalConfig {
    /// Local port to bind
    pub local_port: u16,
    /// Remote IP address (Z/IP ONE)
    pub remote_addr: Ipv4Addr,
    /// Remote port (9151, 9152, or 9153 for reciprocal RTP)
    pub remote_port: u16,
    /// Sample rate (48000)
    pub sample_rate: u32,
    /// Number of channels (1 or 2)
    pub channels: u16,
    /// Output codec
    pub output_codec: PayloadCodec,
    /// Output bitrate for compressed codecs (kbps)
    pub output_bitrate: u32,
    /// Jitter buffer depth in milliseconds
    pub jitter_ms: u32,
    /// Network interface to bind to (0.0.0.0 for any)
    pub interface_addr: Ipv4Addr,
}

impl Default for BidirectionalConfig {
    fn default() -> Self {
        Self {
            local_port: 0, // Ephemeral port
            remote_addr: Ipv4Addr::new(0, 0, 0, 0),
            remote_port: 9152, // Same codec reply
            sample_rate: 48000,
            channels: 2,
            output_codec: PayloadCodec::Pcm16,
            output_bitrate: 256,
            jitter_ms: 20,
            interface_addr: Ipv4Addr::new(0, 0, 0, 0),
        }
    }
}

/// Combined statistics for bidirectional stream.
pub struct BidirectionalStats {
    /// Packets received
    pub rx_packets: u64,
    /// Packets sent
    pub tx_packets: u64,
    /// Bytes received
    pub rx_bytes: u64,
    /// Bytes sent
    pub tx_bytes: u64,
    /// Receive decode errors
    pub rx_decode_errors: u64,
    /// Transmit encode errors
    pub tx_encode_errors: u64,
    /// Receive underruns
    pub rx_underruns: u64,
    /// Transmit underruns
    pub tx_underruns: u64,
    /// Buffer fill percentage (0-100)
    pub buffer_fill_percent: u32,
    /// Detected input payload type
    pub detected_input_pt: u8,
}

/// Bidirectional RTP stream.
///
/// Manages both input (receive) and output (transmit) on a single socket.
pub struct BidirectionalStream {
    /// Input stream (receives from Z/IP ONE)
    input: RtpInputStream,
    /// Output stream (sends to Z/IP ONE)
    output: Option<RtpOutputStream>,
    /// Socket for both directions
    socket: Option<RtpSocket>,
    /// Configuration
    config: BidirectionalConfig,
    /// Running flag
    running: Arc<AtomicBool>,
    /// BASS source channel (for output)
    source_channel: HSTREAM,
}

impl BidirectionalStream {
    /// Create a new bidirectional RTP stream.
    ///
    /// # Arguments
    /// * `source_channel` - BASS channel to read audio from for transmission
    /// * `config` - Stream configuration
    pub fn new(source_channel: HSTREAM, config: BidirectionalConfig) -> Result<Self, String> {
        // Create input stream
        let input_config = RtpInputConfig {
            sample_rate: config.sample_rate,
            channels: config.channels,
            jitter_ms: config.jitter_ms,
        };
        let input = RtpInputStream::new(input_config)?;

        Ok(Self {
            input,
            output: None,
            socket: None,
            config,
            running: Arc::new(AtomicBool::new(false)),
            source_channel,
        })
    }

    /// Start the bidirectional stream.
    pub fn start(&mut self) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("Stream already running".to_string());
        }

        // Create and bind socket
        let bind_addr = SocketAddrV4::new(self.config.interface_addr, self.config.local_port);
        let socket = RtpSocket::bind(SocketAddr::V4(bind_addr))?;

        // Clone socket for output (both use same underlying socket)
        let socket_for_output = socket.try_clone()?;

        // Start input stream
        self.input.start(socket)?;

        // Create and start output stream
        let output_config = RtpOutputConfig {
            sample_rate: self.config.sample_rate,
            channels: self.config.channels,
            codec: self.config.output_codec,
            bitrate: self.config.output_bitrate,
            frame_duration_ms: 1, // 1ms packets for low latency
        };

        let mut output = RtpOutputStream::new(self.source_channel, output_config)?;
        let remote_addr = SocketAddr::V4(SocketAddrV4::new(
            self.config.remote_addr,
            self.config.remote_port,
        ));
        output.start(socket_for_output, remote_addr)?;

        self.output = Some(output);
        self.running.store(true, Ordering::SeqCst);

        Ok(())
    }

    /// Stop the bidirectional stream.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);

        self.input.stop();
        if let Some(ref mut output) = self.output {
            output.stop();
        }
        self.output = None;
        self.socket = None;
    }

    /// Check if stream is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Get the input BASS stream handle.
    ///
    /// This handle can be used to play the received audio.
    pub fn input_handle(&self) -> HSTREAM {
        self.input.handle
    }

    /// Set the input BASS stream handle (after BASS_StreamCreate).
    pub fn set_input_handle(&mut self, handle: HSTREAM) {
        self.input.handle = handle;
    }

    /// Get combined statistics.
    pub fn stats(&self) -> BidirectionalStats {
        let input_stats = self.input.stats();
        let output_stats = self.output.as_ref().map(|o| o.stats());

        BidirectionalStats {
            rx_packets: input_stats.packets_received.load(Ordering::Relaxed),
            tx_packets: output_stats.map_or(0, |s| s.packets_sent.load(Ordering::Relaxed)),
            rx_bytes: 0, // TODO: Track in input
            tx_bytes: output_stats.map_or(0, |s| s.bytes_sent.load(Ordering::Relaxed)),
            rx_decode_errors: input_stats.decode_errors.load(Ordering::Relaxed),
            tx_encode_errors: output_stats.map_or(0, |s| s.encode_errors.load(Ordering::Relaxed)),
            rx_underruns: input_stats.underruns.load(Ordering::Relaxed),
            tx_underruns: output_stats.map_or(0, |s| s.underruns.load(Ordering::Relaxed)),
            buffer_fill_percent: self.input.buffer_fill_percent(),
            detected_input_pt: self.input.detected_payload_type(),
        }
    }

    /// Get buffer fill percentage.
    pub fn buffer_fill_percent(&self) -> u32 {
        self.input.buffer_fill_percent()
    }

    /// Get detected input payload type.
    pub fn detected_input_pt(&self) -> u8 {
        self.input.detected_payload_type()
    }

    /// Get mutable reference to input stream (for STREAMPROC access).
    pub fn input_mut(&mut self) -> &mut RtpInputStream {
        &mut self.input
    }
}

impl Drop for BidirectionalStream {
    fn drop(&mut self) {
        self.stop();
    }
}
