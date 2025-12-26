//! WebRTC Bidirectional Audio Example
//!
//! Demonstrates true bidirectional WebRTC audio using WebSocket signaling with room support.
//! This example:
//! 1. Starts a WebSocket signaling server with room-based routing
//! 2. Creates a WebRTC peer that connects to the signaling server in a specific room
//! 3. Sends a 440Hz test tone to the browser
//! 4. Receives audio from the browser and plays it locally
//!
//! Usage:
//!   webrtc_bidirectional --port 8080 --room my-session
//!
//! Then open test_client_websocket.html in a browser and connect to the same room.

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// Use the library directly
use bass_webrtc::signaling::{SignalingServer, WebRtcPeer};
use bass_webrtc::ice;
use webrtc::media::Sample;

// BASS FFI types
type DWORD = u32;
type BOOL = i32;
type HSTREAM = DWORD;

const FALSE: BOOL = 0;

#[link(name = "bass")]
extern "system" {
    fn BASS_Init(device: i32, freq: DWORD, flags: DWORD, win: *mut c_void, dsguid: *const c_void) -> BOOL;
    fn BASS_Free() -> BOOL;
    fn BASS_ErrorGetCode() -> i32;
}

// Thread-safe tone generator
struct ToneGenerator {
    phase: f32,
    phase_increment: f32,
    amplitude: f32,
}

impl ToneGenerator {
    fn new(frequency: f32, sample_rate: f32, amplitude: f32) -> Self {
        Self {
            phase: 0.0,
            phase_increment: 2.0 * std::f32::consts::PI * frequency / sample_rate,
            amplitude,
        }
    }

    fn generate(&mut self, buffer: &mut [f32]) {
        for chunk in buffer.chunks_mut(2) {
            let sample = self.phase.sin() * self.amplitude;
            chunk[0] = sample;
            if chunk.len() > 1 {
                chunk[1] = sample;
            }
            self.phase += self.phase_increment;
            if self.phase > 2.0 * std::f32::consts::PI {
                self.phase -= 2.0 * std::f32::consts::PI;
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut port: u16 = 8080;
    let mut room_id = String::from("default");

    // Parse arguments
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" if i + 1 < args.len() => {
                port = args[i + 1].parse().unwrap_or(8080);
                i += 1;
            }
            "--room" if i + 1 < args.len() => {
                room_id = args[i + 1].clone();
                i += 1;
            }
            "--help" | "-h" => {
                println!("WebRTC Bidirectional Audio Example");
                println!();
                println!("Usage:");
                println!("  webrtc_bidirectional [--port <port>] [--room <room_id>]");
                println!();
                println!("Options:");
                println!("  --port <port>      WebSocket signaling server port (default: 8080)");
                println!("  --room <room_id>   Room ID for signaling isolation (default: 'default')");
                println!();
                println!("Example:");
                println!("  webrtc_bidirectional --port 8080 --room studio-1");
                println!();
                println!("Then open test_client_websocket.html in a browser and connect to the same room.");
                return;
            }
            _ => {}
        }
        i += 1;
    }

    println!("==========================================");
    println!("  WebRTC Bidirectional Audio Example");
    println!("==========================================");
    println!();

    // Setup Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        println!("\nReceived Ctrl+C, stopping...");
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl-C handler");

