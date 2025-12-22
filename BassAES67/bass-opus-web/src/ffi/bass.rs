//! BASS audio library FFI type bindings.
//! Minimal types needed for bass_opus_web.

use std::ffi::c_void;

// Basic types matching BASS definitions
pub type DWORD = u32;
pub type BOOL = i32;

// Boolean constants
pub const TRUE: BOOL = 1;
pub const FALSE: BOOL = 0;

// BASS_ChannelGetData flags
pub const BASS_DATA_FLOAT: DWORD = 0x40000000;

// BASS library function imports
#[link(name = "bass")]
extern "system" {
    pub fn BASS_ErrorGetCode() -> i32;
    pub fn BASS_ChannelGetData(handle: DWORD, buffer: *mut c_void, length: DWORD) -> DWORD;
}
