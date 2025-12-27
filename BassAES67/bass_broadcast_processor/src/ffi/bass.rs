//! BASS audio library FFI type bindings.
//! These types match the definitions in bass.h from the BASS SDK.

use std::ffi::c_void;

// Basic types matching BASS definitions
pub type DWORD = u32;
pub type QWORD = u64;
pub type BOOL = i32;

// Handle types
pub type HSTREAM = DWORD;

// Boolean constants
pub const TRUE: BOOL = 1;
pub const FALSE: BOOL = 0;

// BASS version
pub const BASSVERSION: DWORD = 0x204;

// Stream flags
pub const BASS_SAMPLE_FLOAT: DWORD = 0x100;
pub const BASS_STREAM_DECODE: DWORD = 0x200000;

// Data flags for BASS_ChannelGetData
pub const BASS_DATA_FLOAT: DWORD = 0x40000000;

// STREAMPROC return flags
pub const BASS_STREAMPROC_END: DWORD = 0x80000000;

/// Stream callback function type.
/// Returns number of bytes written, optionally ORed with BASS_STREAMPROC_END.
pub type StreamProc = unsafe extern "system" fn(
    handle: HSTREAM,
    buffer: *mut c_void,
    length: DWORD,
    user: *mut c_void,
) -> DWORD;

// BASS library function imports (dynamically linked)
#[link(name = "bass")]
extern "system" {
    /// Get BASS version number.
    pub fn BASS_GetVersion() -> DWORD;

    /// Get data from a channel (decode or record).
    /// Returns bytes read, or -1 (0xFFFFFFFF) on error.
    pub fn BASS_ChannelGetData(handle: DWORD, buffer: *mut c_void, length: DWORD) -> DWORD;

    /// Create a user sample stream.
    pub fn BASS_StreamCreate(
        freq: DWORD,
        chans: DWORD,
        flags: DWORD,
        proc_: StreamProc,
        user: *mut c_void,
    ) -> HSTREAM;

    /// Free a stream.
    pub fn BASS_StreamFree(handle: HSTREAM) -> BOOL;
}
