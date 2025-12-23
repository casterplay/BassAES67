//! BASS AES67 Plugin
//!
//! This plugin provides AES67 network audio support for the BASS audio library.
//! - Input: Receive AES67 RTP multicast streams and play them through BASS
//! - Output: Extract PCM from BASS channels and transmit via AES67 RTP multicast
//!
//! Audio format notes:
//! - AES67 uses 48kHz, 24-bit linear PCM
//! - BASS works internally with 32-bit float at 48kHz
//! - We convert between 24-bit PCM and 32-bit float as needed

mod ffi;
mod input;
mod output;
mod clock_bindings;

// Re-export output module for external use
pub use output::{Aes67OutputStream, Aes67OutputConfig, OutputStats};

use std::collections::HashMap;
use std::ffi::{c_void, CStr};
use std::net::Ipv4Addr;
use std::ptr;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};

use lazy_static::lazy_static;
use parking_lot::RwLock;

use ffi::*;
use input::{Aes67Stream, Aes67Url, ADDON_FUNCS, stream::stream_proc};

// Plugin version (matches BASS version format: 0xAABBCCDD)
const VERSION: DWORD = 0x02040000;

// Track initialization state
static INITIALIZED: AtomicBool = AtomicBool::new(false);

// AES67 audio format constants
pub const AES67_SAMPLE_RATE: DWORD = 48000;  // 48kHz standard
pub const AES67_BIT_DEPTH: DWORD = 24;       // 24-bit PCM
pub const AES67_BYTES_PER_SAMPLE: DWORD = 3; // 24-bit = 3 bytes

// Config options for AES67
pub const BASS_CONFIG_AES67_PT: DWORD = 0x20000;           // Payload type (default 96)
pub const BASS_CONFIG_AES67_INTERFACE: DWORD = 0x20001;    // Network interface
pub const BASS_CONFIG_AES67_JITTER: DWORD = 0x20002;       // Jitter buffer depth in ms

// PTP config options
pub const BASS_CONFIG_AES67_PTP_DOMAIN: DWORD = 0x20003;   // PTP domain (default 0)
pub const BASS_CONFIG_AES67_PTP_STATS: DWORD = 0x20004;    // Get PTP stats string (ptr)
pub const BASS_CONFIG_AES67_PTP_OFFSET: DWORD = 0x20005;   // Get PTP offset in ns (i64)
pub const BASS_CONFIG_AES67_PTP_STATE: DWORD = 0x20006;    // Get PTP state (DWORD)
pub const BASS_CONFIG_AES67_PTP_ENABLED: DWORD = 0x20007;  // Enable/disable PTP

// Buffer level config (for adaptive sample rate control)
pub const BASS_CONFIG_AES67_BUFFER_LEVEL: DWORD = 0x20010; // Get buffer fill % (0-200, 100=target)
pub const BASS_CONFIG_AES67_JITTER_UNDERRUNS: DWORD = 0x20011; // Get jitter buffer underrun count
pub const BASS_CONFIG_AES67_PACKETS_RECEIVED: DWORD = 0x20012; // Get total packets received
pub const BASS_CONFIG_AES67_PACKETS_LATE: DWORD = 0x20013; // Get late packets dropped count
pub const BASS_CONFIG_AES67_BUFFER_PACKETS: DWORD = 0x20014; // Get current buffer level in packets
pub const BASS_CONFIG_AES67_TARGET_PACKETS: DWORD = 0x20015; // Get target buffer level in packets
pub const BASS_CONFIG_AES67_PACKET_TIME: DWORD = 0x20016; // Get detected packet time in microseconds
pub const BASS_CONFIG_AES67_PTP_LOCKED: DWORD = 0x20017;  // PTP locked status (0/1)
pub const BASS_CONFIG_AES67_PTP_FREQ: DWORD = 0x20018;    // PTP frequency in PPM × 1000
pub const BASS_CONFIG_AES67_CLOCK_MODE: DWORD = 0x20019;  // Clock mode: 0=PTP, 1=Livewire, 2=System
pub const BASS_CONFIG_AES67_CLOCK_FALLBACK_TIMEOUT: DWORD = 0x2001A; // Fallback timeout in seconds (0=disabled)

