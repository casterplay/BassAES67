//! cpal audio output example for BASS AES67 plugin.
//!
//! Usage: cargo run --example cpal_output
//!
//! This example demonstrates using cpal to output audio from AES67
//! when BASS has no soundcard (device 0). Audio flows:
//!
//! AES67 Network → BASS decode → Ring buffer → cpal → Soundcard

use std::ffi::{c_char, c_void, CString};
use std::ptr;
use std::sync::atomic::{AtomicU64, AtomicPtr, Ordering};
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{traits::*, HeapRb};

// BASS types
type DWORD = u32;
type BOOL = i32;
type HSTREAM = DWORD;
type HPLUGIN = DWORD;

const FALSE: BOOL = 0;

// BASS functions
#[link(name = "bass")]
extern "system" {
    fn BASS_Init(device: i32, freq: DWORD, flags: DWORD, win: *mut c_void, dsguid: *const c_void) -> BOOL;
    fn BASS_Free() -> BOOL;
    fn BASS_GetVersion() -> DWORD;
    fn BASS_ErrorGetCode() -> i32;
    fn BASS_PluginLoad(file: *const i8, flags: DWORD) -> HPLUGIN;
    fn BASS_StreamCreateURL(url: *const i8, offset: DWORD, flags: DWORD, proc: *const c_void, user: *mut c_void) -> HSTREAM;
    fn BASS_ChannelGetData(handle: DWORD, buffer: *mut c_void, length: DWORD) -> DWORD;
    fn BASS_ChannelStop(handle: DWORD) -> BOOL;
    fn BASS_ChannelIsActive(handle: DWORD) -> DWORD;
    fn BASS_StreamFree(handle: HSTREAM) -> BOOL;
    fn BASS_PluginFree(handle: HPLUGIN) -> BOOL;
    fn BASS_SetConfig(option: DWORD, value: DWORD) -> BOOL;
    fn BASS_SetConfigPtr(option: DWORD, value: *const c_void) -> BOOL;
    fn BASS_GetConfig(option: DWORD) -> DWORD;
    fn BASS_Update(length: DWORD) -> BOOL;
}

// bass_ptp function types (loaded dynamically)
type PtpStartFn = unsafe extern "C" fn(*const c_char, u8) -> i32;
type PtpStopFn = unsafe extern "C" fn() -> i32;
type PtpGetStatsStringFn = unsafe extern "C" fn(*mut c_char, i32) -> i32;
type PtpIsLockedFn = unsafe extern "C" fn() -> i32;
type PtpGetFrequencyPpmFn = unsafe extern "C" fn() -> f64;
type PtpTimerStartFn = unsafe extern "C" fn(u32, Option<TimerCallback>, *mut c_void) -> i32;
type PtpTimerStopFn = unsafe extern "C" fn() -> i32;
type PtpTimerSetPllFn = unsafe extern "C" fn(i32) -> i32;

type TimerCallback = unsafe extern "C" fn(*mut c_void);

// Windows API for dynamic loading
#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(lpLibFileName: *const u16) -> *mut c_void;
    fn GetProcAddress(hModule: *mut c_void, lpProcName: *const i8) -> *mut c_void;
}

#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct PtpFunctions {
    start: PtpStartFn,
    stop: PtpStopFn,
    get_stats_string: PtpGetStatsStringFn,
    is_locked: PtpIsLockedFn,
    get_frequency_ppm: PtpGetFrequencyPpmFn,
    timer_start: PtpTimerStartFn,
    timer_stop: PtpTimerStopFn,
    timer_set_pll: PtpTimerSetPllFn,
}

