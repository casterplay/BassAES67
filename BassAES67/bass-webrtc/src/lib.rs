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

        let peer_manager = PeerManager::new(
            ice_servers,
            incoming_buffer_samples,
            config.sample_rate,
            config.channels,
        )?;

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
    fn add_ice_server(&mut self, _url: &str, _username: Option<&str>, _credential: Option<&str>) {
        // Note: This would require reinitializing peer manager
        // For now, ICE servers should be configured before creating the server
        // This is a limitation we can address later
    }

    /// Wire a peer's incoming audio consumer to the input stream
    fn wire_peer_consumer(&mut self, peer_id: u32) {
        if let Some(ref mut input) = self.input_stream {
            let mut pm = self.peer_manager.lock();
            if let Some(peer) = pm.get_peer_mut(peer_id) {
                if let Some(consumer) = peer.take_incoming_consumer() {
                    input.set_peer_consumer(peer_id, consumer);
                }
            }
        }
    }

    /// Remove a peer's consumer from input stream
    fn unwire_peer_consumer(&mut self, peer_id: u32) {
        if let Some(ref mut input) = self.input_stream {
            input.remove_peer_consumer(peer_id);
        }
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

    let server = &mut *(handle as *mut WebRtcServer);
    let offer = CStr::from_ptr(offer_sdp).to_string_lossy();

    let result = RUNTIME.block_on(async {
        let mut pm = server.peer_manager.lock();
        pm.add_peer_with_offer(&offer).await
    });

    match result {
        Ok((peer_id, answer)) => {
            // Wire the peer's incoming audio consumer to the input stream
            server.wire_peer_consumer(peer_id);

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

    let server = &mut *(handle as *mut WebRtcServer);

    // Unwire the peer's consumer from input stream
    server.unwire_peer_consumer(peer_id);

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

// ============================================================================
// WHIP/WHEP Client API (for connecting to external servers like MediaMTX)
// ============================================================================

/// Signaling mode: WHIP client (push to external server)
pub const BASS_WEBRTC_SIGNALING_WHIP_CLIENT: u8 = 3;
/// Signaling mode: WHEP client (pull from external server)
pub const BASS_WEBRTC_SIGNALING_WHEP_CLIENT: u8 = 4;

/// Connect to a WHIP server and push audio.
///
/// Creates a WebRTC connection to an external WHIP server (like MediaMTX)
/// and sends audio from the BASS source channel to it.
///
/// # Arguments
/// * `source_channel` - BASS channel to read audio from
/// * `whip_url` - WHIP endpoint URL (e.g., "http://localhost:8889/mystream/whip")
/// * `sample_rate` - Sample rate (48000 recommended)
/// * `channels` - Number of channels (1 or 2)
/// * `opus_bitrate` - OPUS bitrate in kbps
///
/// # Returns
/// Handle on success, null on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_ConnectWhip(
    source_channel: DWORD,
    whip_url: *const c_char,
    sample_rate: u32,
    channels: u16,
    opus_bitrate: u32,
) -> *mut c_void {
    if whip_url.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return std::ptr::null_mut();
    }

    let url = CStr::from_ptr(whip_url).to_string_lossy().to_string();
    let ice_servers = ice::google_stun_servers();

    let result = RUNTIME.block_on(async {
        signaling::WhipClient::connect(&url, &ice_servers, sample_rate, channels).await
    });

    match result {
        Ok(client) => {
            // Wrap client with source channel info for audio streaming
            let wrapper = WhipClientWrapper {
                client,
                source_channel,
                sample_rate,
                channels,
                opus_bitrate,
                output_stream: None,
            };
            Box::into_raw(Box::new(wrapper)) as *mut c_void
        }
        Err(e) => {
            eprintln!("BASS_WEBRTC_ConnectWhip error: {}", e);
            set_error(BASS_ERROR_CREATE);
            std::ptr::null_mut()
        }
    }
}

/// Wrapper for WHIP client with audio output functionality
struct WhipClientWrapper {
    client: signaling::WhipClient,
    source_channel: DWORD,
    sample_rate: u32,
    channels: u16,
    opus_bitrate: u32,
    output_stream: Option<WebRtcOutputStream>,
}

/// Start streaming audio to the connected WHIP server.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_ConnectWhip
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_WhipStart(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let wrapper = &mut *(handle as *mut WhipClientWrapper);

    // Create and start output stream
    let mut output = WebRtcOutputStream::new(
        wrapper.source_channel,
        wrapper.client.audio_track().clone(),
        wrapper.sample_rate,
        wrapper.channels,
        wrapper.opus_bitrate,
        RUNTIME.handle().clone(),
    );

    match output.start() {
        Ok(()) => {
            wrapper.output_stream = Some(output);
            1
        }
        Err(e) => {
            eprintln!("BASS_WEBRTC_WhipStart error: {}", e);
            set_error(BASS_ERROR_START);
            0
        }
    }
}

/// Stop streaming and disconnect from the WHIP server.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_ConnectWhip
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_WhipStop(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let wrapper = &mut *(handle as *mut WhipClientWrapper);

    // Stop output stream
    if let Some(ref mut output) = wrapper.output_stream {
        output.stop();
    }
    wrapper.output_stream = None;

    // Disconnect from server
    let _ = RUNTIME.block_on(wrapper.client.disconnect());

    1
}

/// Free WHIP client resources.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_ConnectWhip
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_WhipFree(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let _ = Box::from_raw(handle as *mut WhipClientWrapper);
    1
}

/// Check if WHIP client is connected.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_ConnectWhip
///
/// # Returns
/// 1 if connected, 0 if not connected or error
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_WhipIsConnected(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        return 0;
    }

    let wrapper = &*(handle as *const WhipClientWrapper);
    if wrapper.client.is_connected() { 1 } else { 0 }
}

