# bass-webrtc Development Progress

## Project Overview

**bass-webrtc** is a WebRTC audio streaming plugin for BASS, enabling peer-to-peer audio communication between your application and web browsers. It follows the same architectural patterns as the existing bass-aes67, bass-srt, and bass-rtp projects.

### Key Features
- **Peer-to-peer WebRTC** with up to 5 simultaneous browser connections
- **Bidirectional audio**: BASS channel -> browsers AND browsers -> BASS channel
- **OPUS codec** at 48kHz stereo (mandatory for WebRTC)
- **Multiple signaling modes**: Callback-based AND WHIP/WHEP HTTP signaling
- **STUN + TURN** support for NAT traversal
- **Pure Rust** WebRTC implementation (webrtc-rs crate)

---

## Current Status: INITIAL IMPLEMENTATION COMPLETE

The project structure is complete and **compiles successfully**. All core modules have been implemented but require testing with actual browser clients.

### Build Status
```
cargo build  ✅ SUCCESS (with warnings for unused imports/variables)
```

---

## File Structure

```
bass-webrtc/
├── Cargo.toml              ✅ Dependencies configured
├── build.rs                ✅ Library paths for BASS and OPUS
├── docs/
│   └── DEVELOPMENT_STEPS_1.md  (this file)
├── src/
│   ├── lib.rs              ✅ FFI exports, WebRtcServer, DllMain
│   ├── ffi/
│   │   ├── mod.rs          ✅ Module exports
│   │   └── bass.rs         ✅ BASS types and bindings
│   ├── codec/
│   │   ├── mod.rs          ✅ AudioFormat, CodecError
│   │   └── opus.rs         ✅ OPUS encoder/decoder wrappers
│   ├── peer/
│   │   ├── mod.rs          ✅ Module exports
│   │   ├── connection.rs   ✅ WebRtcPeer with RTCPeerConnection
│   │   └── manager.rs      ✅ PeerManager (5 peer slots)
│   ├── stream/
│   │   ├── mod.rs          ✅ Module exports
│   │   ├── output.rs       ✅ BASS -> WebRTC (TX thread)
│   │   └── input.rs        ✅ WebRTC -> BASS (STREAMPROC)
│   ├── signaling/
│   │   ├── mod.rs          ✅ Module exports
│   │   ├── callback.rs     ✅ FFI callbacks for SDP/ICE
│   │   ├── whip.rs         ✅ WHIP HTTP signaling (RFC 9725)
│   │   └── whep.rs         ✅ WHEP HTTP signaling
│   └── ice/
│       └── mod.rs          ✅ STUN/TURN helpers
```

---

## Dependencies (Cargo.toml)

```toml
webrtc = "0.11"           # Pure Rust WebRTC
tokio = "1"               # Async runtime (required by webrtc-rs)
ringbuf = "0.4"           # Lock-free ring buffer
hyper = "1"               # HTTP server for WHIP/WHEP
hyper-util = "0.1"
http-body-util = "0.1"
bytes = "1"
parking_lot = "0.12"      # Fast mutex (non-audio path only)
lazy_static = "1.4"
windows-sys = "0.59"      # Windows thread priority
```

---

## Architecture

### Audio Flow: BASS -> WebRTC (Output)

```
BASS Source Channel
       │
       ▼
┌──────────────────┐
│  TX Thread       │  (high priority, spin-loop timing)
│  - BASS_ChannelGetData()
│  - OPUS encode (20ms frames)
│  - TrackLocalStaticSample::write_sample()
└──────────────────┘
       │
       ▼
┌──────────────────┐
│  Shared Track    │  (broadcasts to ALL connected peers)
└──────────────────┘
       │
       ├──► Peer 0 (Browser)
       ├──► Peer 1 (Browser)
       ├──► Peer 2 (Browser)
       ├──► Peer 3 (Browser)
       └──► Peer 4 (Browser)
```

### Audio Flow: WebRTC -> BASS (Input)

```
Peer 0 ──► on_track ──► OPUS decode ──► Ring Buffer 0 ─┐
Peer 1 ──► on_track ──► OPUS decode ──► Ring Buffer 1 ─┤
Peer 2 ──► on_track ──► OPUS decode ──► Ring Buffer 2 ─┼──► STREAMPROC (mix) ──► BASS
Peer 3 ──► on_track ──► OPUS decode ──► Ring Buffer 3 ─┤
Peer 4 ──► on_track ──► OPUS decode ──► Ring Buffer 4 ─┘
```

### Signaling Options

1. **Callback Mode** (`BASS_WEBRTC_SIGNALING_CALLBACK`)
   - User provides FFI callbacks for SDP/ICE exchange
   - Most flexible - integrate with any signaling server

2. **WHIP Mode** (`BASS_WEBRTC_SIGNALING_WHIP`)
   - Built-in HTTP server
   - POST /whip → SDP offer/answer
   - PATCH /whip/{id} → trickle ICE
   - DELETE /whip/{id} → close

3. **WHEP Mode** (`BASS_WEBRTC_SIGNALING_WHEP`)
   - Same as WHIP but for egress (browser pulls)

