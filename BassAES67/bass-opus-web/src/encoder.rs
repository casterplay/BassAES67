//! Opus web encoder - pulls PCM from BASS, encodes to Opus, delivers via callback.
//!
//! Lock-free design: encoder thread reads from BASS and encodes at precise
//! 5ms intervals. No Mutex in the audio path.

use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::codec::opus;
use crate::ffi::{DWORD, BASS_DATA_FLOAT, BASS_ChannelGetData};

/// Configuration for the Opus web encoder.
#[derive(Clone)]
pub struct EncoderConfig {
    /// Sample rate (must be 48000 for Opus)
    pub sample_rate: u32,
    /// Number of channels (1 or 2)
    pub channels: u16,
    /// Opus bitrate in kbps (64-256 typical)
    pub bitrate_kbps: u32,
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            channels: 2,
            bitrate_kbps: 128,
        }
    }
}

/// Statistics for the encoder (atomic for lock-free access).
pub(crate) struct AtomicStats {
    pub frames_encoded: AtomicU64,
    pub samples_processed: AtomicU64,
    pub underruns: AtomicU64,
    pub callback_errors: AtomicU64,
}

impl AtomicStats {
    pub fn new() -> Self {
        Self {
            frames_encoded: AtomicU64::new(0),
            samples_processed: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            callback_errors: AtomicU64::new(0),
        }
    }
}

/// Statistics snapshot for external access.
#[derive(Debug, Default, Clone)]
pub struct EncoderStats {
    /// Total Opus frames encoded
    pub frames_encoded: u64,
    /// Total PCM samples processed
    pub samples_processed: u64,
    /// Buffer underruns (not enough samples from source)
    pub underruns: u64,
    /// Callback delivery errors (no callback registered)
    pub callback_errors: u64,
}

/// Opus web encoder - lock-free design.
/// Reads PCM from a BASS channel, encodes to Opus, delivers via callback.
pub struct OpusWebEncoder {
    /// Running flag (shared with thread)
    running: Arc<AtomicBool>,
    /// Statistics (atomic for lock-free access)
    stats: Arc<AtomicStats>,
    /// Encoder thread handle
    encoder_thread: Option<JoinHandle<()>>,
    /// Configuration (saved for reference)
    config: EncoderConfig,
    /// BASS channel handle
    source_channel: DWORD,
}

impl OpusWebEncoder {
    /// Create a new Opus web encoder.
    pub fn new(source_channel: DWORD, config: EncoderConfig) -> Result<Self, String> {
        // Validate configuration
        if config.sample_rate != 48000 {
            return Err("Opus requires 48000 Hz sample rate".to_string());
        }
        if config.channels < 1 || config.channels > 2 {
            return Err("Opus requires 1 or 2 channels".to_string());
        }

        Ok(Self {
            running: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(AtomicStats::new()),
            encoder_thread: None,
            config,
            source_channel,
        })
    }