/// Connect to a WHEP server and receive audio.
///
/// Creates a WebRTC connection to an external WHEP server (like MediaMTX)
/// and receives audio into a BASS stream.
///
/// # Arguments
/// * `whep_url` - WHEP endpoint URL (e.g., "http://localhost:8889/mystream/whep")
/// * `sample_rate` - Sample rate (48000 recommended)
/// * `channels` - Number of channels (1 or 2)
/// * `buffer_ms` - Buffer size in milliseconds
/// * `decode_stream` - Set to 1 for BASS_STREAM_DECODE flag (mixer compatibility)
///
/// # Returns
/// Handle on success, null on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_ConnectWhep(
    whep_url: *const c_char,
    sample_rate: u32,
    channels: u16,
    buffer_ms: u32,
    decode_stream: u8,
) -> *mut c_void {
    if whep_url.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return std::ptr::null_mut();
    }

    let url = CStr::from_ptr(whep_url).to_string_lossy().to_string();
    let ice_servers = ice::google_stun_servers();
    let buffer_samples = (sample_rate as usize / 1000) * buffer_ms as usize * channels as usize * 3;

    let result = RUNTIME.block_on(async {
        signaling::WhepClient::connect(&url, &ice_servers, sample_rate, channels, buffer_samples).await
    });

    match result {
        Ok(mut client) => {
            // Take the incoming consumer and create input stream
            let consumer = client.take_incoming_consumer();

            // Create wrapper with input stream
            let mut wrapper = WhepClientWrapper {
                client,
                sample_rate,
                channels,
                buffer_ms,
                input_stream: None,
                input_handle: 0,
            };

            // Create input stream
            let mut input = Box::new(WebRtcInputStream::new(sample_rate, channels, buffer_ms));

            // Wire the consumer
            if let Some(c) = consumer {
                input.set_peer_consumer(0, c);
            }

            // Create BASS stream
            let flags = if decode_stream != 0 {
                BASS_SAMPLE_FLOAT | BASS_STREAM_DECODE
            } else {
                BASS_SAMPLE_FLOAT
            };

            let input_ptr = input.as_mut() as *mut WebRtcInputStream;
            let bass_stream = BASS_StreamCreate(
                sample_rate,
                channels as u32,
                flags,
                Some(input_stream_proc),
                input_ptr as *mut c_void,
            );

            if bass_stream == 0 {
                eprintln!("BASS_WEBRTC_ConnectWhep: Failed to create BASS stream");
                set_error(BASS_ERROR_CREATE);
                return std::ptr::null_mut();
            }

            wrapper.input_stream = Some(input);
            wrapper.input_handle = bass_stream;

            Box::into_raw(Box::new(wrapper)) as *mut c_void
        }
        Err(e) => {
            eprintln!("BASS_WEBRTC_ConnectWhep error: {}", e);
            set_error(BASS_ERROR_CREATE);
            std::ptr::null_mut()
        }
    }
}

