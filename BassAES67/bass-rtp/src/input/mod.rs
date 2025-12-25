//! RTP Input module - WE connect TO Z/IP ONE.
//!
//! This module implements bidirectional RTP streaming where we initiate
//! the connection to a remote device (like Z/IP ONE):
//! - We SEND our audio TO them
//! - We RECEIVE return audio FROM them
//!
//! API:
//! - Takes a BASS channel handle for outgoing audio (what we send)
//! - Returns a BASS channel handle for incoming audio (what we receive)

mod stream;

pub use stream::{
    RtpInput,
    RtpInputConfig,
    RtpInputStats,
    BufferMode,
    input_return_stream_proc,
};