    /// Start the encoder.
    pub fn start(&mut self) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Err("Encoder already running".to_string());
        }

        self.running.store(true, Ordering::SeqCst);

        // Clone shared state for thread
        let running = self.running.clone();
        let stats = self.stats.clone();
        let source_channel = self.source_channel;
        let bitrate_kbps = self.config.bitrate_kbps;

        // Spawn encoder thread
        let handle = thread::Builder::new()
            .name("opus-web-encoder".to_string())
            .spawn(move || {
                Self::encoder_loop(running, stats, source_channel, bitrate_kbps);
            })
            .map_err(|e| format!("Thread spawn failed: {}", e))?;

        self.encoder_thread = Some(handle);
        Ok(())
    }

    /// Stop the encoder.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);

        if let Some(thread) = self.encoder_thread.take() {
            let _ = thread.join();
        }
    }

    /// Encoder thread - reads from BASS and encodes at precise 5ms intervals.
    fn encoder_loop(
        running: Arc<AtomicBool>,
        stats: Arc<AtomicStats>,
        source_channel: DWORD,
        bitrate_kbps: u32,
    ) {
        // Set thread priority high for better timing (Linux)
        #[cfg(target_os = "linux")]
        {
            unsafe {
                libc::nice(-20);
            }
        }

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

        // Create Opus encoder (5ms frames = 240 samples/channel at 48kHz)
        let mut opus_encoder = match opus::Encoder::new_audio_48k_stereo_5ms() {
            Ok(mut e) => {
                if let Err(err) = e.set_bitrate(bitrate_kbps as i32 * 1000) {
                    eprintln!("[OpusWeb] Failed to set bitrate: {}", err);
                }
                e
            }
            Err(e) => {
                eprintln!("[OpusWeb] Failed to create Opus encoder: {}", e);
                return;
            }
        };

        // 5ms at 48kHz = 240 samples per channel, 480 total for stereo
        let frame_samples = opus_encoder.total_samples_per_frame();
        let mut audio_buffer = vec![0.0f32; frame_samples];
        let mut opus_buffer = vec![0u8; 4000]; // Max Opus frame size
        let bytes_needed = (frame_samples * 4) as DWORD;

        // 5ms frame interval
        let frame_interval = Duration::from_micros(5000);
        let mut next_encode = Instant::now() + frame_interval;
        let mut timestamp_ms: u64 = 0;

        println!("[OpusWeb] Encoder started: {}kbps, {} samples/frame", bitrate_kbps, frame_samples);

        while running.load(Ordering::SeqCst) {
            // Precision wait (sleep + spin)
            let now = Instant::now();
            if next_encode > now {
                let sleep_time = next_encode - now;
                if sleep_time > Duration::from_millis(2) {
                    thread::sleep(sleep_time - Duration::from_millis(1));
                }
                while Instant::now() < next_encode {
                    std::hint::spin_loop();
                }
            }

            let target_time = next_encode;

            // Pull PCM from BASS (no mutex, direct call)
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
                if samples_read < frame_samples {
                    // Partial read - fill rest with silence
                    for i in samples_read..frame_samples {
                        audio_buffer[i] = 0.0;
                    }
                    if samples_read == 0 {
                        stats.underruns.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }

            // Encode to Opus
            match opus_encoder.encode_float(&audio_buffer, &mut opus_buffer) {
                Ok(encoded_len) if encoded_len > 0 => {
                    // Deliver via callback
                    Self::deliver_frame(&opus_buffer[..encoded_len], timestamp_ms, &stats);
                    stats.frames_encoded.fetch_add(1, Ordering::Relaxed);
                    stats.samples_processed.fetch_add(frame_samples as u64, Ordering::Relaxed);
                }
                Ok(_) => {
                    // No output (shouldn't happen with 5ms frames)
                }
                Err(e) => {
                    eprintln!("[OpusWeb] Encode error: {}", e);
                }
            }

            timestamp_ms += 5; // 5ms per frame

            // Schedule next encode
            next_encode = target_time + frame_interval;

            // Reset if fallen too far behind
            if Instant::now() > next_encode + frame_interval {
                next_encode = Instant::now() + frame_interval;
            }
        }

        println!("[OpusWeb] Encoder stopped");
    }

    /// Deliver frame to registered callback.
    fn deliver_frame(data: &[u8], timestamp_ms: u64, stats: &Arc<AtomicStats>) {
        let callback_ptr = crate::FRAME_CALLBACK.load(Ordering::Acquire);
        let user_ptr = crate::FRAME_CALLBACK_USER.load(Ordering::Acquire);

        if !callback_ptr.is_null() {
            let callback: crate::OpusFrameCallback =
                unsafe { std::mem::transmute(callback_ptr) };
            callback(data.as_ptr(), data.len() as u32, timestamp_ms, user_ptr);
        } else {
            stats.callback_errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Get current statistics (lock-free snapshot).
    pub fn stats(&self) -> EncoderStats {
        EncoderStats {
            frames_encoded: self.stats.frames_encoded.load(Ordering::Relaxed),
            samples_processed: self.stats.samples_processed.load(Ordering::Relaxed),
            underruns: self.stats.underruns.load(Ordering::Relaxed),
            callback_errors: self.stats.callback_errors.load(Ordering::Relaxed),
        }
    }

    /// Check if encoder is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Drop for OpusWebEncoder {
    fn drop(&mut self) {
        self.stop();
    }
}

unsafe impl Send for OpusWebEncoder {}