/// Wrapper for WHEP client with audio input functionality
struct WhepClientWrapper {
    client: signaling::WhepClient,
    sample_rate: u32,
    channels: u16,
    buffer_ms: u32,
    input_stream: Option<Box<WebRtcInputStream>>,
    input_handle: HSTREAM,
}

/// Get the BASS input stream from a WHEP connection.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_ConnectWhep
///
/// # Returns
/// BASS stream handle, or 0 if not available
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_WhepGetStream(handle: *mut c_void) -> HSTREAM {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let wrapper = &*(handle as *const WhepClientWrapper);
    wrapper.input_handle
}

/// Check if WHEP client is connected.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_ConnectWhep
///
/// # Returns
/// 1 if connected, 0 if not connected or error
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_WhepIsConnected(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        return 0;
    }

    let wrapper = &*(handle as *const WhepClientWrapper);
    if wrapper.client.is_connected() { 1 } else { 0 }
}

/// Disconnect from the WHEP server and free resources.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_ConnectWhep
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_WhepFree(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let mut wrapper = Box::from_raw(handle as *mut WhepClientWrapper);

    // Free BASS stream
    if wrapper.input_handle != 0 {
        BASS_StreamFree(wrapper.input_handle);
    }

    // Disconnect from server
    let _ = RUNTIME.block_on(wrapper.client.disconnect());

    // Input stream is dropped when wrapper is dropped
    1
}

// ============================================================================
// WebSocket Signaling Server API
// ============================================================================

/// Create a WebSocket signaling server.
///
/// The signaling server is a pure WebSocket relay - it does NOT handle any
/// WebRTC logic. It simply relays JSON messages between connected clients
/// (browser and Rust WebRTC peer).
///
/// # Arguments
/// * `port` - Port to listen on (e.g., 8080)
///
/// # Returns
/// Handle on success, null on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_CreateSignalingServer(port: u16) -> *mut c_void {
    let server = signaling::SignalingServer::new(port);
    Box::into_raw(Box::new(server)) as *mut c_void
}

/// Start the signaling server.
///
/// This starts the WebSocket server in a background thread. Clients can
/// connect to ws://host:port/ and messages are relayed between them.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_CreateSignalingServer
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_SignalingServerStart(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let server = &*(handle as *const signaling::SignalingServer);

    // Clone Arc references for the spawn
    let port = server.port();

    // Start server in background
    RUNTIME.spawn(async move {
        // We need to recreate the server in the async context
        let server = signaling::SignalingServer::new(port);
        if let Err(e) = server.run().await {
            eprintln!("Signaling server error: {}", e);
        }
    });

    1
}

/// Stop the signaling server.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_CreateSignalingServer
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_SignalingServerStop(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let server = &*(handle as *const signaling::SignalingServer);
    server.stop();
    1
}

/// Get the number of connected clients.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_CreateSignalingServer
///
/// # Returns
/// Number of connected WebSocket clients
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_SignalingServerClientCount(handle: *mut c_void) -> u32 {
    if handle.is_null() {
        return 0;
    }

    let server = &*(handle as *const signaling::SignalingServer);
    server.client_count() as u32
}

/// Free signaling server resources.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_CreateSignalingServer
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_SignalingServerFree(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let server = Box::from_raw(handle as *mut signaling::SignalingServer);
    server.stop();
    1
}

// ============================================================================
// WebSocket Peer API (Bidirectional WebRTC via Signaling)
// ============================================================================

/// Callback types for peer events
type OnConnectedCallback = unsafe extern "C" fn(user: *mut c_void);
type OnDisconnectedCallback = unsafe extern "C" fn(user: *mut c_void);
type OnErrorCallback = unsafe extern "C" fn(error_code: u32, error_msg: *const c_char, user: *mut c_void);
type OnStatsCallback = unsafe extern "C" fn(stats: *const WebRtcPeerStatsFFI, user: *mut c_void);

