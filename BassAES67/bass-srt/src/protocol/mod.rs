//! Packet framing protocol for bass_srt multi-codec streaming.
//!
//! # Packet Format
//!
//! ```text
//! ┌────────┬────────┬────────┬────────────────────────┐
//! │ Type   │ Format │ Length │ Payload                │
//! │ 1 byte │ 1 byte │ 2 bytes│ variable               │
//! └────────┴────────┴────────┴────────────────────────┘
//! ```
//!
//! Length is little-endian u16, representing payload size (not including header).

use std::io::{self, Read, Write};

/// Header size in bytes
pub const HEADER_SIZE: usize = 4;

/// Maximum payload size (SRT live mode limit minus header)
pub const MAX_PAYLOAD_SIZE: usize = 1316 - HEADER_SIZE;

// Packet types
pub const TYPE_AUDIO: u8 = 0x01;
pub const TYPE_JSON: u8 = 0x02;

// Audio formats (when Type = TYPE_AUDIO)
pub const FORMAT_PCM_L16: u8 = 0x00;
pub const FORMAT_OPUS: u8 = 0x01;
pub const FORMAT_MP2: u8 = 0x02;
pub const FORMAT_FLAC: u8 = 0x03;

// JSON formats (when Type = TYPE_JSON)
pub const FORMAT_JSON_UTF8: u8 = 0x00;

/// Packet header
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketHeader {
    /// Packet type (TYPE_AUDIO or TYPE_JSON)
    pub ptype: u8,
    /// Format within the type (e.g., FORMAT_PCM_L16, FORMAT_OPUS, FORMAT_MP2)
    pub format: u8,
    /// Payload length in bytes (not including header)
    pub length: u16,
}

impl PacketHeader {
    /// Create a new packet header
    pub fn new(ptype: u8, format: u8, length: u16) -> Self {
        Self { ptype, format, length }
    }

    /// Create an audio packet header
    pub fn audio(format: u8, length: u16) -> Self {
        Self::new(TYPE_AUDIO, format, length)
    }

    /// Create a JSON packet header
    pub fn json(length: u16) -> Self {
        Self::new(TYPE_JSON, FORMAT_JSON_UTF8, length)
    }

    /// Encode header to 4-byte array
    pub fn encode(&self) -> [u8; HEADER_SIZE] {
        [
            self.ptype,
            self.format,
            (self.length & 0xFF) as u8,        // Length low byte
            ((self.length >> 8) & 0xFF) as u8, // Length high byte
        ]
    }

    /// Decode header from byte slice
    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < HEADER_SIZE {
            return None;
        }
        Some(Self {
            ptype: data[0],
            format: data[1],
            length: u16::from_le_bytes([data[2], data[3]]),
        })
    }

    /// Write header to a writer
    pub fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&self.encode())
    }

    /// Read header from a reader
    pub fn read_from<R: Read>(reader: &mut R) -> io::Result<Self> {
        let mut buf = [0u8; HEADER_SIZE];
        reader.read_exact(&mut buf)?;
        Self::decode(&buf).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Invalid packet header")
        })
    }

    /// Check if this is an audio packet
    pub fn is_audio(&self) -> bool {
        self.ptype == TYPE_AUDIO
    }

    /// Check if this is a JSON packet
    pub fn is_json(&self) -> bool {
        self.ptype == TYPE_JSON
    }

    /// Get audio format name for debugging
    pub fn format_name(&self) -> &'static str {
        match (self.ptype, self.format) {
            (TYPE_AUDIO, FORMAT_PCM_L16) => "PCM L16",
            (TYPE_AUDIO, FORMAT_OPUS) => "OPUS",
            (TYPE_AUDIO, FORMAT_MP2) => "MP2",
            (TYPE_AUDIO, FORMAT_FLAC) => "FLAC",
            (TYPE_JSON, FORMAT_JSON_UTF8) => "JSON UTF-8",
            _ => "Unknown",
        }
    }
}

