# bass-rtp Development Plan

## Overview

Create a BASS plugin for bidirectional unicast RTP audio with Telos Z/IP ONE broadcast codec.

**Location:** `C:\Dev\CasterPlay2025\BassAES67\BassAES67\bass-rtp`

## Requirements Summary

- **Bidirectional RTP** on single UDP socket (unicast, not multicast)
- **Z/IP ONE Reciprocal RTP**: Send to port 9151/9152/9153 to get automatic reply
  - Base + 1 (9151): Reply with G.722
  - Base + 2 (9152): Reply with same codec as received
  - Base + 3 (9153): Reply with current codec setting
- **48kHz sample rate only**
- **Reuse clock DLLs** from bass-aes67 (PTP/Livewire/System)
- **Configurable target port** (let user choose reciprocal port behavior)

## Codecs to Support

| Codec | Payload Type | Priority |
|-------|--------------|----------|
| PCM-16 | PT 21 | High |
| PCM-24 | PT 22 | High |
| MP2 | PT 14 or 96 | High |
| OPUS | Dynamic | Medium (reuse from bass-srt) |
| FLAC | Dynamic | Medium (reuse from bass-srt) |

## Project Structure

```
bass-rtp/
├── Cargo.toml
├── src/
│   ├── lib.rs                  # Plugin entry, FFI exports, DllMain
│   ├── ffi/
│   │   ├── mod.rs              # (copy from bass-aes67)
│   │   ├── bass.rs             # (copy from bass-aes67)
│   │   └── addon.rs            # (copy from bass-aes67)
│   ├── rtp/
│   │   ├── mod.rs
│   │   ├── header.rs           # RTP header parse/build
│   │   ├── payload.rs          # Payload type registry
│   │   └── socket.rs           # Bidirectional UDP socket
│   ├── codec/
│   │   ├── mod.rs              # Traits, AudioFormat, CodecError
│   │   ├── pcm.rs              # PCM 16/24-bit (new)
│   │   ├── opus.rs             # (copy from bass-srt)
│   │   ├── twolame.rs          # MP2 encoder (copy from bass-srt)
│   │   ├── mpg123.rs           # MP2 decoder (copy from bass-srt)
│   │   └── flac.rs             # (copy from bass-srt)
│   ├── stream/
│   │   ├── mod.rs
│   │   ├── bidirectional.rs    # Main stream (new - core logic)
│   │   ├── input.rs            # Input side (decode -> ring buffer -> BASS)
│   │   └── output.rs           # Output side (BASS -> encode -> send)
│   ├── url.rs                  # URL parser for rtp://
│   └── clock_bindings.rs       # (copy from bass-aes67)
├── docs/
│   └── DEVELOPMENT_PLAN.md     # This file
```

## Critical Files to Reference

| Source File | Purpose |
|-------------|---------|
| `bass-aes67/src/lib.rs` | Plugin entry, FFI patterns, DllMain |
| `bass-aes67/src/input/stream.rs` | Lock-free ring buffer, adaptive resampling |
| `bass-aes67/src/output/stream.rs` | Transmitter loop, clock correction |
| `bass-aes67/src/clock_bindings.rs` | Clock DLL loading (copy entirely) |
| `bass-srt/src/codec/*.rs` | Codec implementations (copy and adapt) |
| `bass-srt/src/input/stream.rs` | Multi-codec decoder switching pattern |

## Implementation Steps

### Step 1: Project Setup
- Create Cargo.toml with dependencies (socket2, ringbuf, windows-sys, libc)
- Copy FFI files from bass-aes67 (ffi/mod.rs, bass.rs, addon.rs)
- Copy clock_bindings.rs from bass-aes67
- Create basic lib.rs with DllMain structure

### Step 2: RTP Module
- Implement RTP header parsing/building in rtp/header.rs
- Create payload type registry in rtp/payload.rs (map PT to codec)
- Implement bidirectional UDP socket wrapper in rtp/socket.rs