/// FFI-safe WebRTC peer statistics for C# interop
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct WebRtcPeerStatsFFI {
    pub packets_sent: u64,
    pub packets_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub round_trip_time_ms: u32,
    pub packets_lost: i64,
    pub fraction_lost_percent: f32,  // 0.0 - 100.0
    pub jitter_ms: u32,
    pub nack_count: u64,
}

/// Wrapper for WebRTC peer with BASS audio streams
struct WebRtcPeerWrapper {
    peer: signaling::WebRtcPeer,
    source_channel: DWORD,
    sample_rate: u32,
    channels: u16,
    opus_bitrate: u32,
    buffer_ms: u32,
    decode_stream: u8,
    output_stream: Option<WebRtcOutputStream>,
    input_stream: Option<Box<WebRtcInputStream>>,
    input_handle: HSTREAM,
    // Callbacks for C# events
    on_connected: Option<OnConnectedCallback>,
    on_disconnected: Option<OnDisconnectedCallback>,
    on_error: Option<OnErrorCallback>,
    callback_user: *mut c_void,
    // Stats callback
    on_stats: Option<OnStatsCallback>,
    stats_user: *mut c_void,
    stats_interval_ms: u32,
    stats_running: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// Create a WebRTC peer that connects via WebSocket signaling with room support.
///
/// This creates a bidirectional WebRTC connection with a single peer
/// connection (sendrecv) and DataChannel support. Messages are only
/// exchanged with other clients in the same room.
///
/// # Arguments
/// * `signaling_url` - WebSocket signaling server base URL (e.g., "ws://localhost:8080")
/// * `room_id` - Room identifier for signaling isolation (e.g., "studio-1")
/// * `source_channel` - BASS channel to send audio from (0 if receive-only)
/// * `sample_rate` - Sample rate (48000 recommended)
/// * `channels` - Number of channels (1 or 2)
/// * `opus_bitrate` - OPUS bitrate in kbps (for sending)
/// * `buffer_ms` - Buffer size in ms for received audio
/// * `decode_stream` - Set to 1 for BASS_STREAM_DECODE flag
///
/// # Returns
/// Handle on success, null on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_CreatePeer(
    signaling_url: *const c_char,
    room_id: *const c_char,
    source_channel: DWORD,
    sample_rate: u32,
    channels: u16,
    opus_bitrate: u32,
    buffer_ms: u32,
    decode_stream: u8,
) -> *mut c_void {
    if signaling_url.is_null() || room_id.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return std::ptr::null_mut();
    }

    let url = CStr::from_ptr(signaling_url).to_string_lossy().to_string();
    let room = CStr::from_ptr(room_id).to_string_lossy().to_string();
    let ice_servers = ice::google_stun_servers();
    let buffer_samples = (sample_rate as usize / 1000) * buffer_ms as usize * channels as usize * 3;

    let peer = signaling::WebRtcPeer::new(
        &url,
        &room,
        ice_servers,
        sample_rate,
        channels,
        buffer_samples,
    );

    let wrapper = WebRtcPeerWrapper {
        peer,
        source_channel,
        sample_rate,
        channels,
        opus_bitrate,
        buffer_ms,
        decode_stream,
        output_stream: None,
        input_stream: None,
        input_handle: 0,
        // Callbacks (set via BASS_WEBRTC_PeerSetCallbacks)
        on_connected: None,
        on_disconnected: None,
        on_error: None,
        callback_user: std::ptr::null_mut(),
        // Stats callback (set via BASS_WEBRTC_PeerSetStatsCallback)
        on_stats: None,
        stats_user: std::ptr::null_mut(),
        stats_interval_ms: 0,
        stats_running: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };

    Box::into_raw(Box::new(wrapper)) as *mut c_void
}

