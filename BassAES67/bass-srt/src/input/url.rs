//! URL parser for srt:// scheme.
//! Format: srt://host:port?latency=120&packet_size=20&channels=2&rate=48000&mode=caller

use std::net::Ipv4Addr;

// Default configuration values
const DEFAULT_PORT: u16 = 9000;
const DEFAULT_LATENCY_MS: u32 = 120;
const DEFAULT_PACKET_SIZE_MS: u32 = 20;
const DEFAULT_CHANNELS: u16 = 2;
const DEFAULT_SAMPLE_RATE: u32 = 48000;
const DEFAULT_TIMEOUT_MS: u32 = 3000;

/// SRT connection mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionMode {
    /// Connect to a remote SRT listener (default)
    #[default]
    Caller,
    /// Listen for incoming SRT connections
    Listener,
    /// Both sides connect simultaneously (NAT traversal)
    Rendezvous,
}

impl ConnectionMode {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "caller" | "call" => Some(ConnectionMode::Caller),
            "listener" | "listen" | "server" => Some(ConnectionMode::Listener),
            "rendezvous" | "rdv" => Some(ConnectionMode::Rendezvous),
            _ => None,
        }
    }

    pub fn as_u32(&self) -> u32 {
        match self {
            ConnectionMode::Caller => 0,
            ConnectionMode::Listener => 1,
            ConnectionMode::Rendezvous => 2,
        }
    }
}

// Parsed SRT URL configuration
#[derive(Debug, Clone)]
pub struct SrtUrl {
    pub host: String,
    pub port: u16,
    pub latency_ms: u32,
    pub packet_size_ms: u32,
    pub channels: u16,
    pub sample_rate: u32,
    pub stream_id: Option<String>,
    pub passphrase: Option<String>,
    /// Connection mode: caller, listener, or rendezvous
    pub mode: ConnectionMode,
    /// Receive buffer size in bytes (0 = auto)
    pub rcvbuf: u32,
    /// Send buffer size in bytes (0 = auto)
    pub sndbuf: u32,
    /// Connection timeout in ms
    pub timeout_ms: u32,
}

impl Default for SrtUrl {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: DEFAULT_PORT,
            latency_ms: DEFAULT_LATENCY_MS,
            packet_size_ms: DEFAULT_PACKET_SIZE_MS,
            channels: DEFAULT_CHANNELS,
            sample_rate: DEFAULT_SAMPLE_RATE,
            stream_id: None,
            passphrase: None,
            mode: ConnectionMode::default(),
            rcvbuf: 0,
            sndbuf: 0,
            timeout_ms: DEFAULT_TIMEOUT_MS,
        }
    }
}

