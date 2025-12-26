# Headless SIP Client in Rust - Implementation Guide

## Goal

Create a SIP client that can make regular phone calls where audio input/output is handled programmatically via PCM float arrays (f32) instead of using a soundcard. This is useful for:

- Voice bots / IVR systems
- Text-to-Speech (TTS) integration
- Speech-to-Text (STT) pipelines
- Audio processing applications
- Automated testing of VoIP systems

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        YOUR APPLICATION                          │
│                                                                  │
│   generate_audio() ──► Vec<f32> ──► tx_audio.send()             │
│   (TTS, audio file)                                             │
│                                                                  │
│   process_audio() ◄── Vec<f32> ◄── rx_audio.recv()              │
│   (STT, recording)                                              │
└─────────────────────────────────────────────────────────────────┘
                              │
                    ┌─────────▼─────────┐
                    │   AudioBridge     │
                    │  (channel pair)   │
                    └─────────┬─────────┘
                              │
┌─────────────────────────────▼───────────────────────────────────┐
│                     RtpMediaHandler                              │
│                                                                  │
│   f32 → i16 → G711.encode() → RtpPacketBuilder → UDP send       │
│                                                                  │
│   UDP recv → RtpReader → G711.decode() → i16 → f32              │
└─────────────────────────────────────────────────────────────────┘
                              │
                    ┌─────────▼─────────┐
                    │    rsipstack      │
                    │  (SIP signaling)  │
                    │  INVITE/BYE/etc   │
                    └───────────────────┘
                              │
                         UDP/TCP/TLS
                              │
                    ┌─────────▼─────────┐
                    │   SIP Server /    │
                    │   VoIP Provider   │
                    └───────────────────┘
```

---

## Layer Responsibilities

| Layer | Library | Purpose |
|-------|---------|---------|
| **SIP Signaling** | `rsipstack` | Call setup (INVITE), registration (REGISTER), teardown (BYE), session management |
| **RTP Transport** | `rtp-rs` or `webrtc-rs/rtp` | Packetize/depacketize audio for network transport |
| **Audio Codec** | `rvoip-codec-core` | Encode/decode G.711 (PCMU/PCMA) |
| **Sample Conversion** | `dasp` | Convert between sample formats (f32 ↔ i16) |
| **Async Runtime** | `tokio` | Async networking and task management |

---

## Required Dependencies

```toml
[dependencies]
# SIP signaling stack (RFC 3261/3262 compliant)
rsipstack = "0.2"

# RTP packet parsing and building (RFC 3550)
rtp-rs = "0.6"

# G.711 audio codec (PCMU/PCMA)
rvoip-codec-core = "0.1"

# Audio sample format conversion and DSP
dasp = { version = "0.11", features = ["signal", "sample"] }

# Async runtime
tokio = { version = "1", features = ["full", "sync", "net", "time", "macros", "rt-multi-thread"] }

# Error handling
anyhow = "1.0"
thiserror = "1.0"

# Logging
tracing = "0.1"
tracing-subscriber = "0.3"
```

### Alternative/Additional Crates

```toml
# If you need Opus codec (higher quality, WebRTC compatible)
# audiopus = "0.3"

# If you need more RTP features (RTCP, etc.)
# webrtc-rtp = "0.9"
# webrtc-rtcp = "0.9"

# For SDP parsing (if not using rsipstack's built-in)
# sdp = "0.6"
```

---

## Key Technical Parameters

### G.711 Codec (Most Compatible with PSTN/SIP)

| Parameter | Value |
|-----------|-------|
| Sample Rate | 8000 Hz |
| Bit Depth | 8-bit compressed (from 16-bit linear) |
| Frame Duration | 20ms (typical) |
| Samples per Frame | 160 samples |
| Bitrate | 64 kbps |
| RTP Payload Type | PCMU = 0, PCMA = 8 |

### Opus Codec (WebRTC, Higher Quality)

| Parameter | Value |
|-----------|-------|
| Sample Rate | 48000 Hz |
| Frame Duration | 20ms (typical) |
| Samples per Frame | 960 samples |
| Bitrate | 6-510 kbps (variable) |
| RTP Payload Type | Dynamic (96-127, negotiated via SDP) |

### PCM Sample Format Conversion

```rust
// Your app uses f32 in range [-1.0, 1.0]
// Codec uses i16 in range [-32768, 32767]