// Clock mode values
pub const BASS_AES67_CLOCK_PTP: DWORD = 0;
pub const BASS_AES67_CLOCK_LIVEWIRE: DWORD = 1;
pub const BASS_AES67_CLOCK_SYSTEM: DWORD = 2;

// Default configuration values
static mut CONFIG_PT: DWORD = 96;
static mut CONFIG_INTERFACE: [u8; 64] = [0; 64];
static mut CONFIG_JITTER_MS: DWORD = 10;
static mut CONFIG_PTP_DOMAIN: DWORD = 0;
static mut CONFIG_PTP_ENABLED: DWORD = 1; // Enabled by default
static mut CONFIG_CLOCK_MODE: DWORD = 0;  // 0=PTP (default), 1=Livewire, 2=System
static mut CONFIG_FALLBACK_TIMEOUT: DWORD = 5; // 5 seconds default fallback timeout

// Wrapper for raw pointer to allow Send + Sync in HashMap.
// This is safe because we carefully manage the pointer lifetime:
// - Pointer is only added when stream is created
// - Pointer is removed before stream is freed
// - All access is through RwLock (synchronized)
#[derive(Clone, Copy)]
struct StreamPtr(*mut Aes67Stream);
unsafe impl Send for StreamPtr {}
unsafe impl Sync for StreamPtr {}

// Stream registry for buffer level queries - supports multiple simultaneous streams.
// Uses RwLock since registry access is NOT in the audio callback path.
lazy_static! {
    static ref STREAM_REGISTRY: RwLock<HashMap<HSTREAM, StreamPtr>> =
        RwLock::new(HashMap::new());
}

/// Register a stream in the registry (called when stream is created).
fn register_stream(handle: HSTREAM, stream: *mut Aes67Stream) {
    STREAM_REGISTRY.write().insert(handle, StreamPtr(stream));
}

/// Unregister a stream from the registry (called when stream is freed).
pub fn unregister_stream(handle: HSTREAM) {
    STREAM_REGISTRY.write().remove(&handle);
}

/// Get any registered stream for backwards-compatible stats queries.
/// Returns the first registered stream, or None if no streams exist.
fn get_any_stream() -> Option<*mut Aes67Stream> {
    STREAM_REGISTRY.read().values().next().map(|ptr| ptr.0)
}

/// Plugin format information - defines what formats this plugin handles
/// For URL schemes, the exts field should contain the scheme (e.g., "aes67://")
static PLUGIN_FORMATS: [BassPluginForm; 1] = [
    BassPluginForm {
        ctype: BASS_CTYPE_STREAM_AES67,
        name: b"AES67 Network Audio\0".as_ptr() as *const i8,
        exts: b"aes67://\0".as_ptr() as *const i8, // URL scheme
    },
];

/// Plugin info structure returned by BASSplugin
static PLUGIN_INFO: BassPluginInfo = BassPluginInfo {
    version: VERSION,
    formatc: 1,
    formats: PLUGIN_FORMATS.as_ptr(),
};

/// Convert 24-bit PCM samples to 32-bit float using BASS's conversion function.
/// AES67 uses 24-bit linear PCM, BASS works with 32-bit float internally.
///
/// # Arguments
/// * `src` - Source buffer with 24-bit PCM data
/// * `dst` - Destination buffer for 32-bit float data
/// * `sample_count` - Number of samples to convert
#[allow(dead_code)]
unsafe fn convert_24bit_to_float(src: *const u8, dst: *mut f32, sample_count: usize) {
    if let Some(func) = bassfunc() {
        if let Some(int2float) = func.data.int2float {
            // res=3 means 24-bit (3 bytes per sample)
            int2float(src as *const c_void, dst, sample_count as DWORD, 3);
        }
    }
}

