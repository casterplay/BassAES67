# bass_srt Development Steps - Session 1

## Overview

This document captures the development progress of the `bass_srt` Rust crate, which provides SRT (Secure Reliable Transport) audio streaming for the BASS audio library. The library enables low-latency, reliable audio streaming over the internet with encryption support.

## Project Location

```
/home/kennet/dev/BassAES67/BassAES67/bass-srt/
```

## What Was Accomplished

### Phase 1: Basic SRT Input (Completed Previously)
- SRT input plugin that receives SRT streams and feeds PCM audio into BASS
- Lock-free ring buffer architecture (no mutex in audio path)
- Adaptive resampling with PI controller for buffer-level management
- L16 PCM format (16-bit signed little-endian, 48kHz, stereo)
- 5ms packet size for low latency

### Phase 2: Multi-Codec Protocol (Completed Previously)
- Framing protocol with 4-byte header: `[Type][Format][Length:2]`
- Codec support: PCM L16, OPUS, MP2
- JSON metadata packets with callback mechanism
- Codec/bitrate detection exposed via BASS config options
- Decoder warmup mechanism for glitch-free startup

### Phase 3: SRT Configuration & Connection Modes (Completed This Session)

#### 3.1 Encryption Support
- Added `set_sock_opt_str()` helper for string socket options
- Passphrase URL parameter: `passphrase=secretkey123` (10-79 chars)
- AES encryption via SRT's built-in SRTO_PASSPHRASE
- Sender CLI option: `--passphrase "secretkey123"`
- Wrong passphrase correctly rejected with BADSECRET error

**Files modified:**
- `src/srt_bindings.rs` - Added `set_passphrase()`, `set_streamid()` helpers
- `src/input/stream.rs` - Apply passphrase in `configure_socket()`

#### 3.2 Connection Modes
Implemented three SRT connection modes:

| Mode | Description | Use Case |
|------|-------------|----------|
| **Caller** (default) | Connect to remote SRT listener | Reporter with dynamic IP connects to studio |
| **Listener** | Accept incoming connections | Studio with fixed IP waits for reporters |
| **Rendezvous** | Both sides connect simultaneously | NAT traversal when both behind NAT |

**URL Examples:**
```
srt://host:port                    # Caller mode (default)
srt://0.0.0.0:9000?mode=listener   # Listener mode
srt://host:port?mode=rendezvous    # Rendezvous mode
```

**Listener Mode Features:**
- Reconnect loop: When caller disconnects, accepts next caller automatically
- Audio continues seamlessly between callers (if same format)
- Only stops when user explicitly stops the stream

**Files modified:**
- `src/input/url.rs` - Added `ConnectionMode` enum, URL parsing
- `src/input/stream.rs` - Refactored `receiver_loop()` to handle all modes
- `src/srt_bindings.rs` - Added `bind()`, `listen()`, `accept()`, `set_rendezvous()`

#### 3.3 Buffer Configuration
Added URL parameters for SRT buffer tuning:
- `rcvbuf` / `recv_buffer` - Receive buffer size in bytes
- `sndbuf` / `send_buffer` - Send buffer size in bytes
- `timeout` / `connect_timeout` - Connection timeout in ms

**Latency Guidelines:**
- Rule of thumb: `latency >= 4 × RTT`
- LAN (<1ms RTT): 120ms default
- Same city (5-20ms): 80-120ms
- Cross-country (30-50ms): 200-250ms
- Intercontinental/Starlink (100-200ms): 500-1000ms

#### 3.4 BASS Config Options Added
```rust
pub const BASS_CONFIG_SRT_ENCRYPTED: DWORD = 0x21010;  // Returns: 1 if passphrase was set
pub const BASS_CONFIG_SRT_MODE: DWORD = 0x21011;       // Returns: 0=caller, 1=listener, 2=rendezvous
```

#### 3.5 Sender Example Updates
Updated `srt_sender_framed.rs` with:
- `--passphrase KEY` - Enable encryption
- `--connect HOST:PORT` - Caller mode (connect to remote listener)

**Sender Modes:**
```bash
# Listener mode (default) - wait for receiver to connect
./srt_sender_framed --port 9000

# Caller mode - connect to receiver in listener mode
./srt_sender_framed --connect 192.168.1.100:9000

# With encryption
./srt_sender_framed --passphrase "secretkey123" --connect 192.168.1.100:9000
```

## Real-World Use Case: Remote Broadcast

**Scenario:** Studio (fixed IP) receives audio from Reporter (mobile/Starlink)

```bash
# Studio (receiver) - Listener mode with encryption
./test_srt_input "srt://0.0.0.0:9000?mode=listener&passphrase=secretkey123&latency=500"

# Reporter (sender) - Caller mode connecting to studio
./srt_sender_framed --connect STUDIO_IP:9000 --passphrase "secretkey123" --codec opus --bitrate 128
```

**Key insight:** The side with known, reachable IP should be the Listener. The side behind NAT/dynamic IP should be the Caller.

## Current File Structure

