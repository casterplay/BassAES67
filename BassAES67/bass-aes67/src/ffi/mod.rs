//! FFI module for BASS audio library bindings.
//! Contains type definitions and function bindings for both BASS core and add-on API.

// Allow unused code in FFI modules - types kept for API completeness
#![allow(dead_code)]

pub mod bass;
pub mod addon;

pub use bass::*;
pub use addon::*;