// f32 → i16 (before encoding)
let pcm_i16: Vec<i16> = pcm_f32.iter()
    .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
    .collect();

// i16 → f32 (after decoding)
let pcm_f32: Vec<f32> = pcm_i16.iter()
    .map(|&s| s as f32 / 32768.0)
    .collect();
```

---

## Implementation Components

### 1. Audio Bridge Interface

This is the interface your application uses to send/receive audio:

```rust
use tokio::sync::mpsc;

/// Audio frame with metadata
pub struct AudioFrame {
    pub samples: Vec<f32>,      // PCM samples in [-1.0, 1.0]
    pub sample_rate: u32,       // e.g., 8000 for G.711
    pub channels: u16,          // 1 for mono
}

/// Bridge between your application and the SIP client
pub struct AudioBridge {
    /// Send audio TO the call (your app → remote party)
    pub tx: mpsc::Sender<AudioFrame>,
    /// Receive audio FROM the call (remote party → your app)
    pub rx: mpsc::Receiver<AudioFrame>,
}

impl AudioBridge {
    pub fn new(buffer_size: usize) -> (Self, AudioBridgeInternal) {
        let (app_tx, internal_rx) = mpsc::channel(buffer_size);
        let (internal_tx, app_rx) = mpsc::channel(buffer_size);
        
        let bridge = AudioBridge {
            tx: app_tx,
            rx: app_rx,
        };
        
        let internal = AudioBridgeInternal {
            rx: internal_rx,
            tx: internal_tx,
        };
        
        (bridge, internal)
    }
}

/// Internal side used by RTP handler
pub struct AudioBridgeInternal {
    pub rx: mpsc::Receiver<AudioFrame>,
    pub tx: mpsc::Sender<AudioFrame>,
}
```

### 2. RTP Media Handler

Handles encoding/decoding and RTP packetization:

```rust
use rtp_rs::{RtpReader, RtpPacketBuilder, Seq, Pad};
use codec_core::codecs::g711::G711Codec;
use codec_core::types::{AudioCodec, CodecConfig, CodecType, SampleRate};
use tokio::net::UdpSocket;
use std::net::SocketAddr;

pub struct RtpMediaHandler {
    socket: UdpSocket,
    remote_addr: SocketAddr,
    codec: G711Codec,
    ssrc: u32,
    sequence: u16,
    timestamp: u32,
    payload_type: u8,
    
    // Connection to AudioBridge
    audio_rx: mpsc::Receiver<AudioFrame>,
    audio_tx: mpsc::Sender<AudioFrame>,
}

impl RtpMediaHandler {
    pub async fn new(
        local_addr: SocketAddr,
        remote_addr: SocketAddr,
        audio_internal: AudioBridgeInternal,
    ) -> anyhow::Result<Self> {
        let socket = UdpSocket::bind(local_addr).await?;
        
        // Create G.711 μ-law codec
        let config = CodecConfig::new(CodecType::G711Pcmu)
            .with_sample_rate(SampleRate::Rate8000)
            .with_channels(1);
        let codec = G711Codec::new_pcmu(config)?;
        
        Ok(Self {
            socket,
            remote_addr,
            codec,
            ssrc: rand::random(),
            sequence: rand::random(),
            timestamp: rand::random(),
            payload_type: 0, // PCMU
            audio_rx: audio_internal.rx,
            audio_tx: audio_internal.tx,
        })
    }
    
    pub async fn run(&mut self) -> anyhow::Result<()> {
        let mut recv_buf = [0u8; 1500];
        let frame_duration = tokio::time::Duration::from_millis(20);
        let mut send_interval = tokio::time::interval(frame_duration);
        
        loop {
            tokio::select! {
                // Receive RTP from network
                result = self.socket.recv_from(&mut recv_buf) => {
                    let (len, _addr) = result?;
                    self.handle_incoming_rtp(&recv_buf[..len]).await?;
                }
                
                // Send audio at regular intervals
                _ = send_interval.tick() => {
                    self.send_audio_frame().await?;
                }
            }
        }
    }
    