/// Config handler for AES67-specific settings
/// Called by BASS_SetConfig/GetConfig for our custom options
unsafe extern "system" fn config_handler(option: DWORD, flags: DWORD, value: *mut c_void) -> BOOL {
    // Check if this is a pointer value (we don't handle those except for interface)
    let is_set = (flags & BASSCONFIG_SET) != 0;
    let is_ptr = (flags & BASSCONFIG_PTR) != 0;

    match option {
        BASS_CONFIG_AES67_PT => {
            if is_ptr {
                return FALSE;
            }
            let dvalue = value as *mut DWORD;
            if is_set {
                CONFIG_PT = *dvalue;
            } else {
                *dvalue = CONFIG_PT;
            }
            TRUE
        }
        BASS_CONFIG_AES67_JITTER => {
            if is_ptr {
                return FALSE;
            }
            let dvalue = value as *mut DWORD;
            if is_set {
                CONFIG_JITTER_MS = *dvalue;
            } else {
                *dvalue = CONFIG_JITTER_MS;
            }
            TRUE
        }
        BASS_CONFIG_AES67_INTERFACE => {
            if !is_ptr {
                return FALSE;
            }
            if is_set {
                // Set interface from string
                let cstr = CStr::from_ptr(value as *const i8);
                if let Ok(s) = cstr.to_str() {
                    let bytes = s.as_bytes();
                    let max_len = 63; // CONFIG_INTERFACE.len() - 1
                    let len = bytes.len().min(max_len);
                    // Use ptr::copy_nonoverlapping to avoid creating references
                    let dst = ptr::addr_of_mut!(CONFIG_INTERFACE) as *mut u8;
                    ptr::copy_nonoverlapping(bytes.as_ptr(), dst, len);
                    *dst.add(len) = 0;
                }
            } else {
                // Return pointer to our interface string using addr_of
                let iface_ptr = ptr::addr_of!(CONFIG_INTERFACE) as *const u8;
                *(value as *mut *const u8) = iface_ptr;
            }
            TRUE
        }
        // PTP options
        BASS_CONFIG_AES67_PTP_DOMAIN => {
            if is_ptr {
                return FALSE;
            }
            let dvalue = value as *mut DWORD;
            if is_set {
                CONFIG_PTP_DOMAIN = *dvalue;
            } else {
                *dvalue = CONFIG_PTP_DOMAIN;
            }
            TRUE
        }
        BASS_CONFIG_AES67_PTP_ENABLED => {
            if is_ptr {
                return FALSE;
            }
            let dvalue = value as *mut DWORD;
            if is_set {
                CONFIG_PTP_ENABLED = *dvalue;
            } else {
                *dvalue = CONFIG_PTP_ENABLED;
            }
            TRUE
        }
        BASS_CONFIG_AES67_PTP_STATS => {
            // Read-only: return stats string (copy to provided buffer via ptr)
            if is_set {
                return FALSE;
            }
            if !is_ptr {
                return FALSE;
            }
            // Get stats string and store pointer
            // Note: This returns a static string that's valid until next call
            let stats = clock_bindings::clock_get_stats_string();

            // Store the string in a static buffer for FFI compatibility
            static mut STATS_BUFFER: [u8; 256] = [0; 256];
            let bytes = stats.as_bytes();
            let len = bytes.len().min(255);
            ptr::copy_nonoverlapping(bytes.as_ptr(), STATS_BUFFER.as_mut_ptr(), len);
            STATS_BUFFER[len] = 0;
            *(value as *mut *const i8) = STATS_BUFFER.as_ptr() as *const i8;
            TRUE
        }
        BASS_CONFIG_AES67_PTP_OFFSET => {
            // Read-only: return current offset in nanoseconds
            if is_set {
                return FALSE;
            }
            if is_ptr {
                return FALSE;
            }
            let offset = clock_bindings::clock_get_offset();
            *(value as *mut i64) = offset;
            TRUE
        }
        BASS_CONFIG_AES67_PTP_STATE => {
            // Read-only: return clock state
            if is_set {
                return FALSE;
            }
            if is_ptr {
                return FALSE;
            }
            let state = clock_bindings::clock_get_state() as DWORD;
            *(value as *mut DWORD) = state;
            TRUE
        }
        BASS_CONFIG_AES67_BUFFER_LEVEL => {
            // Read-only: return jitter buffer fill percentage (0-200, 100 = at target)
            if is_set {
                return FALSE;
            }
            if is_ptr {
                return FALSE;
            }
            let level = if let Some(stream_ptr) = get_any_stream() {
                (*stream_ptr).buffer_fill_percent()
            } else {
                100  // Default to 100% (at target) if no active stream
            };
            *(value as *mut DWORD) = level;
            TRUE
        }
        BASS_CONFIG_AES67_JITTER_UNDERRUNS => {
            // Read-only: return jitter buffer underrun count
            if is_set {
                return FALSE;
            }
            if is_ptr {
                return FALSE;
            }
            let underruns = if let Some(stream_ptr) = get_any_stream() {
                (*stream_ptr).jitter_underruns() as DWORD
            } else {
                0
            };
            *(value as *mut DWORD) = underruns;
            TRUE
        }
        BASS_CONFIG_AES67_PACKETS_RECEIVED => {
            // Read-only: return total packets received
            if is_set {
                return FALSE;
            }
            if is_ptr {
                return FALSE;
            }
            let received = if let Some(stream_ptr) = get_any_stream() {
                (*stream_ptr).packets_received() as DWORD
            } else {
                0
            };
            *(value as *mut DWORD) = received;
            TRUE
        }
        BASS_CONFIG_AES67_PACKETS_LATE => {
            // Read-only: return late packets dropped count
            if is_set {
                return FALSE;
            }
            if is_ptr {
                return FALSE;
            }
            let late = if let Some(stream_ptr) = get_any_stream() {
                (*stream_ptr).packets_late() as DWORD
            } else {
                0
            };
            *(value as *mut DWORD) = late;
            TRUE
        }
        BASS_CONFIG_AES67_BUFFER_PACKETS => {
            // Read-only: return current buffer level in packets
            if is_set {
                return FALSE;
            }
            if is_ptr {
                return FALSE;
            }
            let packets = if let Some(stream_ptr) = get_any_stream() {
                (*stream_ptr).buffer_packets() as DWORD
            } else {
                0
            };
            *(value as *mut DWORD) = packets;
            TRUE
        }
        BASS_CONFIG_AES67_TARGET_PACKETS => {
            // Read-only: return target buffer level in packets
            if is_set {
                return FALSE;
            }
            if is_ptr {
                return FALSE;
            }
            let target = if let Some(stream_ptr) = get_any_stream() {
                (*stream_ptr).target_packets() as DWORD
            } else {
                0
            };
            *(value as *mut DWORD) = target;
            TRUE
        }
        BASS_CONFIG_AES67_PACKET_TIME => {
            // Read-only: return detected packet time in microseconds
            if is_set {
                return FALSE;
            }
            if is_ptr {
                return FALSE;
            }
            let packet_time = if let Some(stream_ptr) = get_any_stream() {
                (*stream_ptr).detected_packet_time_us() as DWORD
            } else {
                0
            };
            *(value as *mut DWORD) = packet_time;
            TRUE
        }
        BASS_CONFIG_AES67_PTP_LOCKED => {
            // Read-only: return clock locked status (0 or 1)
            if is_set || is_ptr {
                return FALSE;
            }
            let locked = if clock_bindings::clock_is_locked() { 1 } else { 0 };
            *(value as *mut DWORD) = locked;
            TRUE
        }
        BASS_CONFIG_AES67_PTP_FREQ => {
            // Read-only: return clock frequency in PPM × 1000 (for precision)
            if is_set || is_ptr {
                return FALSE;
            }
            let ppm = clock_bindings::clock_get_frequency_ppm();
            // Return as i32 × 1000 for precision (e.g., +3.45 ppm → 3450)
            *(value as *mut i32) = (ppm * 1000.0) as i32;
            TRUE
        }
        BASS_CONFIG_AES67_CLOCK_MODE => {
            // Clock mode: 0=PTP, 1=Livewire, 2=System
            if is_ptr {
                return FALSE;
            }
            let dvalue = value as *mut DWORD;
            if is_set {
                CONFIG_CLOCK_MODE = *dvalue;
            } else {
                *dvalue = CONFIG_CLOCK_MODE;
            }
            TRUE
        }
        BASS_CONFIG_AES67_CLOCK_FALLBACK_TIMEOUT => {
            // Fallback timeout in seconds (0=disabled)
            if is_ptr {
                return FALSE;
            }
            let dvalue = value as *mut DWORD;
            if is_set {
                CONFIG_FALLBACK_TIMEOUT = *dvalue;
                // Also update the clock_bindings fallback timeout
                clock_bindings::set_fallback_timeout(*dvalue);
            } else {
                *dvalue = CONFIG_FALLBACK_TIMEOUT;
            }
            TRUE
        }
        _ => FALSE,
    }
}

