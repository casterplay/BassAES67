//! bass_srt - SRT (Secure Reliable Transport) plugin for BASS audio library.
//!
//! Provides input streaming (SRT → BASS) and output streaming (BASS → SRT).
//! Uses lock-free ring buffers for audio transfer.

use std::ffi::{c_void, CStr};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

mod ffi;
mod srt_bindings;
mod input;
mod output;
pub mod protocol;
pub mod codec;

use ffi::*;
use ffi::addon::*;

// Re-export for external use
pub use output::{SrtOutputStream, SrtOutputConfig, OutputStats};
pub use input::stream::{MetadataCallback, set_metadata_callback, clear_metadata_callback};

// Plugin info strings (static, null-terminated)
static PLUGIN_NAME: &[u8] = b"SRT Audio\0";
static PLUGIN_EXTS: &[u8] = b"srt\0";

// Plugin format descriptor
static PLUGIN_FORMAT: BassPluginForm = BassPluginForm {
    ctype: BASS_CTYPE_STREAM_SRT,
    name: PLUGIN_NAME.as_ptr() as *const i8,
    exts: PLUGIN_EXTS.as_ptr() as *const i8,
};

// Plugin info structure
static PLUGIN_INFO: BassPluginInfo = BassPluginInfo {
    version: BASSVERSION,
    formatc: 1,
    formats: &PLUGIN_FORMAT,
};

// Initialization flag
static INITIALIZED: AtomicBool = AtomicBool::new(false);

// Config options for SRT plugin (0x21000 range - different from AES67)
pub const BASS_CONFIG_SRT_LATENCY: DWORD = 0x21000;
pub const BASS_CONFIG_SRT_BUFFER_LEVEL: DWORD = 0x21001;
pub const BASS_CONFIG_SRT_PACKETS_RECEIVED: DWORD = 0x21002;
pub const BASS_CONFIG_SRT_PACKETS_DROPPED: DWORD = 0x21003;
pub const BASS_CONFIG_SRT_UNDERRUNS: DWORD = 0x21004;
pub const BASS_CONFIG_SRT_CODEC: DWORD = 0x21005;  // Returns: 0=unknown, 1=PCM, 2=OPUS, 3=MP2
pub const BASS_CONFIG_SRT_BITRATE: DWORD = 0x21006;  // Returns: bitrate in kbps (0 for PCM)
pub const BASS_CONFIG_SRT_ENCRYPTED: DWORD = 0x21010;  // Returns: 1 if passphrase was set
pub const BASS_CONFIG_SRT_MODE: DWORD = 0x21011;  // Returns: 0=caller, 1=listener, 2=rendezvous

// Initialize the plugin
fn init_plugin() -> bool {
    if INITIALIZED.swap(true, Ordering::SeqCst) {
        return true;  // Already initialized
    }

    unsafe {
        // Get BASS functions table
        if get_bass_func().is_none() {
            INITIALIZED.store(false, Ordering::SeqCst);
            return false;
        }

        // Register config handler
        if let Some(func) = bassfunc() {
            if let Some(register) = func.register_plugin {
                register(config_handler as *const c_void, PLUGIN_CONFIG_ADD);
            }
        }
    }

    true
}

// Cleanup the plugin
fn cleanup_plugin() {
    if !INITIALIZED.swap(false, Ordering::SeqCst) {
        return;  // Not initialized
    }

    unsafe {
        // Unregister config handler
        if let Some(func) = bassfunc() {
            if let Some(register) = func.register_plugin {
                register(config_handler as *const c_void, PLUGIN_CONFIG_REMOVE);
            }
        }
    }
}

