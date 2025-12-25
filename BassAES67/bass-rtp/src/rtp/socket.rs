//! Bidirectional UDP socket for RTP.
//!
//! Provides a wrapper around UDP socket that supports both sending and receiving
//! on the same port, as required for Telos Z/IP ONE reciprocal RTP.
//!
//! IMPORTANT: Each RtpSocket instance is independent - multiple instances can
//! coexist in the same application, each bound to different local ports.

use socket2::{Domain, Protocol, Socket, Type};
use std::io::{self, ErrorKind};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::time::Duration;

/// Bidirectional UDP socket for RTP communication.
///
/// Each instance is independent and can be used for a separate RTP stream.
/// Multiple instances can coexist in the same application.
pub struct RtpSocket {
    /// The underlying UDP socket
    socket: UdpSocket,
    /// Local address this socket is bound to
    local_addr: SocketAddrV4,
    /// Remote address to send to
    remote_addr: SocketAddrV4,
}

impl RtpSocket {
    /// Bind to a local address for receiving.
    ///
    /// This is useful when you only need to receive, or when you'll set
    /// the remote address later.
    pub fn bind(local_addr: SocketAddr) -> Result<Self, String> {
        let _local_v4 = match local_addr {
            SocketAddr::V4(v4) => v4,
            _ => return Err("IPv4 only".to_string()),
        };

        // Create socket
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
            .map_err(|e| e.to_string())?;

        // Allow address reuse
        socket.set_reuse_address(true).map_err(|e| e.to_string())?;

        // Bind
        socket.bind(&local_addr.into()).map_err(|e| e.to_string())?;

        // Set non-blocking with timeout for recv
        socket.set_read_timeout(Some(Duration::from_millis(10))).map_err(|e| e.to_string())?;

        // Increase buffer sizes
        let _ = socket.set_recv_buffer_size(1024 * 1024);
        let _ = socket.set_send_buffer_size(1024 * 1024);

        // Convert to std UdpSocket
        let socket: UdpSocket = socket.into();

        // Get actual bound address
        let actual_local = socket.local_addr().map_err(|e| e.to_string())?;
        let actual_local = match actual_local {
            SocketAddr::V4(addr) => addr,
            _ => return Err("IPv4 only".to_string()),
        };

        Ok(RtpSocket {
            socket,
            local_addr: actual_local,
            remote_addr: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0),
        })
    }

    /// Create a new bidirectional RTP socket.
    ///
    /// # Arguments
    /// * `local_port` - Local port to bind to (required for receiving return audio)
    /// * `remote_addr` - Remote address (Z/IP ONE IP and port)
    /// * `interface` - Optional interface IP to bind to (None = any interface)
    ///
    /// # Returns
    /// A new RtpSocket instance, or an error if binding fails
    pub fn new(
        local_port: u16,
        remote_addr: SocketAddrV4,
        interface: Option<Ipv4Addr>,
    ) -> io::Result<Self> {
        // Create socket
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;

        // Allow address reuse (important for quick restarts)
        socket.set_reuse_address(true)?;

        // Bind to local address
        let local_ip = interface.unwrap_or(Ipv4Addr::UNSPECIFIED);
        let local_addr = SocketAddrV4::new(local_ip, local_port);
        socket.bind(&local_addr.into())?;

        // Set non-blocking with timeout for recv
        socket.set_read_timeout(Some(Duration::from_millis(10)))?;

        // Increase buffer sizes for audio streaming
        let _ = socket.set_recv_buffer_size(1024 * 1024); // 1MB
        let _ = socket.set_send_buffer_size(1024 * 1024);

        // Convert to std UdpSocket
        let socket: UdpSocket = socket.into();

        // Get the actual bound address (in case port was 0)
        let actual_local = socket.local_addr()?;
        let actual_local = match actual_local {
            std::net::SocketAddr::V4(addr) => addr,
            _ => return Err(io::Error::new(ErrorKind::InvalidInput, "IPv4 only")),
        };

        Ok(RtpSocket {
            socket,
            local_addr: actual_local,
            remote_addr,
        })
    }

    /// Send data to the remote address.
    pub fn send(&self, data: &[u8]) -> io::Result<usize> {
        self.socket.send_to(data, self.remote_addr)
    }

    /// Send data to a specific address.
    pub fn send_to(&self, data: &[u8], addr: SocketAddr) -> io::Result<usize> {
        self.socket.send_to(data, addr)
    }

    /// Receive data from any source.
    ///
    /// Returns the number of bytes received, or WouldBlock/TimedOut if no data.
    pub fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        match self.socket.recv_from(buf) {
            Ok((len, _addr)) => Ok(len),
            Err(e) => Err(e),
        }
    }

    /// Receive data from any source with source address.
    ///
    /// Returns the number of bytes received and the source address.
    pub fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.socket.recv_from(buf)
    }

    /// Receive data with source address filtering.
    ///
    /// Only accepts packets from the configured remote address.
    pub fn recv_from_remote(&self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match self.socket.recv_from(buf) {
                Ok((len, addr)) => {
                    // Check if packet is from expected remote
                    if let std::net::SocketAddr::V4(v4_addr) = addr {
                        if v4_addr.ip() == self.remote_addr.ip() {
                            return Ok(len);
                        }
                    }
                    // Packet from unexpected source, continue waiting
                    continue;
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => return Err(e),
                Err(e) if e.kind() == ErrorKind::TimedOut => return Err(e),
                Err(e) => return Err(e),
            }
        }
    }

    /// Get the local address this socket is bound to.
    pub fn local_addr(&self) -> SocketAddrV4 {
        self.local_addr
    }

    /// Get the remote address this socket sends to.
    pub fn remote_addr(&self) -> SocketAddrV4 {
        self.remote_addr
    }

    /// Set the remote address (for changing targets).
    pub fn set_remote_addr(&mut self, addr: SocketAddrV4) {
        self.remote_addr = addr;
    }

    /// Set the receive timeout.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.socket.set_read_timeout(timeout)
    }

    /// Try to clone the socket for use in multiple threads.
    ///
    /// Note: The cloned socket shares the same underlying OS socket,
    /// so sends and receives can happen from either instance.
    pub fn try_clone(&self) -> Result<Self, String> {
        Ok(RtpSocket {
            socket: self.socket.try_clone().map_err(|e| e.to_string())?,
            local_addr: self.local_addr,
            remote_addr: self.remote_addr,
        })
    }
}

impl std::fmt::Debug for RtpSocket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RtpSocket")
            .field("local_addr", &self.local_addr)
            .field("remote_addr", &self.remote_addr)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_creation() {
        let remote = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 9152);
        let socket = RtpSocket::new(0, remote, None);
        assert!(socket.is_ok());

        let socket = socket.unwrap();
        assert_eq!(socket.remote_addr(), remote);
        assert_ne!(socket.local_addr().port(), 0); // Should have been assigned
    }

    #[test]
    fn test_socket_clone() {
        let remote = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 9152);
        let socket = RtpSocket::new(0, remote, None).unwrap();
        let cloned = socket.try_clone();
        assert!(cloned.is_ok());

        let cloned = cloned.unwrap();
        assert_eq!(socket.local_addr(), cloned.local_addr());
        assert_eq!(socket.remote_addr(), cloned.remote_addr());
    }
}
