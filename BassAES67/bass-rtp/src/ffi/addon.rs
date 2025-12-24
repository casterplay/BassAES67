//! BASS add-on API FFI bindings.
//! These types match the definitions in bass-addon.h from the BASS SDK.

use std::ffi::c_void;
use super::bass::*;

// Opaque file handle type for BASS file operations
pub type BassFile = *mut c_void;

// BASSplugin "faces" - what the plugin function returns
pub const BASSPLUGIN_INFO: DWORD = 0;
pub const BASSPLUGIN_CREATE: DWORD = 1;
pub const BASSPLUGIN_CREATEURL: DWORD = 2;
pub const BASSPLUGIN_CREATEURL2: DWORD = 3;

// Add-on flags
pub const ADDON_OWNPOS: DWORD = 1;
pub const ADDON_DECODETO: DWORD = 2;
pub const ADDON_LOCK: DWORD = 8;

// Config options for getting BASS_FUNCTIONS
pub const BASS_CONFIG_ADDON: DWORD = 0x8000;
pub const BASS_CONFIG_ADDON2: DWORD = 0x8004;

// BASSFILE flags
pub const BASSFILE_BUFFERED: DWORD = 1;

// RegisterPlugin modes
pub const PLUGIN_CONFIG_ADD: DWORD = 0;
pub const PLUGIN_CONFIG_REMOVE: DWORD = 1;

// BASSCONFIGPROC flags
pub const BASSCONFIG_SET: DWORD = 1;
pub const BASSCONFIG_PTR: DWORD = 2;

/// Stream create callback for file/user streams
pub type StreamCreateProc = unsafe extern "system" fn(
    file: BassFile,
    flags: DWORD,
) -> HSTREAM;

/// Stream create callback for URL streams (custom schemes like rtp://)
pub type StreamCreateUrlProc = unsafe extern "system" fn(
    url: *const i8,
    offset: DWORD,
    flags: DWORD,
    proc_: Option<DownloadProc>,
    user: *mut c_void,
) -> HSTREAM;

/// Config callback for handling BASS_SetConfig/GetConfig
pub type BassConfigProc = unsafe extern "system" fn(
    option: DWORD,
    flags: DWORD,
    value: *mut c_void,
) -> BOOL;

/// Add-on functions structure - callbacks the add-on must implement
#[repr(C)]
pub struct AddonFunctions {
    pub flags: DWORD,
    pub free: Option<unsafe extern "system" fn(inst: *mut c_void)>,
    pub get_length: Option<unsafe extern "system" fn(inst: *mut c_void, mode: DWORD) -> QWORD>,
    pub get_tags: Option<unsafe extern "system" fn(inst: *mut c_void, tags: DWORD) -> *const i8>,
    pub get_file_position: Option<unsafe extern "system" fn(inst: *mut c_void, mode: DWORD) -> QWORD>,
    pub get_info: Option<unsafe extern "system" fn(inst: *mut c_void, info: *mut BassChannelInfo)>,
    pub can_set_position: Option<unsafe extern "system" fn(inst: *mut c_void, pos: QWORD, mode: DWORD) -> BOOL>,
    pub set_position: Option<unsafe extern "system" fn(inst: *mut c_void, pos: QWORD, mode: DWORD) -> QWORD>,
    pub get_position: Option<unsafe extern "system" fn(inst: *mut c_void, pos: QWORD, mode: DWORD) -> QWORD>,
    pub set_sync: Option<unsafe extern "system" fn(inst: *mut c_void, type_: DWORD, param: QWORD, proc_: SyncProc, user: *mut c_void) -> HSYNC>,
    pub remove_sync: Option<unsafe extern "system" fn(inst: *mut c_void, sync: HSYNC)>,
    pub can_resume: Option<unsafe extern "system" fn(inst: *mut c_void) -> BOOL>,
    pub set_flags: Option<unsafe extern "system" fn(inst: *mut c_void, flags: DWORD) -> DWORD>,
    pub attribute: Option<unsafe extern "system" fn(inst: *mut c_void, attrib: DWORD, value: *mut f32, set: BOOL) -> BOOL>,
    pub attribute_ex: Option<unsafe extern "system" fn(inst: *mut c_void, attrib: DWORD, value: *mut c_void, typesize: DWORD, set: BOOL) -> DWORD>,
}

