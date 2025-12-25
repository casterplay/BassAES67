# bass-webrtc Development Session 2 - December 25, 2024

## Session Summary

This session focused on fixing build issues and getting the test example running with MediaMTX.

### Status at End of Session: WORKING

WebRTC audio streaming via MediaMTX is now functional.

---

## Issues Fixed This Session

### Issue 1: Example Linking Error (LNK1181)

**Problem:**
```
error: linking with `link.exe` failed: exit code: 1181
LINK : fatal error LNK1181: cannot open input file 'bass_webrtc.lib'
```

The example was using `#[link(name = "bass_webrtc")]` FFI which tries to link against a .lib file at compile time. Since bass-webrtc is built as a `cdylib` (DLL), there's no static .lib for examples to link against in the same build.

**Solution:**
Changed the example from FFI linking to direct Rust imports:

```rust
// OLD (broken):
#[link(name = "bass_webrtc")]
extern "system" {
    fn BASS_WEBRTC_ConnectWhip(...) -> *mut c_void;
}

// NEW (working):
use bass_webrtc::{
    BASS_WEBRTC_ConnectWhip, BASS_WEBRTC_ConnectWhep,
    BASS_WEBRTC_WhipStart, BASS_WEBRTC_WhipStop, BASS_WEBRTC_WhipFree,
    BASS_WEBRTC_WhepGetStream, BASS_WEBRTC_WhepFree,
};
```

**File Modified:** `examples/webrtc_mediamtx_test.rs`

---

### Issue 2: DLL Not Found (0xc0000135)

**Problem:**
```
error: process didn't exit successfully: exit code: 0xc0000135, STATUS_DLL_NOT_FOUND
```

The example executable requires runtime DLLs that weren't in the PATH.

**Solution:**
Copy the required DLLs to the examples output folder:
```
bass-webrtc/target/release/examples/
├── bass.dll          (from bass24/c/x64/)
├── opus.dll          (from Windows_need_builds/opus-1.6/build/Release/)
└── webrtc_mediamtx_test.exe
```

**DLL Locations:**
- `bass.dll`: `c:\Dev\CasterPlay2025\BassAES67\BassAES67\bass24\c\x64\bass.dll`
- `opus.dll`: `c:\Dev\CasterPlay2025\BassAES67\BassAES67\Windows_need_builds\opus-1.6\build\Release\opus.dll`

---

## Current Build Commands

### Build Library
```cmd
cd c:\Dev\CasterPlay2025\BassAES67\BassAES67\bass-webrtc
cargo build --release
```

### Build and Run Example
```cmd
cd c:\Dev\CasterPlay2025\BassAES67\BassAES67\bass-webrtc
cargo build --release --example webrtc_mediamtx_test

# Copy DLLs (if not already done)
copy ..\bass24\c\x64\bass.dll target\release\examples\
copy ..\Windows_need_builds\opus-1.6\build\Release\opus.dll target\release\examples\

# Run with WHIP (send audio to MediaMTX)
target\release\examples\webrtc_mediamtx_test.exe --whip http://localhost:8889/mystream/whip

# Run with WHEP (receive audio from MediaMTX)
target\release\examples\webrtc_mediamtx_test.exe --whep http://localhost:8889/mystream/whep

# Run both directions
target\release\examples\webrtc_mediamtx_test.exe --whip http://localhost:8889/out/whip --whep http://localhost:8889/in/whep
```

---

## Files Modified This Session

| File | Change |
|------|--------|
| `examples/webrtc_mediamtx_test.rs` | Changed from FFI `#[link]` to direct Rust `use bass_webrtc::*` imports |

---

## Build Warnings (Acceptable)

The build completes with ~22 warnings about unused imports and fields. These are expected during development and can be cleaned up in a future session:

- Unused imports in lib.rs, connection.rs, manager.rs, callback.rs, whip.rs
- Unused fields in WhipClient, WhepClient, WhepClientWrapper, WebRtcInputStream, WebRtcPeer
- Unused constant `DEFAULT_BUFFER_MS`
- Unused method `add_ice_server`

---

## Test Setup with MediaMTX

### Prerequisites
1. Download MediaMTX from: https://github.com/bluenviron/mediamtx/releases
2. Extract and run `mediamtx.exe` (uses default config, listens on port 8889 for WHIP/WHEP)

### Test Flow: BASS -> Browser

1. Start MediaMTX
2. Run: `webrtc_mediamtx_test.exe --whip http://localhost:8889/mystream/whip`
3. Open `examples/test_client.html` in browser
4. Set stream name to "mystream"
5. Click "Connect & Play" (WHEP)
6. Should hear 440Hz test tone

