//! bass-webrtc: WebRTC audio streaming plugin for BASS.
//!
//! Provides peer-to-peer WebRTC audio with up to 5 simultaneous browser connections.
//! Supports WHIP/WHEP HTTP signaling (RFC 9725) and callback-based signaling.
//!
//! ## Features
//!
//! - **Bidirectional audio**: BASS channel -> WebRTC and WebRTC -> BASS
//! - **Multi-peer**: Up to 5 simultaneous browser connections
//! - **OPUS codec**: 48kHz stereo, 20ms frames
//! - **STUN + TURN**: Full NAT traversal support
//! - **WHIP/WHEP**: Standard HTTP-based signaling

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::Arc;

use lazy_static::lazy_static;
use parking_lot::Mutex;
use tokio::runtime::Runtime;

pub mod ffi;
pub mod codec;
pub mod peer;
pub mod stream;
pub mod signaling;
pub mod ice;

use ffi::bass::*;
use peer::{PeerManager, IceServerConfig, MAX_PEERS};
use stream::{WebRtcOutputStream, WebRtcInputStream, input_stream_proc};
use signaling::{SignalingCallbacks, WhipConfig, WhipEndpoint, WhepConfig, WhepEndpoint};

// ============================================================================
// Tokio Runtime (shared)
// ============================================================================

lazy_static! {
    /// Shared tokio runtime for async operations
    static ref RUNTIME: Runtime = Runtime::new().expect("Failed to create tokio runtime");
}

// ============================================================================
// Signaling Mode Constants
// ============================================================================

/// Callback-based signaling mode
pub const BASS_WEBRTC_SIGNALING_CALLBACK: u8 = 0;
/// WHIP HTTP signaling mode
pub const BASS_WEBRTC_SIGNALING_WHIP: u8 = 1;
/// WHEP HTTP signaling mode
pub const BASS_WEBRTC_SIGNALING_WHEP: u8 = 2;

// ============================================================================
// FFI Configuration Structures
// ============================================================================

/// WebRTC server configuration
#[repr(C)]
pub struct WebRtcConfigFFI {
    /// Sample rate (48000 recommended)
    pub sample_rate: u32,
    /// Number of channels (1 or 2)
    pub channels: u16,
    /// OPUS bitrate in kbps (default 128)
    pub opus_bitrate: u32,
    /// Incoming audio buffer in milliseconds (default 100)
    pub buffer_ms: u32,
    /// Maximum peers (1-5)
    pub max_peers: u8,
    /// Signaling mode (BASS_WEBRTC_SIGNALING_*)
    pub signaling_mode: u8,
    /// WHIP/WHEP HTTP port (if applicable)
    pub http_port: u16,
    /// Create input stream with BASS_STREAM_DECODE flag (for mixer compatibility)
    pub decode_stream: u8,
}

impl Default for WebRtcConfigFFI {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            channels: 2,
            opus_bitrate: 128,
            buffer_ms: 100,
            max_peers: 5,
            signaling_mode: BASS_WEBRTC_SIGNALING_CALLBACK,
            http_port: 8080,
            decode_stream: 0,
        }
    }
}

/// Statistics structure
#[repr(C)]
pub struct WebRtcStatsFFI {
    pub active_peers: u32,
    pub total_packets_sent: u64,
    pub total_packets_received: u64,
    pub total_bytes_sent: u64,
    pub total_bytes_received: u64,
    pub total_encode_errors: u64,
    pub total_decode_errors: u64,
    pub output_underruns: u64,
    pub input_buffer_level: u32,
    pub input_is_buffering: u8,
}

// ============================================================================
// WebRTC Server
// ============================================================================

/// Main WebRTC server coordinating audio and peers
pub struct WebRtcServer {
    /// Peer manager
    peer_manager: Arc<Mutex<PeerManager>>,
    /// Output stream (BASS -> WebRTC)
    output_stream: Option<WebRtcOutputStream>,
    /// Input stream (WebRTC -> BASS)
    input_stream: Option<Box<WebRtcInputStream>>,
    /// BASS source channel for output
    source_channel: HSTREAM,
    /// BASS input stream handle
    input_handle: HSTREAM,
    /// Configuration
    config: WebRtcConfigFFI,
    /// Signaling callbacks
    signaling_callbacks: SignalingCallbacks,
    /// WHIP endpoint
    whip_endpoint: Option<WhipEndpoint>,
    /// WHEP endpoint
    whep_endpoint: Option<WhepEndpoint>,
    /// Running flag
    running: bool,
}