/// URL stream creation callback
/// Handles aes67:// URLs like: aes67://239.192.76.52:5004?iface=192.168.60.102&pt=96
unsafe extern "system" fn stream_create_url(
    url: *const i8,
    _offset: DWORD,
    flags: DWORD,
    _proc: Option<DownloadProc>,
    _user: *mut c_void,
) -> HSTREAM {
    // Parse URL string
    let url_cstr = CStr::from_ptr(url);
    let url_str = match url_cstr.to_str() {
        Ok(s) => s,
        Err(_) => {
            set_error(BASS_ERROR_FILEOPEN);
            return 0;
        }
    };

    // Parse the AES67 URL
    let mut config = match Aes67Url::parse(url_str) {
        Ok(c) => c,
        Err(_) => {
            set_error(BASS_ERROR_FILEOPEN);
            return 0;
        }
    };

    // Apply global config overrides if URL didn't specify them
    if config.interface.is_none() {
        let iface_ptr = ptr::addr_of!(CONFIG_INTERFACE) as *const u8;
        let iface_cstr = CStr::from_ptr(iface_ptr as *const i8);
        if let Ok(s) = iface_cstr.to_str() {
            if !s.is_empty() {
                if let Ok(addr) = Ipv4Addr::from_str(s) {
                    config.interface = Some(addr);
                }
            }
        }
    }

    // Use global PT config if not specified in URL
    if config.payload_type == 96 {
        config.payload_type = CONFIG_PT as u8;
    }

    // Use global jitter config if not specified in URL
    if config.jitter_ms == 10 {
        config.jitter_ms = CONFIG_JITTER_MS;
    }

    // Create the AES67 stream
    let mut stream = match Aes67Stream::new(config.clone()) {
        Ok(s) => Box::new(s),
        Err(_) => {
            set_error(BASS_ERROR_MEM);
            return 0;
        }
    };

    // Start receiving packets
    if let Err(_) = stream.start() {
        set_error(BASS_ERROR_FILEOPEN);
        return 0;
    }

    // Start clock client if enabled, interface is configured, and clock not already running
    // (Clock may have been started via BASS_AES67_ClockStart)
    if CONFIG_PTP_ENABLED != 0 && !clock_bindings::clock_is_running() {
        if let Some(iface) = config.interface {
            let mode = clock_bindings::ClockMode::from(CONFIG_CLOCK_MODE);
            let _ = clock_bindings::clock_start(iface, CONFIG_PTP_DOMAIN as u8, mode);
        }
    }

    // Get BASS functions
    let bassfunc = match bassfunc() {
        Some(f) => f,
        None => {
            set_error(BASS_ERROR_INIT);
            return 0;
        }
    };

    let create_stream = match bassfunc.create_stream {
        Some(f) => f,
        None => {
            set_error(BASS_ERROR_INIT);
            return 0;
        }
    };

    // Create BASS stream with our callback
    // Use 32-bit float format (BASS_SAMPLE_FLOAT)
    let stream_flags = (flags & (BASS_SAMPLE_LOOP | BASS_STREAM_DECODE | BASS_STREAM_AUTOFREE))
        | BASS_SAMPLE_FLOAT;

    let stream_ptr = Box::into_raw(stream);

    let handle = create_stream(
        config.sample_rate,
        config.channels as DWORD,
        stream_flags,
        stream_proc,
        stream_ptr as *mut c_void,
        &ADDON_FUNCS as *const _,
    );

    if handle == 0 {
        // Clean up on failure
        let _ = Box::from_raw(stream_ptr);
        return 0;
    }

    // Store handle and flags in stream for later reference
    (*stream_ptr).handle = handle;
    (*stream_ptr).stream_flags = stream_flags;

    // Register stream for buffer level queries (supports multiple streams)
    register_stream(handle, stream_ptr);

    handle
}