#[cfg(windows)]
unsafe fn load_ptp_dll() -> Option<PtpFunctions> {
    let handle = LoadLibraryW(to_wide("bass_ptp.dll").as_ptr());
    if handle.is_null() {
        return None;
    }

    macro_rules! load_fn {
        ($name:expr, $ty:ty) => {{
            let ptr = GetProcAddress(handle, concat!($name, "\0").as_ptr() as *const i8);
            if ptr.is_null() {
                return None;
            }
            std::mem::transmute::<*mut c_void, $ty>(ptr)
        }};
    }

    Some(PtpFunctions {
        start: load_fn!("BASS_PTP_Start", PtpStartFn),
        stop: load_fn!("BASS_PTP_Stop", PtpStopFn),
        get_stats_string: load_fn!("BASS_PTP_GetStatsString", PtpGetStatsStringFn),
        is_locked: load_fn!("BASS_PTP_IsLocked", PtpIsLockedFn),
        get_frequency_ppm: load_fn!("BASS_PTP_GetFrequencyPPM", PtpGetFrequencyPpmFn),
        timer_start: load_fn!("BASS_PTP_TimerStart", PtpTimerStartFn),
        timer_stop: load_fn!("BASS_PTP_TimerStop", PtpTimerStopFn),
        timer_set_pll: load_fn!("BASS_PTP_TimerSetPLL", PtpTimerSetPllFn),
    })
}

#[cfg(not(windows))]
unsafe fn load_ptp_dll() -> Option<PtpFunctions> {
    None
}

// Config constants
const BASS_CONFIG_BUFFER: DWORD = 0;
const BASS_CONFIG_UPDATEPERIOD: DWORD = 6;
const BASS_STREAM_DECODE: DWORD = 0x200000;
const BASS_DATA_FLOAT: DWORD = 0x40000000;
const BASS_DATA_AVAILABLE: DWORD = 0;

// AES67 config options
const BASS_CONFIG_AES67_INTERFACE: DWORD = 0x20001;
const BASS_CONFIG_AES67_JITTER: DWORD = 0x20002;
const BASS_CONFIG_AES67_PTP_DOMAIN: DWORD = 0x20003;
const BASS_CONFIG_AES67_BUFFER_LEVEL: DWORD = 0x20010;  // Get buffer fill % (0-200, 100=target)
const BASS_CONFIG_AES67_JITTER_UNDERRUNS: DWORD = 0x20011;  // Get jitter buffer underrun count

// Channel states
const BASS_ACTIVE_STOPPED: DWORD = 0;

// Ring buffer type: producer half for timer, consumer half for cpal
// HeapRb's producer/consumer are lock-free SPSC - NO MUTEX NEEDED
type RingBufProducer = ringbuf::HeapProd<f32>;
type RingBufConsumer = ringbuf::HeapCons<f32>;

// Global pointers - both sides lock-free
static PRODUCER_PTR: AtomicPtr<RingBufProducer> = AtomicPtr::new(ptr::null_mut());
static CONSUMER_PTR: AtomicPtr<RingBufConsumer> = AtomicPtr::new(ptr::null_mut());

// Store stream handle globally for cpal callback to access BASS directly
static STREAM_HANDLE: AtomicU64 = AtomicU64::new(0);

// Flag for direct mode (cpal reads from BASS directly)
static DIRECT_MODE: AtomicBool = AtomicBool::new(false);

use std::sync::atomic::AtomicBool;

// Statistics
static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);
static SAMPLES_PUSHED: AtomicU64 = AtomicU64::new(0);
static SAMPLES_PULLED: AtomicU64 = AtomicU64::new(0);
static UNDERRUNS: AtomicU64 = AtomicU64::new(0);
static EMPTY_READS: AtomicU64 = AtomicU64::new(0);
static CPAL_CALLBACK_SIZE: AtomicU64 = AtomicU64::new(0);
static CPAL_CALLBACKS: AtomicU64 = AtomicU64::new(0);
static NONZERO_SAMPLES: AtomicU64 = AtomicU64::new(0);
static TOTAL_CHECKED: AtomicU64 = AtomicU64::new(0);

// Diagnostic: track how many bytes we get vs requested
static BYTES_GOT: AtomicU64 = AtomicU64::new(0);
static BYTES_REQUESTED: AtomicU64 = AtomicU64::new(0);

// Current resample ratio for display (stored as bits for atomic access)
static CURRENT_RATIO_BITS: AtomicU64 = AtomicU64::new(0);

// Current jitter buffer level for display (0-200, 100 = at target)
static BUFFER_LEVEL_PCT: AtomicU64 = AtomicU64::new(100);

// Store PTP function pointers globally for cpal callback access
static PTP_IS_LOCKED: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static PTP_GET_FREQ_PPM: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

