//! bass-webrtc-ndi - WebRTC to NDI video/audio bridge
//!
//! This crate provides all bass-webrtc functionality PLUS NDI video output.
//!
//! Features:
//! - WebRTC peer connection management (up to 5 simultaneous peers)
//! - WHIP/WHEP client and server support
//! - WebSocket-based signaling with room support
//! - Audio encoding/decoding with OPUS
//! - Video decoding with FFmpeg (H.264, VP8, VP9)
//! - Lock-free audio mixing from multiple peers
//! - NDI video output
//!
//! Audio output: BASS channel (user controls playback destination)
//! Video output: NDI (via NdiSender)

#![allow(dead_code)]

// NDI modules (existing from Phase 1)
pub mod frame;
pub mod sender;

// bass-webrtc modules (copied)
pub mod ffi;
pub mod codec;
pub mod ice;
pub mod peer;
pub mod stream;
pub mod signaling;

// Re-exports from NDI modules
pub use frame::{AudioFrame, VideoFrame, VideoFormat};
pub use sender::{NdiSender, NdiError, init_ndi};

// Re-exports from bass-webrtc modules
pub use codec::{AudioFormat, CodecError};
pub use codec::video::{VideoCodec, VideoDecoder, is_available as is_ffmpeg_available};
pub use peer::{IceServerConfig, PeerManager, WebRtcPeer, MAX_PEERS};
pub use stream::{WebRtcInputStream, WebRtcOutputStream, input_stream_proc};
pub use signaling::{
    WhepClient, WhipClient,
    WhepEndpoint, WhipEndpoint,
    WhepConfig, WhipConfig,
    SignalingServer,
    SignalingCallbacks,
    WhepNdiClient, WhepNdiClientStats, WhepNdiConfig,
};
pub use ice::google_stun_servers;

// Global tokio runtime for async operations
use std::sync::Arc;
use lazy_static::lazy_static;
use tokio::runtime::Runtime;

lazy_static! {
    /// Global tokio runtime shared across all async operations.
    /// This allows FFI functions to use async code.
    pub static ref RUNTIME: Arc<Runtime> = Arc::new(
        Runtime::new().expect("Failed to create tokio runtime")
    );
}
