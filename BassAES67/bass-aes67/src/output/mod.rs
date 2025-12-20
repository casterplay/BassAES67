//! AES67 output module.
//! Provides functionality to transmit audio from BASS channels over AES67/RTP multicast.

mod rtp;
pub mod stream;

pub use stream::{Aes67OutputStream, Aes67OutputConfig, OutputStats};
