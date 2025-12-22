//! Framed SRT sender for testing bass_srt plugin with codec support.
//!
//! This sends a test tone using the framing protocol with selectable codec.
//! It acts as an SRT listener (server) that the bass_srt plugin connects to.
//!
//! Usage: cargo run --example srt_sender_framed [OPTIONS]
//!
//! Options:
//!   --port PORT       Port to listen on (default: 9000)
//!   --freq FREQ       Tone frequency in Hz (default: 440)
//!   --codec CODEC     Codec to use: pcm, opus, mp2, flac (default: pcm)
//!   --bitrate RATE    Bitrate for encoded audio in kbps (default: 192)
//!
//! Examples:
//!   cargo run --example srt_sender_framed
//!   cargo run --example srt_sender_framed --codec opus --bitrate 128
//!   cargo run --example srt_sender_framed --codec mp2 --bitrate 256
//!   cargo run --example srt_sender_framed --codec flac

use std::ffi::c_int;
use std::time::{Duration, Instant};
use std::thread;
use std::f32::consts::PI;
use std::io::Write;

use bass_srt::protocol::{self, Packet, PacketHeader, HEADER_SIZE};
use bass_srt::codec::{opus, twolame, flac};

// SRT types and bindings
type SRTSOCKET = i32;
const SRT_INVALID_SOCK: SRTSOCKET = -1;
const SRT_ERROR: c_int = -1;

#[repr(C)]
struct SockaddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: u32,
    sin_zero: [u8; 8],
}

#[repr(C)]
struct Sockaddr {
    sa_family: u16,
    sa_data: [u8; 14],
}

const SRTO_TRANSTYPE: c_int = 50;
const SRTO_LATENCY: c_int = 23;
const SRTO_PASSPHRASE: c_int = 26;
const SRTT_LIVE: c_int = 0;

#[link(name = "srt-gnutls")]
extern "C" {
    fn srt_startup() -> c_int;
    fn srt_cleanup() -> c_int;
    fn srt_create_socket() -> SRTSOCKET;
    fn srt_close(sock: SRTSOCKET) -> c_int;
    fn srt_bind(sock: SRTSOCKET, name: *const Sockaddr, namelen: c_int) -> c_int;
    fn srt_listen(sock: SRTSOCKET, backlog: c_int) -> c_int;
    fn srt_accept(sock: SRTSOCKET, addr: *mut Sockaddr, addrlen: *mut c_int) -> SRTSOCKET;
    fn srt_connect(sock: SRTSOCKET, name: *const Sockaddr, namelen: c_int) -> c_int;
    fn srt_send(sock: SRTSOCKET, buf: *const i8, len: c_int) -> c_int;
    fn srt_setsockflag(sock: SRTSOCKET, opt: c_int, optval: *const std::ffi::c_void, optlen: c_int) -> c_int;
    fn srt_getlasterror_str() -> *const i8;
}

