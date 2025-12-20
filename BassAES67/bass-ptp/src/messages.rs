//! PTPv2 (IEEE 1588-2008) message parsing.

/// PTP message types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PtpMessageType {
    Sync = 0x0,
    DelayReq = 0x1,
    PDelayReq = 0x2,
    PDelayResp = 0x3,
    FollowUp = 0x8,
    DelayResp = 0x9,
    PDelayRespFollowUp = 0xA,
    Announce = 0xB,
    Signaling = 0xC,
    Management = 0xD,
    Unknown = 0xFF,
}

impl From<u8> for PtpMessageType {
    fn from(value: u8) -> Self {
        match value & 0x0F {
            0x0 => Self::Sync,
            0x1 => Self::DelayReq,
            0x2 => Self::PDelayReq,
            0x3 => Self::PDelayResp,
            0x8 => Self::FollowUp,
            0x9 => Self::DelayResp,
            0xA => Self::PDelayRespFollowUp,
            0xB => Self::Announce,
            0xC => Self::Signaling,
            0xD => Self::Management,
            _ => Self::Unknown,
        }
    }
}

/// PTP clock identity (EUI-64 format, 8 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClockIdentity(pub [u8; 8]);

impl ClockIdentity {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut id = [0u8; 8];
        if bytes.len() >= 8 {
            id.copy_from_slice(&bytes[..8]);
        }
        Self(id)
    }

    pub fn to_u64(&self) -> u64 {
        u64::from_be_bytes(self.0)
    }

    /// Format as hex string (e.g., "2ccf67fffe55b29a")
    pub fn to_hex_string(&self) -> String {
        format!(
            "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3],
            self.0[4], self.0[5], self.0[6], self.0[7]
        )
    }
}

/// Port identity (clock identity + port number)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PortIdentity {
    pub clock_identity: ClockIdentity,
    pub port_number: u16,
}

impl PortIdentity {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 10 {
            return None;
        }
        Some(Self {
            clock_identity: ClockIdentity::from_bytes(&bytes[0..8]),
            port_number: u16::from_be_bytes([bytes[8], bytes[9]]),
        })
    }
}

/// PTP timestamp (48-bit seconds + 32-bit nanoseconds)
#[derive(Debug, Clone, Copy, Default)]
pub struct PtpTimestamp {
    pub seconds: u64,      // Actually 48 bits in protocol
    pub nanoseconds: u32,
}

impl PtpTimestamp {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 10 {
            return None;
        }
        // 6 bytes for seconds (48-bit), 4 bytes for nanoseconds
        let seconds = u64::from_be_bytes([
            0, 0, bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5],
        ]);
        let nanoseconds = u32::from_be_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]);
        Some(Self { seconds, nanoseconds })
    }

    pub fn to_ns(&self) -> i64 {
        self.seconds as i64 * 1_000_000_000 + self.nanoseconds as i64
    }
}

/// Common PTP header (34 bytes)
#[derive(Debug, Clone)]
pub struct PtpHeader {
    pub message_type: PtpMessageType,
    pub version: u8,
    pub message_length: u16,
    pub domain_number: u8,
    pub flags: u16,
    pub correction_field: i64,
    pub source_port_identity: PortIdentity,
    pub sequence_id: u16,
    pub control_field: u8,
    pub log_message_interval: i8,
}

impl PtpHeader {
    pub const SIZE: usize = 34;

    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }

        let message_type = PtpMessageType::from(data[0]);
        let version = data[1] & 0x0F;
        let message_length = u16::from_be_bytes([data[2], data[3]]);
        let domain_number = data[4];
        // byte 5 is reserved
        let flags = u16::from_be_bytes([data[6], data[7]]);
        let correction_field = i64::from_be_bytes([
            data[8], data[9], data[10], data[11],
            data[12], data[13], data[14], data[15],
        ]);
        // bytes 16-19 are reserved
        let source_port_identity = PortIdentity::from_bytes(&data[20..30])?;
        let sequence_id = u16::from_be_bytes([data[30], data[31]]);
        let control_field = data[32];
        let log_message_interval = data[33] as i8;

        Some(Self {
            message_type,
            version,
            message_length,
            domain_number,
            flags,
            correction_field,
            source_port_identity,
            sequence_id,
            control_field,
            log_message_interval,
        })
    }

    /// Check if this is a two-step message (Follow_Up will contain precise timestamp)
    pub fn is_two_step(&self) -> bool {
        (self.flags & 0x0200) != 0
    }
}