/// PI controller for buffer-level based drift compensation
/// Based on JACK alsa_out.c algorithm with adaptations for our use case
struct BufferLevelController {
    /// Target buffer level (1.0 = at jitter buffer target)
    target: f64,
    /// Proportional gain (based on JACK catch_factor)
    kp: f64,
    /// Integral gain (based on JACK catch_factor * catch_factor2)
    ki: f64,
    /// Accumulated integral
    integral: f64,
    /// Current ratio output
    ratio: f64,
    /// Smoothed buffer level (low-pass filtered)
    smooth_level: f64,
    /// Smoothing coefficient (0.0-1.0, lower = more smoothing)
    smooth_alpha: f64,
}

impl BufferLevelController {
    fn new() -> Self {
        Self {
            target: 1.0,         // Target 100% (at target level from jitter buffer)
            // Increased gains for faster response to buffer level changes
            kp: 0.0005,          // P gain: 50ppm per 10% error
            ki: 0.00001,         // I gain: 1ppm per 10% error per update
            integral: 0.0,
            ratio: 1.0,
            smooth_level: 1.0,
            smooth_alpha: 0.05,  // Slower smoothing for more stability
        }
    }

    /// Update with current buffer level (1.0 = at target, <1.0 = draining, >1.0 = filling)
    fn update(&mut self, level: f64) -> f64 {
        // Low-pass filter the buffer level to reduce jitter
        self.smooth_level = self.smooth_alpha * level + (1.0 - self.smooth_alpha) * self.smooth_level;

        let error = self.smooth_level - self.target;

        // Small dead-zone to prevent chatter
        let error = if error.abs() < 0.05 { 0.0 } else { error };  // 5% dead-zone

        // PI control
        self.integral += error * self.ki;
        self.integral = self.integral.clamp(-0.0005, 0.0005);  // ±500ppm max from I

        let adjustment = error * self.kp + self.integral;
        self.ratio = (1.0 + adjustment).clamp(0.9995, 1.0005);  // ±500ppm max total

        self.ratio
    }
}

/// Simple stateless linear interpolation for stretching/compressing audio
/// Designed for very small ratio adjustments (±500ppm)
fn resample_linear(input: &[f32], output: &mut [f32]) {
    let in_frames = input.len() / 2;
    let out_frames = output.len() / 2;

    if in_frames == 0 || out_frames == 0 {
        return;
    }

    // Calculate ratio: how many input frames per output frame
    let ratio = (in_frames as f64) / (out_frames as f64);

    for out_frame in 0..out_frames {
        // Calculate input position for this output frame
        let in_pos = (out_frame as f64) * ratio;
        let in_frame = in_pos.floor() as usize;
        let t = in_pos.fract() as f32;

        // Get samples with bounds checking
        let idx = in_frame * 2;
        let (c_l, c_r) = if idx + 1 < input.len() {
            (input[idx], input[idx + 1])
        } else {
            (0.0, 0.0)
        };

        let (n_l, n_r) = if idx + 3 < input.len() {
            (input[idx + 2], input[idx + 3])
        } else {
            (c_l, c_r)  // No next frame, use current
        };

        // Linear interpolation
        output[out_frame * 2] = c_l + (n_l - c_l) * t;
        output[out_frame * 2 + 1] = c_r + (n_r - c_r) * t;
    }
}

/// Dummy struct to satisfy existing code structure
struct LinearResampler;
impl LinearResampler {
    fn new() -> Self { Self }
}

// Global controllers - protected by mutex for thread safety
use std::sync::Mutex;
static BUFFER_CONTROLLER: Mutex<Option<BufferLevelController>> = Mutex::new(None);
static RESAMPLER: Mutex<Option<LinearResampler>> = Mutex::new(None);

