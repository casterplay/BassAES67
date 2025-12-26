//! ICE (Interactive Connectivity Establishment) configuration.
//!
//! Provides helpers for configuring STUN and TURN servers.

pub use crate::peer::IceServerConfig;

/// Default Google STUN servers
pub fn google_stun_servers() -> Vec<IceServerConfig> {
    vec![
        IceServerConfig::stun("stun:stun.l.google.com:19302"),
        IceServerConfig::stun("stun:stun1.l.google.com:19302"),
    ]
}

/// Create a STUN server config
pub fn stun_server(url: &str) -> IceServerConfig {
    IceServerConfig::stun(url)
}

/// Create a TURN server config with credentials
pub fn turn_server(url: &str, username: &str, credential: &str) -> IceServerConfig {
    IceServerConfig::turn(url, username, credential)
}