    // Start signaling server in background
    let server_port = port;
    thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let server = SignalingServer::new(server_port);
            println!("[SignalingServer] Starting on port {}", server_port);
            if let Err(e) = server.run().await {
                eprintln!("Signaling server error: {}", e);
            }
        });
    });

    // Give signaling server time to start
    thread::sleep(Duration::from_millis(500));

    println!("[OK] Signaling server started on port {}", port);
    println!("[OK] Room ID: '{}'", room_id);
    println!();
    println!("WebSocket URL: ws://localhost:{}/{}", port, room_id);
    println!();

    unsafe {
        // Initialize BASS
        if BASS_Init(-1, 48000, 0, ptr::null_mut(), ptr::null()) == FALSE {
            println!("ERROR: Failed to initialize BASS (error: {})", BASS_ErrorGetCode());
            return;
        }
        println!("[OK] BASS initialized");
    }

    println!("[OK] Will generate 440Hz test tone");

    println!();
    println!("Instructions:");
    println!("  1. Open test_client_websocket.html in a browser");
    println!("  2. Enter WebSocket URL: ws://localhost:{}", port);
    println!("  3. Enter Room ID: {}", room_id);
    println!("  4. Click Connect");
    println!("  5. Audio will flow bidirectionally");
    println!();

    // WebRTC peer configuration
    let signaling_url = format!("ws://127.0.0.1:{}", port);
    let ice_servers = ice::google_stun_servers();
    let buffer_samples = 48000 * 2 * 3; // 3 seconds buffer
    let peer_room_id = room_id.clone();

    println!("[OK] WebRTC peer configuration ready");
    println!();
    println!("--- Waiting for browser to connect to room '{}' ... ---", room_id);
    println!();

    // Run the peer connection and audio streaming in a dedicated thread
    // This thread handles reconnection automatically
    let running_clone = running.clone();
    let (status_tx, status_rx) = std::sync::mpsc::channel::<&'static str>();

    thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut connection_count = 0u32;

            // Reconnection loop - keeps accepting new connections
            while running_clone.load(Ordering::SeqCst) {
                connection_count += 1;
                println!("[WebRTC Peer] === Connection attempt #{} (room: '{}') ===",
                         connection_count, peer_room_id);

                // Create a fresh peer for each connection
                let mut peer = WebRtcPeer::new(
                    &signaling_url,
                    &peer_room_id,
                    ice_servers.clone(),
                    48000,
                    2,
                    buffer_samples,
                );

                println!("[WebRTC Peer] Connecting to signaling server...");
                let _ = status_tx.send("Connecting");

                match peer.connect().await {
                    Ok(()) => {
                        println!("[WebRTC Peer] Connected! WebRTC session established.");

                        // Get the audio track for sending
                        let audio_track = match peer.audio_track() {
                            Some(track) => {
                                println!("[WebRTC Peer] Got audio track, starting transmission...");
                                track.clone()
                            }
                            None => {
                                eprintln!("[WebRTC Peer] ERROR: No audio track available!");
                                let _ = status_tx.send("Error: No track");
                                tokio::time::sleep(Duration::from_secs(1)).await;
                                continue;
                            }
                        };

                        let _ = status_tx.send("Streaming");

                        // OPUS encoder setup
                        let mut encoder = match bass_webrtc::codec::opus::Encoder::new_audio_48k_stereo_20ms() {
                            Ok(e) => {
                                println!("[WebRTC Peer] OPUS encoder created");
                                e
                            }
                            Err(e) => {
                                eprintln!("[WebRTC Peer] Failed to create encoder: {:?}", e);
                                let _ = status_tx.send("Error: Encoder");
                                tokio::time::sleep(Duration::from_secs(1)).await;
                                continue;
                            }
                        };

                        // Create tone generator
                        let mut tone_gen = ToneGenerator::new(440.0, 48000.0, 0.3);

                        // Audio buffers
                        let samples_per_frame = 960 * 2; // 20ms at 48kHz stereo
                        let mut pcm_buffer = vec![0.0f32; samples_per_frame];
                        let mut opus_buffer = vec![0u8; 4000];

                        // Streaming loop
                        let frame_duration = std::time::Duration::from_millis(20);
                        let mut last_send = std::time::Instant::now();
                        let mut frames_sent: u64 = 0;

                        println!("[WebRTC Peer] Starting audio loop...");

                        while running_clone.load(Ordering::SeqCst) && peer.is_connected() {
                            tone_gen.generate(&mut pcm_buffer);

                            match encoder.encode_float(&pcm_buffer, &mut opus_buffer) {
                                Ok(encoded_len) => {
                                    let sample = Sample {
                                        data: bytes::Bytes::copy_from_slice(&opus_buffer[..encoded_len]),
                                        duration: frame_duration,
                                        ..Default::default()
                                    };

                                    if let Err(e) = audio_track.write_sample(&sample).await {
                                        eprintln!("[WebRTC Peer] Write sample error: {}", e);
                                        break;
                                    } else {
                                        frames_sent += 1;
                                        if frames_sent % 50 == 0 {
                                            println!("[WebRTC Peer] Sent {} frames ({} seconds)",
                                                     frames_sent, frames_sent / 50);
                                        }
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[WebRTC Peer] Encode error: {:?}", e);
                                }
                            }

                            let elapsed = last_send.elapsed();
                            if elapsed < frame_duration {
                                tokio::time::sleep(frame_duration - elapsed).await;
                            }
                            last_send = std::time::Instant::now();
                        }

                        println!("[WebRTC Peer] Connection #{} ended after {} frames",
                                 connection_count, frames_sent);
                    }
                    Err(e) => {
                        eprintln!("[WebRTC Peer] Connection error: {}", e);
                    }
                }

                // Clean up and wait before reconnecting
                let _ = peer.disconnect().await;
                let _ = status_tx.send("Waiting");

                if running_clone.load(Ordering::SeqCst) {
                    println!("[WebRTC Peer] Waiting for next connection...");
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        });
    });

    // Main monitoring loop
    let mut frame_count: u64 = 0;
    let mut current_status = "Waiting";

    while running.load(Ordering::SeqCst) {
        frame_count += 1;

        // Check for status updates from the peer thread
        while let Ok(status) = status_rx.try_recv() {
            current_status = status;
        }

        // Print status every second
        if frame_count % 10 == 0 {
            let seconds = frame_count / 10;
            print!("\r[{}s] Status: {}          ", seconds, current_status);
            std::io::Write::flush(&mut std::io::stdout()).unwrap();
        }

        thread::sleep(Duration::from_millis(100));
    }

    println!();
    println!();
    println!("Cleaning up...");

    unsafe {
        BASS_Free();
    }
    println!("[OK] BASS freed");

    println!();
    println!("Done!");
}
