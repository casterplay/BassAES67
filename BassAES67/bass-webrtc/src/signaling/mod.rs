//! Signaling module for WebRTC connection establishment.
//!
//! Supports multiple signaling modes:
//! - Callback-based: User provides FFI callbacks for SDP/ICE exchange
//! - WHIP: HTTP POST-based ingress signaling (RFC 9725)
//! - WHEP: HTTP-based egress signaling

pub mod callback;
pub mod whip;
pub mod whep;

pub use callback::*;
pub use whip::*;
pub use whep::*;
