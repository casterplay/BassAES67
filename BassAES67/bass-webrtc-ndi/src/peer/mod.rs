//! Peer connection management for WebRTC.
//!
//! Handles RTCPeerConnection lifecycle and multi-peer support.

pub mod connection;
pub mod manager;

pub use connection::*;
pub use manager::*;
