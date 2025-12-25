//! BASS audio library FFI type bindings.
//! These types match the definitions in bass.h from the BASS SDK.

use std::ffi::c_void;

// Basic types matching BASS definitions
pub type DWORD = u32;
pub type QWORD = u64;
pub type WORD = u16;
pub type BYTE = u8;
pub type BOOL = i32;

// Handle types
pub type HSTREAM = DWORD;
pub type HSYNC = DWORD;
pub type HDSP = DWORD;
pub type HFX = DWORD;
pub type HPLUGIN = DWORD;
pub type HCHANNEL = DWORD;

// Boolean constants
pub const TRUE: BOOL = 1;
pub const FALSE: BOOL = 0;

// BASS version
pub const BASSVERSION: DWORD = 0x204;
pub const BASSVERSIONTEXT: &str = "2.4";

// Error codes
pub const BASS_OK: i32 = 0;
pub const BASS_ERROR_MEM: i32 = 1;
pub const BASS_ERROR_FILEOPEN: i32 = 2;
pub const BASS_ERROR_DRIVER: i32 = 3;
pub const BASS_ERROR_BUFLOST: i32 = 4;
pub const BASS_ERROR_HANDLE: i32 = 5;
pub const BASS_ERROR_FORMAT: i32 = 6;
pub const BASS_ERROR_POSITION: i32 = 7;
pub const BASS_ERROR_INIT: i32 = 8;
pub const BASS_ERROR_START: i32 = 9;
pub const BASS_ERROR_SSL: i32 = 10;
pub const BASS_ERROR_CREATE: i32 = 18;
pub const BASS_ERROR_NOTAVAIL: i32 = 37;
pub const BASS_ERROR_VERSION: i32 = 43;
pub const BASS_ERROR_UNKNOWN: i32 = -1;

// Stream flags
pub const BASS_SAMPLE_8BITS: DWORD = 1;
pub const BASS_SAMPLE_FLOAT: DWORD = 0x100;
pub const BASS_SAMPLE_MONO: DWORD = 2;
pub const BASS_SAMPLE_LOOP: DWORD = 4;
pub const BASS_SAMPLE_3D: DWORD = 8;
pub const BASS_SAMPLE_SOFTWARE: DWORD = 0x10;
pub const BASS_SAMPLE_FX: DWORD = 0x80;

pub const BASS_STREAM_DECODE: DWORD = 0x200000;
pub const BASS_STREAM_AUTOFREE: DWORD = 0x40000;
pub const BASS_STREAM_BLOCK: DWORD = 0x100000;
pub const BASS_STREAM_RESTRATE: DWORD = 0x80000;

pub const BASS_ASYNCFILE: DWORD = 0x40000000;
pub const BASS_UNICODE: DWORD = 0x80000000;

// STREAMPROC return flags
pub const BASS_STREAMPROC_END: DWORD = 0x80000000;

// Position modes
pub const BASS_POS_BYTE: DWORD = 0;

// Channel types
pub const BASS_CTYPE_STREAM: DWORD = 0x10000;

// Custom channel type for RTP
pub const BASS_CTYPE_STREAM_RTP: DWORD = 0x1f300;

// BASS_ChannelGetData flags
pub const BASS_DATA_FLOAT: DWORD = 0x40000000;

/// Channel info structure returned by BASS_ChannelGetInfo
#[repr(C)]
pub struct BassChannelInfo {
    pub freq: DWORD,
    pub chans: DWORD,
    pub flags: DWORD,
    pub ctype: DWORD,
    pub origres: DWORD,
    pub plugin: HPLUGIN,
    pub sample: DWORD,
    pub filename: *const i8,
}

/// Plugin format info structure
#[repr(C)]
pub struct BassPluginForm {
    pub ctype: DWORD,
    pub name: *const i8,
    pub exts: *const i8,
}

// Safety: BassPluginForm only contains pointers to static string data
unsafe impl Sync for BassPluginForm {}

/// Plugin info structure returned by BASSplugin
#[repr(C)]
pub struct BassPluginInfo {
    pub version: DWORD,
    pub formatc: DWORD,
    pub formats: *const BassPluginForm,
}

// Safety: BassPluginInfo only contains pointer to static BassPluginForm array
unsafe impl Sync for BassPluginInfo {}

/// File procedures for custom file handling
#[repr(C)]
pub struct BassFileProcs {
    pub close: Option<unsafe extern "system" fn(user: *mut c_void)>,
    pub length: Option<unsafe extern "system" fn(user: *mut c_void) -> QWORD>,
    pub read: Option<unsafe extern "system" fn(buffer: *mut c_void, length: DWORD, user: *mut c_void) -> DWORD>,
    pub seek: Option<unsafe extern "system" fn(offset: QWORD, user: *mut c_void) -> BOOL>,
}

/// Stream callback function type
/// Returns number of bytes written, optionally ORed with BASS_STREAMPROC_END
pub type StreamProc = unsafe extern "system" fn(
    handle: HSTREAM,
    buffer: *mut c_void,
    length: DWORD,
    user: *mut c_void,
) -> DWORD;

/// Sync callback function type
pub type SyncProc = unsafe extern "system" fn(
    handle: HSYNC,
    channel: DWORD,
    data: DWORD,
    user: *mut c_void,
);

/// DSP callback function type
pub type DspProc = unsafe extern "system" fn(
    handle: HDSP,
    channel: DWORD,
    buffer: *mut c_void,
    length: DWORD,
    user: *mut c_void,
);

/// Download callback function type
pub type DownloadProc = unsafe extern "system" fn(
    buffer: *const c_void,
    length: DWORD,
    user: *mut c_void,
);

// BASS library function imports (dynamically linked)
#[link(name = "bass")]
extern "system" {
    pub fn BASS_GetVersion() -> DWORD;
    pub fn BASS_ErrorGetCode() -> i32;
    pub fn BASS_GetConfig(option: DWORD) -> DWORD;
    pub fn BASS_GetConfigPtr(option: DWORD) -> *const c_void;
    pub fn BASS_SetConfig(option: DWORD, value: DWORD) -> BOOL;
    pub fn BASS_StreamCreate(
        freq: DWORD,
        chans: DWORD,
        flags: DWORD,
        proc_: Option<StreamProc>,
        user: *mut c_void,
    ) -> HSTREAM;
    pub fn BASS_StreamFree(handle: HSTREAM) -> BOOL;
    pub fn BASS_ChannelLock(handle: DWORD, lock: BOOL) -> BOOL;
    pub fn BASS_ChannelGetData(handle: DWORD, buffer: *mut c_void, length: DWORD) -> DWORD;
}

/// Thread-local last error code (used when BASS functions not available)
use std::cell::Cell;
thread_local! {
    static LAST_ERROR: Cell<i32> = const { Cell::new(BASS_OK) };
}

/// Set the last error code
pub fn set_error(error: i32) {
    LAST_ERROR.with(|e| e.set(error));
}

/// Get the last error code
pub fn get_error() -> i32 {
    LAST_ERROR.with(|e| e.get())
}
