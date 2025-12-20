//! URL parser for aes67:// scheme.
//! Parses URLs like: aes67://239.192.76.52:5004?iface=192.168.60.102&pt=96&jitter=10

use std::net::Ipv4Addr;
use std::str::FromStr;

/// Parsed AES67 URL with all stream parameters
#[derive(Debug, Clone)]
pub struct Aes67Url {
    /// Multicast group address
    pub multicast_addr: Ipv4Addr,
    /// UDP port (default: 5004 for RTP)
    pub port: u16,
    /// Network interface IP to bind to
    pub interface: Option<Ipv4Addr>,
    /// RTP payload type (default: 96)
    pub payload_type: u8,
    /// Jitter buffer depth in milliseconds (default: 10)
    pub jitter_ms: u32,
    /// Number of audio channels (default: 2)
    pub channels: u16,
    /// Sample rate in Hz (default: 48000)
    pub sample_rate: u32,
}

impl Default for Aes67Url {
    fn default() -> Self {
        Self {
            multicast_addr: Ipv4Addr::new(239, 192, 76, 52),
            port: 5004,
            interface: None,
            payload_type: 96,
            jitter_ms: 10,
            channels: 2,
            sample_rate: 48000,
        }
    }
}

impl Aes67Url {
    /// Parse an aes67:// URL string.
    /// Format: aes67://MULTICAST_IP:PORT?iface=IP&pt=N&jitter=MS&ch=N&rate=HZ
    pub fn parse(url: &str) -> Result<Self, String> {
        // Check scheme
        if !url.starts_with("aes67://") {
            return Err("URL must start with aes67://".to_string());
        }

        let rest = &url[8..]; // Skip "aes67://"
        let mut result = Self::default();

        // Split path and query
        let (host_port, query) = match rest.find('?') {
            Some(pos) => (&rest[..pos], Some(&rest[pos + 1..])),
            None => (rest, None),
        };

        // Parse host:port
        let (host, port_str) = match host_port.rfind(':') {
            Some(pos) => (&host_port[..pos], Some(&host_port[pos + 1..])),
            None => (host_port, None),
        };

        // Parse multicast address
        result.multicast_addr = Ipv4Addr::from_str(host)
            .map_err(|e| format!("Invalid multicast address '{}': {}", host, e))?;

        // Parse port if specified
        if let Some(port_str) = port_str {
            result.port = port_str
                .parse()
                .map_err(|e| format!("Invalid port '{}': {}", port_str, e))?;
        }

        // Parse query parameters
        if let Some(query) = query {
            for param in query.split('&') {
                let mut parts = param.splitn(2, '=');
                let key = parts.next().unwrap_or("");
                let value = parts.next().unwrap_or("");

                match key {
                    "iface" | "interface" => {
                        result.interface = Some(
                            Ipv4Addr::from_str(value)
                                .map_err(|e| format!("Invalid interface '{}': {}", value, e))?,
                        );
                    }
                    "pt" | "payload" => {
                        result.payload_type = value
                            .parse()
                            .map_err(|e| format!("Invalid payload type '{}': {}", value, e))?;
                    }
                    "jitter" => {
                        result.jitter_ms = value
                            .parse()
                            .map_err(|e| format!("Invalid jitter '{}': {}", value, e))?;
                    }
                    "ch" | "channels" => {
                        result.channels = value
                            .parse()
                            .map_err(|e| format!("Invalid channels '{}': {}", value, e))?;
                    }
                    "rate" | "samplerate" => {
                        result.sample_rate = value
                            .parse()
                            .map_err(|e| format!("Invalid sample rate '{}': {}", value, e))?;
                    }
                    _ => {
                        // Ignore unknown parameters
                    }
                }
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic() {
        let url = Aes67Url::parse("aes67://239.192.76.52:5004").unwrap();
        assert_eq!(url.multicast_addr, Ipv4Addr::new(239, 192, 76, 52));
        assert_eq!(url.port, 5004);
    }

    #[test]
    fn test_parse_with_params() {
        let url = Aes67Url::parse(
            "aes67://239.192.76.52:5004?iface=192.168.60.102&pt=96&jitter=10",
        )
        .unwrap();
        assert_eq!(url.interface, Some(Ipv4Addr::new(192, 168, 60, 102)));
        assert_eq!(url.payload_type, 96);
        assert_eq!(url.jitter_ms, 10);
    }
}