/// Announce message body (after common header)
#[derive(Debug, Clone)]
pub struct AnnounceMessage {
    pub header: PtpHeader,
    pub origin_timestamp: PtpTimestamp,
    pub current_utc_offset: i16,
    pub grandmaster_priority1: u8,
    pub grandmaster_clock_quality: ClockQuality,
    pub grandmaster_priority2: u8,
    pub grandmaster_identity: ClockIdentity,
    pub steps_removed: u16,
    pub time_source: u8,
}

/// Clock quality information
#[derive(Debug, Clone, Copy, Default)]
pub struct ClockQuality {
    pub clock_class: u8,
    pub clock_accuracy: u8,
    pub offset_scaled_log_variance: u16,
}

impl ClockQuality {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 4 {
            return None;
        }
        Some(Self {
            clock_class: bytes[0],
            clock_accuracy: bytes[1],
            offset_scaled_log_variance: u16::from_be_bytes([bytes[2], bytes[3]]),
        })
    }
}

impl AnnounceMessage {
    pub fn parse(data: &[u8]) -> Option<Self> {
        let header = PtpHeader::parse(data)?;
        if header.message_type != PtpMessageType::Announce {
            return None;
        }

        // Announce body starts at byte 34
        let body = &data[PtpHeader::SIZE..];
        if body.len() < 30 {
            return None;
        }

        let origin_timestamp = PtpTimestamp::from_bytes(&body[0..10])?;
        let current_utc_offset = i16::from_be_bytes([body[10], body[11]]);
        // byte 12 is reserved
        let grandmaster_priority1 = body[13];
        let grandmaster_clock_quality = ClockQuality::from_bytes(&body[14..18])?;
        let grandmaster_priority2 = body[18];
        let grandmaster_identity = ClockIdentity::from_bytes(&body[19..27]);
        let steps_removed = u16::from_be_bytes([body[27], body[28]]);
        let time_source = body[29];

        Some(Self {
            header,
            origin_timestamp,
            current_utc_offset,
            grandmaster_priority1,
            grandmaster_clock_quality,
            grandmaster_priority2,
            grandmaster_identity,
            steps_removed,
            time_source,
        })
    }
}

/// Sync message body
#[derive(Debug, Clone)]
pub struct SyncMessage {
    pub header: PtpHeader,
    pub origin_timestamp: PtpTimestamp,
}

impl SyncMessage {
    pub fn parse(data: &[u8]) -> Option<Self> {
        let header = PtpHeader::parse(data)?;
        if header.message_type != PtpMessageType::Sync {
            return None;
        }

        let body = &data[PtpHeader::SIZE..];
        if body.len() < 10 {
            return None;
        }

        let origin_timestamp = PtpTimestamp::from_bytes(&body[0..10])?;

        Some(Self {
            header,
            origin_timestamp,
        })
    }
}

/// Follow_Up message body
#[derive(Debug, Clone)]
pub struct FollowUpMessage {
    pub header: PtpHeader,
    pub precise_origin_timestamp: PtpTimestamp,
}

impl FollowUpMessage {
    pub fn parse(data: &[u8]) -> Option<Self> {
        let header = PtpHeader::parse(data)?;
        if header.message_type != PtpMessageType::FollowUp {
            return None;
        }

        let body = &data[PtpHeader::SIZE..];
        if body.len() < 10 {
            return None;
        }

        let precise_origin_timestamp = PtpTimestamp::from_bytes(&body[0..10])?;

        Some(Self {
            header,
            precise_origin_timestamp,
        })
    }
}

