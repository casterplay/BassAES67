//! RTP packet builder for AES67 output streams.
//! Builds RTP packets with 24-bit big-endian PCM audio payload.

/// RTP packet builder for transmitting audio.
/// Manages sequence numbers and timestamps automatically.
pub struct RtpPacketBuilder {
    /// Synchronization source identifier (random per stream)
    ssrc: u32,
    /// Current sequence number (wraps at 65535)
    sequence: u16,
    /// Current timestamp (sample count)
    timestamp: u32,
    /// Payload type (dynamic, typically 96-127)
    payload_type: u8,
    /// Pre-allocated packet buffer
    packet_buffer: Vec<u8>,
}

impl RtpPacketBuilder {
    /// Create a new RTP packet builder.
    ///
    /// # Arguments
    /// * `ssrc` - Synchronization source identifier (use random value)
    /// * `payload_type` - RTP payload type (typically 96 for L24/48000/2)
    pub fn new(ssrc: u32, payload_type: u8) -> Self {
        Self {
            ssrc,
            sequence: 0,
            timestamp: 0,
            payload_type,
            // Pre-allocate for typical max packet: 12 byte header + 240 samples * 2 ch * 3 bytes
            packet_buffer: Vec::with_capacity(12 + 1440),
        }
    }

    /// Build an RTP packet from float samples.
    /// Returns the complete packet ready to send.
    ///
    /// # Arguments
    /// * `samples` - Interleaved float samples in [-1.0, 1.0] range
    /// * `channels` - Number of audio channels
    ///
    /// # Returns
    /// Slice of the internal buffer containing the complete RTP packet
    pub fn build_packet(&mut self, samples: &[f32], channels: u16) -> &[u8] {
        let sample_count = samples.len() / channels as usize;
        let payload_size = samples.len() * 3; // 24-bit = 3 bytes per sample
        let packet_size = 12 + payload_size;

        // Resize buffer if needed
        self.packet_buffer.clear();
        self.packet_buffer.resize(packet_size, 0);

        // Build RTP header (12 bytes)
        // Byte 0: V=2, P=0, X=0, CC=0 -> 0x80
        self.packet_buffer[0] = 0x80;
        // Byte 1: M=0, PT
        self.packet_buffer[1] = self.payload_type & 0x7F;
        // Bytes 2-3: Sequence number (big-endian)
        self.packet_buffer[2..4].copy_from_slice(&self.sequence.to_be_bytes());
        // Bytes 4-7: Timestamp (big-endian)
        self.packet_buffer[4..8].copy_from_slice(&self.timestamp.to_be_bytes());
        // Bytes 8-11: SSRC (big-endian)
        self.packet_buffer[8..12].copy_from_slice(&self.ssrc.to_be_bytes());

        // Convert float samples to 24-bit big-endian payload
        convert_float_to_24bit_be(samples, &mut self.packet_buffer[12..]);

        // Advance sequence and timestamp for next packet
        self.sequence = self.sequence.wrapping_add(1);
        self.timestamp = self.timestamp.wrapping_add(sample_count as u32);

        &self.packet_buffer[..packet_size]
    }

    /// Get current sequence number (for diagnostics)
    pub fn sequence(&self) -> u16 {
        self.sequence
    }

    /// Get current timestamp (for diagnostics)
    pub fn timestamp(&self) -> u32 {
        self.timestamp
    }

    /// Get SSRC (for diagnostics)
    pub fn ssrc(&self) -> u32 {
        self.ssrc
    }
}