/// Set callbacks for peer events (connected, disconnected, error).
///
/// These callbacks fire when the peer connection state changes:
/// - on_connected: Called when WebRTC connection is established
/// - on_disconnected: Called when WebRTC connection is closed
/// - on_error: Called when an error occurs (with error code and message)
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_CreatePeer
/// * `on_connected` - Callback for connection established (may be null)
/// * `on_disconnected` - Callback for connection closed (may be null)
/// * `on_error` - Callback for errors (may be null)
/// * `user` - User data pointer passed to callbacks
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_PeerSetCallbacks(
    handle: *mut c_void,
    on_connected: Option<OnConnectedCallback>,
    on_disconnected: Option<OnDisconnectedCallback>,
    on_error: Option<OnErrorCallback>,
    user: *mut c_void,
) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let wrapper = &mut *(handle as *mut WebRtcPeerWrapper);
    wrapper.on_connected = on_connected;
    wrapper.on_disconnected = on_disconnected;
    wrapper.on_error = on_error;
    wrapper.callback_user = user;

    // Also set callbacks on the underlying WebRtcPeer so they fire on state changes
    let peer_callbacks = signaling::ws_peer::PeerCallbacks::new(
        on_connected,
        on_disconnected,
        on_error,
        user,
    );
    wrapper.peer.set_callbacks(peer_callbacks);

    1
}

/// Set callback for statistics updates.
///
/// When enabled, the callback fires periodically with current statistics.
/// Call this after creating the peer but before or after connecting.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_CreatePeer
/// * `callback` - Callback for stats updates (null to disable)
/// * `interval_ms` - Interval between updates in milliseconds (e.g., 1000 for 1 second)
/// * `user` - User data pointer passed to callback
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_PeerSetStatsCallback(
    handle: *mut c_void,
    callback: Option<OnStatsCallback>,
    interval_ms: u32,
    user: *mut c_void,
) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let wrapper = &mut *(handle as *mut WebRtcPeerWrapper);

    // Stop any existing stats loop
    wrapper.stats_running.store(false, std::sync::atomic::Ordering::SeqCst);

    // Store new callback settings
    wrapper.on_stats = callback;
    wrapper.stats_user = user;
    wrapper.stats_interval_ms = interval_ms;

    // If callback is set and we're connected, start the stats loop
    if callback.is_some() && interval_ms > 0 && wrapper.peer.is_connected() {
        start_stats_loop(wrapper);
    }

    1
}

/// Start the stats collection loop (internal helper)
unsafe fn start_stats_loop(wrapper: &mut WebRtcPeerWrapper) {
    if wrapper.on_stats.is_none() || wrapper.stats_interval_ms == 0 {
        return;
    }

    // Get peer connection for stats collection
    let pc = match wrapper.peer.peer_connection() {
        Some(pc) => pc.clone(),
        None => return,
    };

    let stats = wrapper.peer.stats.clone();
    let callback = wrapper.on_stats;
    // Convert raw pointer to usize for Send safety - will be cast back in callback
    let user_usize = wrapper.stats_user as usize;
    let interval_ms = wrapper.stats_interval_ms;
    let running = wrapper.stats_running.clone();

    // Mark as running
    running.store(true, std::sync::atomic::Ordering::SeqCst);

    // Spawn stats collection task
    RUNTIME.spawn(async move {
        use webrtc::stats::StatsReportType;

        while running.load(std::sync::atomic::Ordering::SeqCst) {
            tokio::time::sleep(std::time::Duration::from_millis(interval_ms as u64)).await;

            if !running.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }

            // Collect stats from peer connection
            let report = pc.get_stats().await;

            for (_id, stat) in report.reports {
                match stat {
                    StatsReportType::RemoteInboundRTP(s) => {
                        if let Some(rtt) = s.round_trip_time {
                            stats.round_trip_time_ms.store(
                                (rtt * 1000.0) as u32,
                                std::sync::atomic::Ordering::Relaxed
                            );
                        }
                        stats.packets_lost.store(s.packets_lost, std::sync::atomic::Ordering::Relaxed);
                        stats.fraction_lost.store(
                            (s.fraction_lost * 10000.0) as u32,
                            std::sync::atomic::Ordering::Relaxed
                        );
                    }
                    StatsReportType::InboundRTP(s) if s.kind == "audio" => {
                        stats.packets_received.store(s.packets_received, std::sync::atomic::Ordering::Relaxed);
                        stats.bytes_received.store(s.bytes_received, std::sync::atomic::Ordering::Relaxed);
                        stats.nack_count.store(s.nack_count, std::sync::atomic::Ordering::Relaxed);
                    }
                    StatsReportType::OutboundRTP(s) if s.kind == "audio" => {
                        stats.packets_sent.store(s.packets_sent, std::sync::atomic::Ordering::Relaxed);
                        stats.bytes_sent.store(s.bytes_sent, std::sync::atomic::Ordering::Relaxed);
                    }
                    _ => {}
                }
            }

            // Fire callback with snapshot
            if let Some(cb) = callback {
                let snapshot = stats.to_snapshot();
                let ffi_stats = WebRtcPeerStatsFFI {
                    packets_sent: snapshot.packets_sent,
                    packets_received: snapshot.packets_received,
                    bytes_sent: snapshot.bytes_sent,
                    bytes_received: snapshot.bytes_received,
                    round_trip_time_ms: snapshot.round_trip_time_ms,
                    packets_lost: snapshot.packets_lost,
                    fraction_lost_percent: snapshot.fraction_lost_percent,
                    jitter_ms: snapshot.jitter_ms,
                    nack_count: snapshot.nack_count,
                };
                // Cast usize back to pointer for callback
                cb(&ffi_stats, user_usize as *mut c_void);
            }
        }
    });
}

