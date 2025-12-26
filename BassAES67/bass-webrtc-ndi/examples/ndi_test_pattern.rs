//! NDI Test Pattern Example
//!
//! Sends SMPTE color bars to NDI for testing.
//! Open NDI Studio Monitor to view the output.
//!
//! Usage: cargo run --release --example ndi_test_pattern

use bass_webrtc_ndi::{NdiSender, VideoFrame, init_ndi};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    println!("bass-webrtc-ndi Test Pattern");
    println!("============================");
    println!();
    println!("This will send SMPTE color bars via NDI.");
    println!("Open 'NDI Studio Monitor' to view the output.");
    println!();
    println!("Press Ctrl+C to stop.");
    println!();

    // Setup Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        println!("\nStopping...");
        r.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl+C handler");

    // Initialize NDI
    println!("Initializing NDI...");
    let ndi = match init_ndi() {
        Ok(n) => n,
        Err(e) => {
            eprintln!("Failed to initialize NDI: {}", e);
            eprintln!();
            eprintln!("Make sure NDI SDK is installed at:");
            eprintln!("  Windows: C:\\Program Files\\NDI\\NDI 6 SDK");
            eprintln!("  Linux: /usr/share/NDI SDK for Linux");
            return;
        }
    };

    // Create NDI sender
    let source_name = "bass-webrtc-ndi Test Pattern";
    println!("Creating NDI source: {}", source_name);

    let sender = match NdiSender::new(&ndi, source_name) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to create NDI sender: {}", e);
            return;
        }
    };

    println!("NDI source created successfully!");
    println!();

    // Create test pattern frame (1920x1080 SMPTE color bars)
    let width = 1920;
    let height = 1080;
    let mut frame = VideoFrame::test_pattern_bars(width, height);
    frame.frame_rate_n = 30000;
    frame.frame_rate_d = 1001; // 29.97 fps

    println!("Sending {}x{} @ {:.2} fps", width, height, 30000.0 / 1001.0);
    println!();

    // Main loop - send frames
    let start_time = Instant::now();
    let mut frame_count: u64 = 0;
    let mut last_report = Instant::now();

    while running.load(Ordering::SeqCst) {
        // Send the frame
        if let Err(e) = sender.send_video(&frame) {
            eprintln!("Failed to send frame: {}", e);
            break;
        }

        frame_count += 1;

        // Report stats every 5 seconds
        if last_report.elapsed() >= Duration::from_secs(5) {
            let elapsed = start_time.elapsed().as_secs_f64();
            let fps = frame_count as f64 / elapsed;
            let connections = sender.connection_count();

            println!(
                "Frames: {} | FPS: {:.2} | Connections: {}",
                frame_count, fps, connections
            );

            last_report = Instant::now();
        }

        // NDI's clock_video handles timing, but we'll add a small sleep
        // to not spin the CPU when no receivers are connected
        if !sender.has_connections() {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    let elapsed = start_time.elapsed().as_secs_f64();
    println!();
    println!("Sent {} frames in {:.1} seconds ({:.2} fps average)",
             frame_count, elapsed, frame_count as f64 / elapsed);
    println!("Done!");
}
