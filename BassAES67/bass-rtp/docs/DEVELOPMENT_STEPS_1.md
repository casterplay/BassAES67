# bass-rtp Development Progress - Session 1

## Project Overview

**bass-rtp** is a BASS plugin for bidirectional unicast RTP audio communication with Telos Z/IP ONE broadcast codec.

### Key Differences from bass-aes67
- **Unicast** instead of multicast
- **Bidirectional** RTP on single UDP socket
- **Z/IP ONE reciprocal RTP** support (ports 9150-9153)
- **48kHz only** sample rate
- Reuses clock DLLs from bass-aes67

## Completed Implementation Steps

### Step 1: Project Setup ✅
- Created `Cargo.toml` with dependencies (socket2, ringbuf, parking_lot, lazy_static, windows-sys)
- Copied FFI files from bass-aes67 (`ffi/mod.rs`, `bass.rs`, `addon.rs`)
- Copied `clock_bindings.rs` from bass-aes67
- Created `lib.rs` with DllMain, BASSplugin entry, config handlers
- Created `build.rs` for library linking

### Step 2: RTP Module ✅
- `rtp/header.rs` - RTP header parsing/building (RFC 3550)
- `rtp/payload.rs` - Payload type registry mapping PT to codecs
- `rtp/socket.rs` - Bidirectional UDP socket wrapper with `bind()`, `send_to()`, `try_clone()`

### Step 3: PCM Codecs ✅
- `codec/pcm.rs` - PCM 16-bit and 24-bit encode/decode
- **Big-endian** (network byte order) for RTP
- Implements `AudioEncoder` and `AudioDecoder` traits

### Step 4: Input Stream ✅
- `stream/input.rs` - RtpInputStream with lock-free architecture
- Receiver thread with codec auto-detection from payload type
- STREAMPROC callback (`input_stream_proc`)
- Adaptive resampling with PI controller for clock sync
- Ring buffer using `ringbuf` crate

### Step 5: Output Stream ✅
- `stream/output.rs` - RtpOutputStream
- Transmitter thread with `BASS_ChannelGetData()`
- RTP packet building with sequence/timestamp management
- Clock PPM correction for timing

### Step 6: Bidirectional Integration ✅
- `stream/bidirectional.rs` - BidirectionalStream combining input/output
- Single socket shared between send/receive threads via `try_clone()`
- Complete FFI exports in `lib.rs`:
  - `BASS_RTP_Create()`
  - `BASS_RTP_Start()`
  - `BASS_RTP_Stop()`
  - `BASS_RTP_GetInputStream()`
  - `BASS_RTP_GetStats()`
  - `BASS_RTP_IsRunning()`
  - `BASS_RTP_Free()`

### Step 7: Additional Codecs ✅
Copied from bass-srt and adapted:
- `codec/opus.rs` - OPUS codec (libopus)
- `codec/twolame.rs` - MP2 encoder (libtwolame)
- `codec/mpg123.rs` - MP2 decoder (libmpg123)
- `codec/flac.rs` - FLAC codec (libFLAC)

Libraries linked via `build.rs` from `Windows_need_builds/` folder.

### Step 8: URL Parser & Config ✅
- `url.rs` - Parses `rtp://host:port?options` URLs
- Options: codec, bitrate, jitter, channels, local_port, interface

## Project Structure

```
bass-rtp/
├── Cargo.toml
├── build.rs
├── src/
│   ├── lib.rs                  # Plugin entry, FFI exports
│   ├── ffi/
│   │   ├── mod.rs
│   │   ├── bass.rs             # BASS types and functions
│   │   └── addon.rs            # BASS addon interface
│   ├── rtp/
│   │   ├── mod.rs
│   │   ├── header.rs           # RTP header parse/build
│   │   ├── payload.rs          # Payload type registry
│   │   └── socket.rs           # Bidirectional UDP socket
│   ├── codec/
│   │   ├── mod.rs              # Traits, AudioFormat, CodecError
│   │   ├── pcm.rs              # PCM 16/24-bit (BE)
│   │   ├── opus.rs             # OPUS codec
│   │   ├── twolame.rs          # MP2 encoder
│   │   ├── mpg123.rs           # MP2 decoder
│   │   └── flac.rs             # FLAC codec
│   ├── stream/
│   │   ├── mod.rs
│   │   ├── input.rs            # RX: network → BASS
│   │   ├── output.rs           # TX: BASS → network
│   │   └── bidirectional.rs    # Combined stream
│   ├── clock_bindings.rs       # Clock DLL loading
│   └── url.rs                  # rtp:// URL parser
└── docs/
    ├── DEVELOPMENT_PLAN.md
    └── DEVELOPMENT_STEPS_1.md  # This file
```

