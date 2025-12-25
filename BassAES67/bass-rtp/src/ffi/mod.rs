//! FFI module for BASS audio library bindings.
//! Contains type definitions and function bindings for BASS core API.

// Allow unused code in FFI modules - types kept for API completeness
#![allow(dead_code)]

pub mod bass;

pub use bass::*;
