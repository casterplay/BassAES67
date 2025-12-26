//! bass-webrtc-ndi - WebRTC to NDI video/audio bridge
//!
//! This crate provides NDI output capabilities for WebRTC streams,
//! enabling video and audio from WebRTC to be sent over NDI networks.

pub mod frame;
pub mod sender;

pub use frame::{AudioFrame, VideoFrame, VideoFormat};
pub use sender::{NdiSender, NdiError, init_ndi};
