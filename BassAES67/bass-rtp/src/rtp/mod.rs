//! RTP (Real-time Transport Protocol) module.
//!
//! Provides RTP packet parsing, building, payload type handling,
//! and bidirectional UDP socket management.

pub mod header;
pub mod payload;
pub mod socket;

pub use header::*;
pub use payload::*;
pub use socket::*;