// Config handler for BASS_SetConfig/GetConfig
unsafe extern "system" fn config_handler(
    option: DWORD,
    flags: DWORD,
    value: *mut c_void,
) -> BOOL {
    let is_set = (flags & BASSCONFIG_SET) != 0;

    match option {
        BASS_CONFIG_SRT_BUFFER_LEVEL => {
            if !is_set && !value.is_null() {
                let stream_ptr = input::stream::get_active_stream();
                if !stream_ptr.is_null() {
                    let stream = &*stream_ptr;
                    *(value as *mut DWORD) = stream.buffer_fill_percent();
                    return TRUE;
                }
                *(value as *mut DWORD) = 0;
                return TRUE;
            }
            FALSE
        }
        BASS_CONFIG_SRT_PACKETS_RECEIVED => {
            if !is_set && !value.is_null() {
                let stream_ptr = input::stream::get_active_stream();
                if !stream_ptr.is_null() {
                    let stream = &*stream_ptr;
                    *(value as *mut u64) = stream.packets_received();
                    return TRUE;
                }
                *(value as *mut u64) = 0;
                return TRUE;
            }
            FALSE
        }
        BASS_CONFIG_SRT_PACKETS_DROPPED => {
            if !is_set && !value.is_null() {
                let stream_ptr = input::stream::get_active_stream();
                if !stream_ptr.is_null() {
                    let stream = &*stream_ptr;
                    *(value as *mut u64) = stream.packets_dropped();
                    return TRUE;
                }
                *(value as *mut u64) = 0;
                return TRUE;
            }
            FALSE
        }
        BASS_CONFIG_SRT_UNDERRUNS => {
            if !is_set && !value.is_null() {
                let stream_ptr = input::stream::get_active_stream();
                if !stream_ptr.is_null() {
                    let stream = &*stream_ptr;
                    *(value as *mut u64) = stream.underruns();
                    return TRUE;
                }
                *(value as *mut u64) = 0;
                return TRUE;
            }
            FALSE
        }
        BASS_CONFIG_SRT_CODEC => {
            if !is_set && !value.is_null() {
                let stream_ptr = input::stream::get_active_stream();
                if !stream_ptr.is_null() {
                    let stream = &*stream_ptr;
                    *(value as *mut DWORD) = stream.detected_codec();
                    return TRUE;
                }
                *(value as *mut DWORD) = 0;
                return TRUE;
            }
            FALSE
        }
        BASS_CONFIG_SRT_BITRATE => {
            if !is_set && !value.is_null() {
                let stream_ptr = input::stream::get_active_stream();
                if !stream_ptr.is_null() {
                    let stream = &*stream_ptr;
                    *(value as *mut DWORD) = stream.detected_bitrate();
                    return TRUE;
                }
                *(value as *mut DWORD) = 0;
                return TRUE;
            }
            FALSE
        }
        BASS_CONFIG_SRT_ENCRYPTED => {
            if !is_set && !value.is_null() {
                let stream_ptr = input::stream::get_active_stream();
                if !stream_ptr.is_null() {
                    let stream = &*stream_ptr;
                    *(value as *mut DWORD) = if stream.is_encrypted() { 1 } else { 0 };
                    return TRUE;
                }
                *(value as *mut DWORD) = 0;
                return TRUE;
            }
            FALSE
        }
        BASS_CONFIG_SRT_MODE => {
            if !is_set && !value.is_null() {
                let stream_ptr = input::stream::get_active_stream();
                if !stream_ptr.is_null() {
                    let stream = &*stream_ptr;
                    *(value as *mut DWORD) = stream.connection_mode();
                    return TRUE;
                }
                *(value as *mut DWORD) = 0;
                return TRUE;
            }
            FALSE
        }
        _ => FALSE,
    }
}