// Timer callback - called every 5ms by bass_ptp timer
// Pulls audio from BASS and pushes to ring buffer (lock-free)
unsafe extern "C" fn timer_callback(user: *mut c_void) {
    TIMER_TICKS.fetch_add(1, Ordering::Relaxed);

    let stream = *(user as *const DWORD);
    if stream == 0 {
        return;
    }

    // Buffer for 5ms of audio @ 48kHz stereo = 480 floats
    let mut buffer = [0f32; 480];
    let requested = (buffer.len() * 4) as DWORD;

    let bytes = BASS_ChannelGetData(
        stream,
        buffer.as_mut_ptr() as *mut c_void,
        requested | BASS_DATA_FLOAT,
    );

    BYTES_REQUESTED.fetch_add(requested as u64, Ordering::Relaxed);

    if bytes == 0xFFFFFFFF {
        EMPTY_READS.fetch_add(1, Ordering::Relaxed);
        return;
    }

    // BASS returns actual bytes written - use ONLY that amount
    let actual_bytes = bytes as usize;
    if actual_bytes == 0 {
        EMPTY_READS.fetch_add(1, Ordering::Relaxed);
        return;
    }

    BYTES_GOT.fetch_add(actual_bytes as u64, Ordering::Relaxed);

    let samples = actual_bytes / 4;
    TOTAL_CHECKED.fetch_add(samples as u64, Ordering::Relaxed);

    // Count non-zero samples for diagnostics (only in returned portion)
    let nonzero = buffer[..samples].iter().filter(|&&x| x != 0.0).count();
    NONZERO_SAMPLES.fetch_add(nonzero as u64, Ordering::Relaxed);

    // Push exactly what BASS gave us to the ring buffer
    let producer_ptr = PRODUCER_PTR.load(Ordering::Acquire);
    if !producer_ptr.is_null() {
        let producer = &mut *producer_ptr;
        let pushed = producer.push_slice(&buffer[..samples]);
        SAMPLES_PUSHED.fetch_add(pushed as u64, Ordering::Relaxed);
    }
}

