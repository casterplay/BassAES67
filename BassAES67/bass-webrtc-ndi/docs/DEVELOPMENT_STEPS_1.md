# bass-webrtc-ndi Development - Phase 1 Complete

## Project Overview

**bass-webrtc-ndi** is a Rust crate that bridges WebRTC video/audio streams to NDI (Network Device Interface) output. It's designed to work alongside bass-webrtc, receiving video tracks from WebRTC and sending them over NDI networks.

### Target Architecture

```
OBS/Browser                MediaMTX               bass-webrtc + bass-webrtc-ndi
┌─────────────┐           ┌─────────────┐        ┌────────────────────────────────┐
│ Video+Audio │──WHIP────►│    SFU      │──WHEP─►│  WhepClient                    │
└─────────────┘           └─────────────┘        │    │                           │
                                                 │    ├─► on_track (audio/opus)   │
                                                 │    │     └──► bass_aes67 ──────► AES67
                                                 │    │                           │
                                                 │    └─► on_track (video/*)      │
                                                 │          └──► bass-webrtc-ndi ─► NDI
                                                 └────────────────────────────────┘
```

---

## Phase 1 Status: COMPLETE

### What Was Built

| File | Purpose |
|------|---------|
| `Cargo.toml` | Crate config with grafton-ndi v0.9 dependency |
| `build.rs` | NDI SDK path configuration for Windows/Linux/macOS |
| `src/lib.rs` | Module exports |
| `src/sender.rs` | NdiSender wrapper around grafton-ndi |
| `src/frame.rs` | VideoFrame, AudioFrame, VideoFormat types |
| `examples/ndi_test_pattern.rs` | SMPTE color bars test pattern |

### Key Types

```rust
// Video frame for NDI transmission
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub format: VideoFormat,      // BGRA, UYVY, NV12, I420, etc.
    pub data: Vec<u8>,
    pub stride: u32,
    pub timestamp: i64,
    pub frame_rate_n: u32,        // e.g., 30000
    pub frame_rate_d: u32,        // e.g., 1001 for 29.97fps
}

// Audio frame for NDI transmission
pub struct AudioFrame {
    pub sample_rate: u32,         // e.g., 48000
    pub channels: u16,
    pub samples_per_channel: u32,
    pub data: Vec<f32>,           // Interleaved f32 samples
    pub timestamp: i64,
}

// NDI sender
pub struct NdiSender<'a> {
    ndi: Arc<NDI>,
    sender: Sender<'a>,
    name: String,
}

impl<'a> NdiSender<'a> {
    pub fn new(ndi: &'a Arc<NDI>, name: &str) -> Result<Self, NdiError>;
    pub fn send_video(&self, frame: &VideoFrame) -> Result<(), NdiError>;
    pub fn send_audio(&self, frame: &AudioFrame) -> Result<(), NdiError>;
    pub fn has_connections(&self) -> bool;
    pub fn connection_count(&self) -> u32;
}

// Initialize NDI (call once at startup)
pub fn init_ndi() -> Result<Arc<NDI>, NdiError>;
```

### Usage Example

```rust
use bass_webrtc_ndi::{NdiSender, VideoFrame, init_ndi};

fn main() {
    // Initialize NDI once
    let ndi = init_ndi().expect("Failed to init NDI");

    // Create sender
    let sender = NdiSender::new(&ndi, "My Source").expect("Failed to create sender");

    // Create and send a frame
    let frame = VideoFrame::test_pattern_bars(1920, 1080);
    sender.send_video(&frame).expect("Failed to send");
}
```

---

## Build Requirements

### Windows (Current Development Platform)

1. **NDI 6 SDK** - Installed at `C:\Program Files\NDI\NDI 6 SDK`

2. **LLVM/Clang** - Required by grafton-ndi's bindgen
   - Install from: https://github.com/llvm/llvm-project/releases
   - Add to PATH or set `LIBCLANG_PATH=C:\Program Files\LLVM\bin`