/// Main plugin entry point - called by BASS to get plugin information
///
/// # Arguments
/// * `face` - The "face" of the plugin being requested:
///   - BASSPLUGIN_INFO: Return plugin info structure
///   - BASSPLUGIN_CREATE: Return file stream creation function
///   - BASSPLUGIN_CREATEURL: Return URL stream creation function
#[no_mangle]
pub unsafe extern "system" fn BASSplugin(face: DWORD) -> *const c_void {
    if !INITIALIZED.load(Ordering::SeqCst) {
        return ptr::null();
    }

    match face {
        BASSPLUGIN_INFO => &PLUGIN_INFO as *const _ as *const c_void,
        BASSPLUGIN_CREATE => ptr::null(), // We don't handle file streams
        BASSPLUGIN_CREATEURL => stream_create_url as *const c_void,
        _ => ptr::null(),
    }
}

/// Start clock independently for output-only mode
/// Requires BASS_CONFIG_AES67_INTERFACE to be set first via BASS_SetConfigPtr
/// Returns 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_AES67_ClockStart() -> i32 {
    if !INITIALIZED.load(Ordering::SeqCst) {
        return 0;
    }

    // Get interface from config
    let iface_str = std::str::from_utf8(&CONFIG_INTERFACE)
        .ok()
        .and_then(|s| s.trim_end_matches('\0').parse::<std::net::Ipv4Addr>().ok());

    if let Some(iface) = iface_str {
        let mode = clock_bindings::ClockMode::from(CONFIG_CLOCK_MODE);
        match clock_bindings::clock_start(iface, CONFIG_PTP_DOMAIN as u8, mode) {
            Ok(_) => 1,
            Err(_) => 0,
        }
    } else {
        0 // No interface configured
    }
}

