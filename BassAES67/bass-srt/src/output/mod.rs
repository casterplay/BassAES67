//! SRT output module - sends PCM audio from BASS via SRT.
//! This module will be implemented in a later phase.

pub mod stream;

pub use stream::{SrtOutputStream, SrtOutputConfig, OutputStats};
