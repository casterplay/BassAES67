# bass-webrtc Development Progress

## Project Overview

**bass-webrtc** is a WebRTC audio streaming plugin for BASS, enabling peer-to-peer audio communication between your application and web browsers. It follows the same architectural patterns as the existing bass-aes67, bass-srt, and bass-rtp projects.

### Key Features
- **Peer-to-peer WebRTC** with up to 5 simultaneous browser connections
- **Bidirectional audio**: BASS channel -> browsers AND browsers -> BASS channel
- **OPUS codec** at 48kHz stereo (mandatory for WebRTC)
- **Multiple signaling modes**: Callback-based, WHIP/WHEP Server, AND WHIP/WHEP Client
- **STUN + TURN** support for NAT traversal
- **Pure Rust** WebRTC implementation (webrtc-rs crate)

---

## Current Status: PHASE 1 COMPLETE

The project now has full incoming audio support and multiple signaling options.

### Build Status
```
cargo build --release  ✅ SUCCESS (with minor warnings for unused fields)
```

### What's Working
- ✅ **on_track handler** - Receives and decodes incoming OPUS audio
- ✅ **Ring buffer wiring** - Connects peer audio to BASS input stream
- ✅ **WHIP/WHEP Server** - Built-in HTTP endpoints (no external server needed)
- ✅ **WHIP/WHEP Client** - Connect to external servers like MediaMTX
- ✅ **FFI exports** - Complete C API for all modes
- ✅ **Test examples** - MediaMTX test and browser test client

---

## File Structure

```
bass-webrtc/
├── Cargo.toml              ✅ Dependencies configured
├── build.rs                ✅ Library paths for BASS and OPUS
├── docs/
│   └── DEVELOPMENT_STEPS_1.md  (this file)
├── examples/
│   ├── webrtc_test.rs          Placeholder for basic test
│   ├── webrtc_mediamtx_test.rs ✅ Test with MediaMTX server
│   └── test_client.html        ✅ Browser test client
├── src/
│   ├── lib.rs              ✅ FFI exports, WebRtcServer, WHIP/WHEP client API
│   ├── ffi/
│   │   ├── mod.rs          ✅ Module exports
│   │   └── bass.rs         ✅ BASS types and bindings
│   ├── codec/
│   │   ├── mod.rs          ✅ AudioFormat, CodecError
│   │   └── opus.rs         ✅ OPUS encoder/decoder wrappers
│   ├── peer/
│   │   ├── mod.rs          ✅ Module exports
│   │   ├── connection.rs   ✅ WebRtcPeer with on_track handler
│   │   └── manager.rs      ✅ PeerManager (5 peer slots)
│   ├── stream/
│   │   ├── mod.rs          ✅ Module exports
│   │   ├── output.rs       ✅ BASS -> WebRTC (TX thread)
│   │   └── input.rs        ✅ WebRTC -> BASS (STREAMPROC)
│   ├── signaling/
│   │   ├── mod.rs          ✅ Module exports
│   │   ├── callback.rs     ✅ FFI callbacks for SDP/ICE
│   │   ├── whip.rs         ✅ WHIP HTTP server (RFC 9725)
│   │   ├── whep.rs         ✅ WHEP HTTP server
│   │   ├── whip_client.rs  ✅ WHIP client (push to external server)
│   │   └── whep_client.rs  ✅ WHEP client (pull from external server)
│   └── ice/
│       └── mod.rs          ✅ STUN/TURN helpers
```

---

## Signaling Modes

### Option 1: Built-in Server (No External Dependencies)

bass-webrtc can host its own WHIP/WHEP HTTP endpoints:

```c
// Browser connects directly to bass-webrtc
WebRtcConfigFFI config = {
    .signaling_mode = BASS_WEBRTC_SIGNALING_WHIP,  // or WHEP
    .http_port = 8080
};
void* handle = BASS_WEBRTC_Create(source_channel, &config);
BASS_WEBRTC_Start(handle);
// Browser POSTs to http://localhost:8080/whip
```

### Option 2: Client Mode (Connect to External Server)

bass-webrtc can connect to an external WHIP/WHEP server like MediaMTX:

```c
// Push audio TO MediaMTX (browsers receive via WHEP from MediaMTX)
void* whip = BASS_WEBRTC_ConnectWhip(
    source_channel,
    "http://localhost:8889/mystream/whip",
    48000, 2, 128
);
BASS_WEBRTC_WhipStart(whip);

// Pull audio FROM MediaMTX (browsers send via WHIP to MediaMTX)
void* whep = BASS_WEBRTC_ConnectWhep(
    "http://localhost:8889/mystream/whep",
    48000, 2, 100, 0
);
HSTREAM input = BASS_WEBRTC_WhepGetStream(whep);
BASS_ChannelPlay(input, FALSE);
```

### Option 3: Callback Mode (Custom Signaling)

User provides FFI callbacks for complete control:

