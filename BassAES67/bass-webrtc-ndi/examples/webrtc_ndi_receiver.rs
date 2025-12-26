//! WebRTC to NDI Receiver Example
//!
//! Connects to a WHEP endpoint (e.g., MediaMTX) and outputs:
//! - Video: to NDI (viewable in NDI Studio Monitor)
//! - Audio: to BASS channel (for playback or processing)
//!
//! Usage:
//!   cargo run --release --example webrtc_ndi_receiver -- <whep_url> [ndi_name] [--audio-to-ndi]
//!
//! Examples:
//!   # Video to NDI, audio to BASS (default)
//!   cargo run --release --example webrtc_ndi_receiver -- http://localhost:8889/mystream/whep
//!
//!   # Custom NDI source name
//!   cargo run --release --example webrtc_ndi_receiver -- http://localhost:8889/mystream/whep "My WebRTC Source"
//!
//!   # Also send audio to NDI
//!   cargo run --release --example webrtc_ndi_receiver -- http://localhost:8889/mystream/whep "My Source" --audio-to-ndi

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bass_webrtc_ndi::{
    WhepNdiClient, google_stun_servers,
    is_ffmpeg_available,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <whep_url> [ndi_name] [--audio-to-ndi]", args[0]);
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  {} http://localhost:8889/nditest/whep", args[0]);
        eprintln!("  {} http://localhost:8889/nditest/whep \"My WebRTC Source\"", args[0]);
        eprintln!("  {} http://localhost:8889/nditest/whep \"My Source\" --audio-to-ndi", args[0]);
        std::process::exit(1);
    }

    let whep_url = &args[1];
    let ndi_name = if args.len() > 2 && !args[2].starts_with("--") {
        args[2].clone()
    } else {
        "WebRTC-NDI".to_string()
    };
    let audio_to_ndi = args.iter().any(|a| a == "--audio-to-ndi");

    println!("========================================");
    println!("  WebRTC to NDI Receiver");
    println!("========================================");
    println!();
    println!("WHEP URL:      {}", whep_url);
    println!("NDI Name:      {}", ndi_name);
    println!("Audio to NDI:  {}", audio_to_ndi);
    println!();

    // Check FFmpeg availability
    if is_ffmpeg_available() {
        println!("[OK] FFmpeg video libraries available");
    } else {
        println!("[WARN] FFmpeg not found - video decoding disabled");
        println!("       Make sure avcodec-62.dll, avutil-60.dll, swscale-9.dll are in PATH");
    }
    println!();

    // Get Google STUN servers
    let ice_servers = google_stun_servers();

    println!("Connecting to WHEP endpoint...");

    // Create WHEP NDI client
    let mut client = WhepNdiClient::connect(
        whep_url,
        &ice_servers,
        48000,           // sample rate
        2,               // channels
        48000 * 2,       // buffer samples (1 second)
        Some(&ndi_name), // NDI source name
        audio_to_ndi,    // whether to send audio to NDI
    )
    .await
    .map_err(|e| format!("Failed to connect: {}", e))?;

    println!("[OK] Connected to WHEP endpoint");
    println!();

    // Set up ctrl+c handler
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    ctrlc::set_handler(move || {
        println!("\nReceived Ctrl+C, shutting down...");
        running_clone.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    println!("Receiving media... Press Ctrl+C to stop.");
    println!();
    println!("Open NDI Studio Monitor to view the video output.");
    println!();

    // Take the audio consumer (for BASS playback if needed)
    let _audio_consumer = client.take_incoming_consumer();

    // Stats display loop
    let mut last_video_frames = 0u64;
    let mut last_audio_packets = 0u64;

    while running.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_secs(1)).await;

        let stats = &client.stats;
        let video_frames = stats.video_frames_decoded.load(Ordering::Relaxed);
        let video_sent = stats.video_frames_sent_ndi.load(Ordering::Relaxed);
        let audio_packets = stats.audio_packets_received.load(Ordering::Relaxed);
        let audio_ndi = stats.audio_frames_sent_ndi.load(Ordering::Relaxed);
        let ndi_connections = client.get_ndi_connections();

        let video_fps = video_frames - last_video_frames;
        let audio_pps = audio_packets - last_audio_packets;

        println!(
            "Video: {} decoded ({} fps), {} sent to NDI | Audio: {} packets ({}/s), {} to NDI | NDI Receivers: {}",
            video_frames, video_fps, video_sent,
            audio_packets, audio_pps, audio_ndi,
            ndi_connections
        );

        last_video_frames = video_frames;
        last_audio_packets = audio_packets;
    }

    // Disconnect
    println!();
    println!("Disconnecting...");
    client.disconnect().await.map_err(|e| format!("Disconnect error: {}", e))?;
    println!("[OK] Disconnected");

    Ok(())
}