fn main() {
    println!("BASS AES67 cpal Output Example");
    println!("==============================\n");

    // Initialize cpal first to check audio device availability
    println!("Initializing cpal audio output...");
    let host = cpal::default_host();

    let device = match host.default_output_device() {
        Some(d) => {
            println!("  Output device: {}", d.name().unwrap_or("Unknown".to_string()));
            d
        }
        None => {
            println!("ERROR: No audio output device found");
            return;
        }
    };

    // Get supported config
    let supported_config = match device.default_output_config() {
        Ok(c) => {
            println!("  Device native sample rate: {} Hz", c.sample_rate().0);
            println!("  Device native channels: {}", c.channels());
            println!("  Device native sample format: {:?}", c.sample_format());
            println!("  Device native buffer size: {:?}", c.buffer_size());
            c
        }
        Err(e) => {
            println!("ERROR: Failed to get output config: {}", e);
            return;
        }
    };

    // CRITICAL: Use the device's native sample rate, not 48000
    let device_sample_rate = supported_config.sample_rate().0;
    let device_channels = supported_config.channels() as usize;

    if device_sample_rate != 48000 {
        println!("\n  *** WARNING: Device sample rate ({} Hz) differs from AES67 (48000 Hz) ***", device_sample_rate);
        println!("  *** This WILL cause choppy audio! Sample rate conversion needed. ***\n");
    }
    if device_channels != 2 {
        println!("\n  *** WARNING: Device has {} channels, AES67 has 2 ***\n", device_channels);
    }

    // Create ring buffer - large enough to handle clock drift and jitter
    // 48kHz * 2 channels * 2s = 192000 samples (2 second capacity)
    // Note: For production AES67_IN → BASS → AES67_OUT, no cpal is needed
    let ring_buf = HeapRb::<f32>::new(192000);
    let (producer, consumer) = ring_buf.split();

    // Box both halves so we can get stable pointers - completely lock-free!
    let mut producer_box = Box::new(producer);
    let mut consumer_box = Box::new(consumer);

    let producer_ptr = &mut *producer_box as *mut RingBufProducer;
    let consumer_ptr = &mut *consumer_box as *mut RingBufConsumer;

    PRODUCER_PTR.store(producer_ptr, Ordering::Release);
    CONSUMER_PTR.store(consumer_ptr, Ordering::Release);

    println!("  Ring buffer: 2000ms (192000 samples) - lock-free");

    // Build cpal output stream
    let config = cpal::StreamConfig {
        channels: 2,
        sample_rate: cpal::SampleRate(48000),
        buffer_size: cpal::BufferSize::Default,
    };

    let cpal_stream = match device.build_output_stream(
        &config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            CPAL_CALLBACKS.fetch_add(1, Ordering::Relaxed);
            CPAL_CALLBACK_SIZE.store(data.len() as u64, Ordering::Relaxed);

            // Direct mode: read from BASS directly in cpal callback
            // WORKING VERSION - simple 1:1 read, no resampling
            // TODO: Add proper resampling for clock drift compensation
            if DIRECT_MODE.load(Ordering::Relaxed) {
                let stream = STREAM_HANDLE.load(Ordering::Relaxed) as DWORD;
                if stream != 0 {
                    // Get buffer level for display
                    let buffer_raw = unsafe { BASS_GetConfig(BASS_CONFIG_AES67_BUFFER_LEVEL) };
                    BUFFER_LEVEL_PCT.store(buffer_raw as u64, Ordering::Relaxed);

                    // For now, just store 1.0 ratio (no resampling)
                    CURRENT_RATIO_BITS.store(1.0f64.to_bits(), Ordering::Relaxed);

                    // Read exactly what cpal needs directly into output buffer
                    let bytes = unsafe {
                        BASS_ChannelGetData(
                            stream,
                            data.as_mut_ptr() as *mut c_void,
                            (data.len() * 4) as DWORD | BASS_DATA_FLOAT,
                        )
                    };

                    if bytes == 0xFFFFFFFF {
                        data.fill(0.0);
                        UNDERRUNS.fetch_add(1, Ordering::Relaxed);
                    } else {
                        let got_samples = (bytes / 4) as usize;

                        // Fill remainder with silence if needed
                        if got_samples < data.len() {
                            data[got_samples..].fill(0.0);
                        }

                        SAMPLES_PULLED.fetch_add(got_samples as u64, Ordering::Relaxed);

                        // Count non-zero samples
                        let nonzero = data[..got_samples].iter().filter(|&&x| x != 0.0).count();
                        NONZERO_SAMPLES.fetch_add(nonzero as u64, Ordering::Relaxed);
                        TOTAL_CHECKED.fetch_add(got_samples as u64, Ordering::Relaxed);
                    }
                } else {
                    data.fill(0.0);
                }
                return;
            }

            // Ring buffer mode (used during pre-buffer)
            let consumer_ptr = CONSUMER_PTR.load(Ordering::Acquire);
            if !consumer_ptr.is_null() {
                // SAFETY: This is the only thread accessing consumer
                let consumer = unsafe { &mut *consumer_ptr };
                let available = consumer.occupied_len();
                let pulled = consumer.pop_slice(data);
                SAMPLES_PULLED.fetch_add(pulled as u64, Ordering::Relaxed);

                // Fill remaining with silence if underrun
                if pulled < data.len() {
                    data[pulled..].fill(0.0);
                    if available == 0 {
                        UNDERRUNS.fetch_add(1, Ordering::Relaxed);
                    }
                }
            } else {
                data.fill(0.0);
            }
        },
        |err| {
            eprintln!("cpal error: {}", err);
        },
        None,
    ) {
        Ok(s) => s,
        Err(e) => {
            println!("ERROR: Failed to build output stream: {}", e);
            return;
        }
    };

    // cpal stream created - will start after pre-buffering
    println!("  cpal stream created (will start after pre-buffer)\n");

    unsafe {
        // Load bass_ptp.dll
        println!("Loading bass_ptp.dll...");
        let ptp = match load_ptp_dll() {
            Some(p) => {
                println!("  bass_ptp.dll loaded");

                // Store PTP function pointers globally for cpal callback access
                PTP_IS_LOCKED.store(p.is_locked as *mut c_void, Ordering::Release);
                PTP_GET_FREQ_PPM.store(p.get_frequency_ppm as *mut c_void, Ordering::Release);

                // Initialize buffer-level controller and resampler for drift compensation
                if let Ok(mut ctrl) = BUFFER_CONTROLLER.lock() {
                    *ctrl = Some(BufferLevelController::new());
                }
                if let Ok(mut resampler) = RESAMPLER.lock() {
                    *resampler = Some(LinearResampler::new());
                }

                p
            }
            None => {
                println!("ERROR: Failed to load bass_ptp.dll");
                return;
            }
        };

        // Get BASS version
        let version = BASS_GetVersion();
        println!("BASS version: {}.{}.{}.{}",
            (version >> 24) & 0xFF,
            (version >> 16) & 0xFF,
            (version >> 8) & 0xFF,
            version & 0xFF);

        // Configure BASS for no-soundcard mode
        // Larger buffer to handle network jitter - 100ms
        println!("\nConfiguring BASS (no soundcard mode)...");
        BASS_SetConfig(BASS_CONFIG_BUFFER, 100);
        BASS_SetConfig(BASS_CONFIG_UPDATEPERIOD, 0);

        if BASS_Init(0, 48000, 0, ptr::null_mut(), ptr::null()) == FALSE {
            println!("ERROR: Failed to initialize BASS (error {})", BASS_ErrorGetCode());
            return;
        }
        println!("  BASS initialized (device=0)");

        // Load AES67 plugin
        let plugin_path = CString::new("bass_aes67.dll").unwrap();
        let plugin = BASS_PluginLoad(plugin_path.as_ptr(), 0);
        if plugin == 0 {
            println!("ERROR: Failed to load bass_aes67.dll (error {})", BASS_ErrorGetCode());
            BASS_Free();
            return;
        }
        println!("  bass_aes67.dll loaded");

        // Configure AES67
        // For Livewire (200pkt/sec = 5ms packets), need larger jitter buffer
        let interface = CString::new("192.168.60.102").unwrap();
        BASS_SetConfigPtr(BASS_CONFIG_AES67_INTERFACE, interface.as_ptr() as *const c_void);
        BASS_SetConfig(BASS_CONFIG_AES67_JITTER, 50);  // 50ms jitter buffer for Livewire
        BASS_SetConfig(BASS_CONFIG_AES67_PTP_DOMAIN, 1);

        // Start PTP
        let ptp_interface = CString::new("192.168.60.102").unwrap();
        let _ = (ptp.start)(ptp_interface.as_ptr(), 1);
        println!("  PTP client started");

        // Create AES67 decode stream
        println!("\nConnecting to AES67 stream...");
        let url = CString::new("aes67://239.192.76.52:5004").unwrap();
        let stream = BASS_StreamCreateURL(url.as_ptr(), 0, BASS_STREAM_DECODE, ptr::null(), ptr::null_mut());

        if stream == 0 {
            println!("ERROR: Failed to create stream (error {})", BASS_ErrorGetCode());
            (ptp.stop)();
            BASS_PluginFree(plugin);
            BASS_Free();
            return;
        }
        println!("  Stream created (handle: {})", stream);

        // Store stream handle for timer and for cpal direct mode
        let mut stream_handle = stream;
        STREAM_HANDLE.store(stream as u64, Ordering::Release);

        // Start PTP timer BEFORE cpal to pre-buffer
        // Using 5ms interval for low latency
        println!("\nStarting PTP timer (5ms)...");
        (ptp.timer_set_pll)(1);
        let timer_result = (ptp.timer_start)(
            5,
            Some(timer_callback),
            &mut stream_handle as *mut DWORD as *mut c_void,
        );

        if timer_result != 0 {
            println!("ERROR: Failed to start timer (error {})", timer_result);
            BASS_StreamFree(stream);
            (ptp.stop)();
            BASS_PluginFree(plugin);
            BASS_Free();
            return;
        }
        println!("  Timer started with PLL");

        // Pre-buffer: wait for ~1000ms (96000 samples) for stable playback
        println!("\nPre-buffering (waiting for 1000ms of audio)...");
        loop {
            let fill = {
                let consumer_ptr = CONSUMER_PTR.load(Ordering::Acquire);
                if !consumer_ptr.is_null() {
                    (*consumer_ptr).occupied_len()
                } else {
                    0
                }
            };

            if fill >= 96000 {
                println!("  Pre-buffer complete: {} samples ({:.0}ms)", fill, fill as f64 / 96.0);
                break;
            }

            thread::sleep(Duration::from_millis(10));
        }

        // Stop timer - we'll let cpal drive reading directly from now on
        (ptp.timer_stop)();
        println!("  Timer stopped - switching to direct mode");

        // Enable direct mode - cpal callback will read from BASS directly
        DIRECT_MODE.store(true, Ordering::Release);

        // NOW start cpal playback
        if let Err(e) = cpal_stream.play() {
            println!("ERROR: Failed to start playback: {}", e);
            BASS_StreamFree(stream);
            (ptp.stop)();
            BASS_PluginFree(plugin);
            BASS_Free();
            return;
        }

        println!("\n>>> Audio should now be playing through cpal (direct mode) <<<");
        println!("Press Ctrl+C to stop\n");

        // Monitor loop
        loop {
            let state = BASS_ChannelIsActive(stream);
            if state == BASS_ACTIVE_STOPPED {
                println!("\nStream ended");
                break;
            }

            // Get buffer fill level - lock-free
            let fill = {
                let consumer_ptr = CONSUMER_PTR.load(Ordering::Acquire);
                if !consumer_ptr.is_null() {
                    (*consumer_ptr).occupied_len()
                } else {
                    0
                }
            };
            let fill_pct = (fill as f64 / 192000.0) * 100.0;

            // Get PTP stats
            let mut ptp_buffer = [0i8; 256];
            (ptp.get_stats_string)(ptp_buffer.as_mut_ptr(), 256);
            let ptp_stats = std::ffi::CStr::from_ptr(ptp_buffer.as_ptr())
                .to_string_lossy()
                .into_owned();

            let ptp_locked = (ptp.is_locked)() != 0;
            let underruns = UNDERRUNS.load(Ordering::Relaxed);

            // Display status
            let empty_reads = EMPTY_READS.load(Ordering::Relaxed);
            let ticks = TIMER_TICKS.load(Ordering::Relaxed);

            let cpal_size = CPAL_CALLBACK_SIZE.load(Ordering::Relaxed);
            let cpal_calls = CPAL_CALLBACKS.load(Ordering::Relaxed);

            // Calculate production rate - how many samples/sec we're pushing
            let pushed = SAMPLES_PUSHED.load(Ordering::Relaxed);
            let pulled = SAMPLES_PULLED.load(Ordering::Relaxed);

            // Get audio quality stats (in direct mode, these are updated by cpal callback)
            let nonzero = NONZERO_SAMPLES.load(Ordering::Relaxed);
            let total = TOTAL_CHECKED.load(Ordering::Relaxed);
            let audio_pct = if total > 0 { (nonzero as f64 / total as f64) * 100.0 } else { 0.0 };

            // Get buffer level, ratio, and jitter underruns
            let buffer_level = BUFFER_LEVEL_PCT.load(Ordering::Relaxed);
            let ratio = f64::from_bits(CURRENT_RATIO_BITS.load(Ordering::Relaxed));
            let jitter_underruns = unsafe { BASS_GetConfig(BASS_CONFIG_AES67_JITTER_UNDERRUNS) };

            print!("\r\x1b[K");
            print!("Audio: {:5.1}% | Buf: {:3}% | Ratio: {:.6} | JitUnd: {} | {}",
                audio_pct,
                buffer_level,
                ratio,
                jitter_underruns,
                ptp_stats);
            use std::io::Write;
            std::io::stdout().flush().unwrap();

            thread::sleep(Duration::from_millis(500));
        }

        // Cleanup
        println!("\nCleaning up...");

        // Disable direct mode
        DIRECT_MODE.store(false, Ordering::Release);
        STREAM_HANDLE.store(0, Ordering::Release);

        // Clear pointers so callbacks won't use them
        PRODUCER_PTR.store(ptr::null_mut(), Ordering::Release);
        CONSUMER_PTR.store(ptr::null_mut(), Ordering::Release);

        BASS_ChannelStop(stream);
        BASS_StreamFree(stream);
        (ptp.stop)();
        BASS_PluginFree(plugin);
        BASS_Free();
    }

    // Keep boxes alive until after cleanup
    drop(producer_box);
    drop(consumer_box);

    // cpal_stream is dropped here, stopping playback

    // Final stats
    let pushed = SAMPLES_PUSHED.load(Ordering::Relaxed);
    let pulled = SAMPLES_PULLED.load(Ordering::Relaxed);
    let underruns = UNDERRUNS.load(Ordering::Relaxed);

    println!("\nStatistics:");
    println!("  Samples pushed: {}", pushed);
    println!("  Samples pulled: {}", pulled);
    println!("  Underruns: {}", underruns);
    println!("Done!");
}