/// Convert 32-bit float to 24-bit big-endian PCM.
/// Inverse of our input conversion which does:
///   sample_i32 = (b0 << 24) | (b1 << 16) | (b2 << 8)
///   sample_i32 = sample_i32 >> 8  (sign-extend)
///   float = sample_i32 / 8388608.0
///
/// So output does the reverse:
///   sample_i24 = float * 8388607.0
///   Extract low 24 bits as big-endian bytes
///
/// # Arguments
/// * `input` - Float samples in [-1.0, 1.0] range
/// * `output` - Output buffer (must be at least input.len() * 3 bytes)
pub fn convert_float_to_24bit_be(input: &[f32], output: &mut [u8]) {
    for (i, &sample) in input.iter().enumerate() {
        let offset = i * 3;
        if offset + 2 >= output.len() {
            break;
        }

        // Clamp and scale to 24-bit range
        let clamped = sample.clamp(-1.0, 1.0);
        let sample_i24 = (clamped * 8388607.0) as i32;

        // Extract low 24 bits as big-endian
        // For negative numbers (e.g., -1.0 -> -8388607 = 0xFF800001):
        //   & 0xFFFFFF gives 0x800001, shifted gives bytes 80 00 01
        // For positive numbers (e.g., 1.0 -> 8388607 = 0x007FFFFF):
        //   & 0xFFFFFF gives 0x7FFFFF, shifted gives bytes 7F FF FF
        let u24 = (sample_i24 as u32) & 0x00FFFFFF;
        output[offset] = (u24 >> 16) as u8;     // MSB
        output[offset + 1] = (u24 >> 8) as u8;
        output[offset + 2] = u24 as u8;         // LSB
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_float_to_24bit_roundtrip() {
        // Test values
        let input = [0.0f32, 0.5, -0.5, 1.0, -1.0];
        let mut output = vec![0u8; input.len() * 3];

        convert_float_to_24bit_be(&input, &mut output);

        // Verify by converting back using same method as input/rtp.rs:
        // sample_i32 = (b0 << 24) | (b1 << 16) | (b2 << 8)
        // sample_i32 = sample_i32 >> 8 (sign-extend)
        // float = sample_i32 / 8388608.0
        for (i, &original) in input.iter().enumerate() {
            let offset = i * 3;
            let b0 = output[offset] as i32;
            let b1 = output[offset + 1] as i32;
            let b2 = output[offset + 2] as i32;

            let sample_i32 = (b0 << 24) | (b1 << 16) | (b2 << 8);
            let sample_i32 = sample_i32 >> 8; // Arithmetic shift to sign-extend
            let recovered = sample_i32 as f32 / 8388608.0;

            // Should be very close (within quantization error)
            assert!(
                (recovered - original).abs() < 0.001,
                "Mismatch at {}: original={}, recovered={}, bytes=[{:02X},{:02X},{:02X}]",
                i,
                original,
                recovered,
                output[offset],
                output[offset + 1],
                output[offset + 2]
            );
        }
    }

    #[test]
    fn test_rtp_packet_builder() {
        let mut builder = RtpPacketBuilder::new(0x12345678, 96);

        // Build first packet with 48 stereo samples (1ms at 48kHz)
        let samples = vec![0.0f32; 96]; // 48 samples * 2 channels
        let packet = builder.build_packet(&samples, 2);

        // Check header
        assert_eq!(packet[0], 0x80); // V=2, P=0, X=0, CC=0
        assert_eq!(packet[1], 96);   // PT=96, M=0
        assert_eq!(u16::from_be_bytes([packet[2], packet[3]]), 0); // First seq
        assert_eq!(u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]), 0); // First timestamp
        assert_eq!(u32::from_be_bytes([packet[8], packet[9], packet[10], packet[11]]), 0x12345678); // SSRC

        // Check packet size: 12 header + 96 samples * 3 bytes
        assert_eq!(packet.len(), 12 + 288);

        // Build second packet - sequence and timestamp should advance
        let packet2 = builder.build_packet(&samples, 2);
        assert_eq!(u16::from_be_bytes([packet2[2], packet2[3]]), 1); // Second seq
        assert_eq!(u32::from_be_bytes([packet2[4], packet2[5], packet2[6], packet2[7]]), 48); // Timestamp advanced by 48 samples
    }
}