    async fn handle_incoming_rtp(&mut self, data: &[u8]) -> anyhow::Result<()> {
        if let Ok(rtp) = RtpReader::new(data) {
            let payload = rtp.payload();
            
            // Decode G.711 → i16
            let pcm_i16 = self.codec.decode(payload)?;
            
            // Convert i16 → f32
            let pcm_f32: Vec<f32> = pcm_i16.iter()
                .map(|&s| s as f32 / 32768.0)
                .collect();
            
            let frame = AudioFrame {
                samples: pcm_f32,
                sample_rate: 8000,
                channels: 1,
            };
            
            // Send to application (non-blocking)
            let _ = self.audio_tx.try_send(frame);
        }
        Ok(())
    }
    
    async fn send_audio_frame(&mut self) -> anyhow::Result<()> {
        // Try to get audio from application
        let frame = match self.audio_rx.try_recv() {
            Ok(frame) => frame,
            Err(_) => {
                // No audio available, send silence
                AudioFrame {
                    samples: vec![0.0; 160],
                    sample_rate: 8000,
                    channels: 1,
                }
            }
        };
        
        // Convert f32 → i16
        let pcm_i16: Vec<i16> = frame.samples.iter()
            .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect();
        
        // Encode to G.711
        let encoded = self.codec.encode(&pcm_i16)?;
        
        // Build RTP packet
        let packet = RtpPacketBuilder::new()
            .payload_type(self.payload_type)
            .ssrc(self.ssrc)
            .sequence(Seq::from(self.sequence))
            .timestamp(self.timestamp)
            .payload(&encoded)
            .build()?;
        
        // Send packet
        self.socket.send_to(&packet, self.remote_addr).await?;
        
        // Increment counters
        self.sequence = self.sequence.wrapping_add(1);
        self.timestamp = self.timestamp.wrapping_add(160); // 160 samples @ 8kHz = 20ms
        
        Ok(())
    }
}
```

### 3. SIP Client using rsipstack

```rust
use rsipstack::{EndpointBuilder, transport::TransportLayer};
use rsipstack::dialog::{DialogLayer, registration::Registration, invitation::InviteOption};
use rsipstack::dialog::authenticate::Credential;
use tokio_util::sync::CancellationToken;
use std::sync::Arc;

pub struct SipClient {
    endpoint: Arc<rsipstack::EndpointInner>,
    dialog_layer: Arc<DialogLayer>,
    credential: Option<Credential>,
    local_addr: SocketAddr,
}

impl SipClient {
    pub async fn new(
        local_addr: SocketAddr,
        username: Option<String>,
        password: Option<String>,
    ) -> anyhow::Result<Self> {
        let cancel_token = CancellationToken::new();
        let transport_layer = TransportLayer::new(cancel_token.clone());
        
        // Add UDP transport
        let udp_listener = rsipstack::transport::udp::UdpConnection::create_connection(
            local_addr,
            None,
            Some(cancel_token.child_token()),
        ).await?;
        transport_layer.add_transport(udp_listener.into());
        
        let endpoint = EndpointBuilder::new()
            .with_transport_layer(transport_layer)
            .with_cancel_token(cancel_token)
            .build();
        
        let endpoint_inner = endpoint.inner.clone();
        
        // Start endpoint in background
        tokio::spawn(async move {
            if let Err(e) = endpoint.inner.serve().await {
                tracing::error!("Endpoint error: {}", e);
            }
        });
        
        let dialog_layer = Arc::new(DialogLayer::new(endpoint_inner.clone()));
        
        let credential = match (username, password) {
            (Some(u), Some(p)) => Some(Credential {
                username: u,
                password: p,
                realm: None,
            }),
            _ => None,
        };
        
        Ok(Self {
            endpoint: endpoint_inner,
            dialog_layer,
            credential,
            local_addr,
        })
    }
    
    /// Register with a SIP registrar
    pub async fn register(&self, registrar_uri: &str) -> anyhow::Result<()> {
        let mut registration = Registration::new(
            self.endpoint.clone(),
            self.credential.clone(),
        );
        
        let uri = registrar_uri.parse()?;
        let _response = registration.register(uri, None).await?;
        
        Ok(())
    }
    