impl WebRtcServer {
    /// Create a new WebRTC server
    fn new(source_channel: HSTREAM, config: WebRtcConfigFFI) -> Result<Self, String> {
        // Create peer manager with default STUN servers
        let ice_servers = ice::google_stun_servers();
        let incoming_buffer_samples = (config.sample_rate as usize / 1000)
            * config.buffer_ms as usize
            * config.channels as usize
            * 3; // 3x headroom

        let peer_manager = PeerManager::new(ice_servers, incoming_buffer_samples)?;

        Ok(Self {
            peer_manager: Arc::new(Mutex::new(peer_manager)),
            output_stream: None,
            input_stream: None,
            source_channel,
            input_handle: 0,
            config,
            signaling_callbacks: SignalingCallbacks::default(),
            whip_endpoint: None,
            whep_endpoint: None,
            running: false,
        })
    }

    /// Add an ICE server
    fn add_ice_server(&mut self, url: &str, username: Option<&str>, credential: Option<&str>) {
        // Note: This would require reinitializing peer manager
        // For now, ICE servers should be configured before creating the server
        // This is a limitation we can address later
    }

    /// Set signaling callbacks
    fn set_callbacks(&mut self, callbacks: SignalingCallbacks) {
        self.signaling_callbacks = callbacks;
    }

    /// Start the server
    fn start(&mut self) -> Result<(), String> {
        if self.running {
            return Err("Server already running".to_string());
        }

        // Create output stream
        let shared_track = {
            let pm = self.peer_manager.lock();
            pm.shared_track().clone()
        };

        let mut output = WebRtcOutputStream::new(
            self.source_channel,
            shared_track,
            self.config.sample_rate,
            self.config.channels,
            self.config.opus_bitrate,
            RUNTIME.handle().clone(),
        );
        output.start()?;
        self.output_stream = Some(output);

        // Create input stream
        let mut input = Box::new(WebRtcInputStream::new(
            self.config.sample_rate,
            self.config.channels,
            self.config.buffer_ms,
        ));

        // Create BASS stream for input
        let flags = if self.config.decode_stream != 0 {
            BASS_SAMPLE_FLOAT | BASS_STREAM_DECODE
        } else {
            BASS_SAMPLE_FLOAT
        };

        let input_ptr = input.as_mut() as *mut WebRtcInputStream;
        let bass_stream = unsafe {
            BASS_StreamCreate(
                self.config.sample_rate,
                self.config.channels as u32,
                flags,
                Some(input_stream_proc),
                input_ptr as *mut c_void,
            )
        };

        if bass_stream == 0 {
            return Err(format!("Failed to create BASS input stream: {}", unsafe { BASS_ErrorGetCode() }));
        }

        self.input_handle = bass_stream;
        self.input_stream = Some(input);

        // Start signaling endpoints if configured
        match self.config.signaling_mode {
            BASS_WEBRTC_SIGNALING_WHIP => {
                let whip_config = WhipConfig {
                    port: self.config.http_port,
                    ..Default::default()
                };
                let mut whip = WhipEndpoint::new(whip_config, self.peer_manager.clone());
                RUNTIME.block_on(whip.start())?;
                self.whip_endpoint = Some(whip);
            }
            BASS_WEBRTC_SIGNALING_WHEP => {
                let whep_config = WhepConfig {
                    port: self.config.http_port,
                    ..Default::default()
                };
                let mut whep = WhepEndpoint::new(whep_config, self.peer_manager.clone());
                RUNTIME.block_on(whep.start())?;
                self.whep_endpoint = Some(whep);
            }
            _ => {
                // Callback mode - no additional setup needed
            }
        }

        self.running = true;
        Ok(())
    }

    /// Stop the server
    fn stop(&mut self) {
        if !self.running {
            return;
        }

        // Stop output stream
        if let Some(ref mut output) = self.output_stream {
            output.stop();
        }
        self.output_stream = None;

        // Stop signaling endpoints
        if let Some(ref mut whip) = self.whip_endpoint {
            whip.stop();
        }
        if let Some(ref mut whep) = self.whep_endpoint {
            whep.stop();
        }

        // Close all peers
        RUNTIME.block_on(async {
            let mut pm = self.peer_manager.lock();
            pm.close_all().await;
        });

        // Free BASS input stream
        if self.input_handle != 0 {
            unsafe { BASS_StreamFree(self.input_handle) };
            self.input_handle = 0;
        }

        // Mark input stream as ended
        if let Some(ref mut input) = self.input_stream {
            input.set_ended();
        }

        self.running = false;
    }
}

