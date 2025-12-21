//! SRT input module - receives SRT streams and feeds PCM audio into BASS.

pub mod stream;
pub mod url;

pub use stream::SrtStream;
pub use stream::ADDON_FUNCS;
pub use url::SrtUrl;
pub use url::ConnectionMode;
