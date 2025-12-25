//! Output stream: BASS channel -> WebRTC (send audio to all browser peers).
//!
//! High-priority TX thread reads from BASS, encodes to OPUS, and broadcasts
//! via the shared TrackLocalStaticSample to all connected peers.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use bytes::Bytes;
use webrtc::media::Sample;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;

use crate::codec::opus::{Encoder as OpusEncoder, OPUS_APPLICATION_AUDIO};
use crate::codec::AudioFormat;
use crate::ffi::bass::{BASS_ChannelGetData, BASS_DATA_FLOAT, DWORD, HSTREAM};

/// Frame duration for WebRTC (20ms is standard)
const FRAME_DURATION_MS: u32 = 20;

/// Output stream statistics
#[derive(Default)]
pub struct OutputStats {
    pub packets_sent: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub encode_errors: AtomicU64,
    pub underruns: AtomicU64,
}

/// Output stream: reads from BASS channel and sends to WebRTC.
pub struct WebRtcOutputStream {
    /// BASS source channel to read from
    source_channel: HSTREAM,
    /// Shared audio track (broadcasts to all peers)
    shared_track: Arc<TrackLocalStaticSample>,
    /// Sample rate (48000)
    sample_rate: u32,
    /// Channels (2 for stereo)
    channels: u16,
    /// OPUS bitrate in bps
    bitrate: u32,
    /// Running flag
    running: Arc<AtomicBool>,
    /// TX thread handle
    tx_thread: Option<JoinHandle<()>>,
    /// Statistics
    pub stats: Arc<OutputStats>,
    /// Tokio runtime handle for async sample writing
    runtime: tokio::runtime::Handle,
}

impl WebRtcOutputStream {
    /// Create a new output stream.
    ///
    /// # Arguments
    /// * `source_channel` - BASS channel to read audio from
    /// * `shared_track` - Shared TrackLocalStaticSample for broadcasting
    /// * `sample_rate` - Sample rate (should be 48000 for WebRTC)
    /// * `channels` - Number of channels (1 or 2)
    /// * `bitrate` - OPUS bitrate in kbps (e.g., 128)
    /// * `runtime` - Tokio runtime handle
    pub fn new(
        source_channel: HSTREAM,
        shared_track: Arc<TrackLocalStaticSample>,
        sample_rate: u32,
        channels: u16,
        bitrate_kbps: u32,
        runtime: tokio::runtime::Handle,
    ) -> Self {
        Self {
            source_channel,
            shared_track,
            sample_rate,
            channels,
            bitrate: bitrate_kbps * 1000,
            running: Arc::new(AtomicBool::new(false)),
            tx_thread: None,
            stats: Arc::new(OutputStats::default()),
            runtime,
        }
    }

    /// Start the output stream.
    pub fn start(&mut self) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("Output stream already running".to_string());
        }

        self.running.store(true, Ordering::SeqCst);

        let source_channel = self.source_channel;
        let shared_track = self.shared_track.clone();
        let sample_rate = self.sample_rate;
        let channels = self.channels;
        let bitrate = self.bitrate;
        let running = self.running.clone();
        let stats = self.stats.clone();
        let runtime = self.runtime.clone();

        let handle = thread::Builder::new()
            .name("webrtc-output-tx".to_string())
            .spawn(move || {
                // Set thread priority (platform-specific)
                #[cfg(windows)]
                unsafe {
                    use windows_sys::Win32::System::Threading::{
                        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
                    };
                    SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
                }

                #[cfg(target_os = "linux")]
                unsafe {
                    extern "C" {
                        fn nice(inc: i32) -> i32;
                    }
                    nice(-20);
                }

                // Create OPUS encoder
                let format = AudioFormat::new(sample_rate, channels as u8);
                let mut encoder = match OpusEncoder::new(format, FRAME_DURATION_MS as f32, OPUS_APPLICATION_AUDIO) {
                    Ok(mut e) => {
                        let _ = e.set_bitrate(bitrate as i32);
                        e
                    }
                    Err(e) => {
                        eprintln!("Failed to create OPUS encoder: {}", e);
                        return;
                    }
                };

                let samples_per_frame = encoder.total_samples_per_frame();
                let bytes_per_frame = samples_per_frame * 4; // 4 bytes per float sample

                // Audio buffer for BASS_ChannelGetData
                let mut audio_buffer = vec![0.0f32; samples_per_frame];
                // OPUS output buffer
                let mut opus_buffer = vec![0u8; 4000];

                let frame_duration = Duration::from_millis(FRAME_DURATION_MS as u64);
                let mut next_tx = Instant::now() + frame_duration;

                while running.load(Ordering::SeqCst) {
                    // Wait until next TX time with precision timing
                    let now = Instant::now();
                    if next_tx > now {
                        let sleep_time = next_tx - now;
                        if sleep_time > Duration::from_millis(2) {
                            thread::sleep(sleep_time - Duration::from_millis(1));
                        }
                        // Spin-wait for final precision
                        while Instant::now() < next_tx {
                            std::hint::spin_loop();
                        }
                    }

                    // Read samples from BASS channel
                    let bytes_read = unsafe {
                        BASS_ChannelGetData(
                            source_channel,
                            audio_buffer.as_mut_ptr() as *mut std::ffi::c_void,
                            bytes_per_frame as DWORD | BASS_DATA_FLOAT,
                        )
                    };

                    // Handle underrun
                    if bytes_read == 0xFFFFFFFF || bytes_read == 0 {
                        audio_buffer.fill(0.0);
                        stats.underruns.fetch_add(1, Ordering::Relaxed);
                    }

                    // Encode to OPUS
                    match encoder.encode_float(&audio_buffer, &mut opus_buffer) {
                        Ok(encoded_len) => {
                            let data = Bytes::copy_from_slice(&opus_buffer[..encoded_len]);

                            // Send via shared track (async, but we block on it)
                            let track = shared_track.clone();
                            let sample = Sample {
                                data,
                                duration: frame_duration,
                                ..Default::default()
                            };

                            // Use runtime to send the sample
                            let _ = runtime.block_on(async {
                                track.write_sample(&sample).await
                            });

                            stats.packets_sent.fetch_add(1, Ordering::Relaxed);
                            stats.bytes_sent.fetch_add(encoded_len as u64, Ordering::Relaxed);
                        }
                        Err(e) => {
                            stats.encode_errors.fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    // Schedule next TX
                    next_tx += frame_duration;

                    // Reset if fallen too far behind
                    if Instant::now() > next_tx + frame_duration {
                        next_tx = Instant::now() + frame_duration;
                    }
                }
            })
            .map_err(|e| format!("Failed to spawn TX thread: {}", e))?;

        self.tx_thread = Some(handle);
        Ok(())
    }

    /// Stop the output stream.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);

        if let Some(handle) = self.tx_thread.take() {
            let _ = handle.join();
        }
    }

    /// Check if running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Get statistics snapshot
    pub fn get_stats(&self) -> OutputStatsSnapshot {
        OutputStatsSnapshot {
            packets_sent: self.stats.packets_sent.load(Ordering::Relaxed),
            bytes_sent: self.stats.bytes_sent.load(Ordering::Relaxed),
            encode_errors: self.stats.encode_errors.load(Ordering::Relaxed),
            underruns: self.stats.underruns.load(Ordering::Relaxed),
        }
    }
}

impl Drop for WebRtcOutputStream {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Statistics snapshot
#[derive(Debug, Clone, Default)]
pub struct OutputStatsSnapshot {
    pub packets_sent: u64,
    pub bytes_sent: u64,
    pub encode_errors: u64,
    pub underruns: u64,
}