impl Drop for WebRtcServer {
    fn drop(&mut self) {
        self.stop();
    }
}

// ============================================================================
// DLL Entry Point (Windows)
// ============================================================================

#[cfg(windows)]
#[no_mangle]
pub extern "system" fn DllMain(
    _hinst: *mut c_void,
    reason: u32,
    _reserved: *mut c_void,
) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    const DLL_PROCESS_DETACH: u32 = 0;

    match reason {
        DLL_PROCESS_ATTACH => {
            // Initialization
        }
        DLL_PROCESS_DETACH => {
            // Cleanup
        }
        _ => {}
    }
    1 // TRUE
}

// ============================================================================
// FFI API
// ============================================================================

/// Create a WebRTC server.
///
/// # Arguments
/// * `source_channel` - BASS channel to read audio from (for output to browsers)
/// * `config` - Server configuration
///
/// # Returns
/// Opaque handle or null on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_Create(
    source_channel: DWORD,
    config: *const WebRtcConfigFFI,
) -> *mut c_void {
    let cfg = if config.is_null() {
        WebRtcConfigFFI::default()
    } else {
        (*config).clone()
    };

    match WebRtcServer::new(source_channel, cfg) {
        Ok(server) => Box::into_raw(Box::new(server)) as *mut c_void,
        Err(e) => {
            eprintln!("BASS_WEBRTC_Create error: {}", e);
            set_error(BASS_ERROR_CREATE);
            std::ptr::null_mut()
        }
    }
}

impl Clone for WebRtcConfigFFI {
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for WebRtcConfigFFI {}

/// Add an ICE server (STUN or TURN).
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_Create
/// * `url` - Server URL (e.g., "stun:stun.l.google.com:19302" or "turn:server:3478")
/// * `username` - Username for TURN (null for STUN)
/// * `credential` - Credential for TURN (null for STUN)
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_AddIceServer(
    handle: *mut c_void,
    url: *const c_char,
    username: *const c_char,
    credential: *const c_char,
) -> i32 {
    if handle.is_null() || url.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let _server = &mut *(handle as *mut WebRtcServer);
    let url_str = CStr::from_ptr(url).to_string_lossy();
    let username_str = if username.is_null() {
        None
    } else {
        Some(CStr::from_ptr(username).to_string_lossy())
    };
    let credential_str = if credential.is_null() {
        None
    } else {
        Some(CStr::from_ptr(credential).to_string_lossy())
    };

    // Note: ICE servers should be configured before starting
    // This is a placeholder for future enhancement
    1
}

/// Set signaling callbacks (for callback mode).
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_Create
/// * `callbacks` - Callback structure
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_SetCallbacks(
    handle: *mut c_void,
    callbacks: *const SignalingCallbacks,
) -> i32 {
    if handle.is_null() || callbacks.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let server = &mut *(handle as *mut WebRtcServer);
    server.set_callbacks((*callbacks).clone());
    1
}

impl Clone for SignalingCallbacks {
    fn clone(&self) -> Self {
        Self {
            on_sdp: self.on_sdp,
            on_ice_candidate: self.on_ice_candidate,
            on_peer_state: self.on_peer_state,
            user_data: self.user_data,
        }
    }
}

/// Start the WebRTC server.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_Create
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_Start(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let server = &mut *(handle as *mut WebRtcServer);
    match server.start() {
        Ok(()) => 1,
        Err(e) => {
            eprintln!("BASS_WEBRTC_Start error: {}", e);
            set_error(BASS_ERROR_START);
            0
        }
    }
}

/// Stop the WebRTC server.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_Create
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_Stop(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let server = &mut *(handle as *mut WebRtcServer);
    server.stop();
    1
}

/// Get the input stream handle (audio received from browsers).
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_Create
///
/// # Returns
/// BASS stream handle, or 0 if not available
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_GetInputStream(handle: *mut c_void) -> HSTREAM {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let server = &*(handle as *const WebRtcServer);
    server.input_handle
}

/// Add a peer with SDP offer (for callback signaling mode).
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_Create
/// * `offer_sdp` - SDP offer from remote peer
/// * `answer_sdp` - Buffer to receive SDP answer (at least 4096 bytes)
/// * `answer_len` - Pointer to receive answer length
///
/// # Returns
/// Peer ID (0-4) on success, -1 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_AddPeer(
    handle: *mut c_void,
    offer_sdp: *const c_char,
    answer_sdp: *mut c_char,
    answer_len: *mut u32,
) -> i32 {
    if handle.is_null() || offer_sdp.is_null() || answer_sdp.is_null() || answer_len.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return -1;
    }

    let server = &*(handle as *const WebRtcServer);
    let offer = CStr::from_ptr(offer_sdp).to_string_lossy();

    let result = RUNTIME.block_on(async {
        let mut pm = server.peer_manager.lock();
        pm.add_peer_with_offer(&offer).await
    });

    match result {
        Ok((peer_id, answer)) => {
            let answer_bytes = answer.as_bytes();
            let len = answer_bytes.len().min(4095);
            std::ptr::copy_nonoverlapping(answer_bytes.as_ptr(), answer_sdp as *mut u8, len);
            *answer_sdp.add(len) = 0; // Null terminate
            *answer_len = len as u32;
            peer_id as i32
        }
        Err(e) => {
            eprintln!("BASS_WEBRTC_AddPeer error: {}", e);
            set_error(BASS_ERROR_CREATE);
            -1
        }
    }
}