impl SrtUrl {
    // Parse an srt:// URL
    // Format: srt://host:port?latency=120&packet_size=20&channels=2&rate=48000
    pub fn parse(url: &str) -> Result<Self, String> {
        let mut config = SrtUrl::default();

        // Check scheme
        let url = if url.starts_with("srt://") {
            &url[6..]
        } else {
            return Err("URL must start with srt://".to_string());
        };

        // Split host:port from query string
        let (host_port, query) = if let Some(idx) = url.find('?') {
            (&url[..idx], Some(&url[idx + 1..]))
        } else {
            (url, None)
        };

        // Parse host:port
        if let Some(idx) = host_port.rfind(':') {
            config.host = host_port[..idx].to_string();
            config.port = host_port[idx + 1..]
                .parse()
                .map_err(|_| "Invalid port number")?;
        } else {
            config.host = host_port.to_string();
        }

        // Validate host is not empty
        if config.host.is_empty() {
            return Err("Host cannot be empty".to_string());
        }

        // Parse query parameters
        if let Some(query) = query {
            for param in query.split('&') {
                if param.is_empty() {
                    continue;
                }

                let (key, value) = if let Some(idx) = param.find('=') {
                    (&param[..idx], &param[idx + 1..])
                } else {
                    (param, "")
                };

                match key.to_lowercase().as_str() {
                    "latency" => {
                        config.latency_ms = value
                            .parse()
                            .map_err(|_| "Invalid latency value")?;
                    }
                    "packet_size" | "packetsize" | "psize" => {
                        config.packet_size_ms = value
                            .parse()
                            .map_err(|_| "Invalid packet_size value")?;
                    }
                    "channels" | "ch" => {
                        config.channels = value
                            .parse()
                            .map_err(|_| "Invalid channels value")?;
                    }
                    "rate" | "samplerate" | "sr" => {
                        config.sample_rate = value
                            .parse()
                            .map_err(|_| "Invalid sample rate value")?;
                    }
                    "streamid" | "stream_id" | "sid" => {
                        if !value.is_empty() {
                            config.stream_id = Some(value.to_string());
                        }
                    }
                    "passphrase" | "password" | "pass" => {
                        if !value.is_empty() {
                            config.passphrase = Some(value.to_string());
                        }
                    }
                    "mode" => {
                        if let Some(mode) = ConnectionMode::from_str(value) {
                            config.mode = mode;
                        }
                    }
                    "rcvbuf" | "recv_buffer" | "recvbuf" => {
                        config.rcvbuf = value
                            .parse()
                            .map_err(|_| "Invalid rcvbuf value")?;
                    }
                    "sndbuf" | "send_buffer" | "sendbuf" => {
                        config.sndbuf = value
                            .parse()
                            .map_err(|_| "Invalid sndbuf value")?;
                    }
                    "timeout" | "connect_timeout" => {
                        config.timeout_ms = value
                            .parse()
                            .map_err(|_| "Invalid timeout value")?;
                    }
                    _ => {
                        // Ignore unknown parameters
                    }
                }
            }
        }

        // Validate values
        if config.channels == 0 || config.channels > 8 {
            return Err("Channels must be between 1 and 8".to_string());
        }
        if config.sample_rate < 8000 || config.sample_rate > 192000 {
            return Err("Sample rate must be between 8000 and 192000".to_string());
        }
        if config.packet_size_ms == 0 || config.packet_size_ms > 100 {
            return Err("Packet size must be between 1 and 100 ms".to_string());
        }

        Ok(config)
    }

    // Get the IP address (resolves hostname if needed)
    pub fn get_ip(&self) -> Result<Ipv4Addr, String> {
        // Try to parse as IP address first
        if let Ok(ip) = self.host.parse::<Ipv4Addr>() {
            return Ok(ip);
        }

        // For now, only support IP addresses
        // DNS resolution would require additional dependencies
        Err(format!("Could not resolve hostname: {}", self.host))
    }

    // Calculate samples per packet based on packet_size_ms
    pub fn samples_per_packet(&self) -> usize {
        (self.sample_rate as usize * self.packet_size_ms as usize) / 1000
    }

    // Calculate bytes per packet (L16 format: 2 bytes per sample per channel)
    pub fn bytes_per_packet(&self) -> usize {
        self.samples_per_packet() * self.channels as usize * 2
    }

    // Calculate target buffer samples for jitter absorption
    pub fn target_buffer_samples(&self) -> usize {
        // Use latency_ms as buffer target
        let samples_per_ms = self.sample_rate as usize / 1000;
        samples_per_ms * self.latency_ms as usize * self.channels as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_url() {
        let url = SrtUrl::parse("srt://192.168.1.100:9000").unwrap();
        assert_eq!(url.host, "192.168.1.100");
        assert_eq!(url.port, 9000);
        assert_eq!(url.latency_ms, 120);
        assert_eq!(url.channels, 2);
    }

    #[test]
    fn test_parse_url_with_params() {
        let url = SrtUrl::parse("srt://10.0.0.1:5000?latency=200&channels=1&rate=44100").unwrap();
        assert_eq!(url.host, "10.0.0.1");
        assert_eq!(url.port, 5000);
        assert_eq!(url.latency_ms, 200);
        assert_eq!(url.channels, 1);
        assert_eq!(url.sample_rate, 44100);
    }

    #[test]
    fn test_parse_url_default_port() {
        let url = SrtUrl::parse("srt://localhost").unwrap();
        assert_eq!(url.host, "localhost");
        assert_eq!(url.port, 9000);
    }

    #[test]
    fn test_samples_per_packet() {
        let url = SrtUrl {
            sample_rate: 48000,
            packet_size_ms: 20,
            channels: 2,
            ..Default::default()
        };
        assert_eq!(url.samples_per_packet(), 960); // 48000 * 20 / 1000
        assert_eq!(url.bytes_per_packet(), 3840); // 960 * 2 * 2
    }
}
