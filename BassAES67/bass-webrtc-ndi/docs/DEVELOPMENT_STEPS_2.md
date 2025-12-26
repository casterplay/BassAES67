# bass-webrtc-ndi Development Steps - Session 2

## Session Overview

This session focused on completing the WHEP NDI client with video support, fixing track detection issues, and cleaning up debug output.

## What Was Built

### Core Functionality
- **WhepNdiClient** - WHEP client that receives WebRTC audio+video and outputs:
  - Video to NDI via NdiSender
  - Audio to BASS ring buffer (for user-controlled playback)
  - Optional: Audio to NDI (configurable)

### Key Files Modified/Created

| File | Description |
|------|-------------|
| `src/signaling/whep_ndi_client.rs` | Main WHEP client with video+NDI support |
| `src/codec/video.rs` | FFmpeg video decoder (H.264, VP8, VP9) |
| `examples/webrtc_ndi_receiver.rs` | Example application |
| `examples/test_client_video.html` | Browser test client for sending video via WHIP |

## Technical Details

### H.264 RTP Depacketization (RFC 6184)

WebRTC sends H.264 in RTP packets that must be converted to Annex B format for FFmpeg. The `H264Depacketizer` handles:

1. **Single NAL units (types 1-23)** - Direct pass-through with start code
2. **STAP-A (type 24)** - Aggregated NAL units, split and add start codes
3. **FU-A (type 28)** - Fragmented NAL units, reassemble with reconstructed header

```rust
struct H264Depacketizer {
    frame_buffer: Vec<u8>,    // Accumulates NAL units for one frame
    fu_buffer: Vec<u8>,       // Reassembles FU-A fragments
    in_fu: bool,              // Currently receiving fragments
    last_timestamp: u32,      // Detect frame boundaries
}
```

Frame boundaries are detected by:
- RTP marker bit (indicates last packet of frame)
- Timestamp changes (new frame started)

### webrtc-rs on_track Bug Workaround

**Problem**: When both audio and video tracks exist, only the first track's `on_track` callback fires. The second track is delivered to the transceiver receiver but the callback never triggers.

**Symptoms observed**:
- Audio-only mode: Works fine
- Video-only mode: Works fine
- Audio+Video mode: Only audio `on_track` fires, video never does

**Solution implemented** (lines 444-498 in whep_ndi_client.rs):
1. Wait up to 2 seconds for `on_track` callbacks
2. After timeout, check transceivers directly for tracks
3. If video track exists on receiver but wasn't signaled, manually spawn video reader

```rust
// After initial wait, check transceivers for any tracks that didn't trigger on_track
if !has_video_via_callback && expect_video {
    for t in transceivers.iter() {
        if t.kind() == RTPCodecType::Video {
            let receiver = t.receiver().await;
            let tracks = receiver.tracks().await;
            for track in tracks.iter() {
                // Manually spawn video reader
                tokio::spawn(async move {
                    spawn_video_track_reader(track_clone, ndi_ctx_clone, video_codec, stats_clone).await;
                });
            }
        }
    }
}
```

### FFmpeg Integration

FFmpeg is loaded dynamically from DLLs. The codec/video.rs uses the same pattern as bass-rtp/src/codec/ffmpeg_aac.rs.

**Required DLLs** (from `Windows_need_builds/ffmpeg-gpl-shared`):
- `avcodec-62.dll`
- `avutil-60.dll`
- `swscale-9.dll`

**Video pipeline**:
1. RTP packets → H264Depacketizer → Annex B NAL units
2. Annex B → FFmpeg avcodec → YUV frame
3. YUV frame → swscale → BGRA
4. BGRA → NdiSender → NDI network

### Initial Frame Errors

When connecting mid-stream, the decoder receives frames without SPS/PPS parameter sets. This causes FFmpeg errors:

```
[h264 @ ...] non-existing PPS 0 referenced
[h264 @ ...] no frame!
```

**This is expected behavior** - the decoder waits for a keyframe with SPS/PPS. Once received (typically within 1-2 seconds), decoding works normally. These errors are now silently tracked in stats rather than logged.

## Test Results

### Working Configuration
- **Source**: OBS → Browser (test_client_video.html) → MediaMTX
- **Receiver**: webrtc_ndi_receiver → NDI Studio Monitor
- **Video**: 1280x720 @ 30fps H.264
- **Audio**: Opus 48kHz stereo, ~50 packets/second
- **NDI**: 2 receivers connected

### Performance
- Video decode: 30 fps consistent
- Audio receive: 50 packets/second
- Connection time: ~2-3 seconds
- Track detection: Works with webrtc-rs workaround

## Known Issues

### 1. webrtc-rs on_track Callback Bug
- **Status**: Worked around (not fixed in library)
- **Impact**: Requires 2-second wait + transceiver polling
- **Tracking**: This is a bug in the webrtc-rs library

### 2. VP8/VP9 Depacketization
- **Status**: Not fully implemented
- **Impact**: VP8/VP9 may not decode correctly
- **Details**: Only marker-bit packets are passed through; proper depacketization needed

