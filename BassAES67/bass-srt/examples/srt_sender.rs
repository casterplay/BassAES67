//! Simple SRT sender for testing bass_srt plugin.
//!
//! This sends a test tone (sine wave) as raw L16 PCM over SRT.
//! It acts as an SRT listener (server) that the bass_srt plugin connects to.
//!
//! Usage: cargo run --example srt_sender [port] [frequency]
//!
//! Default: port=9000, frequency=440Hz

use std::ffi::c_int;
use std::time::{Duration, Instant};
use std::thread;
use std::f32::consts::PI;

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

// SRT socket options (from srt.h)
const SRTO_TRANSTYPE: c_int = 50;  // Transmission type
const SRTO_LATENCY: c_int = 23;    // Latency in ms
const SRTT_LIVE: c_int = 0;        // Live transmission mode

#[link(name = "srt-gnutls")]
extern "C" {
    fn srt_startup() -> c_int;
    fn srt_cleanup() -> c_int;
    fn srt_create_socket() -> SRTSOCKET;
    fn srt_close(sock: SRTSOCKET) -> c_int;
    fn srt_bind(sock: SRTSOCKET, name: *const Sockaddr, namelen: c_int) -> c_int;
    fn srt_listen(sock: SRTSOCKET, backlog: c_int) -> c_int;
    fn srt_accept(sock: SRTSOCKET, addr: *mut Sockaddr, addrlen: *mut c_int) -> SRTSOCKET;
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

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let port: u16 = args.get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(9000);

    let frequency: f32 = args.get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(440.0);

    println!("SRT Test Sender");
    println!("===============");
    println!("Port: {}", port);
    println!("Frequency: {} Hz", frequency);
    println!();

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

        // Bind to address
        let addr = SockaddrIn {
            sin_family: 2, // AF_INET
            sin_port: port.to_be(),
            sin_addr: 0, // INADDR_ANY
            sin_zero: [0; 8],
        };

        if srt_bind(sock, &addr as *const _ as *const Sockaddr, std::mem::size_of::<SockaddrIn>() as c_int) == SRT_ERROR {
            println!("ERROR: Failed to bind: {}", get_error());
            srt_close(sock);
            srt_cleanup();
            return;
        }
        println!("Bound to port {}", port);

        // Listen
        if srt_listen(sock, 1) == SRT_ERROR {
            println!("ERROR: Failed to listen: {}", get_error());
            srt_close(sock);
            srt_cleanup();
            return;
        }
        println!("Listening for connections...");
        println!();
        println!("Run the receiver with:");
        println!("  ./target/release/examples/test_srt_input srt://127.0.0.1:{}", port);
        println!();

        // Accept connection
        let mut client_addr: Sockaddr = std::mem::zeroed();
        let mut addr_len: c_int = std::mem::size_of::<Sockaddr>() as c_int;

        let client = srt_accept(sock, &mut client_addr, &mut addr_len);
        if client == SRT_INVALID_SOCK {
            println!("ERROR: Failed to accept: {}", get_error());
            srt_close(sock);
            srt_cleanup();
            return;
        }
        println!("Client connected!");

        // Audio parameters
        let sample_rate = 48000;
        let channels = 2;
        // SRT live mode max payload is 1316 bytes
        // For L16 stereo: 1316 / 4 = 329 samples max
        // Use 5ms packets: 48000 * 5 / 1000 = 240 samples = 960 bytes (fits!)
        let packet_duration_ms = 5;
        let samples_per_packet = sample_rate * packet_duration_ms / 1000;
        let total_samples = samples_per_packet * channels;

        // Packet buffer (L16 = 2 bytes per sample)
        let mut packet = vec![0i16; total_samples];
        let packet_bytes = total_samples * 2;

        println!("Packet size: {} bytes ({} samples, {}ms)", packet_bytes, total_samples, packet_duration_ms);

        // Timing
        let packet_interval = Duration::from_millis(packet_duration_ms as u64);
        let mut next_send = Instant::now();
        let mut phase: f32 = 0.0;
        let phase_increment = 2.0 * PI * frequency / sample_rate as f32;

        let mut packets_sent: u64 = 0;
        let start_time = Instant::now();

        println!("Sending {} Hz tone at {}Hz, {} channels, {}ms packets",
            frequency, sample_rate, channels, packet_duration_ms);
        println!("Press Ctrl+C to stop...");
        println!();

        loop {
            // Generate sine wave samples
            for i in 0..samples_per_packet {
                let sample = (phase.sin() * 16000.0) as i16;  // -16000 to +16000

                // Interleave stereo (same signal on both channels)
                packet[i * 2] = sample;      // Left
                packet[i * 2 + 1] = sample;  // Right

                phase += phase_increment;
                if phase > 2.0 * PI {
                    phase -= 2.0 * PI;
                }
            }

            // Send packet
            let result = srt_send(
                client,
                packet.as_ptr() as *const i8,
                packet_bytes as c_int
            );

            if result == SRT_ERROR {
                println!("\nClient disconnected or error: {}", get_error());
                break;
            }

            packets_sent += 1;

            // Print status every second
            if packets_sent % (1000 / packet_duration_ms as u64) == 0 {
                let elapsed = start_time.elapsed().as_secs();
                print!("\rSent {} packets ({} seconds)", packets_sent, elapsed);
                use std::io::Write;
                std::io::stdout().flush().ok();
            }

            // Wait for next packet time
            next_send += packet_interval;
            let now = Instant::now();
            if next_send > now {
                thread::sleep(next_send - now);
            } else {
                // Fallen behind - reset timing
                next_send = Instant::now() + packet_interval;
            }
        }

        // Cleanup
        println!("\nCleaning up...");
        srt_close(client);
        srt_close(sock);
        srt_cleanup();
        println!("Done!");
    }
}
