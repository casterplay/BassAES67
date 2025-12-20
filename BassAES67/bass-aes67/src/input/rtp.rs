//! RTP packet parser for AES67 streams.
//! AES67 uses RTP with linear 24-bit PCM audio payload.

/// RTP packet header (12 bytes minimum)
#[derive(Debug, Clone)]
pub struct RtpHeader {
    /// RTP version (should be 2)
    pub version: u8,
    /// Padding flag
    pub padding: bool,
    /// Extension flag
    pub extension: bool,
    /// CSRC count
    pub csrc_count: u8,
    /// Marker bit
    pub marker: bool,
    /// Payload type (typically 96-127 for dynamic types)
    pub payload_type: u8,
    /// Sequence number (wraps at 65535)
    pub sequence: u16,
    /// Timestamp (sample count)
    pub timestamp: u32,
    /// Synchronization source identifier
    pub ssrc: u32,
}

/// Parsed RTP packet with header and audio payload
#[derive(Debug)]
pub struct RtpPacket<'a> {
    pub header: RtpHeader,
    /// Raw audio payload (24-bit PCM samples, big-endian)
    pub payload: &'a [u8],
}

impl RtpHeader {
    /// Parse RTP header from bytes (minimum 12 bytes required)
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }

        let first = data[0];
        let second = data[1];

        let version = (first >> 6) & 0x03;
        if version != 2 {
            return None; // Only RTP version 2 supported
        }

        Some(Self {
            version,
            padding: (first & 0x20) != 0,
            extension: (first & 0x10) != 0,
            csrc_count: first & 0x0F,
            marker: (second & 0x80) != 0,
            payload_type: second & 0x7F,
            sequence: u16::from_be_bytes([data[2], data[3]]),
            timestamp: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
            ssrc: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
        })
    }

    /// Calculate total header size including CSRC and extension
    pub fn header_size(&self, data: &[u8]) -> usize {
        let mut size = 12 + (self.csrc_count as usize * 4);

        // Handle extension header if present
        if self.extension && data.len() >= size + 4 {
            let ext_len = u16::from_be_bytes([data[size + 2], data[size + 3]]) as usize;
            size += 4 + (ext_len * 4);
        }

        size
    }
}

impl<'a> RtpPacket<'a> {
    /// Parse complete RTP packet from bytes
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        let header = RtpHeader::parse(data)?;
        let header_size = header.header_size(data);

        if data.len() <= header_size {
            return None;
        }

        // Handle padding
        let payload_end = if header.padding {
            let padding_len = data[data.len() - 1] as usize;
            if padding_len > data.len() - header_size {
                return None;
            }
            data.len() - padding_len
        } else {
            data.len()
        };

        Some(Self {
            header,
            payload: &data[header_size..payload_end],
        })
    }

    /// Get number of audio samples in payload.
    /// AES67 uses 24-bit (3 bytes) per sample per channel.
    pub fn sample_count(&self, channels: u16) -> usize {
        let bytes_per_sample = 3; // 24-bit
        let bytes_per_frame = bytes_per_sample * channels as usize;
        if bytes_per_frame == 0 {
            return 0;
        }
        self.payload.len() / bytes_per_frame
    }
}

/// Convert 24-bit big-endian PCM to 32-bit float.
/// AES67 uses 24-bit linear PCM in network byte order (big-endian).
/// Uses same algorithm as professional AoIP implementations.
pub fn convert_24bit_be_to_float(src: &[u8], dst: &mut [f32], _channels: u16) {
    let bytes_per_sample = 3;
    let samples = src.len() / bytes_per_sample;

    // Normalization constant: 1.0 / 8388608.0 (2^23)
    const NORMALIZE: f32 = 0.00000011920929;

    for i in 0..samples.min(dst.len()) {
        let offset = i * bytes_per_sample;
        if offset + 3 > src.len() {
            break;
        }

        // Read 24-bit big-endian sample
        // MSB (byte 0) is treated as signed for sign extension
        // LSB bytes are unsigned
        let b0 = src[offset] as i8 as i32;      // MSB - sign extends
        let b1 = src[offset + 1] as u8 as i32;  // unsigned
        let b2 = src[offset + 2] as u8 as i32;  // LSB - unsigned

        // Combine to 24-bit signed value (matches pro implementation)
        let sample_i32 = (b0 << 16) | (b1 << 8) | b2;

        // Convert to float in range [-1.0, 1.0]
        dst[i] = sample_i32 as f32 * NORMALIZE;
    }

    // Silence any remaining samples
    for i in samples..dst.len() {
        dst[i] = 0.0;
    }
}

/// Calculate sequence number difference handling wrap-around.
/// Returns positive value if b is ahead of a, negative if behind.
pub fn sequence_diff(a: u16, b: u16) -> i32 {
    let diff = b.wrapping_sub(a) as i16;
    diff as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sequence_diff() {
        assert_eq!(sequence_diff(100, 101), 1);
        assert_eq!(sequence_diff(100, 99), -1);
        assert_eq!(sequence_diff(65535, 0), 1);  // Wrap around
        assert_eq!(sequence_diff(0, 65535), -1); // Wrap around backward
    }

    #[test]
    fn test_24bit_conversion() {
        // Test maximum positive value (0x7FFFFF = 8388607)
        let src = [0x7F, 0xFF, 0xFF];
        let mut dst = [0.0f32];
        convert_24bit_be_to_float(&src, &mut dst, 1);
        // 8388607 * 0.00000011920929 = ~0.99999988
        assert!((dst[0] - 0.99999988).abs() < 0.0001);

        // Test zero
        let src = [0x00, 0x00, 0x00];
        convert_24bit_be_to_float(&src, &mut dst, 1);
        assert_eq!(dst[0], 0.0);

        // Test minimum negative value (0x800000 = -8388608)
        let src = [0x80, 0x00, 0x00];
        convert_24bit_be_to_float(&src, &mut dst, 1);
        // -8388608 * 0.00000011920929 = -1.0
        assert!((dst[0] + 1.0).abs() < 0.0001);
    }
}