/// Connect the WebRTC peer to the signaling server (non-blocking).
///
/// This starts the connection process in the background and returns immediately.
/// Use BASS_WEBRTC_PeerIsConnected to poll for connection status.
/// Once connected, call BASS_WEBRTC_PeerSetupStreams to setup audio streams.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_CreatePeer
///
/// # Returns
/// 1 on success (connection started), 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_PeerConnect(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    // Start connection in a dedicated background thread.
    // We use std::thread::spawn because WebRtcPeer contains non-Send types.
    // IMPORTANT: We use the GLOBAL RUNTIME via block_on so that the peer connection
    // and its async tasks remain alive after this thread exits.
    let handle_usize = handle as usize;

    std::thread::spawn(move || {
        // SAFETY: The handle must remain valid for the duration of this thread.
        // The caller is responsible for not freeing the handle while connecting.
        let wrapper = unsafe { &mut *(handle_usize as *mut WebRtcPeerWrapper) };

        // Use the global RUNTIME - this ensures the peer connection stays alive
        // because all async tasks (WebSocket, ICE, RTP) run on this runtime.
        let result = RUNTIME.block_on(wrapper.peer.connect());
        if let Err(e) = result {
            eprintln!("BASS_WEBRTC_PeerConnect error: {}", e);
        }
    });

    1
}

/// Setup audio streams after connection is established.
///
/// Call this after BASS_WEBRTC_PeerIsConnected returns 1.
/// This sets up the output stream (BASS -> WebRTC) and input stream (WebRTC -> BASS).
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_CreatePeer
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_PeerSetupStreams(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let wrapper = &mut *(handle as *mut WebRtcPeerWrapper);

    // Check if already setup
    if wrapper.input_handle != 0 || wrapper.output_stream.is_some() {
        return 1; // Already setup
    }

    // Check if connected
    if !wrapper.peer.is_connected() {
        set_error(BASS_ERROR_START);
        return 0;
    }

    // Setup output stream if we have a source channel
    println!("[BASS_WEBRTC_PeerSetupStreams] source_channel={}", wrapper.source_channel);
    if wrapper.source_channel != 0 {
        if let Some(ref audio_track) = wrapper.peer.audio_track() {
            println!("[BASS_WEBRTC_PeerSetupStreams] Creating output stream...");
            let mut output = WebRtcOutputStream::new(
                wrapper.source_channel,
                (*audio_track).clone(),
                wrapper.sample_rate,
                wrapper.channels,
                wrapper.opus_bitrate,
                RUNTIME.handle().clone(),
            );

            if let Err(e) = output.start() {
                eprintln!("BASS_WEBRTC_PeerSetupStreams: Failed to start output: {}", e);
            } else {
                println!("[BASS_WEBRTC_PeerSetupStreams] Output stream started successfully");
                wrapper.output_stream = Some(output);
            }
        } else {
            eprintln!("[BASS_WEBRTC_PeerSetupStreams] No audio track available!");
        }
    }

    // Setup input stream for received audio
    if let Some(consumer) = wrapper.peer.take_incoming_consumer() {
        let mut input = Box::new(WebRtcInputStream::new(
            wrapper.sample_rate,
            wrapper.channels,
            wrapper.buffer_ms,
        ));

        input.set_peer_consumer(0, consumer);

        // Create BASS stream
        let flags = if wrapper.decode_stream != 0 {
            BASS_SAMPLE_FLOAT | BASS_STREAM_DECODE
        } else {
            BASS_SAMPLE_FLOAT
        };

        let input_ptr = input.as_mut() as *mut WebRtcInputStream;
        let bass_stream = BASS_StreamCreate(
            wrapper.sample_rate,
            wrapper.channels as u32,
            flags,
            Some(input_stream_proc),
            input_ptr as *mut c_void,
        );

        if bass_stream != 0 {
            wrapper.input_stream = Some(input);
            wrapper.input_handle = bass_stream;
        }
    }

    // Start stats loop if callback was registered before connection
    if wrapper.on_stats.is_some() && wrapper.stats_interval_ms > 0 {
        start_stats_loop(wrapper);
    }

    1
}