// Create stream from URL (srt://host:port?options)
unsafe extern "system" fn create_stream_url(
    url: *const i8,
    _offset: DWORD,
    flags: DWORD,
    _proc: Option<DownloadProc>,
    _user: *mut c_void,
) -> HSTREAM {
    if url.is_null() {
        set_error(BASS_ERROR_FILEOPEN);
        return 0;
    }

    let url_str = match CStr::from_ptr(url).to_str() {
        Ok(s) => s,
        Err(_) => {
            set_error(BASS_ERROR_FILEOPEN);
            return 0;
        }
    };

    // Parse URL
    let config = match input::SrtUrl::parse(url_str) {
        Ok(c) => c,
        Err(_) => {
            set_error(BASS_ERROR_FILEOPEN);
            return 0;
        }
    };

    // Create stream
    let mut stream = match input::SrtStream::new(config.clone()) {
        Ok(s) => s,
        Err(_) => {
            set_error(BASS_ERROR_MEM);
            return 0;
        }
    };

    // Store flags for get_info
    stream.stream_flags = flags & (BASS_STREAM_DECODE | BASS_STREAM_AUTOFREE);

    // Start receiving
    if stream.start().is_err() {
        set_error(BASS_ERROR_FILEOPEN);
        return 0;
    }

    // Box the stream and get raw pointer
    let stream_ptr = Box::into_raw(Box::new(stream));

    // Set as active stream for config queries
    input::stream::set_active_stream(stream_ptr);

    // Get BASS functions and create stream
    let bassfunc = match bassfunc() {
        Some(f) => f,
        None => {
            drop(Box::from_raw(stream_ptr));
            set_error(BASS_ERROR_INIT);
            return 0;
        }
    };

    let create_stream = match bassfunc.create_stream {
        Some(f) => f,
        None => {
            drop(Box::from_raw(stream_ptr));
            set_error(BASS_ERROR_INIT);
            return 0;
        }
    };

    // Create BASS stream with our callback
    let stream_flags = BASS_SAMPLE_FLOAT | (flags & (BASS_STREAM_DECODE | BASS_STREAM_AUTOFREE));

    let handle = create_stream(
        config.sample_rate,
        config.channels as DWORD,
        stream_flags,
        input::stream::stream_proc,
        stream_ptr as *mut c_void,
        &input::ADDON_FUNCS,
    );

    if handle == 0 {
        drop(Box::from_raw(stream_ptr));
        input::stream::set_active_stream(ptr::null_mut());
        return 0;
    }

    // Store handle in stream
    (*stream_ptr).handle = handle;

    handle
}

// Plugin entry point - called by BASS to get plugin functions
#[no_mangle]
pub unsafe extern "system" fn BASSplugin(face: DWORD) -> *const c_void {
    match face {
        BASSPLUGIN_INFO => &PLUGIN_INFO as *const _ as *const c_void,
        BASSPLUGIN_CREATEURL => create_stream_url as *const c_void,
        _ => ptr::null(),
    }
}

/// Set the metadata callback for JSON packets received over SRT.
///
/// The callback will be called from the receiver thread whenever a JSON
/// metadata packet is received. The callback receives:
/// - json: Pointer to UTF-8 JSON string (not null-terminated)
/// - len: Length of the JSON string in bytes
/// - user: User data pointer passed to this function
///
/// To clear the callback, call BASS_SRT_ClearMetadataCallback().
///
/// # Safety
/// The callback must be thread-safe as it's called from the receiver thread.
#[no_mangle]
pub unsafe extern "C" fn BASS_SRT_SetMetadataCallback(
    callback: input::stream::MetadataCallback,
    user: *mut c_void,
) {
    input::stream::set_metadata_callback(callback, user);
}

/// Clear the metadata callback.
#[no_mangle]
pub unsafe extern "C" fn BASS_SRT_ClearMetadataCallback() {
    input::stream::clear_metadata_callback();
}

// Windows DLL entry point
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
            if !init_plugin() {
                return FALSE;
            }
        }
        DLL_PROCESS_DETACH => {
            cleanup_plugin();
        }
        _ => {}
    }
    TRUE
}

// Linux/macOS initialization using constructor/destructor sections
#[cfg(not(windows))]
mod unix_init {
    use super::*;

    #[used]
    #[link_section = ".init_array"]
    static INIT: extern "C" fn() = init_function;

    #[used]
    #[link_section = ".fini_array"]
    static FINI: extern "C" fn() = fini_function;

    extern "C" fn init_function() {
        init_plugin();
    }

    extern "C" fn fini_function() {
        cleanup_plugin();
    }
}