    /// Make an outgoing call
    pub async fn call(
        &self,
        callee: &str,
        caller: &str,
        rtp_port: u16,
    ) -> anyhow::Result<CallSession> {
        let (state_tx, state_rx) = tokio::sync::mpsc::unbounded_channel();
        
        // Build SDP offer with audio
        let sdp = self.build_sdp_offer(rtp_port);
        
        let invite_option = InviteOption {
            callee: callee.parse()?,
            caller: caller.parse()?,
            content_type: Some("application/sdp".to_string()),
            offer: Some(sdp.into_bytes()),
            contact: format!("sip:{}:{}", self.local_addr.ip(), self.local_addr.port()).parse()?,
            credential: self.credential.clone(),
            headers: None,
        };
        
        let (dialog, response) = self.dialog_layer
            .do_invite(invite_option, state_tx)
            .await?;
        
        // Parse SDP answer to get remote RTP address
        let remote_rtp_addr = self.parse_sdp_answer(&response)?;
        
        Ok(CallSession {
            dialog,
            state_rx,
            remote_rtp_addr,
        })
    }
    
    fn build_sdp_offer(&self, rtp_port: u16) -> String {
        let ip = self.local_addr.ip();
        format!(
            "v=0\r\n\
             o=- {} 0 IN IP4 {}\r\n\
             s=rsipstack call\r\n\
             c=IN IP4 {}\r\n\
             t=0 0\r\n\
             m=audio {} RTP/AVP 0 8\r\n\
             a=rtpmap:0 PCMU/8000\r\n\
             a=rtpmap:8 PCMA/8000\r\n\
             a=sendrecv\r\n",
            chrono::Utc::now().timestamp(),
            ip,
            ip,
            rtp_port,
        )
    }
    
    fn parse_sdp_answer(&self, response: &rsip::Response) -> anyhow::Result<SocketAddr> {
        // Parse the SDP from response body to extract remote RTP address
        // This is simplified - real implementation needs proper SDP parsing
        let body = std::str::from_utf8(response.body())?;
        
        // Extract c= line for IP and m= line for port
        // ... parsing logic here ...
        
        // Placeholder - implement proper SDP parsing
        Ok("0.0.0.0:0".parse()?)
    }
}

pub struct CallSession {
    pub dialog: rsipstack::dialog::invitation::InviteDialog,
    pub state_rx: tokio::sync::mpsc::UnboundedReceiver<rsipstack::dialog::DialogState>,
    pub remote_rtp_addr: SocketAddr,
}
```

### 4. Main Integration

```rust
use tokio::sync::mpsc;

pub struct HeadlessSipClient {
    sip_client: SipClient,
    audio_bridge: AudioBridge,
}

impl HeadlessSipClient {
    pub async fn new(config: ClientConfig) -> anyhow::Result<Self> {
        let sip_client = SipClient::new(
            config.local_sip_addr,
            config.username,
            config.password,
        ).await?;
        
        let (audio_bridge, _internal) = AudioBridge::new(32);
        
        Ok(Self {
            sip_client,
            audio_bridge,
        })
    }
    
    /// Make a call and return audio channels
    pub async fn call(&mut self, destination: &str) -> anyhow::Result<ActiveCall> {
        // Allocate RTP port
        let rtp_port = self.allocate_rtp_port().await?;
        
        // Create audio bridge for this call
        let (audio_bridge, audio_internal) = AudioBridge::new(32);
        
        // Start SIP call
        let call_session = self.sip_client.call(
            destination,
            &format!("sip:user@{}", self.sip_client.local_addr),
            rtp_port,
        ).await?;
        
        // Start RTP handler
        let local_rtp_addr = SocketAddr::new(
            self.sip_client.local_addr.ip(),
            rtp_port,
        );
        
        let mut rtp_handler = RtpMediaHandler::new(
            local_rtp_addr,
            call_session.remote_rtp_addr,
            audio_internal,
        ).await?;
        
        // Spawn RTP handler task
        let rtp_handle = tokio::spawn(async move {
            if let Err(e) = rtp_handler.run().await {
                tracing::error!("RTP handler error: {}", e);
            }
        });
        
        Ok(ActiveCall {
            session: call_session,
            audio: audio_bridge,
            rtp_handle,
        })
    }
    
    async fn allocate_rtp_port(&self) -> anyhow::Result<u16> {
        // Bind to port 0 to get an available port
        let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
        Ok(socket.local_addr()?.port())
    }
}

pub struct ActiveCall {
    pub session: CallSession,
    pub audio: AudioBridge,
    rtp_handle: tokio::task::JoinHandle<()>,
}