/// Stop clock
/// Returns 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_AES67_ClockStop() -> i32 {
    if !INITIALIZED.load(Ordering::SeqCst) {
        return 0;
    }
    clock_bindings::clock_stop();
    1
}

/// Check if clock is locked (stable synchronization)
/// Returns 1 if locked, 0 if not locked or not initialized
#[no_mangle]
pub unsafe extern "system" fn BASS_AES67_ClockIsLocked() -> i32 {
    if !INITIALIZED.load(Ordering::SeqCst) {
        return 0;
    }
    if clock_bindings::clock_is_locked() { 1 } else { 0 }
}

/// Get clock stats string (for Linux where BASS_GetConfigPtr may not work)
/// Returns pointer to static null-terminated string, valid until next call
#[no_mangle]
pub unsafe extern "system" fn BASS_AES67_GetClockStats() -> *const i8 {
    static mut CLOCK_STATS_BUFFER: [u8; 256] = [0; 256];

    if !INITIALIZED.load(Ordering::SeqCst) {
        return b"Not initialized\0".as_ptr() as *const i8;
    }

    let stats = clock_bindings::clock_get_stats_string();
    let bytes = stats.as_bytes();
    let len = bytes.len().min(255);
    ptr::copy_nonoverlapping(bytes.as_ptr(), CLOCK_STATS_BUFFER.as_mut_ptr(), len);
    CLOCK_STATS_BUFFER[len] = 0;
    CLOCK_STATS_BUFFER.as_ptr() as *const i8
}