/// Complete packet with header and payload
#[derive(Debug, Clone)]
pub struct Packet {
    pub header: PacketHeader,
    pub payload: Vec<u8>,
}

impl Packet {
    /// Create a new packet
    pub fn new(ptype: u8, format: u8, payload: Vec<u8>) -> Self {
        let header = PacketHeader::new(ptype, format, payload.len() as u16);
        Self { header, payload }
    }

    /// Create an audio packet
    pub fn audio(format: u8, payload: Vec<u8>) -> Self {
        Self::new(TYPE_AUDIO, format, payload)
    }

    /// Create a PCM L16 audio packet
    pub fn pcm_l16(samples: &[i16]) -> Self {
        let payload: Vec<u8> = samples
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        Self::audio(FORMAT_PCM_L16, payload)
    }

    /// Create a JSON packet
    pub fn json(json_str: &str) -> Self {
        Self::new(TYPE_JSON, FORMAT_JSON_UTF8, json_str.as_bytes().to_vec())
    }

    /// Encode packet to bytes (header + payload)
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(HEADER_SIZE + self.payload.len());
        buf.extend_from_slice(&self.header.encode());
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Decode packet from bytes
    pub fn decode(data: &[u8]) -> Option<Self> {
        let header = PacketHeader::decode(data)?;
        let payload_start = HEADER_SIZE;
        let payload_end = payload_start + header.length as usize;

        if data.len() < payload_end {
            return None;
        }

        let payload = data[payload_start..payload_end].to_vec();
        Some(Self { header, payload })
    }

    /// Total size of encoded packet
    pub fn total_size(&self) -> usize {
        HEADER_SIZE + self.payload.len()
    }

    /// Extract PCM L16 samples from payload
    pub fn as_pcm_l16(&self) -> Option<Vec<i16>> {
        if self.header.ptype != TYPE_AUDIO || self.header.format != FORMAT_PCM_L16 {
            return None;
        }
        if self.payload.len() % 2 != 0 {
            return None;
        }

        Some(
            self.payload
                .chunks_exact(2)
                .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
                .collect()
        )
    }

    /// Extract JSON string from payload
    pub fn as_json(&self) -> Option<&str> {
        if self.header.ptype != TYPE_JSON {
            return None;
        }
        std::str::from_utf8(&self.payload).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_encode_decode() {
        let header = PacketHeader::audio(FORMAT_PCM_L16, 960);
        let encoded = header.encode();
        let decoded = PacketHeader::decode(&encoded).unwrap();

        assert_eq!(decoded.ptype, TYPE_AUDIO);
        assert_eq!(decoded.format, FORMAT_PCM_L16);
        assert_eq!(decoded.length, 960);
    }

    #[test]
    fn test_packet_pcm_l16() {
        let samples: Vec<i16> = vec![1000, -1000, 2000, -2000];
        let packet = Packet::pcm_l16(&samples);

        assert_eq!(packet.header.ptype, TYPE_AUDIO);
        assert_eq!(packet.header.format, FORMAT_PCM_L16);
        assert_eq!(packet.header.length, 8); // 4 samples * 2 bytes

        let decoded_samples = packet.as_pcm_l16().unwrap();
        assert_eq!(decoded_samples, samples);
    }

    #[test]
    fn test_packet_json() {
        let json = r#"{"type":"metadata","value":42}"#;
        let packet = Packet::json(json);

        assert_eq!(packet.header.ptype, TYPE_JSON);
        assert_eq!(packet.as_json(), Some(json));
    }

    #[test]
    fn test_packet_encode_decode() {
        let original = Packet::json(r#"{"test":true}"#);
        let encoded = original.encode();
        let decoded = Packet::decode(&encoded).unwrap();

        assert_eq!(decoded.header.ptype, original.header.ptype);
        assert_eq!(decoded.header.format, original.header.format);
        assert_eq!(decoded.payload, original.payload);
    }
}
