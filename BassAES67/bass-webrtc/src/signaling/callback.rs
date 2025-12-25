//! Callback-based signaling for WebRTC.
//!
//! Allows users to provide FFI callbacks for SDP and ICE candidate exchange.

use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::Arc;

/// Callback for sending SDP offer/answer to remote peer.
///
/// # Arguments
/// * `peer_id` - Peer identifier (0-4)
/// * `sdp_type` - "offer" or "answer"
/// * `sdp` - SDP content (null-terminated)
/// * `user` - User data pointer
pub type SdpCallback = unsafe extern "C" fn(
    peer_id: u32,
    sdp_type: *const c_char,
    sdp: *const c_char,
    user: *mut c_void,
);

/// Callback for sending ICE candidate to remote peer.
///
/// # Arguments
/// * `peer_id` - Peer identifier (0-4)
/// * `candidate` - ICE candidate string (null-terminated)
/// * `sdp_mid` - SDP media ID (null-terminated, may be null)
/// * `sdp_mline_index` - SDP media line index
/// * `user` - User data pointer
pub type IceCandidateCallback = unsafe extern "C" fn(
    peer_id: u32,
    candidate: *const c_char,
    sdp_mid: *const c_char,
    sdp_mline_index: u32,
    user: *mut c_void,
);

/// Callback for peer state changes.
///
/// # Arguments
/// * `peer_id` - Peer identifier (0-4)
/// * `state` - New state (PEER_STATE_* constants)
/// * `user` - User data pointer
pub type PeerStateCallback = unsafe extern "C" fn(
    peer_id: u32,
    state: u32,
    user: *mut c_void,
);

/// Signaling callbacks structure (FFI-safe)
#[repr(C)]
pub struct SignalingCallbacks {
    /// Called when we have an SDP offer/answer to send
    pub on_sdp: Option<SdpCallback>,
    /// Called when we have an ICE candidate to send
    pub on_ice_candidate: Option<IceCandidateCallback>,
    /// Called when peer connection state changes
    pub on_peer_state: Option<PeerStateCallback>,
    /// User data pointer passed to all callbacks
    pub user_data: *mut c_void,
}

// Safety: Callbacks are function pointers, user_data is opaque
unsafe impl Send for SignalingCallbacks {}
unsafe impl Sync for SignalingCallbacks {}

impl Default for SignalingCallbacks {
    fn default() -> Self {
        Self {
            on_sdp: None,
            on_ice_candidate: None,
            on_peer_state: None,
            user_data: std::ptr::null_mut(),
        }
    }
}

impl SignalingCallbacks {
    /// Check if any callbacks are registered
    pub fn has_callbacks(&self) -> bool {
        self.on_sdp.is_some() || self.on_ice_candidate.is_some() || self.on_peer_state.is_some()
    }

    /// Send an SDP message via callback
    pub fn send_sdp(&self, peer_id: u32, sdp_type: &str, sdp: &str) {
        if let Some(callback) = self.on_sdp {
            if let (Ok(type_cstr), Ok(sdp_cstr)) = (CString::new(sdp_type), CString::new(sdp)) {
                unsafe {
                    callback(peer_id, type_cstr.as_ptr(), sdp_cstr.as_ptr(), self.user_data);
                }
            }
        }
    }

    /// Send an ICE candidate via callback
    pub fn send_ice_candidate(
        &self,
        peer_id: u32,
        candidate: &str,
        sdp_mid: Option<&str>,
        sdp_mline_index: u32,
    ) {
        if let Some(callback) = self.on_ice_candidate {
            if let Ok(candidate_cstr) = CString::new(candidate) {
                let sdp_mid_cstr = sdp_mid.and_then(|s| CString::new(s).ok());
                let sdp_mid_ptr = sdp_mid_cstr
                    .as_ref()
                    .map(|s| s.as_ptr())
                    .unwrap_or(std::ptr::null());

                unsafe {
                    callback(
                        peer_id,
                        candidate_cstr.as_ptr(),
                        sdp_mid_ptr,
                        sdp_mline_index,
                        self.user_data,
                    );
                }
            }
        }
    }

    /// Notify peer state change via callback
    pub fn notify_peer_state(&self, peer_id: u32, state: u32) {
        if let Some(callback) = self.on_peer_state {
            unsafe {
                callback(peer_id, state, self.user_data);
            }
        }
    }
}

/// Signaling handler that wraps callbacks and provides async interface
pub struct CallbackSignaling {
    callbacks: SignalingCallbacks,
}

impl CallbackSignaling {
    /// Create a new callback signaling handler
    pub fn new(callbacks: SignalingCallbacks) -> Self {
        Self { callbacks }
    }

    /// Get reference to callbacks
    pub fn callbacks(&self) -> &SignalingCallbacks {
        &self.callbacks
    }

    /// Send SDP offer
    pub fn send_offer(&self, peer_id: u32, sdp: &str) {
        self.callbacks.send_sdp(peer_id, "offer", sdp);
    }

    /// Send SDP answer
    pub fn send_answer(&self, peer_id: u32, sdp: &str) {
        self.callbacks.send_sdp(peer_id, "answer", sdp);
    }

    /// Send ICE candidate
    pub fn send_ice_candidate(
        &self,
        peer_id: u32,
        candidate: &str,
        sdp_mid: Option<&str>,
        sdp_mline_index: u32,
    ) {
        self.callbacks.send_ice_candidate(peer_id, candidate, sdp_mid, sdp_mline_index);
    }

    /// Notify state change
    pub fn notify_state(&self, peer_id: u32, state: u32) {
        self.callbacks.notify_peer_state(peer_id, state);
    }
}