/// File operation functions provided by BASS
#[repr(C)]
pub struct BassFileFunctions {
    pub open: Option<unsafe extern "system" fn(filetype: DWORD, file: *const c_void, offset: QWORD, length: QWORD, flags: DWORD, exflags: DWORD) -> BassFile>,
    pub open_url: Option<unsafe extern "system" fn(url: *const i8, offset: DWORD, flags: DWORD, proc_: Option<DownloadProc>, user: *mut c_void, exflags: DWORD) -> BassFile>,
    pub open_user: Option<unsafe extern "system" fn(system: DWORD, flags: DWORD, proc_: *const BassFileProcs, user: *mut c_void, exflags: DWORD) -> BassFile>,
    pub close: Option<unsafe extern "system" fn(file: BassFile)>,
    pub get_file_name: Option<unsafe extern "system" fn(file: BassFile, unicode: *mut BOOL) -> *const i8>,
    pub set_stream: Option<unsafe extern "system" fn(file: BassFile, handle: HSTREAM) -> BOOL>,
    pub get_flags: Option<unsafe extern "system" fn(file: BassFile) -> DWORD>,
    pub set_flags: Option<unsafe extern "system" fn(file: BassFile, flags: DWORD)>,
    pub read: Option<unsafe extern "system" fn(file: BassFile, buf: *mut c_void, len: DWORD) -> DWORD>,
    pub seek: Option<unsafe extern "system" fn(file: BassFile, pos: QWORD) -> BOOL>,
    pub get_pos: Option<unsafe extern "system" fn(file: BassFile, mode: DWORD) -> QWORD>,
    pub eof: Option<unsafe extern "system" fn(file: BassFile) -> BOOL>,
    pub get_tags: Option<unsafe extern "system" fn(file: BassFile, tags: DWORD) -> *const i8>,
    pub start_thread: Option<unsafe extern "system" fn(file: BassFile, bitrate: DWORD, offset: DWORD) -> BOOL>,
    pub can_resume: Option<unsafe extern "system" fn(file: BassFile) -> BOOL>,
}

/// Data conversion functions provided by BASS
#[repr(C)]
pub struct BassDataFunctions {
    pub float2int: Option<unsafe extern "system" fn(src: *const f32, dst: *mut c_void, len: DWORD, res: DWORD)>,
    pub int2float: Option<unsafe extern "system" fn(src: *const c_void, dst: *mut f32, len: DWORD, res: DWORD)>,
    pub swap: Option<unsafe extern "system" fn(src: *const c_void, dst: *mut c_void, len: DWORD, res: DWORD)>,
}

/// Main BASS_FUNCTIONS structure provided to add-ons
#[repr(C)]
pub struct BassFunctions {
    pub set_error: Option<unsafe extern "system" fn(error: i32)>,
    pub register_plugin: Option<unsafe extern "system" fn(proc_: *const c_void, mode: DWORD)>,
    pub create_stream: Option<unsafe extern "system" fn(
        freq: DWORD,
        chans: DWORD,
        flags: DWORD,
        proc_: StreamProc,
        inst: *mut c_void,
        funcs: *const AddonFunctions,
    ) -> HSTREAM>,
    pub set_fx: Option<unsafe extern "system" fn(
        handle: DWORD,
        proc_: DspProc,
        inst: *mut c_void,
        priority: i32,
        funcs: *const c_void,
    ) -> HFX>,
    pub get_inst: Option<unsafe extern "system" fn(handle: HSTREAM, funcs: *const AddonFunctions) -> *mut c_void>,
    pub reserved1: *const c_void,
    pub new_sync: Option<unsafe extern "system" fn(handle: HSTREAM, type_: DWORD, proc_: SyncProc, user: *mut c_void) -> HSYNC>,
    pub trigger_sync: Option<unsafe extern "system" fn(handle: HSTREAM, sync: HSYNC, pos: QWORD, data: DWORD) -> BOOL>,
    pub get_count: Option<unsafe extern "system" fn(handle: DWORD, output: BOOL) -> QWORD>,
    pub get_position: Option<unsafe extern "system" fn(handle: DWORD, count: QWORD, mode: DWORD) -> QWORD>,
    pub file: BassFileFunctions,
    pub data: BassDataFunctions,
}

// Static storage for the BASS functions pointer
static mut BASSFUNC: Option<*const BassFunctions> = None;

/// Get the BASS_FUNCTIONS table from BASS
/// Must be called during plugin initialization
pub unsafe fn get_bass_func() -> Option<*const BassFunctions> {
    let ptr = BASS_GetConfigPtr(BASS_CONFIG_ADDON);
    if ptr.is_null() {
        None
    } else {
        BASSFUNC = Some(ptr as *const BassFunctions);
        Some(ptr as *const BassFunctions)
    }
}

/// Access the cached BASS_FUNCTIONS pointer
pub unsafe fn bassfunc() -> Option<&'static BassFunctions> {
    BASSFUNC.and_then(|p| p.as_ref())
}

/// Set error code helper
pub unsafe fn set_error(error: i32) {
    if let Some(func) = bassfunc() {
        if let Some(set_err) = func.set_error {
            set_err(error);
        }
    }
}
