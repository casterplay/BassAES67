//! Signaling module for WebRTC connection establishment.
//!
//! Supports multiple signaling modes:
//! - Callback-based: User provides FFI callbacks for SDP/ICE exchange
//! - WHIP Server: HTTP POST-based ingress signaling (RFC 9725)
//! - WHEP Server: HTTP-based egress signaling
//! - WHIP Client: Connect to external WHIP server (push audio)
//! - WHEP Client: Connect to external WHEP server (pull audio)

pub mod callback;
pub mod whip;
pub mod whep;
pub mod whip_client;
pub mod whep_client;

pub use callback::*;
pub use whip::*;
pub use whep::*;
pub use whip_client::*;
pub use whep_client::*;