3. **NDI Runtime DLL** - Copy to executable location:
   ```
   Copy "C:\Program Files\NDI\NDI 6 SDK\Bin\x64\Processing.NDI.Lib.x64.dll"
     to target\release\examples\
   ```

### Build Commands

```cmd
cd bass-webrtc-ndi

# Set LLVM path if not in PATH
set LIBCLANG_PATH=C:\Program Files\LLVM\bin

# Build library
cargo build --release

# Build and run test pattern
cargo build --release --example ndi_test_pattern
target\release\examples\ndi_test_pattern.exe
```

### Linux (Future)

```bash
# Install NDI SDK
export NDI_SDK_DIR=/usr/share/NDI\ SDK\ for\ Linux
export LD_LIBRARY_PATH=$NDI_SDK_DIR/lib/x86_64-linux-gnu:$LD_LIBRARY_PATH

# Build
cargo build --release
```

---

## Dependencies

```toml
[dependencies]
grafton-ndi = "0.9"      # NDI 6 SDK bindings (requires LLVM for bindgen)
tokio = { version = "1", features = ["rt-multi-thread", "sync", "time", "macros"] }
log = "0.4"
env_logger = "0.11"
thiserror = "1.0"

[dev-dependencies]
ctrlc = "3.4"
```

---

## Next Steps: Phase 2 - Video Track Reception

### Goal
Modify bass-webrtc's WHEP client to receive video tracks and pass them to bass-webrtc-ndi.

### Files to Modify in bass-webrtc

1. **`src/signaling/whep_client.rs`**
   - Add video transceiver (currently only audio)
   - Modify `on_track` handler to detect video tracks
   - Route video to NDI sender

2. **`Cargo.toml`**
   - Add dependency on bass-webrtc-ndi

### Code Changes Needed

```rust
// In whep_client.rs, add video transceiver:
peer_connection
    .add_transceiver_from_kind(
        RTPCodecType::Video,  // NEW - currently only Audio
        Some(RTCRtpTransceiverInit {
            direction: RTCRtpTransceiverDirection::Recvonly,
            send_encodings: vec![],
        }),
    )
    .await?;

// In on_track handler, detect video:
let mime = codec.capability.mime_type.to_lowercase();
if mime.contains("video/") {
    // Handle video track - spawn video reader
    spawn_video_track_reader(track, ndi_sender).await;
} else if mime.contains("opus") {
    // Existing audio handling
    spawn_track_reader(track, producer, stats, sample_rate, channels).await;
}
```

### Phase 3: Video Decoding (ffmpeg-next)

WebRTC sends encoded video (H.264, VP8, VP9, AV1). Need to decode to raw frames for NDI.

**Planned approach:**
- Add `ffmpeg-next` crate for decoding
- Requires FFmpeg libraries installed
- Handle RTP depayloading (NAL unit reassembly for H.264)

### Phase 4: Full Pipeline

Connect everything:
1. OBS sends video+audio via WHIP to MediaMTX
2. bass-webrtc receives via WHEP
3. Audio goes to bass_aes67 (existing)
4. Video decodes and goes to NDI via bass-webrtc-ndi
5. Verify in NDI Studio Monitor

---

## Tested & Working

- NDI sender initialization
- Video frame transmission (BGRA format, 1920x1080, 29.97fps)
- Test pattern visible in NDI Studio Monitor
- Connection count monitoring

---

## Session Notes

- **Date**: December 26, 2024
- **Platform**: Windows 10 x64
- **NDI SDK**: Version 6
- **grafton-ndi**: Version 0.9.0
- **Key discovery**: grafton-ndi requires LLVM/Clang for bindgen at build time


## If FFMPEG is needed
- See if this will do: C:\Dev\CasterPlay2025\BassAES67\BassAES67\Windows_need_builds\ffmpeg-gpl-shared