### 3. FFmpeg Logging
- **Status**: FFmpeg logs directly to stderr
- **Impact**: "non-existing PPS" messages visible to user
- **Mitigation**: Could set FFmpeg log level via `av_log_set_level(AV_LOG_ERROR)` or lower

### 4. Audio Producer Already Taken
- **Status**: Minor issue
- **Impact**: If on_track fires twice for audio, second call fails
- **Details**: Logged as error but doesn't affect functionality

## API Reference

### WhepNdiClient

```rust
// Connect to WHEP endpoint with NDI output
let client = WhepNdiClient::connect(
    "http://localhost:8889/stream/whep",  // WHEP URL
    &ice_servers,                          // ICE servers (use google_stun_servers())
    48000,                                 // Audio sample rate
    2,                                     // Audio channels
    48000 * 2,                             // Buffer size (samples)
    Some("My NDI Source"),                 // NDI name (None = no NDI)
    false,                                 // audio_to_ndi
).await?;

// Check status
client.is_connected();
client.has_ndi();
client.get_ndi_connections();

// Get stats
let stats = &client.stats;
stats.video_frames_decoded.load(Ordering::Relaxed);
stats.video_frames_sent_ndi.load(Ordering::Relaxed);
stats.audio_packets_received.load(Ordering::Relaxed);

// Take audio consumer for BASS
let consumer = client.take_incoming_consumer();

// Disconnect
client.disconnect().await?;
```

### Statistics Available

```rust
pub struct WhepNdiClientStats {
    // Audio
    pub audio_packets_received: AtomicU64,
    pub audio_bytes_received: AtomicU64,
    pub audio_decode_errors: AtomicU64,
    pub audio_frames_sent_ndi: AtomicU64,

    // Video
    pub video_packets_received: AtomicU64,
    pub video_bytes_received: AtomicU64,
    pub video_frames_decoded: AtomicU64,
    pub video_frames_sent_ndi: AtomicU64,
    pub video_decode_errors: AtomicU64,
}
```

## Build & Run

### Build
```bash
cargo build --release --manifest-path BassAES67/bass-webrtc-ndi/Cargo.toml
```

### Run Example
```bash
# Basic usage
cargo run --release --example webrtc_ndi_receiver -- http://localhost:8889/nditest/whep "OBS via WebRTC"

# With audio to NDI
cargo run --release --example webrtc_ndi_receiver -- http://localhost:8889/nditest/whep "OBS via WebRTC" --audio-to-ndi
```

### Runtime DLLs Required
Copy to executable directory or add to PATH:
- `Processing.NDI.Lib.x64.dll` (NDI SDK)
- `avcodec-62.dll` (FFmpeg)
- `avutil-60.dll` (FFmpeg)
- `swscale-9.dll` (FFmpeg)
- `bass.dll` (BASS audio - if using audio features)
- `opus.dll` (OPUS codec)

## Test Setup

### MediaMTX Configuration
```yaml
paths:
  nditest:
    source: publisher
```

### Browser Test Client
Open `examples/test_client_video.html` in Chrome/Firefox:
1. Allow camera/microphone access
2. Select codec (H.264 recommended)
3. Check/uncheck "Send Audio" as needed
4. Click "Start Streaming"

## Architecture

```
Browser/OBS
    ↓ WHIP (WebRTC)
MediaMTX (SFU)
    ↓ WHEP (WebRTC)
WhepNdiClient
    ├─→ Audio Track (Opus)
    │   ↓ OpusDecoder
    │   ↓ RingBuffer
    │   ↓ BASS Channel (user handles playback)
    │   └─→ [Optional] NdiSender (if audio_to_ndi=true)
    │
    └─→ Video Track (H.264/VP8/VP9)
        ↓ H264Depacketizer (RTP → Annex B)
        ↓ FFmpeg VideoDecoder
        ↓ swscale (YUV → BGRA)
        ↓ VideoFrame
        ↓ NdiSender
        ↓ NDI Network
        ↓ NDI Studio Monitor / other receivers
```

## Next Steps (Future Sessions)

1. **FFI API Extensions** - C# bindings for WhepNdiClient
2. **VP8/VP9 Depacketizers** - Proper RTP depacketization for non-H.264 codecs
3. **Suppress FFmpeg Logging** - Set log level to reduce stderr noise
4. **Test WHIP/WHEP Servers** - The bass-webrtc WHIP/WHEP servers haven't been tested
5. **Audio Playback Integration** - Connect BASS ring buffer to actual audio output

## Session Summary

### Completed
- Video track detection with webrtc-rs workaround
- H.264 RTP depacketization (RFC 6184)
- FFmpeg video decoding pipeline
- NDI video output at 30fps
- Audio receiving at 50 packets/s
- Removed verbose debug messages
- Created browser test client

### Not Completed
- VP8/VP9 depacketization
- FFI API for C#
- WHIP/WHEP server testing