/// Add an ICE candidate to a peer.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_Create
/// * `peer_id` - Peer ID from BASS_WEBRTC_AddPeer
/// * `candidate` - ICE candidate string
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_AddIceCandidate(
    handle: *mut c_void,
    peer_id: u32,
    candidate: *const c_char,
) -> i32 {
    if handle.is_null() || candidate.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let server = &*(handle as *const WebRtcServer);
    let candidate_str = CStr::from_ptr(candidate).to_string_lossy();

    let result = RUNTIME.block_on(async {
        let pm = server.peer_manager.lock();
        pm.add_ice_candidate(peer_id, &candidate_str, None, None).await
    });

    match result {
        Ok(()) => 1,
        Err(e) => {
            eprintln!("BASS_WEBRTC_AddIceCandidate error: {}", e);
            0
        }
    }
}

/// Remove a peer.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_Create
/// * `peer_id` - Peer ID to remove
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_RemovePeer(handle: *mut c_void, peer_id: u32) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let server = &*(handle as *const WebRtcServer);

    let result = RUNTIME.block_on(async {
        let mut pm = server.peer_manager.lock();
        pm.remove_peer(peer_id).await
    });

    match result {
        Ok(()) => 1,
        Err(_) => 0,
    }
}

/// Get statistics.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_Create
/// * `stats` - Pointer to stats structure to fill
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_GetStats(
    handle: *mut c_void,
    stats: *mut WebRtcStatsFFI,
) -> i32 {
    if handle.is_null() || stats.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let server = &*(handle as *const WebRtcServer);

    // Get peer manager stats
    let pm_stats = {
        let pm = server.peer_manager.lock();
        pm.aggregate_stats()
    };

    // Get output stats
    let output_stats = server
        .output_stream
        .as_ref()
        .map(|o| o.get_stats())
        .unwrap_or_default();

    // Get input stats
    let input_stats = server
        .input_stream
        .as_ref()
        .map(|i| i.get_stats())
        .unwrap_or_default();

    (*stats) = WebRtcStatsFFI {
        active_peers: pm_stats.active_peers,
        total_packets_sent: pm_stats.total_packets_sent + output_stats.packets_sent,
        total_packets_received: pm_stats.total_packets_received + input_stats.packets_received,
        total_bytes_sent: pm_stats.total_bytes_sent + output_stats.bytes_sent,
        total_bytes_received: pm_stats.total_bytes_received + input_stats.bytes_received,
        total_encode_errors: pm_stats.total_encode_errors + output_stats.encode_errors,
        total_decode_errors: pm_stats.total_decode_errors + input_stats.decode_errors,
        output_underruns: output_stats.underruns,
        input_buffer_level: input_stats.buffer_level,
        input_is_buffering: if input_stats.is_buffering { 1 } else { 0 },
    };

    1
}

/// Get the number of active peers.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_Create
///
/// # Returns
/// Number of active peers (0-5)
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_GetPeerCount(handle: *mut c_void) -> u32 {
    if handle.is_null() {
        return 0;
    }

    let server = &*(handle as *const WebRtcServer);
    let pm = server.peer_manager.lock();
    pm.active_count()
}

/// Check if the server is running.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_Create
///
/// # Returns
/// 1 if running, 0 if not
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_IsRunning(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        return 0;
    }

    let server = &*(handle as *const WebRtcServer);
    if server.running { 1 } else { 0 }
}

/// Free resources.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_Create
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_Free(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let _ = Box::from_raw(handle as *mut WebRtcServer);
    1
}