```
bass-srt/
├── Cargo.toml
├── build.rs
├── DEVELOPMENT_STEPS_SRT_1.md    # This file
├── src/
│   ├── lib.rs                    # Plugin entry, FFI exports, BASS config
│   ├── srt_bindings.rs           # Raw libsrt FFI bindings
│   ├── ffi/
│   │   ├── mod.rs
│   │   ├── bass.rs               # BASS types
│   │   └── addon.rs              # BASS addon API
│   ├── input/
│   │   ├── mod.rs
│   │   ├── stream.rs             # SrtStream - receiver + STREAMPROC
│   │   └── url.rs                # srt:// URL parsing, ConnectionMode
│   ├── output/
│   │   ├── mod.rs
│   │   └── stream.rs             # SrtOutputStream (stub)
│   ├── protocol/
│   │   └── mod.rs                # Framing protocol (header encode/decode)
│   └── codec/
│       ├── mod.rs
│       ├── opus.rs               # OPUS encoder/decoder FFI
│       ├── twolame.rs            # MP2 encoder FFI
│       └── mpg123.rs             # MP2 decoder FFI
└── examples/
    ├── test_srt_input.rs         # BASS plugin receiver test
    ├── srt_sender.rs             # Simple unframed sender
    └── srt_sender_framed.rs      # Full-featured sender with codecs
```

## URL Parameter Summary

| Parameter | Aliases | Description | Default |
|-----------|---------|-------------|---------|
| `latency` | `latency_ms` | Target latency (ms) | 120 |
| `passphrase` | `password`, `pass` | Encryption key (10-79 chars) | none |
| `streamid` | `stream_id`, `sid` | Stream identifier | none |
| `mode` | - | caller/listener/rendezvous | caller |
| `rcvbuf` | `recv_buffer` | Receive buffer (bytes) | auto |
| `sndbuf` | `send_buffer` | Send buffer (bytes) | auto |
| `timeout` | `connect_timeout` | Connect timeout (ms) | 3000 |
| `packet_size` | `psize` | Packet duration (ms) | 20 |
| `channels` | `ch` | Audio channels | 2 |
| `rate` | `samplerate`, `sr` | Sample rate (Hz) | 48000 |

## Build Requirements

```bash
# Ubuntu/Debian
sudo apt-get install libsrt-gnutls-dev libopus-dev libtwolame-dev libmpg123-dev

# Build
cd BassAES67/bass-srt
cargo build --release
```

## Test Commands

```bash
# Set library path
export LD_LIBRARY_PATH=./target/release:../bass-aes67/target/release:../bass24-linux/libs/x86_64:$LD_LIBRARY_PATH

# Basic test (sender as listener, receiver as caller)
./target/release/examples/srt_sender_framed &
./target/release/examples/test_srt_input srt://127.0.0.1:9000

# Encrypted test
./target/release/examples/srt_sender_framed --passphrase "testsecretkey123" &
./target/release/examples/test_srt_input "srt://127.0.0.1:9000?passphrase=testsecretkey123"

# Reversed roles (receiver as listener, sender as caller)
./target/release/examples/test_srt_input "srt://0.0.0.0:9000?mode=listener" &
./target/release/examples/srt_sender_framed --connect 127.0.0.1:9000

# With OPUS codec
./target/release/examples/srt_sender_framed --codec opus --bitrate 128 &
./target/release/examples/test_srt_input srt://127.0.0.1:9000
```

## Known Issues / Warnings

1. **Compiler warnings** - Several unused imports and dead code warnings in `srt_bindings.rs` (functions for future use)
2. **Output module** - `src/output/stream.rs` is a stub, not yet implemented
3. **IPv6** - Not tested, only IPv4 currently
4. **DNS resolution** - Not implemented in sender example, must use IP addresses

## What's Next (Future Sessions)

### Priority 1: C# Integration
- Create P/Invoke bindings similar to `bass-aes67`
- Expose SRT functions to .NET applications
- Reference: `BassAES67/aes67_dotnet/Aes67Native.cs`

### Priority 2: Bidirectional Audio
- Allow studio to talk back to reporter
- Options to explore:
  1. Two separate SRT streams (one each direction)
  2. Single SRT connection with bidirectional audio (more complex)
  3. Output module implementation (`src/output/stream.rs`)

### Priority 3: Additional Features
- SRT statistics exposure (RTT, bandwidth, retransmits)
- Connection bonding for redundancy
- Additional codecs (AAC, FLAC)

## Architecture Notes

### Lock-Free Audio Path
Following CLAUDE.md guidelines, the audio callback path uses no mutexes:
- `ringbuf` crate for lock-free ring buffer
- Atomic counters for statistics
- PI controller for adaptive resampling

### Connection Mode Implementation
The `receiver_loop()` in `stream.rs` was refactored to use a common socket configuration closure and handle all three modes:

```rust
let configure_socket = |sock: SRTSOCKET| -> Result<(), ()> {
    // Live mode, latency, passphrase, streamid, buffers
};

match config.mode {
    ConnectionMode::Caller => { /* connect() */ }
    ConnectionMode::Listener => { /* bind() + listen() + accept loop */ }
    ConnectionMode::Rendezvous => { /* set_rendezvous() + bind() + connect() */ }
}
```

### Framing Protocol
```
┌────────┬────────┬────────────────┬────────────────────────┐
│ Type   │ Format │ Length (BE)    │ Payload                │
│ 1 byte │ 1 byte │ 2 bytes        │ variable               │
└────────┴────────┴────────────────┴────────────────────────┘

Type: 0x01=Audio, 0x02=JSON
Format (Audio): 0x00=PCM, 0x01=OPUS, 0x02=MP2
```

## Session Summary

This session completed Phase 3 of the bass_srt development:
- Full encryption support with passphrase
- All three SRT connection modes (caller, listener, rendezvous)
- Buffer configuration parameters
- Updated sender example with caller mode and encryption
- Tested all features successfully

The library is now production-ready for basic remote broadcast scenarios where a reporter with dynamic IP needs to send audio to a studio with a fixed IP.