// =============================================================================
// AES67 OUTPUT STREAM FFI
// =============================================================================

/// FFI-compatible output configuration
#[repr(C)]
pub struct Aes67OutputConfigFFI {
    /// Multicast IP as 4 bytes (a.b.c.d)
    pub multicast_addr: [u8; 4],
    /// UDP port
    pub port: u16,
    /// Interface IP as 4 bytes (0.0.0.0 for default)
    pub interface_addr: [u8; 4],
    /// RTP payload type
    pub payload_type: u8,
    /// Number of channels
    pub channels: u16,
    /// Sample rate in Hz
    pub sample_rate: u32,
    /// Packet time in microseconds
    pub packet_time_us: u32,
}

/// FFI-compatible output statistics
#[repr(C)]
pub struct OutputStatsFFI {
    pub packets_sent: u64,
    pub samples_sent: u64,
    pub send_errors: u64,
    pub underruns: u64,
}

/// Create an AES67 output stream
/// Returns opaque handle (pointer), or null on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_AES67_OutputCreate(
    bass_channel: DWORD,
    config: *const Aes67OutputConfigFFI,
) -> *mut c_void {
    if !INITIALIZED.load(Ordering::SeqCst) || config.is_null() {
        return ptr::null_mut();
    }

    let cfg = &*config;

    // Convert FFI config to Rust config
    let rust_config = Aes67OutputConfig {
        multicast_addr: Ipv4Addr::new(
            cfg.multicast_addr[0],
            cfg.multicast_addr[1],
            cfg.multicast_addr[2],
            cfg.multicast_addr[3],
        ),
        port: cfg.port,
        interface: if cfg.interface_addr == [0, 0, 0, 0] {
            None
        } else {
            Some(Ipv4Addr::new(
                cfg.interface_addr[0],
                cfg.interface_addr[1],
                cfg.interface_addr[2],
                cfg.interface_addr[3],
            ))
        },
        payload_type: cfg.payload_type,
        channels: cfg.channels,
        sample_rate: cfg.sample_rate,
        packet_time_us: cfg.packet_time_us,
    };

    // Create output stream
    match Aes67OutputStream::new(bass_channel, rust_config) {
        Ok(stream) => Box::into_raw(Box::new(stream)) as *mut c_void,
        Err(_) => ptr::null_mut(),
    }
}

/// Start the output stream (begins transmitting)
/// Returns 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_AES67_OutputStart(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        return 0;
    }

    let stream = &mut *(handle as *mut Aes67OutputStream);
    match stream.start() {
        Ok(_) => 1,
        Err(_) => 0,
    }
}

/// Stop the output stream (stops transmitting, can be restarted)
/// Returns 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_AES67_OutputStop(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        return 0;
    }

    let stream = &mut *(handle as *mut Aes67OutputStream);
    stream.stop();
    1
}

/// Get output stream statistics (lock-free)
/// Returns 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_AES67_OutputGetStats(
    handle: *mut c_void,
    stats: *mut OutputStatsFFI,
) -> i32 {
    if handle.is_null() || stats.is_null() {
        return 0;
    }

    let stream = &*(handle as *mut Aes67OutputStream);
    let rust_stats = stream.stats();

    (*stats).packets_sent = rust_stats.packets_sent;
    (*stats).samples_sent = rust_stats.samples_sent;
    (*stats).send_errors = rust_stats.send_errors;
    (*stats).underruns = rust_stats.underruns;
    1
}