/// Delay_Req message body
#[derive(Debug, Clone)]
pub struct DelayReqMessage {
    pub header: PtpHeader,
    pub origin_timestamp: PtpTimestamp,
}

impl DelayReqMessage {
    pub const SIZE: usize = PtpHeader::SIZE + 10;

    /// Create a new Delay_Req message
    pub fn new(
        source_port: PortIdentity,
        sequence_id: u16,
        domain: u8,
    ) -> Self {
        Self {
            header: PtpHeader {
                message_type: PtpMessageType::DelayReq,
                version: 2,
                message_length: Self::SIZE as u16,
                domain_number: domain,
                flags: 0,
                correction_field: 0,
                source_port_identity: source_port,
                sequence_id,
                control_field: 1, // Delay_Req control
                log_message_interval: 0x7F, // No periodic sending
            },
            origin_timestamp: PtpTimestamp::default(),
        }
    }

    /// Serialize to bytes for transmission
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = vec![0u8; Self::SIZE];

        // Header
        buf[0] = self.header.message_type as u8;
        buf[1] = self.header.version;
        buf[2..4].copy_from_slice(&self.header.message_length.to_be_bytes());
        buf[4] = self.header.domain_number;
        buf[6..8].copy_from_slice(&self.header.flags.to_be_bytes());
        buf[8..16].copy_from_slice(&self.header.correction_field.to_be_bytes());
        buf[20..28].copy_from_slice(&self.header.source_port_identity.clock_identity.0);
        buf[28..30].copy_from_slice(&self.header.source_port_identity.port_number.to_be_bytes());
        buf[30..32].copy_from_slice(&self.header.sequence_id.to_be_bytes());
        buf[32] = self.header.control_field;
        buf[33] = self.header.log_message_interval as u8;

        // Origin timestamp (zeros - we'll record actual time when sending)
        // bytes 34-43 are already zero

        buf
    }
}

/// Delay_Resp message body
#[derive(Debug, Clone)]
pub struct DelayRespMessage {
    pub header: PtpHeader,
    pub receive_timestamp: PtpTimestamp,
    pub requesting_port_identity: PortIdentity,
}

impl DelayRespMessage {
    pub fn parse(data: &[u8]) -> Option<Self> {
        let header = PtpHeader::parse(data)?;
        if header.message_type != PtpMessageType::DelayResp {
            return None;
        }

        let body = &data[PtpHeader::SIZE..];
        if body.len() < 20 {
            return None;
        }

        let receive_timestamp = PtpTimestamp::from_bytes(&body[0..10])?;
        let requesting_port_identity = PortIdentity::from_bytes(&body[10..20])?;

        Some(Self {
            header,
            receive_timestamp,
            requesting_port_identity,
        })
    }
}

/// Parsed PTP message (any type)
#[derive(Debug, Clone)]
pub enum PtpMessage {
    Announce(AnnounceMessage),
    Sync(SyncMessage),
    FollowUp(FollowUpMessage),
    DelayResp(DelayRespMessage),
    Other(PtpHeader),
}

impl PtpMessage {
    /// Parse any PTP message from bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        let header = PtpHeader::parse(data)?;

        match header.message_type {
            PtpMessageType::Announce => {
                AnnounceMessage::parse(data).map(PtpMessage::Announce)
            }
            PtpMessageType::Sync => {
                SyncMessage::parse(data).map(PtpMessage::Sync)
            }
            PtpMessageType::FollowUp => {
                FollowUpMessage::parse(data).map(PtpMessage::FollowUp)
            }
            PtpMessageType::DelayResp => {
                DelayRespMessage::parse(data).map(PtpMessage::DelayResp)
            }
            _ => Some(PtpMessage::Other(header)),
        }
    }

    pub fn header(&self) -> &PtpHeader {
        match self {
            PtpMessage::Announce(m) => &m.header,
            PtpMessage::Sync(m) => &m.header,
            PtpMessage::FollowUp(m) => &m.header,
            PtpMessage::DelayResp(m) => &m.header,
            PtpMessage::Other(h) => h,
        }
    }
}
