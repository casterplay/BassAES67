//! RTP header parsing and building.
//!
//! Implements RFC 3550 RTP header format.

/// RTP header structure (12 bytes minimum)
#[derive(Debug, Clone)]
pub struct RtpHeader {
    /// RTP version (always 2)
    pub version: u8,
    /// Padding flag
    pub padding: bool,
    /// Extension flag
    pub extension: bool,
    /// CSRC count
    pub csrc_count: u8,
    /// Marker bit
    pub marker: bool,
    /// Payload type (0-127)
    pub payload_type: u8,
    /// Sequence number (wraps at 65535)
    pub sequence: u16,
    /// Timestamp
    pub timestamp: u32,
    /// Synchronization source identifier
    pub ssrc: u32,
}

impl RtpHeader {
    /// Parse RTP header from bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }

        let byte0 = data[0];
        let version = (byte0 >> 6) & 0x03;
        if version != 2 {
            return None;
        }

        let padding = (byte0 & 0x20) != 0;
        let extension = (byte0 & 0x10) != 0;
        let csrc_count = byte0 & 0x0F;

        let byte1 = data[1];
        let marker = (byte1 & 0x80) != 0;
        let payload_type = byte1 & 0x7F;

        let sequence = u16::from_be_bytes([data[2], data[3]]);
        let timestamp = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let ssrc = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);

        Some(RtpHeader {
            version,
            padding,
            extension,
            csrc_count,
            marker,
            payload_type,
            sequence,
            timestamp,
            ssrc,
        })
    }

    /// Get the total header size including CSRC list
    pub fn header_size(&self) -> usize {
        12 + (self.csrc_count as usize * 4)
    }

    /// Encode RTP header to bytes
    pub fn encode(&self, buffer: &mut [u8]) -> usize {
        if buffer.len() < 12 {
            return 0;
        }

        let byte0 = (self.version << 6)
            | if self.padding { 0x20 } else { 0 }
            | if self.extension { 0x10 } else { 0 }
            | (self.csrc_count & 0x0F);

        let byte1 = if self.marker { 0x80 } else { 0 } | (self.payload_type & 0x7F);

        buffer[0] = byte0;
        buffer[1] = byte1;
        buffer[2..4].copy_from_slice(&self.sequence.to_be_bytes());
        buffer[4..8].copy_from_slice(&self.timestamp.to_be_bytes());
        buffer[8..12].copy_from_slice(&self.ssrc.to_be_bytes());

        12
    }
}

/// Parsed RTP packet with header and payload reference
#[derive(Debug)]
pub struct RtpPacket<'a> {
    /// Parsed header
    pub header: RtpHeader,
    /// Payload data (after header, CSRC list, and extension)
    pub payload: &'a [u8],
}

impl<'a> RtpPacket<'a> {
    /// Parse an RTP packet from bytes
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        let header = RtpHeader::parse(data)?;
        let mut offset = header.header_size();

        // Handle extension header if present
        if header.extension {
            if data.len() < offset + 4 {
                return None;
            }
            // Extension length is in 32-bit words
            let ext_length = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
            offset += 4 + (ext_length * 4);
        }

        if offset > data.len() {
            return None;
        }

        // Handle padding
        let payload_end = if header.padding && !data.is_empty() {
            let padding_len = data[data.len() - 1] as usize;
            if padding_len > data.len() - offset {
                return None;
            }
            data.len() - padding_len
        } else {
            data.len()
        };

        Some(RtpPacket {
            header,
            payload: &data[offset..payload_end],
        })
    }
}

/// RTP packet builder for output
pub struct RtpPacketBuilder {
    /// SSRC for this stream
    ssrc: u32,
    /// Current sequence number
    sequence: u16,
    /// Current timestamp
    timestamp: u32,
    /// Payload type
    payload_type: u8,
    /// Pre-allocated packet buffer
    buffer: Vec<u8>,
}

impl RtpPacketBuilder {
    /// Create a new packet builder with random SSRC
    pub fn new(payload_type: u8) -> Self {
        // Generate random SSRC
        let ssrc = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u32)
            .unwrap_or(0x12345678)
            ^ std::process::id();

        Self {
            ssrc,
            sequence: 0,
            timestamp: 0,
            payload_type,
            buffer: vec![0u8; 1500], // MTU size
        }
    }

    /// Create a new packet builder with specific SSRC
    pub fn with_ssrc(ssrc: u32, payload_type: u8) -> Self {
        Self {
            ssrc,
            sequence: 0,
            timestamp: 0,
            payload_type,
            buffer: vec![0u8; 1500],
        }
    }

    /// Set the payload type
    pub fn set_payload_type(&mut self, pt: u8) {
        self.payload_type = pt;
    }

    /// Build an RTP packet with the given payload
    /// Returns the complete packet slice
    pub fn build_packet(&mut self, payload: &[u8], samples_per_packet: u32) -> &[u8] {
        let header = RtpHeader {
            version: 2,
            padding: false,
            extension: false,
            csrc_count: 0,
            marker: false,
            payload_type: self.payload_type,
            sequence: self.sequence,
            timestamp: self.timestamp,
            ssrc: self.ssrc,
        };

        let header_len = header.encode(&mut self.buffer);
        let total_len = header_len + payload.len();

        if total_len <= self.buffer.len() {
            self.buffer[header_len..total_len].copy_from_slice(payload);
        }

        // Advance sequence and timestamp for next packet
        self.sequence = self.sequence.wrapping_add(1);
        self.timestamp = self.timestamp.wrapping_add(samples_per_packet);

        &self.buffer[..total_len]
    }

    /// Get current sequence number
    pub fn sequence(&self) -> u16 {
        self.sequence
    }

    /// Get current timestamp
    pub fn timestamp(&self) -> u32 {
        self.timestamp
    }

    /// Get SSRC
    pub fn ssrc(&self) -> u32 {
        self.ssrc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_parse() {
        // RTP header: V=2, P=0, X=0, CC=0, M=0, PT=96, seq=1234, ts=5678, ssrc=0xDEADBEEF
        let data = [
            0x80, 96, 0x04, 0xD2, 0x00, 0x00, 0x16, 0x2E, 0xDE, 0xAD, 0xBE, 0xEF,
        ];

        let header = RtpHeader::parse(&data).unwrap();
        assert_eq!(header.version, 2);
        assert!(!header.padding);
        assert!(!header.extension);
        assert_eq!(header.csrc_count, 0);
        assert!(!header.marker);
        assert_eq!(header.payload_type, 96);
        assert_eq!(header.sequence, 1234);
        assert_eq!(header.timestamp, 5678);
        assert_eq!(header.ssrc, 0xDEADBEEF);
    }

    #[test]
    fn test_header_roundtrip() {
        let header = RtpHeader {
            version: 2,
            padding: false,
            extension: false,
            csrc_count: 0,
            marker: true,
            payload_type: 21,
            sequence: 42,
            timestamp: 12345,
            ssrc: 0xCAFEBABE,
        };

        let mut buffer = [0u8; 12];
        header.encode(&mut buffer);

        let parsed = RtpHeader::parse(&buffer).unwrap();
        assert_eq!(parsed.payload_type, 21);
        assert_eq!(parsed.sequence, 42);
        assert!(parsed.marker);
    }
}