### Test Flow: Browser -> BASS

1. Start MediaMTX
2. Run: `webrtc_mediamtx_test.exe --whep http://localhost:8889/mystream/whep`
3. Open `examples/test_client.html` in browser
4. Set stream name to "mystream"
5. Click "Connect & Send" (WHIP)
6. BASS should play microphone audio

---

## Architecture Recap

### Signaling Flow with MediaMTX

```
┌─────────────────┐          ┌─────────────────┐          ┌─────────────────┐
│  bass-webrtc    │          │    MediaMTX     │          │    Browser      │
│  (Rust/BASS)    │          │   (Go server)   │          │  (JavaScript)   │
├─────────────────┤          ├─────────────────┤          ├─────────────────┤
│                 │  WHIP    │                 │  WHEP    │                 │
│  WhipClient ────┼─────────►│  /stream/whip   │◄─────────┼── test_client   │
│  (sends audio)  │  POST    │  (SFU relay)    │  POST    │  (receives)     │
│                 │  SDP     │                 │  SDP     │                 │
│                 │          │                 │          │                 │
│                 │  WHEP    │                 │  WHIP    │                 │
│  WhepClient ◄───┼──────────┤  /stream/whep   │◄─────────┼── test_client   │
│  (recv audio)   │  POST    │  (SFU relay)    │  POST    │  (sends mic)    │
└─────────────────┘          └─────────────────┘          └─────────────────┘
```

### Key Components

- **WhipClient** (`src/signaling/whip_client.rs`): Pushes audio TO MediaMTX
- **WhepClient** (`src/signaling/whep_client.rs`): Pulls audio FROM MediaMTX
- **WebRtcOutputStream** (`src/stream/output.rs`): High-priority TX thread, OPUS encode
- **WebRtcInputStream** (`src/stream/input.rs`): STREAMPROC for BASS, mixes peer audio
- **on_track handler** (`src/peer/connection.rs`): Receives incoming RTP, decodes OPUS

---

## Questions to Investigate Next Session

1. **Audio quality**: How does the received audio sound? Any dropouts?
2. **Latency**: What's the end-to-end latency? Can it be optimized?
3. **Browser compatibility**: Does test_client.html work in Firefox/Safari?
4. **TURN support**: Does it work behind NAT without TURN servers?
5. **Multiple streams**: Can we run multiple stream names simultaneously?

---

## Potential Next Steps

### Phase 2: Testing & Polish
- [ ] Verify bidirectional audio quality
- [ ] Measure latency (add timestamps?)
- [ ] Clean up unused code warnings
- [ ] Add proper error handling/messages
- [ ] Test with different browsers

### Phase 3: NAT Traversal
- [ ] Test TURN server support
- [ ] Handle ICE restart scenarios
- [ ] Add configurable ICE servers via FFI

### Phase 4: Performance
- [ ] Profile CPU usage during streaming
- [ ] Optimize buffer sizes
- [ ] Test with multiple simultaneous peers
- [ ] Add jitter buffer tuning

### Phase 5: Integration
- [ ] C# bindings for CasterPlay
- [ ] Integration with existing mixer
- [ ] UI for stream management

---

## Dependencies (Cargo.toml)

```toml
[dependencies]
webrtc = "0.11"                    # Pure Rust WebRTC
tokio = { version = "1", features = ["rt-multi-thread", "sync", "time", "macros"] }
ringbuf = "0.4"                    # Lock-free ring buffer
hyper = { version = "1", features = ["server", "http1", "client"] }
hyper-util = { version = "0.1", features = ["tokio", "client", "client-legacy"] }
http-body-util = "0.1"
hyper-rustls = { version = "0.27", default-features = false, features = ["http1", "ring", "webpki-roots"] }
url = "2"
bytes = "1"
parking_lot = "0.12"
lazy_static = "1.4"
windows-sys = { version = "0.59", features = ["Win32_System_Threading"] }

[dev-dependencies]
ctrlc = "3.4"
```

Note: `hyper-rustls` uses `webpki-roots` instead of `native-roots` to avoid aws-lc-sys CMake build issues on Windows.

---

## Session Notes

- **Date**: December 25, 2024
- **Duration**: Extended session (productive day!)
- **Platform**: Windows x64 (MSVC)
- **webrtc-rs version**: 0.11
- **MediaMTX**: Used for WHIP/WHEP signaling relay

Great progress today - WebRTC is now working end-to-end with MediaMTX!