/// Check if output is running
/// Returns 1 if running, 0 if not
#[no_mangle]
pub unsafe extern "system" fn BASS_AES67_OutputIsRunning(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        return 0;
    }

    let stream = &*(handle as *mut Aes67OutputStream);
    if stream.is_running() { 1 } else { 0 }
}

/// Get applied PPM frequency correction (x1000 for precision)
/// Returns PPM * 1000, or 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_AES67_OutputGetPPM(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        return 0;
    }

    let stream = &*(handle as *mut Aes67OutputStream);
    (stream.applied_ppm() * 1000.0) as i32
}

/// Destroy the output stream and free resources
/// Returns 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_AES67_OutputFree(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        return 0;
    }

    // Take ownership and drop (stop() is called in Drop impl)
    let _ = Box::from_raw(handle as *mut Aes67OutputStream);
    1
}

/// DLL initialization (Windows)
#[cfg(windows)]
#[no_mangle]
pub unsafe extern "system" fn DllMain(
    _hinst: *mut c_void,
    reason: DWORD,
    _reserved: *mut c_void,
) -> BOOL {
    const DLL_PROCESS_ATTACH: DWORD = 1;
    const DLL_PROCESS_DETACH: DWORD = 0;

    match reason {
        DLL_PROCESS_ATTACH => {
            // Verify BASS version and get function table
            let version = BASS_GetVersion();
            if (version >> 16) != BASSVERSION {
                // Wrong BASS version
                return FALSE;
            }

            if get_bass_func().is_none() {
                return FALSE;
            }

            // Register our config handler
            if let Some(func) = bassfunc() {
                if let Some(register) = func.register_plugin {
                    register(config_handler as *const c_void, PLUGIN_CONFIG_ADD);
                }
            }

            // Initialize clock bindings (try to load bass_ptp.dll and bass_livewire_clock.dll)
            clock_bindings::init_clock_bindings();

            INITIALIZED.store(true, Ordering::SeqCst);
            TRUE
        }
        DLL_PROCESS_DETACH => {
            // Stop clock client
            clock_bindings::clock_stop();

            // Unregister config handler
            if let Some(func) = bassfunc() {
                if let Some(register) = func.register_plugin {
                    register(config_handler as *const c_void, PLUGIN_CONFIG_REMOVE);
                }
            }
            INITIALIZED.store(false, Ordering::SeqCst);
            TRUE
        }
        _ => TRUE,
    }
}

/// Library initialization (Linux/macOS)
#[cfg(not(windows))]
#[used]
#[link_section = ".init_array"]
static INIT: extern "C" fn() = {
    extern "C" fn init() {
        unsafe {
            let version = BASS_GetVersion();
            if (version >> 16) != BASSVERSION {
                return;
            }

            if get_bass_func().is_none() {
                return;
            }

            if let Some(func) = bassfunc() {
                if let Some(register) = func.register_plugin {
                    register(config_handler as *const c_void, PLUGIN_CONFIG_ADD);
                }
            }

            // Initialize clock bindings (try to load bass_ptp.so and bass_livewire_clock.so)
            clock_bindings::init_clock_bindings();

            INITIALIZED.store(true, Ordering::SeqCst);
        }
    }
    init
};

/// Library cleanup (Linux/macOS)
#[cfg(not(windows))]
#[used]
#[link_section = ".fini_array"]
static FINI: extern "C" fn() = {
    extern "C" fn fini() {
        unsafe {
            // Stop clock client
            clock_bindings::clock_stop();

            if let Some(func) = bassfunc() {
                if let Some(register) = func.register_plugin {
                    register(config_handler as *const c_void, PLUGIN_CONFIG_REMOVE);
                }
            }
            INITIALIZED.store(false, Ordering::SeqCst);
        }
    }
    fini
};
