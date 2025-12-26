//! Audio stream handling for WebRTC.
//!
//! - `output`: BASS channel -> WebRTC (send to browsers)
//! - `input`: WebRTC -> BASS channel (receive from browsers)

pub mod output;
pub mod input;

pub use output::*;
pub use input::*;
