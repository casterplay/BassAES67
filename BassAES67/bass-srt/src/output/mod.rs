//! SRT output module - sends PCM audio from BASS via SRT.

pub mod encoder;
pub mod stream;

pub use stream::{
    SrtOutputStream, SrtOutputConfig, OutputStats,
    ConnectionMode, OutputCodec,
    OutputConnectionStateCallback,
    set_output_connection_state_callback, clear_output_connection_state_callback,
    CONNECTION_STATE_DISCONNECTED, CONNECTION_STATE_CONNECTING,
    CONNECTION_STATE_CONNECTED, CONNECTION_STATE_RECONNECTING,
};
pub use encoder::{AudioEncoder, create_encoder};