---

## FFI API Summary

### Configuration
```c
typedef struct {
    DWORD sample_rate;      // 48000 recommended
    WORD channels;          // 1 or 2
    DWORD opus_bitrate;     // kbps (default 128)
    DWORD buffer_ms;        // incoming buffer (default 100)
    BYTE max_peers;         // 1-5
    BYTE signaling_mode;    // 0=callback, 1=WHIP, 2=WHEP
    WORD http_port;         // for WHIP/WHEP
    BYTE decode_stream;     // BASS_STREAM_DECODE flag
} WebRtcConfigFFI;
```

### Core Functions
```c
// Create server with BASS source channel
void* BASS_WEBRTC_Create(DWORD source_channel, WebRtcConfigFFI* config);

// Add ICE servers (STUN/TURN)
int BASS_WEBRTC_AddIceServer(void* handle, char* url, char* user, char* pass);

// Set signaling callbacks (for callback mode)
int BASS_WEBRTC_SetCallbacks(void* handle, SignalingCallbacks* callbacks);

// Start/Stop
int BASS_WEBRTC_Start(void* handle);
int BASS_WEBRTC_Stop(void* handle);

// Get input stream (audio received from browsers)
HSTREAM BASS_WEBRTC_GetInputStream(void* handle);

// Peer management (callback mode)
int BASS_WEBRTC_AddPeer(void* handle, char* offer, char* answer, DWORD* len);
int BASS_WEBRTC_AddIceCandidate(void* handle, DWORD peer_id, char* candidate);
int BASS_WEBRTC_RemovePeer(void* handle, DWORD peer_id);

// Monitoring
int BASS_WEBRTC_GetStats(void* handle, WebRtcStatsFFI* stats);
DWORD BASS_WEBRTC_GetPeerCount(void* handle);
int BASS_WEBRTC_IsRunning(void* handle);

// Cleanup
int BASS_WEBRTC_Free(void* handle);
```

### Signaling Callbacks (for callback mode)
```c
typedef struct {
    void (*on_sdp)(DWORD peer_id, char* type, char* sdp, void* user);
    void (*on_ice_candidate)(DWORD peer_id, char* candidate, char* mid, DWORD idx, void* user);
    void (*on_peer_state)(DWORD peer_id, DWORD state, void* user);
    void* user_data;
} SignalingCallbacks;
```

---

## Design Principles (from CLAUDE.md)

The implementation follows these established patterns:

1. **NO MUTEX in audio path** - Only atomics and lock-free ring buffers
2. **High-priority TX thread** with spin-loop for precise timing
3. **Lock-free ring buffers** for audio transfer between threads
4. **Graceful underrun handling** - Fill with silence, track statistics
5. **Atomic statistics** - No locks for stat counters

---

## Known Limitations / TODO

### Not Yet Implemented
1. **on_track handler for incoming audio** - The WebRtcPeer creates ring buffers but doesn't yet wire up the on_track callback to decode incoming RTP and push to buffers
2. **ICE candidate forwarding** - The callback signaling sends candidates but the actual forwarding loop isn't started
3. **Dynamic ICE server addition** - Currently ICE servers must be configured at creation time

### Warnings to Address
- Unused imports (will be used when on_track is wired up)
- Unused variables in add_ice_server (placeholder)
- Unused fields in WebRtcInputStream (for future resampling)

---

## Next Development Steps

### Phase 1: Wire Up Incoming Audio
1. In `peer/connection.rs`: Add on_track handler that:
   - Creates OPUS decoder
   - Reads from TrackRemote
   - Decodes and pushes to ring buffer
2. Connect peer ring buffers to input stream in `lib.rs`

### Phase 2: Create Test Example
1. Create `examples/webrtc_test.rs`
2. Simple test that:
   - Creates a mixer/channel
   - Starts WebRTC server with WHIP
   - Logs when peers connect/disconnect

### Phase 3: Browser Test Client
1. Create simple HTML/JS client for testing
2. Test SDP exchange via WHIP
3. Verify audio flows both directions

### Phase 4: Polish
1. Clean up unused imports/warnings
2. Add proper error messages
3. Test NAT traversal with TURN
4. Performance tuning

---

## Key Files Reference

| For Reference | Look At |
|---------------|---------|
| BASS types | `src/ffi/bass.rs` |
| OPUS codec | `src/codec/opus.rs` |
| Peer connection | `src/peer/connection.rs` |
| Multi-peer management | `src/peer/manager.rs` |
| TX thread pattern | `src/stream/output.rs` |
| STREAMPROC pattern | `src/stream/input.rs` |
| WHIP signaling | `src/signaling/whip.rs` |
| FFI exports | `src/lib.rs` |

---

## How to Continue Development

1. Open the project in your IDE
2. Run `cargo build` to verify compilation
3. Focus on wiring up the on_track handler (most important missing piece)
4. Create a test example to verify audio flow

---

## Session Notes

- **Date**: December 2024
- **webrtc-rs version**: 0.11
- **Build target**: Windows x64 (MSVC)
- **BASS/OPUS libs**: Using existing paths from bass-rtp project
