//! RTP Output module - Z/IP ONE connects TO us.
//!
//! This module implements bidirectional RTP streaming where we listen
//! for incoming connections from a remote device (like Z/IP ONE):
//! - We RECEIVE their audio FROM them
//! - We SEND backfeed audio TO them
//!
//! API:
//! - Takes a BASS channel handle for backfeed audio (what we send back)
//! - Returns a BASS channel handle for incoming audio (what we receive)
//!
//! The remote address is auto-detected from the first incoming RTP packet.

mod stream;

pub use stream::{
    RtpOutput,
    RtpOutputConfig,
    RtpOutputStats,
    ConnectionState,
    ConnectionCallback,
    output_incoming_stream_proc,
};