### Step 3: PCM Codecs
- Implement PCM-16 codec (16-bit big-endian network order)
- Implement PCM-24 codec (adapt from bass-aes67's 24-bit converters)

### Step 4: Input Stream
- Lock-free ring buffer (ringbuf crate)
- Receiver thread with codec auto-detection from incoming PT
- STREAMPROC callback with adaptive resampling
- Decoder switching when PT changes

### Step 5: Output Stream
- Transmitter thread with high priority
- Pull samples from BASS via BASS_ChannelGetData()
- Encode with selected codec
- Clock PPM correction for timing

### Step 6: Bidirectional Integration
- Single socket shared between input/output
- Combined stream struct managing both directions
- FFI exports for C/C# consumers

### Step 7: Additional Codecs
- Copy and adapt opus.rs from bass-srt
- Copy and adapt twolame.rs, mpg123.rs from bass-srt for MP2
- Copy and adapt flac.rs from bass-srt

### Step 8: URL Parser & Config
- Parse rtp:// URLs with parameters
- BASS_SetConfig/GetConfig handlers
- Statistics and monitoring

## Public API

```c
// Create bidirectional RTP stream
void* BASS_RTP_Create(DWORD bass_channel, RtpStreamConfig* config);

// Start/stop
int BASS_RTP_Start(void* handle);
int BASS_RTP_Stop(void* handle);

// Get input stream handle (for playing received audio)
HSTREAM BASS_RTP_GetInputStream(void* handle);

// Statistics
int BASS_RTP_GetStats(void* handle, RtpStats* stats);

// Cleanup
int BASS_RTP_Free(void* handle);
```

## Configuration Structure

```c
struct RtpStreamConfig {
    uint16_t local_port;        // Port to bind
    uint8_t  remote_addr[4];    // Z/IP ONE IP
    uint16_t remote_port;       // 9150, 9151, 9152, or 9153
    uint32_t sample_rate;       // 48000
    uint16_t channels;          // 1 or 2
    uint8_t  output_codec;      // PCM16, PCM24, MP2, OPUS, FLAC
    uint32_t output_bitrate;    // For MP2/OPUS (kbps)
    uint32_t jitter_ms;         // Jitter buffer depth
    uint8_t  interface_addr[4]; // Network interface (0.0.0.0 = default)
};
```

## Key Design Decisions

1. **Single socket for bidirectional**: Use `try_clone()` to share between send/receive threads
2. **Codec auto-detection on receive**: Detect from incoming RTP payload type
3. **Configurable target port**: User chooses 9151/9152/9153 behavior
4. **No mutex in audio path**: Lock-free ring buffer, atomics for stats
5. **Clock reuse**: Same PTP/Livewire/System clock DLLs as bass-aes67

## Architecture Diagrams

### Data Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│                          bass_rtp.dll                                │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌─────────────────────┐              ┌─────────────────────┐       │
│  │   INPUT (Receive)   │              │   OUTPUT (Transmit) │       │
│  │   from Z/IP ONE     │              │   to Z/IP ONE       │       │
│  └──────────┬──────────┘              └──────────┬──────────┘       │
│             │                                    │                   │
│             ▼                                    ▼                   │
│  ┌─────────────────────┐              ┌─────────────────────┐       │
│  │  Receiver Thread    │              │  Transmitter Thread │       │
│  │  (UDP recv)         │              │  (UDP send)         │       │
│  └──────────┬──────────┘              └──────────┬──────────┘       │
│             │                                    │                   │
│             ▼                                    ▼                   │
│  ┌─────────────────────┐              ┌─────────────────────┐       │
│  │  Decoder            │              │  Encoder            │       │
│  │  (PCM/MP2/OPUS/FLAC)│              │  (PCM/MP2/OPUS/FLAC)│       │
│  └──────────┬──────────┘              └──────────┬──────────┘       │
│             │                                    │                   │
│             ▼                                    ▼                   │
│  ┌─────────────────────┐              ┌─────────────────────┐       │
│  │  Lock-Free Ring     │              │  BASS_ChannelGetData│       │
│  │  Buffer (ringbuf)   │              │  (pull from BASS)   │       │
│  └──────────┬──────────┘              └─────────────────────┘       │
│             │                                                        │
│             ▼                                                        │
│  ┌─────────────────────┐                                            │
│  │  BASS STREAMPROC    │◄──── BASS calls this to get samples        │
│  │  Callback           │                                            │
│  └─────────────────────┘                                            │
│                                                                      │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │                 Shared Bidirectional Socket                   │   │
│  │                 (single UDP socket for send/recv)             │   │
│  └──────────────────────────────────────────────────────────────┘   │
│                                                                      │
├─────────────────────────────────────────────────────────────────────┤
│                      Clock Synchronization                           │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐              │
│  │ bass_ptp.dll │  │ bass_lw.dll  │  │ bass_sys.dll │              │
│  │ (PTP clock)  │  │ (Livewire)   │  │ (fallback)   │              │
│  └──────────────┘  └──────────────┘  └──────────────┘              │
└─────────────────────────────────────────────────────────────────────┘
```

### Z/IP ONE Connection

```
┌──────────────┐                              ┌──────────────┐
│  bass-rtp    │                              │  Z/IP ONE    │
│  Plugin      │                              │  Codec       │
├──────────────┤                              ├──────────────┤
│              │   RTP (our audio) ────────►  │ Port 9152    │
│ Local Port   │                              │ (reciprocal) │
│ (e.g. 5004)  │  ◄──────── RTP (reply audio) │              │
│              │                              │              │
└──────────────┘                              └──────────────┘

Port options:
  9150 = Receive only (no reply)
  9151 = Reply with G.722
  9152 = Reply with same codec (recommended)
  9153 = Reply with current codec setting
```