pub struct ClientConfig {
    pub local_sip_addr: SocketAddr,
    pub username: Option<String>,
    pub password: Option<String>,
    pub registrar: Option<String>,
}
```

---

## Usage Example

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::init();
    
    // Create client
    let config = ClientConfig {
        local_sip_addr: "0.0.0.0:5060".parse()?,
        username: Some("alice".to_string()),
        password: Some("secret".to_string()),
        registrar: Some("sip:registrar.example.com".to_string()),
    };
    
    let mut client = HeadlessSipClient::new(config).await?;
    
    // Make a call
    let mut call = client.call("sip:bob@example.com").await?;
    
    // Send audio (e.g., from TTS)
    let tts_audio: Vec<f32> = generate_tts_audio("Hello, world!");
    call.audio.tx.send(AudioFrame {
        samples: tts_audio,
        sample_rate: 8000,
        channels: 1,
    }).await?;
    
    // Receive audio (e.g., for STT)
    while let Some(frame) = call.audio.rx.recv().await {
        process_for_stt(&frame.samples);
    }
    
    Ok(())
}

fn generate_tts_audio(text: &str) -> Vec<f32> {
    // Your TTS implementation
    vec![0.0; 8000] // placeholder
}

fn process_for_stt(samples: &[f32]) {
    // Your STT implementation
}
```

---

## Important Considerations

### 1. Jitter Buffer

For production use, implement a jitter buffer to handle network timing variations:

```rust
pub struct JitterBuffer {
    buffer: VecDeque<(u32, Vec<f32>)>, // (timestamp, samples)
    target_delay_ms: u32,
    sample_rate: u32,
}

impl JitterBuffer {
    pub fn push(&mut self, timestamp: u32, samples: Vec<f32>) {
        // Insert in timestamp order
        let pos = self.buffer.iter()
            .position(|(ts, _)| *ts > timestamp)
            .unwrap_or(self.buffer.len());
        self.buffer.insert(pos, (timestamp, samples));
    }
    
    pub fn pop(&mut self) -> Option<Vec<f32>> {
        if self.buffer.len() > (self.target_delay_ms * self.sample_rate / 1000) as usize {
            self.buffer.pop_front().map(|(_, s)| s)
        } else {
            None
        }
    }
}
```

### 2. DTMF Handling

For sending/receiving DTMF tones (RFC 2833):

```rust
// RTP payload type for telephone-event is typically 101
const DTMF_PAYLOAD_TYPE: u8 = 101;

pub fn send_dtmf(&mut self, digit: char, duration_ms: u32) {
    let event = match digit {
        '0'..='9' => digit as u8 - b'0',
        '*' => 10,
        '#' => 11,
        'A'..='D' => digit as u8 - b'A' + 12,
        _ => return,
    };
    
    // Send RFC 2833 DTMF event packets
    // ... implementation
}
```

### 3. NAT Traversal

For calls through NAT, consider:
- STUN for discovering public IP
- ICE for connectivity checks
- Symmetric RTP (send from same port you receive on)

### 4. Error Handling

Handle common VoIP errors:
- Registration failures (401/407 authentication)
- Call failures (486 Busy, 404 Not Found, etc.)
- RTP timeout (no audio received)
- Network changes mid-call

---

## Testing

### Local Testing with SIP Server

```bash
# Run a local SIP proxy (from rsipstack examples)
cargo run --example proxy -- --port 25060 --addr 127.0.0.1

# Run your client against it
cargo run -- --sip-server 127.0.0.1:25060
```

### Testing with Asterisk/FreeSWITCH

Configure a local Asterisk or FreeSWITCH server for more realistic testing with PSTN gateway capabilities.

---

## References

- [rsipstack GitHub](https://github.com/restsend/rsipstack) - SIP stack documentation
- [RFC 3261](https://datatracker.ietf.org/doc/html/rfc3261) - SIP protocol
- [RFC 3550](https://datatracker.ietf.org/doc/html/rfc3550) - RTP protocol
- [RFC 3551](https://datatracker.ietf.org/doc/html/rfc3551) - RTP audio/video profile
- [G.711 Wikipedia](https://en.wikipedia.org/wiki/G.711) - Codec details
- [dasp crate](https://docs.rs/dasp) - Audio sample processing
- [rtp-rs crate](https://docs.rs/rtp-rs) - RTP packet handling