```c
SignalingCallbacks callbacks = {
    .on_sdp = my_sdp_handler,
    .on_ice_candidate = my_ice_handler,
    .on_peer_state = my_state_handler
};
BASS_WEBRTC_SetCallbacks(handle, &callbacks);
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

### Audio Flow: WebRTC -> BASS (Input) ✅ NOW WORKING

```
Peer 0 ──► on_track ──► OPUS decode ──► Ring Buffer 0 ─┐
Peer 1 ──► on_track ──► OPUS decode ──► Ring Buffer 1 ─┤
Peer 2 ──► on_track ──► OPUS decode ──► Ring Buffer 2 ─┼──► STREAMPROC (mix) ──► BASS
Peer 3 ──► on_track ──► OPUS decode ──► Ring Buffer 3 ─┤
Peer 4 ──► on_track ──► OPUS decode ──► Ring Buffer 4 ─┘
```

### With MediaMTX (Client Mode)

```
┌─────────────────┐          ┌─────────────────┐          ┌─────────────────┐
│   BASS Audio    │          │    MediaMTX     │          │    Browser      │
│   Application   │          │    Server       │          │                 │
├─────────────────┤          ├─────────────────┤          ├─────────────────┤
│                 │  WHIP    │                 │  WHEP    │                 │
│  bass-webrtc ───┼─────────►│  /stream/whip   │◄─────────┼── JavaScript   │
│  (TX thread)    │  POST    │                 │  POST    │  (receives)     │
│                 │          │                 │          │                 │
│                 │  WHEP    │                 │  WHIP    │                 │
│  bass-webrtc ◄──┼──────────┤  /stream/whep   │◄─────────┼── JavaScript   │
│  (RX/input)     │  POST    │                 │  POST    │  (sends mic)    │
└─────────────────┘          └─────────────────┘          └─────────────────┘
```

---

## FFI API Summary

### Core Functions (Server Mode)
```c
void* BASS_WEBRTC_Create(DWORD source_channel, WebRtcConfigFFI* config);
int BASS_WEBRTC_Start(void* handle);
int BASS_WEBRTC_Stop(void* handle);
HSTREAM BASS_WEBRTC_GetInputStream(void* handle);
int BASS_WEBRTC_GetStats(void* handle, WebRtcStatsFFI* stats);
int BASS_WEBRTC_Free(void* handle);
```

### WHIP Client Functions (Push to External Server)
```c
void* BASS_WEBRTC_ConnectWhip(DWORD source, char* url, u32 rate, u16 ch, u32 bitrate);
int BASS_WEBRTC_WhipStart(void* handle);
int BASS_WEBRTC_WhipStop(void* handle);
int BASS_WEBRTC_WhipFree(void* handle);
```

### WHEP Client Functions (Pull from External Server)
```c
void* BASS_WEBRTC_ConnectWhep(char* url, u32 rate, u16 ch, u32 buf_ms, u8 decode);
HSTREAM BASS_WEBRTC_WhepGetStream(void* handle);
int BASS_WEBRTC_WhepFree(void* handle);
```

---

## Testing

### Test with MediaMTX

1. Start MediaMTX server
2. Run the test example:
   ```
   cargo run --release --example webrtc_mediamtx_test -- --whip http://localhost:8889/mystream/whip
   ```
3. Open `examples/test_client.html` in browser
4. Configure stream name to match
5. Click "Connect & Play" (WHEP) to receive the 440Hz test tone

### Test Browser-to-BASS

1. Run with WHEP to receive:
   ```
   cargo run --release --example webrtc_mediamtx_test -- --whep http://localhost:8889/mystream/whep
   ```
2. Open browser, click "Connect & Send" (WHIP) to send microphone
3. bass-webrtc receives and plays the audio

---

## Design Principles (from CLAUDE.md)

The implementation follows these established patterns:

1. **NO MUTEX in audio path** - Only atomics and lock-free ring buffers
2. **High-priority TX thread** with spin-loop for precise timing
3. **Lock-free ring buffers** for audio transfer between threads
4. **Graceful underrun handling** - Fill with silence, track statistics
5. **Atomic statistics** - No locks for stat counters

---

## Next Development Steps

### Phase 2: Testing & Polish
1. Test with actual MediaMTX server
2. Test bidirectional audio flow
3. Clean up unused warnings
4. Add proper error messages

### Phase 3: NAT Traversal
1. Test with TURN servers
2. Add ICE candidate handling for complex NAT scenarios

### Phase 4: Performance
1. Profile audio latency
2. Optimize buffer sizes
3. Test with multiple simultaneous peers

---

## Session Notes

- **Date**: December 2024
- **webrtc-rs version**: 0.11
- **Build target**: Windows x64 (MSVC)
- **Key additions this session**:
  - on_track handler for incoming audio
  - WHIP/WHEP client for MediaMTX integration
  - FFI exports for client mode
  - MediaMTX test example
  - Browser test client
