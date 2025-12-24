//! URL parser for RTP streams.
//!
//! Parses URLs in the format:
//! rtp://host:port[?options]
//!
//! Options:
//! - codec: Output codec (pcm16, pcm24, mp2, opus, flac)
//! - bitrate: Output bitrate in kbps (for mp2/opus)
//! - jitter: Jitter buffer depth in ms
//! - channels: Number of channels (1 or 2)
//! - local_port: Local port to bind

use std::net::Ipv4Addr;
use std::str::FromStr;

use crate::rtp::PayloadCodec;

/// Parsed RTP URL configuration.
#[derive(Debug, Clone)]
pub struct RtpUrl {
    /// Remote host IP address
    pub host: Ipv4Addr,
    /// Remote port
    pub port: u16,
    /// Output codec
    pub codec: PayloadCodec,
    /// Output bitrate in kbps (for compressed codecs)
    pub bitrate: u32,
    /// Jitter buffer depth in ms
    pub jitter_ms: u32,
    /// Number of channels
    pub channels: u16,
    /// Local port to bind (0 = ephemeral)
    pub local_port: u16,
    /// Network interface to bind to
    pub interface: Option<Ipv4Addr>,
}

impl Default for RtpUrl {
    fn default() -> Self {
        Self {
            host: Ipv4Addr::new(0, 0, 0, 0),
            port: 9152,
            codec: PayloadCodec::Pcm16,
            bitrate: 192,
            jitter_ms: 20,
            channels: 2,
            local_port: 0,
            interface: None,
        }
    }
}

/// Parse an RTP URL.
///
/// Format: rtp://host:port[?option=value&...]
///
/// # Arguments
/// * `url` - The URL string to parse
///
/// # Returns
/// Parsed URL configuration, or error message.
///
/// # Examples
/// ```
/// let url = parse_rtp_url("rtp://192.168.1.100:9152").unwrap();
/// let url = parse_rtp_url("rtp://192.168.1.100:9152?codec=mp2&bitrate=256").unwrap();
/// ```
pub fn parse_rtp_url(url: &str) -> Result<RtpUrl, String> {
    // Check scheme
    let url = url.trim();
    if !url.to_lowercase().starts_with("rtp://") {
        return Err("URL must start with rtp://".to_string());
    }

    let rest = &url[6..]; // Skip "rtp://"

    // Split host:port from query string
    let (host_port, query) = if let Some(idx) = rest.find('?') {
        (&rest[..idx], Some(&rest[idx + 1..]))
    } else {
        (rest, None)
    };

    // Parse host:port
    let (host_str, port_str) = if let Some(idx) = host_port.rfind(':') {
        (&host_port[..idx], &host_port[idx + 1..])
    } else {
        return Err("URL must include port (e.g., rtp://host:port)".to_string());
    };

    let host = Ipv4Addr::from_str(host_str)
        .map_err(|_| format!("Invalid IP address: {}", host_str))?;

    let port: u16 = port_str
        .parse()
        .map_err(|_| format!("Invalid port: {}", port_str))?;

    let mut result = RtpUrl {
        host,
        port,
        ..Default::default()
    };

    // Parse query options
    if let Some(query) = query {
        for param in query.split('&') {
            let parts: Vec<&str> = param.splitn(2, '=').collect();
            if parts.len() != 2 {
                continue;
            }

            let key = parts[0].to_lowercase();
            let value = parts[1];

            match key.as_str() {
                "codec" => {
                    result.codec = parse_codec(value)?;
                }
                "bitrate" => {
                    result.bitrate = value
                        .parse()
                        .map_err(|_| format!("Invalid bitrate: {}", value))?;
                }
                "jitter" => {
                    result.jitter_ms = value
                        .parse()
                        .map_err(|_| format!("Invalid jitter: {}", value))?;
                }
                "channels" => {
                    result.channels = value
                        .parse()
                        .map_err(|_| format!("Invalid channels: {}", value))?;
                    if result.channels < 1 || result.channels > 2 {
                        return Err("Channels must be 1 or 2".to_string());
                    }
                }
                "local_port" | "localport" => {
                    result.local_port = value
                        .parse()
                        .map_err(|_| format!("Invalid local_port: {}", value))?;
                }
                "interface" | "if" => {
                    result.interface = Some(
                        Ipv4Addr::from_str(value)
                            .map_err(|_| format!("Invalid interface: {}", value))?,
                    );
                }
                _ => {
                    // Ignore unknown parameters
                }
            }
        }
    }

    Ok(result)
}

