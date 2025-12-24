//! RTP payload type registry and codec mapping.
//!
//! Maps RTP payload types to audio codecs for the Telos Z/IP ONE.

/// Audio codec types supported by the RTP plugin
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadCodec {
    /// G.711 u-Law (PT 0)
    G711Ulaw,
    /// G.722 wideband (PT 9)
    G722,
    /// PCM 16-bit (PT 21)
    Pcm16,
    /// PCM 20-bit (PT 116)
    Pcm20,
    /// PCM 24-bit (PT 22)
    Pcm24,
    /// MPEG-1 Layer 2 (PT 14 or 96)
    Mp2,
    /// OPUS (dynamic PT)
    Opus,
    /// FLAC (dynamic PT)
    Flac,
    /// AAC (PT 99 for MP2-AAC Xstream only - PT 122 LATM not supported)
    Aac,
    /// Unknown codec
    Unknown(u8),
}

impl PayloadCodec {
    /// Get codec from RTP payload type
    ///
    /// Based on Telos Z/IP ONE payload types:
    /// - G711: PT 0
    /// - G722: PT 9
    /// - PCM-16: PT 21
    /// - PCM-20: PT 116
    /// - PCM-24: PT 22
    /// - MP2: PT 14 or 96
    /// - AAC: PT 99 (MP2-AAC Xstream with ADTS format)
    /// - PT 122 (AAC-LATM) is NOT supported - requires native LATM decoder
    pub fn from_pt(pt: u8) -> Self {
        match pt {
            0 => PayloadCodec::G711Ulaw,
            9 => PayloadCodec::G722,
            14 | 96 => PayloadCodec::Mp2,
            21 => PayloadCodec::Pcm16,
            22 => PayloadCodec::Pcm24,
            99 => PayloadCodec::Aac, // MP2-AAC Xstream (ADTS format - works)
            // PT 122 (AAC-LATM) intentionally not mapped - requires native LATM decoder
            116 => PayloadCodec::Pcm20,
            _ => PayloadCodec::Unknown(pt),
        }
    }

    /// Get the default RTP payload type for this codec
    pub fn to_pt(&self) -> u8 {
        match self {
            PayloadCodec::G711Ulaw => 0,
            PayloadCodec::G722 => 9,
            PayloadCodec::Pcm16 => 21,
            PayloadCodec::Pcm20 => 116,
            PayloadCodec::Pcm24 => 22,
            PayloadCodec::Mp2 => 14, // Could also be 96
            PayloadCodec::Opus => 111, // Common dynamic PT for OPUS
            PayloadCodec::Flac => 112, // Custom dynamic PT for FLAC
            PayloadCodec::Aac => 99, // MP2-AAC Xstream (ADTS format)
            PayloadCodec::Unknown(pt) => *pt,
        }
    }

    /// Get codec name as string
    pub fn name(&self) -> &'static str {
        match self {
            PayloadCodec::G711Ulaw => "G.711 u-Law",
            PayloadCodec::G722 => "G.722",
            PayloadCodec::Pcm16 => "PCM 16-bit",
            PayloadCodec::Pcm20 => "PCM 20-bit",
            PayloadCodec::Pcm24 => "PCM 24-bit",
            PayloadCodec::Mp2 => "MP2",
            PayloadCodec::Opus => "OPUS",
            PayloadCodec::Flac => "FLAC",
            PayloadCodec::Aac => "AAC",
            PayloadCodec::Unknown(_) => "Unknown",
        }
    }

    /// Check if this codec is currently supported for decoding
    pub fn is_decode_supported(&self) -> bool {
        matches!(
            self,
            PayloadCodec::Pcm16 | PayloadCodec::Pcm24 | PayloadCodec::Mp2 |
            PayloadCodec::Opus | PayloadCodec::Flac |
            PayloadCodec::G711Ulaw | PayloadCodec::G722 | PayloadCodec::Aac
        )
    }

    /// Check if this codec is currently supported for encoding
    pub fn is_encode_supported(&self) -> bool {
        matches!(
            self,
            PayloadCodec::Pcm16 | PayloadCodec::Pcm24 | PayloadCodec::Mp2 |
            PayloadCodec::Opus | PayloadCodec::Flac | PayloadCodec::Aac
        )
    }

    /// Get typical samples per RTP packet for this codec at 48kHz
    ///
    /// Returns samples per channel per packet
    pub fn samples_per_packet(&self, sample_rate: u32) -> usize {
        match self {
            // PCM: typically 1ms packets (48 samples at 48kHz)
            PayloadCodec::Pcm16 | PayloadCodec::Pcm20 | PayloadCodec::Pcm24 => {
                (sample_rate / 1000) as usize // 1ms worth
            }
            // MP2: 1152 samples per frame (fixed by MPEG spec)
            PayloadCodec::Mp2 => 1152,
            // OPUS: configurable, typically 10ms or 20ms
            PayloadCodec::Opus => (sample_rate / 50) as usize, // 20ms
            // FLAC: typically 1152 samples (matches MP2)
            PayloadCodec::Flac => 1152,
            // G.711: 8kHz, 160 samples (20ms)
            PayloadCodec::G711Ulaw => 160,
            // G.722: 16kHz, 320 samples (20ms)
            PayloadCodec::G722 => 320,
            // AAC: typically 1024 samples per frame
            PayloadCodec::Aac => 1024,
            // Unknown: assume 1ms
            PayloadCodec::Unknown(_) => (sample_rate / 1000) as usize,
        }
    }

    /// Get bytes per sample for PCM codecs
    pub fn bytes_per_sample(&self) -> Option<usize> {
        match self {
            PayloadCodec::Pcm16 => Some(2),
            PayloadCodec::Pcm20 => Some(3), // Packed 20-bit uses 3 bytes
            PayloadCodec::Pcm24 => Some(3),
            _ => None, // Compressed codecs have variable size
        }
    }
}

/// Convert BASS_RTP_CODEC_* constant to PayloadCodec
pub fn codec_from_bass_constant(codec: u8) -> PayloadCodec {
    match codec {
        0 => PayloadCodec::Pcm16,
        1 => PayloadCodec::Pcm24,
        2 => PayloadCodec::Mp2,
        3 => PayloadCodec::Opus,
        4 => PayloadCodec::Flac,
        _ => PayloadCodec::Pcm16, // Default
    }
}

/// Convert PayloadCodec to BASS_RTP_CODEC_* constant
pub fn codec_to_bass_constant(codec: PayloadCodec) -> u8 {
    match codec {
        PayloadCodec::Pcm16 => 0,
        PayloadCodec::Pcm24 => 1,
        PayloadCodec::Mp2 => 2,
        PayloadCodec::Opus => 3,
        PayloadCodec::Flac => 4,
        _ => 0, // Default to PCM16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payload_type_mapping() {
        assert_eq!(PayloadCodec::from_pt(0), PayloadCodec::G711Ulaw);
        assert_eq!(PayloadCodec::from_pt(9), PayloadCodec::G722);
        assert_eq!(PayloadCodec::from_pt(21), PayloadCodec::Pcm16);
        assert_eq!(PayloadCodec::from_pt(22), PayloadCodec::Pcm24);
        assert_eq!(PayloadCodec::from_pt(14), PayloadCodec::Mp2);
        assert_eq!(PayloadCodec::from_pt(96), PayloadCodec::Mp2);
    }

    #[test]
    fn test_codec_roundtrip() {
        let codec = PayloadCodec::Pcm16;
        let pt = codec.to_pt();
        assert_eq!(PayloadCodec::from_pt(pt), codec);
    }
}