fn get_error() -> String {
    unsafe {
        let ptr = srt_getlasterror_str();
        if ptr.is_null() {
            "Unknown error".to_string()
        } else {
            std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Codec {
    Pcm,
    Opus,
    Mp2,
    Flac,
}

impl Codec {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "pcm" | "l16" | "raw" => Some(Codec::Pcm),
            "opus" => Some(Codec::Opus),
            "mp2" | "mpeg" => Some(Codec::Mp2),
            "flac" => Some(Codec::Flac),
            _ => None,
        }
    }

    fn format_byte(&self) -> u8 {
        match self {
            Codec::Pcm => protocol::FORMAT_PCM_L16,
            Codec::Opus => protocol::FORMAT_OPUS,
            Codec::Mp2 => protocol::FORMAT_MP2,
            Codec::Flac => protocol::FORMAT_FLAC,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Codec::Pcm => "PCM L16",
            Codec::Opus => "OPUS",
            Codec::Mp2 => "MP2",
            Codec::Flac => "FLAC",
        }
    }
}

struct Config {
    port: u16,
    frequency: f32,
    codec: Codec,
    bitrate: u32,
    flac_level: u32,  // FLAC compression level 0-8
    passphrase: Option<String>,
    connect_to: Option<String>,  // If set, connect as caller instead of listening
}

fn parse_args() -> Config {
    let args: Vec<String> = std::env::args().collect();
    let mut config = Config {
        port: 9000,
        frequency: 440.0,
        codec: Codec::Pcm,
        bitrate: 192,
        flac_level: 5,  // Default FLAC compression level (balanced)
        passphrase: None,
        connect_to: None,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => {
                if i + 1 < args.len() {
                    config.port = args[i + 1].parse().unwrap_or(9000);
                    i += 1;
                }
            }
            "--freq" => {
                if i + 1 < args.len() {
                    config.frequency = args[i + 1].parse().unwrap_or(440.0);
                    i += 1;
                }
            }
            "--codec" => {
                if i + 1 < args.len() {
                    config.codec = Codec::from_str(&args[i + 1]).unwrap_or(Codec::Pcm);
                    i += 1;
                }
            }
            "--bitrate" => {
                if i + 1 < args.len() {
                    config.bitrate = args[i + 1].parse().unwrap_or(192);
                    i += 1;
                }
            }
            "--level" | "-l" => {
                if i + 1 < args.len() {
                    let level: u32 = args[i + 1].parse().unwrap_or(5);
                    config.flac_level = level.min(8);  // Clamp to 0-8
                    i += 1;
                }
            }
            "--passphrase" | "--pass" | "-p" => {
                if i + 1 < args.len() {
                    config.passphrase = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--connect" | "-c" => {
                if i + 1 < args.len() {
                    config.connect_to = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--help" | "-h" => {
                println!("Usage: srt_sender_framed [OPTIONS]");
                println!();
                println!("Options:");
                println!("  --port PORT       Port to listen on (default: 9000)");
                println!("  --freq FREQ       Tone frequency in Hz (default: 440)");
                println!("  --codec CODEC     Codec: pcm, opus, mp2, flac (default: pcm)");
                println!("  --bitrate RATE    Bitrate in kbps for opus/mp2 (default: 192)");
                println!("  --level LEVEL     FLAC compression 0-8 (default: 5, higher=smaller)");
                println!("  --passphrase KEY  Encryption passphrase (min 10 chars)");
                println!("  --connect HOST:PORT  Connect to receiver (caller mode)");
                std::process::exit(0);
            }
            _ => {}
        }
        i += 1;
    }

    config
}

/// Encoder trait for different codecs
trait AudioEncoder {
    fn encode(&mut self, pcm: &[i16], output: &mut [u8]) -> Result<usize, String>;
    fn frame_size(&self) -> usize;  // samples per channel
    fn format_byte(&self) -> u8;
}

/// PCM encoder (no encoding, just conversion to bytes)
struct PcmEncoder;

impl AudioEncoder for PcmEncoder {
    fn encode(&mut self, pcm: &[i16], output: &mut [u8]) -> Result<usize, String> {
        let bytes_needed = pcm.len() * 2;
        if output.len() < bytes_needed {
            return Err("Output buffer too small".to_string());
        }
        for (i, &sample) in pcm.iter().enumerate() {
            let bytes = sample.to_le_bytes();
            output[i * 2] = bytes[0];
            output[i * 2 + 1] = bytes[1];
        }
        Ok(bytes_needed)
    }

    fn frame_size(&self) -> usize {
        240  // 5ms at 48kHz
    }

    fn format_byte(&self) -> u8 {
        protocol::FORMAT_PCM_L16
    }
}

/// OPUS encoder wrapper
struct OpusEncoderWrapper {
    encoder: opus::Encoder,
}

impl OpusEncoderWrapper {
    fn new(bitrate: u32) -> Result<Self, String> {
        let mut encoder = opus::Encoder::new_audio_48k_stereo_5ms()
            .map_err(|e| format!("Failed to create OPUS encoder: {}", e))?;
        encoder.set_bitrate(bitrate as i32 * 1000)
            .map_err(|e| format!("Failed to set bitrate: {}", e))?;
        Ok(Self { encoder })
    }
}

impl AudioEncoder for OpusEncoderWrapper {
    fn encode(&mut self, pcm: &[i16], output: &mut [u8]) -> Result<usize, String> {
        self.encoder.encode(pcm, output)
            .map_err(|e| format!("OPUS encode error: {}", e))
    }

    fn frame_size(&self) -> usize {
        self.encoder.frame_size()
    }

    fn format_byte(&self) -> u8 {
        protocol::FORMAT_OPUS
    }
}

/// MP2 encoder wrapper
struct Mp2EncoderWrapper {
    encoder: twolame::Encoder,
    // MP2 has fixed 1152 sample frames, so we need to buffer
    sample_buffer: Vec<i16>,
}

impl Mp2EncoderWrapper {
    fn new(bitrate: u32) -> Result<Self, String> {
        let encoder = twolame::Encoder::new(
            bass_srt::codec::AudioFormat::standard(),
            bitrate
        ).map_err(|e| format!("Failed to create MP2 encoder: {}", e))?;

        Ok(Self {
            encoder,
            sample_buffer: Vec::with_capacity(2304),  // 1152 samples * 2 channels
        })
    }
}

impl AudioEncoder for Mp2EncoderWrapper {
    fn encode(&mut self, pcm: &[i16], output: &mut [u8]) -> Result<usize, String> {
        self.encoder.encode(pcm, output)
            .map_err(|e| format!("MP2 encode error: {}", e))
    }

    fn frame_size(&self) -> usize {
        // MP2 uses fixed 1152 samples per frame (MPEG Layer 2 spec)
        // At 48kHz, this is 24ms per frame
        1152
    }

    fn format_byte(&self) -> u8 {
        protocol::FORMAT_MP2
    }
}

/// FLAC encoder wrapper
struct FlacEncoderWrapper {
    encoder: flac::Encoder,
}

impl FlacEncoderWrapper {
    fn new(level: u32) -> Result<Self, String> {
        let encoder = flac::Encoder::new(
            bass_srt::codec::AudioFormat::standard(),
            level
        ).map_err(|e| format!("Failed to create FLAC encoder: {}", e))?;
        Ok(Self { encoder })
    }
}

impl AudioEncoder for FlacEncoderWrapper {
    fn encode(&mut self, pcm: &[i16], output: &mut [u8]) -> Result<usize, String> {
        self.encoder.encode(pcm, output)
            .map_err(|e| format!("FLAC encode error: {}", e))
    }

    fn frame_size(&self) -> usize {
        // FLAC uses same frame size as MP2 for consistency
        flac::DEFAULT_FRAME_SIZE
    }

    fn format_byte(&self) -> u8 {
        protocol::FORMAT_FLAC
    }
}

fn main() {
    let config = parse_args();

    println!("SRT Framed Sender");
    println!("=================");
    if let Some(ref target) = config.connect_to {
        println!("Mode: Caller (connecting to {})", target);
    } else {
        println!("Mode: Listener (port {})", config.port);
    }
    println!("Frequency: {} Hz", config.frequency);
    println!("Codec: {}", config.codec.name());
    if config.codec != Codec::Pcm && config.codec != Codec::Flac {
        println!("Bitrate: {} kbps", config.bitrate);
    }
    if config.codec == Codec::Flac {
        println!("Mode: Lossless (level {})", config.flac_level);
    }
    if config.passphrase.is_some() {
        println!("Encryption: Enabled");
    }
    println!();

    // Create encoder based on codec selection
    let mut encoder: Box<dyn AudioEncoder> = match config.codec {
        Codec::Pcm => Box::new(PcmEncoder),
        Codec::Opus => {
            match OpusEncoderWrapper::new(config.bitrate) {
                Ok(e) => Box::new(e),
                Err(e) => {
                    eprintln!("ERROR: {}", e);
                    return;
                }
            }
        }
        Codec::Mp2 => {
            match Mp2EncoderWrapper::new(config.bitrate) {
                Ok(e) => Box::new(e),
                Err(e) => {
                    eprintln!("ERROR: {}", e);
                    return;
                }
            }
        }
        Codec::Flac => {
            match FlacEncoderWrapper::new(config.flac_level) {
                Ok(e) => Box::new(e),
                Err(e) => {
                    eprintln!("ERROR: {}", e);
                    return;
                }
            }
        }
    };

    unsafe {
        // Initialize SRT
        if srt_startup() != 0 {
            println!("ERROR: Failed to initialize SRT");
            return;
        }
        println!("SRT initialized");

        // Create socket
        let sock = srt_create_socket();
        if sock == SRT_INVALID_SOCK {
            println!("ERROR: Failed to create socket: {}", get_error());
            srt_cleanup();
            return;
        }
        println!("Socket created");

        // Set live mode
        let transtype = SRTT_LIVE;
        if srt_setsockflag(sock, SRTO_TRANSTYPE, &transtype as *const _ as *const std::ffi::c_void, 4) == SRT_ERROR {
            println!("ERROR: Failed to set transtype: {}", get_error());
            srt_close(sock);
            srt_cleanup();
            return;
        }

        // Set latency
        let latency: c_int = 120;
        if srt_setsockflag(sock, SRTO_LATENCY, &latency as *const _ as *const std::ffi::c_void, 4) == SRT_ERROR {
            println!("ERROR: Failed to set latency: {}", get_error());
            srt_close(sock);
            srt_cleanup();
            return;
        }

        // Set passphrase if provided
        if let Some(ref passphrase) = config.passphrase {
            let cstr = std::ffi::CString::new(passphrase.as_str()).unwrap();
            if srt_setsockflag(sock, SRTO_PASSPHRASE, cstr.as_ptr() as *const std::ffi::c_void, passphrase.len() as c_int) == SRT_ERROR {
                println!("ERROR: Failed to set passphrase: {}", get_error());
                println!("Note: Passphrase must be 10-79 characters");
                srt_close(sock);
                srt_cleanup();
                return;
            }
            println!("Encryption enabled with passphrase");
        }

        // Connect or listen based on mode
        let send_socket: SRTSOCKET;
        let listen_socket: Option<SRTSOCKET>;

        if let Some(ref target) = config.connect_to {
            // CALLER MODE - connect to remote receiver
            listen_socket = None;

            // Parse host:port
            let parts: Vec<&str> = target.split(':').collect();
            if parts.len() != 2 {
                println!("ERROR: Invalid connect address. Use HOST:PORT format");
                srt_close(sock);
                srt_cleanup();
                return;
            }

            let host = parts[0];
            let port: u16 = match parts[1].parse() {
                Ok(p) => p,
                Err(_) => {
                    println!("ERROR: Invalid port number");
                    srt_close(sock);
                    srt_cleanup();
                    return;
                }
            };

            // Resolve hostname to IP
            let ip_addr: u32 = if host == "localhost" || host == "127.0.0.1" {
                0x7f000001_u32.to_be()  // 127.0.0.1
            } else {
                // Parse IP address
                let parts: Vec<u8> = host.split('.').filter_map(|s| s.parse().ok()).collect();
                if parts.len() != 4 {
                    println!("ERROR: Invalid IP address (DNS lookup not supported, use IP)");
                    srt_close(sock);
                    srt_cleanup();
                    return;
                }
                u32::from_be_bytes([parts[0], parts[1], parts[2], parts[3]])
            };

            let addr = SockaddrIn {
                sin_family: 2,  // AF_INET
                sin_port: port.to_be(),
                sin_addr: ip_addr,
                sin_zero: [0; 8],
            };

            println!("Connecting to {}:{}...", host, port);
            if srt_connect(sock, &addr as *const _ as *const Sockaddr, std::mem::size_of::<SockaddrIn>() as c_int) == SRT_ERROR {
                println!("ERROR: Failed to connect: {}", get_error());
                srt_close(sock);
                srt_cleanup();
                return;
            }
            println!("Connected!");

            send_socket = sock;
        } else {
            // LISTENER MODE - wait for incoming connection with reconnection support
            let addr = SockaddrIn {
                sin_family: 2,
                sin_port: config.port.to_be(),
                sin_addr: 0,
                sin_zero: [0; 8],
            };

            if srt_bind(sock, &addr as *const _ as *const Sockaddr, std::mem::size_of::<SockaddrIn>() as c_int) == SRT_ERROR {
                println!("ERROR: Failed to bind: {}", get_error());
                srt_close(sock);
                srt_cleanup();
                return;
            }
            println!("Bound to port {}", config.port);

            if srt_listen(sock, 1) == SRT_ERROR {
                println!("ERROR: Failed to listen: {}", get_error());
                srt_close(sock);
                srt_cleanup();
                return;
            }
            println!("Listening for connections...");
            println!();
            println!("Run the receiver with:");
            if config.passphrase.is_some() {
                println!("  ./target/release/examples/test_srt_input \"srt://127.0.0.1:{}?passphrase=YOUR_KEY\"", config.port);
            } else {
                println!("  ./target/release/examples/test_srt_input srt://127.0.0.1:{}", config.port);
            }
            println!();

            // Listener mode with reconnection - accept loop
            let listen_sock = sock;
            let mut client_addr: Sockaddr = std::mem::zeroed();
            let mut addr_len: c_int = std::mem::size_of::<Sockaddr>() as c_int;

            // Audio parameters
            let sample_rate = 48000;
            let channels = 2;
            let frame_size = encoder.frame_size();
            let total_samples = frame_size * channels;
            let packet_duration_ms = (frame_size * 1000) / sample_rate;

            // PCM buffer for sine wave generation
            let mut pcm_buffer = vec![0i16; total_samples];

            // Encoded output buffer (max size)
            let mut encoded_buffer = vec![0u8; 4096];

            // Framed packet buffer
            let mut send_buffer = vec![0u8; 4096 + HEADER_SIZE];

            println!("Frame size: {} samples/ch ({}ms)", frame_size, packet_duration_ms);
            println!("Format: {}", config.codec.name());

            // Timing
            let packet_interval = Duration::from_millis(packet_duration_ms as u64);
            let mut phase: f32 = 0.0;
            let phase_increment = 2.0 * PI * config.frequency / sample_rate as f32;

            // Accept loop - reconnect when client disconnects
            loop {
                println!("Waiting for client connection...");

                let client = srt_accept(listen_sock, &mut client_addr, &mut addr_len);
                if client == SRT_INVALID_SOCK {
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
                println!("Client connected!");

                let mut packets_sent: u64 = 0;
                let mut bytes_sent: u64 = 0;
                let start_time = Instant::now();
                let mut next_send = Instant::now();

                println!("Sending {} Hz tone...", config.frequency);

                // Send loop for this client
                loop {
                    // Generate sine wave samples
                    for i in 0..frame_size {
                        let sample = (phase.sin() * 16000.0) as i16;
                        pcm_buffer[i * 2] = sample;      // Left
                        pcm_buffer[i * 2 + 1] = sample;  // Right

                        phase += phase_increment;
                        if phase > 2.0 * PI {
                            phase -= 2.0 * PI;
                        }
                    }

                    // Encode audio
                    let encoded_len = match encoder.encode(&pcm_buffer, &mut encoded_buffer) {
                        Ok(len) => len,
                        Err(e) => {
                            eprintln!("\nEncode error: {}", e);
                            break;
                        }
                    };

                    // Skip if no output (MP2 buffering)
                    if encoded_len == 0 {
                        continue;
                    }

                    // Create framed packet
                    let header = PacketHeader::audio(encoder.format_byte(), encoded_len as u16);
                    let header_bytes = header.encode();

                    send_buffer[..HEADER_SIZE].copy_from_slice(&header_bytes);
                    send_buffer[HEADER_SIZE..HEADER_SIZE + encoded_len].copy_from_slice(&encoded_buffer[..encoded_len]);

                    let total_len = HEADER_SIZE + encoded_len;

                    // Send packet
                    let result = srt_send(
                        client,
                        send_buffer.as_ptr() as *const i8,
                        total_len as c_int
                    );

                    if result == SRT_ERROR {
                        println!("\nClient disconnected");
                        srt_close(client);
                        break;  // Break inner loop, continue outer accept loop
                    }

                    packets_sent += 1;
                    bytes_sent += total_len as u64;

                    // Print status every second
                    if packets_sent % (1000 / packet_duration_ms.max(1) as u64) == 0 {
                        let elapsed = start_time.elapsed().as_secs();
                        let kbps = if elapsed > 0 { bytes_sent * 8 / (elapsed * 1000) } else { 0 };
                        print!("\rSent {} packets, {} KB ({} kbps, {} seconds)",
                            packets_sent, bytes_sent / 1024, kbps, elapsed);
                        std::io::stdout().flush().ok();
                    }

                    // Wait for next packet time
                    next_send += packet_interval;
                    let now = Instant::now();
                    if next_send > now {
                        thread::sleep(next_send - now);
                    } else {
                        next_send = Instant::now() + packet_interval;
                    }
                }

                println!("\nWaiting for new connection...\n");
            }
            // Note: This loop never exits normally - Ctrl+C to stop
            // Cleanup (unreachable in normal operation)
            #[allow(unreachable_code)]
            {
                srt_close(listen_sock);
                srt_cleanup();
            }
            return;
        }

        // CALLER MODE continues here with send_socket
        // Audio parameters
        let sample_rate = 48000;
        let channels = 2;
        let frame_size = encoder.frame_size();  // samples per channel
        let total_samples = frame_size * channels;
        let packet_duration_ms = (frame_size * 1000) / sample_rate;

        // PCM buffer for sine wave generation
        let mut pcm_buffer = vec![0i16; total_samples];

        // Encoded output buffer (max size)
        let mut encoded_buffer = vec![0u8; 4096];

        // Framed packet buffer
        let mut send_buffer = vec![0u8; 4096 + HEADER_SIZE];

        println!("Frame size: {} samples/ch ({}ms)", frame_size, packet_duration_ms);
        println!("Format: {}", config.codec.name());

        // Timing
        let packet_interval = Duration::from_millis(packet_duration_ms as u64);
        let mut next_send = Instant::now();
        let mut phase: f32 = 0.0;
        let phase_increment = 2.0 * PI * config.frequency / sample_rate as f32;

        let mut packets_sent: u64 = 0;
        let mut bytes_sent: u64 = 0;
        let start_time = Instant::now();

        println!("Sending {} Hz tone...", config.frequency);
        println!("Press Ctrl+C to stop...");
        println!();

        loop {
            // Generate sine wave samples
            for i in 0..frame_size {
                let sample = (phase.sin() * 16000.0) as i16;
                pcm_buffer[i * 2] = sample;      // Left
                pcm_buffer[i * 2 + 1] = sample;  // Right

                phase += phase_increment;
                if phase > 2.0 * PI {
                    phase -= 2.0 * PI;
                }
            }

            // Encode audio
            let encoded_len = match encoder.encode(&pcm_buffer, &mut encoded_buffer) {
                Ok(len) => len,
                Err(e) => {
                    eprintln!("\nEncode error: {}", e);
                    break;
                }
            };

            // Skip if no output (MP2 buffering)
            if encoded_len == 0 {
                continue;
            }

            // Create framed packet
            let header = PacketHeader::audio(encoder.format_byte(), encoded_len as u16);
            let header_bytes = header.encode();

            send_buffer[..HEADER_SIZE].copy_from_slice(&header_bytes);
            send_buffer[HEADER_SIZE..HEADER_SIZE + encoded_len].copy_from_slice(&encoded_buffer[..encoded_len]);

            let total_len = HEADER_SIZE + encoded_len;

            // Send packet
            let result = srt_send(
                send_socket,
                send_buffer.as_ptr() as *const i8,
                total_len as c_int
            );

            if result == SRT_ERROR {
                println!("\nClient disconnected or error: {}", get_error());
                break;
            }

            packets_sent += 1;
            bytes_sent += total_len as u64;

            // Print status every second
            if packets_sent % (1000 / packet_duration_ms.max(1) as u64) == 0 {
                let elapsed = start_time.elapsed().as_secs();
                let kbps = if elapsed > 0 { bytes_sent * 8 / (elapsed * 1000) } else { 0 };
                print!("\rSent {} packets, {} KB ({} kbps, {} seconds)",
                    packets_sent, bytes_sent / 1024, kbps, elapsed);
                std::io::stdout().flush().ok();
            }

            // Wait for next packet time
            next_send += packet_interval;
            let now = Instant::now();
            if next_send > now {
                thread::sleep(next_send - now);
            } else {
                next_send = Instant::now() + packet_interval;
            }
        }

        // Cleanup
        println!("\nCleaning up...");
        srt_close(send_socket);
        if let Some(listen_sock) = listen_socket {
            srt_close(listen_sock);
        }
        srt_cleanup();
        println!("Done!");
    }
}