/// Parse codec name to PayloadCodec.
fn parse_codec(name: &str) -> Result<PayloadCodec, String> {
    match name.to_lowercase().as_str() {
        "pcm16" | "pcm-16" | "l16" => Ok(PayloadCodec::Pcm16),
        "pcm24" | "pcm-24" | "l24" => Ok(PayloadCodec::Pcm24),
        "mp2" | "mpeg2" | "mpa" => Ok(PayloadCodec::Mp2),
        "opus" => Ok(PayloadCodec::Opus),
        "flac" => Ok(PayloadCodec::Flac),
        _ => Err(format!("Unknown codec: {}", name)),
    }
}

/// Build an RTP URL from configuration.
pub fn build_rtp_url(config: &RtpUrl) -> String {
    let mut url = format!("rtp://{}:{}", config.host, config.port);

    let mut params = Vec::new();

    // Only add non-default values
    if config.codec != PayloadCodec::Pcm16 {
        let codec_name = match config.codec {
            PayloadCodec::Pcm16 => "pcm16",
            PayloadCodec::Pcm24 => "pcm24",
            PayloadCodec::Mp2 => "mp2",
            PayloadCodec::Opus => "opus",
            PayloadCodec::Flac => "flac",
            _ => "pcm16",
        };
        params.push(format!("codec={}", codec_name));
    }

    if config.bitrate != 192 {
        params.push(format!("bitrate={}", config.bitrate));
    }

    if config.jitter_ms != 20 {
        params.push(format!("jitter={}", config.jitter_ms));
    }

    if config.channels != 2 {
        params.push(format!("channels={}", config.channels));
    }

    if config.local_port != 0 {
        params.push(format!("local_port={}", config.local_port));
    }

    if let Some(interface) = config.interface {
        params.push(format!("interface={}", interface));
    }

    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }

    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_url() {
        let url = parse_rtp_url("rtp://192.168.1.100:9152").unwrap();
        assert_eq!(url.host, Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(url.port, 9152);
        assert_eq!(url.codec, PayloadCodec::Pcm16);
    }

    #[test]
    fn test_parse_url_with_options() {
        let url = parse_rtp_url("rtp://10.0.0.1:9151?codec=mp2&bitrate=256&jitter=50").unwrap();
        assert_eq!(url.host, Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(url.port, 9151);
        assert_eq!(url.codec, PayloadCodec::Mp2);
        assert_eq!(url.bitrate, 256);
        assert_eq!(url.jitter_ms, 50);
    }

    #[test]
    fn test_parse_url_with_channels() {
        let url = parse_rtp_url("rtp://192.168.1.1:5004?channels=1").unwrap();
        assert_eq!(url.channels, 1);
    }

    #[test]
    fn test_parse_url_with_local_port() {
        let url = parse_rtp_url("rtp://192.168.1.1:9152?local_port=5000").unwrap();
        assert_eq!(url.local_port, 5000);
    }

    #[test]
    fn test_invalid_url_no_scheme() {
        assert!(parse_rtp_url("192.168.1.1:9152").is_err());
    }

    #[test]
    fn test_invalid_url_no_port() {
        assert!(parse_rtp_url("rtp://192.168.1.1").is_err());
    }

    #[test]
    fn test_invalid_ip() {
        assert!(parse_rtp_url("rtp://invalid:9152").is_err());
    }

    #[test]
    fn test_build_url() {
        let config = RtpUrl {
            host: Ipv4Addr::new(192, 168, 1, 100),
            port: 9152,
            codec: PayloadCodec::Mp2,
            bitrate: 256,
            jitter_ms: 20,
            channels: 2,
            local_port: 0,
            interface: None,
        };
        let url = build_rtp_url(&config);
        assert_eq!(url, "rtp://192.168.1.100:9152?codec=mp2&bitrate=256");
    }

    #[test]
    fn test_roundtrip() {
        let original = "rtp://10.0.0.5:9153?codec=opus&bitrate=128&jitter=30";
        let parsed = parse_rtp_url(original).unwrap();
        assert_eq!(parsed.host, Ipv4Addr::new(10, 0, 0, 5));
        assert_eq!(parsed.port, 9153);
        assert_eq!(parsed.codec, PayloadCodec::Opus);
        assert_eq!(parsed.bitrate, 128);
        assert_eq!(parsed.jitter_ms, 30);
    }
}
