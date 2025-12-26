//! Signaling module for WebRTC connection establishment.
//!
//! Supports multiple signaling modes:
//! - Callback-based: User provides FFI callbacks for SDP/ICE exchange
//! - WHIP Server: HTTP POST-based ingress signaling (RFC 9725)
//! - WHEP Server: HTTP-based egress signaling
//! - WHIP Client: Connect to external WHIP server (push audio)
//! - WHEP Client: Connect to external WHEP server (pull audio)
//! - WHEP NDI Client: WHEP client with video output to NDI
//! - WebSocket Signaling: Pure message relay for true bidirectional WebRTC
//! - WebSocket Peer: WebRTC peer that uses WebSocket signaling

pub mod callback;
pub mod whip;
pub mod whep;
pub mod whip_client;
pub mod whep_client;
pub mod whep_ndi_client;
pub mod ws_signaling_server;
pub mod ws_peer;

pub use callback::*;
pub use whip::*;
pub use whep::*;
pub use whip_client::*;
pub use whep_client::*;
pub use whep_ndi_client::*;
pub use ws_signaling_server::*;
pub use ws_peer::*;
