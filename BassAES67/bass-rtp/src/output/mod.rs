//! RTP output module - We connect TO Z/IP ONE.
//!
//! This module implements bidirectional RTP streaming where we initiate
//! the connection to a remote device (like Z/IP ONE) and handle both
//! sending our audio and receiving return audio.
//!
//! Key differences from the "stream" (input) module:
//! - We know the remote address from config (not learned from incoming packets)
//! - Primary direction is send (our audio to them)
//! - Return audio is secondary (their reply back to us)

mod stream;

pub use stream::{
    RtpOutputBidirectional,
    RtpOutputConfig,
    OutputBidirectionalStats,
};