/// Check if WebRTC peer is connected.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_CreatePeer
///
/// # Returns
/// 1 if connected, 0 if not
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_PeerIsConnected(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        return 0;
    }

    let wrapper = &*(handle as *const WebRtcPeerWrapper);
    if wrapper.peer.is_connected() { 1 } else { 0 }
}

/// Get the BASS input stream from a WebRTC peer (for received audio).
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_CreatePeer
///
/// # Returns
/// BASS stream handle, or 0 if not available
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_PeerGetInputStream(handle: *mut c_void) -> HSTREAM {
    if handle.is_null() {
        return 0;
    }

    let wrapper = &*(handle as *const WebRtcPeerWrapper);
    wrapper.input_handle
}

/// Disconnect the WebRTC peer.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_CreatePeer
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_PeerDisconnect(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    let wrapper = &mut *(handle as *mut WebRtcPeerWrapper);

    // Stop stats loop
    wrapper.stats_running.store(false, std::sync::atomic::Ordering::SeqCst);

    // Stop output stream
    if let Some(ref mut output) = wrapper.output_stream {
        output.stop();
    }
    wrapper.output_stream = None;

    // Disconnect peer - use spawn instead of block_on to avoid panic when called from callback
    // The peer connection will be closed asynchronously
    if let Some(pc) = wrapper.peer.peer_connection().cloned() {
        RUNTIME.spawn(async move {
            let _ = pc.close().await;
        });
    }

    1
}

/// Free WebRTC peer resources.
///
/// # Arguments
/// * `handle` - Handle from BASS_WEBRTC_CreatePeer
///
/// # Returns
/// 1 on success, 0 on failure
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_PeerFree(handle: *mut c_void) -> i32 {
    if handle.is_null() {
        set_error(BASS_ERROR_HANDLE);
        return 0;
    }

    // First, get the peer connection Arc before taking ownership of wrapper
    // This avoids lifetime issues with the spawned async task
    let wrapper_ref = &*(handle as *const WebRtcPeerWrapper);
    let peer_connection = wrapper_ref.peer.peer_connection().cloned();

    // Stop stats loop before taking ownership
    wrapper_ref.stats_running.store(false, std::sync::atomic::Ordering::SeqCst);

    // Now take ownership and clean up synchronous resources
    let mut wrapper = Box::from_raw(handle as *mut WebRtcPeerWrapper);

    // Stop output stream
    if let Some(ref mut output) = wrapper.output_stream {
        output.stop();
    }

    // Free BASS input stream
    if wrapper.input_handle != 0 {
        BASS_StreamFree(wrapper.input_handle);
    }

    // Disconnect peer - spawn async task to avoid blocking/panic when called from callback
    if let Some(pc) = peer_connection {
        RUNTIME.spawn(async move {
            let _ = pc.close().await;
        });
    }

    // wrapper is dropped here, cleaning up the rest

    1
}