## Build Output

- `target/release/bass_rtp.dll` (240 KB)
- All tests pass (RTP, PCM codec tests)

## Z/IP ONE Reciprocal RTP Ports

| Port | Behavior |
|------|----------|
| 9150 | Receive only (no reply) |
| 9151 | Reply with G.722 |
| 9152 | Reply with same codec as received |
| 9153 | Reply with current codec setting |

## Payload Types (Telos Z/IP ONE)

| Codec | PT |
|-------|-----|
| G.711 u-Law | 0 |
| G.722 | 9 |
| MP2 | 14 or 96 |
| PCM-16 | 21 |
| PCM-24 | 22 |
| PCM-20 | 116 |

## Public FFI API

```c
// Create bidirectional RTP stream
void* BASS_RTP_Create(DWORD bass_channel, RtpStreamConfigFFI* config);

// Start/stop
int BASS_RTP_Start(void* handle);
int BASS_RTP_Stop(void* handle);

// Get input stream handle (for playing received audio)
HSTREAM BASS_RTP_GetInputStream(void* handle);

// Statistics
int BASS_RTP_GetStats(void* handle, RtpStatsFFI* stats);
int BASS_RTP_IsRunning(void* handle);

// Cleanup
int BASS_RTP_Free(void* handle);
```

## Configuration Structure

```c
struct RtpStreamConfigFFI {
    uint16_t local_port;        // Port to bind
    uint8_t  remote_addr[4];    // Z/IP ONE IP
    uint16_t remote_port;       // 9150, 9151, 9152, or 9153
    uint32_t sample_rate;       // 48000
    uint16_t channels;          // 1 or 2
    uint8_t  output_codec;      // 0=PCM16, 1=PCM24, 2=MP2, 3=OPUS, 4=FLAC
    uint32_t output_bitrate;    // For MP2/OPUS (kbps)
    uint32_t jitter_ms;         // Jitter buffer depth
    uint8_t  interface_addr[4]; // Network interface (0.0.0.0 = default)
};
```

## Dependencies (Windows)

Located in `../Windows_need_builds/`:
- `opus-1.6/build/Release/opus.dll`
- `twolame-main/libtwolame_dll.dll`
- `mpg123-1.32.10/mpg123-1.32.10-x86-64/libmpg123-0.dll`
- `flac-master/build/src/libFLAC/Release/FLAC.dll`
- `../bass24/c/x64/bass.dll`

## Known Issues / Notes

1. **PCM Byte Order**: Z/IP ONE may use BE or LE for PCM - needs testing
2. **Multi-instance**: Each RtpSocket is independent - no shared global state
3. **Clock DLLs**: Optional - falls back to system clock if not available

## Next Steps (Future Sessions)

1. **Testing with Z/IP ONE hardware** - Verify codec compatibility
2. **C# wrapper** - Create managed wrapper for .NET integration
3. **Error handling** - Improve error messages and recovery
4. **Documentation** - API reference and usage examples
5. **Performance tuning** - Profile and optimize hot paths

## How to Build

```bash
cd C:\Dev\CasterPlay2025\BassAES67\BassAES67\bass-rtp
cargo build --release
```

Output: `target\release\bass_rtp.dll`

## How to Test

Tests require codec DLLs in PATH. Basic build verification:
```bash
cargo build
```

## Key Design Decisions

1. **Single socket for bidirectional**: Use `try_clone()` to share between send/receive threads
2. **Codec auto-detection on receive**: Detect from incoming RTP payload type
3. **No mutex in audio path**: Lock-free ring buffer, atomics for stats
4. **Clock reuse**: Same PTP/Livewire/System clock DLLs as bass-aes67
5. **48kHz only**: Simplifies implementation, matches broadcast standard
