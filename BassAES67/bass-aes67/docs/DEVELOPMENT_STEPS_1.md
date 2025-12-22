# BASS AES67 Plugin - Development Story (Session 1)

## Project Overview

**Goal:** Create an AES67 input plugin for the BASS audio library, written in Rust, enabling reception of professional broadcast-quality audio streams over IP networks.

**Target Use Case:** 24/7 broadcast playout systems requiring precise clock synchronization.

---

## Completed Work

### Phase 1: Rust FFI Foundation (COMPLETE)

Created the basic Rust project structure with FFI bindings to the BASS audio library.

**Files Created:**
- `Cargo.toml` - Rust project configuration (cdylib output)
- `src/lib.rs` - Plugin entry point with `BASSplugin` export
- `src/ffi/mod.rs` - FFI module organization
- `src/ffi/bass.rs` - BASS type bindings (HSTREAM, BASS_FUNCTIONS, etc.)
- `src/ffi/addon.rs` - Add-on API bindings (ADDON_FUNCTIONS, STREAMCREATEURLPROC)

**Key Accomplishments:**
- Successfully exports `BASSplugin()` function that BASS recognizes
- Registers `aes67://` URL scheme handler
- Plugin loads correctly in BASS applications

---

### Phase 2: AES67 Input Plugin (COMPLETE)

Implemented full AES67 stream reception with RTP parsing and jitter buffering.

**Files Created:**
- `src/input/mod.rs` - Input module organization
- `src/input/stream.rs` - Aes67Stream struct managing UDP receive and STREAMPROC callback
- `src/input/rtp.rs` - RTP packet parsing (header, sequence numbers, timestamps, payload extraction)
- `src/input/jitter.rs` - Jitter buffer implementation for packet reordering and timing

**Configuration Options Implemented:**
| Parameter | Config Constant | Description |
|-----------|----------------|-------------|
| Network Interface | `BASS_CONFIG_AES67_INTERFACE` (0x20001) | IP address for multicast binding |
| Payload Type | `BASS_CONFIG_AES67_PT` (0x20002) | RTP payload type (default: 96) |
| PTP Domain | `BASS_CONFIG_AES67_PTP_DOMAIN` (0x20003) | PTP domain number |

**URL Format:**
```
aes67://239.192.76.52:5004
```

**Key Accomplishments:**
- UDP multicast reception working
- RTP packet parsing (sequence numbers, timestamps, 24-bit PCM payload)
- Jitter buffer handles packet reordering and timing
- STREAMPROC callback feeds PCM to BASS
- Audio plays successfully from AES67 sources

---

### Phase 3: PTP Clock Integration (COMPLETE)

Implemented custom embedded PTP (IEEE 1588v2) client for clock synchronization.

**Files Created:**
- `src/ptp/mod.rs` - PtpClient struct, global instance, UDP multicast listeners
- `src/ptp/messages.rs` - PTPv2 message parsing (Announce, Sync, Follow_Up, Delay_Req, Delay_Resp)
- `src/ptp/servo.rs` - PI (Proportional-Integral) controller for offset/frequency tracking
- `src/ptp/stats.rs` - Statistics structure and display formatting
- `src/ptp/platform.rs` - Platform-specific high-resolution timestamps

**PTP Features:**
- Listens on multicast `224.0.1.129` ports 319 (event) and 320 (general)
- Parses Announce messages to identify grandmaster
- Processes Sync + Follow_Up (two-step mode) for offset calculation
- Sends Delay_Req and processes Delay_Resp for path delay measurement
- PI servo calculates offset and frequency adjustment
- Configurable PTP domain (tested with domain 0 and domain 10 GPS grandmaster)

**Statistics Display Format:**
```
Slave to: PTP/dca600fffeff2eea:2, δ -270.9µs, Freq: +100.00ppm [LOCKED]
```

**Platform Timestamps:**
- **Windows:** `GetSystemTimePreciseAsFileTime` (epoch-based, ~100ns resolution)
- **Linux/macOS:** `clock_gettime(CLOCK_REALTIME)` (nanosecond resolution)

**Key Technical Fixes Applied:**
1. **Epoch time issue:** Changed from `QueryPerformanceCounter` (time since boot) to `GetSystemTimePreciseAsFileTime` (Unix epoch time) to match PTP timestamps
2. **TAI/UTC offset:** Added `initial_offset_ns` baseline to track relative offset changes rather than absolute offset
3. **Lock threshold:** Relaxed from 1µs to 1ms for software timestamps (achievable on Windows)
4. **Lock indicator:** Shows `[LOCKED]` or `[UNLOCKED]` status

**API for Statistics Access:**
```c
// Get PTP stats string
const char* stats = (const char*)BASS_GetConfigPtr(BASS_CONFIG_AES67_PTP_STATS);
// Returns: "Slave to: PTP/dca600fffeff2eea:2, δ -270.9µs, Freq: +100.00ppm [LOCKED]"
```

---

## Test Application

**File:** `examples/test_stream.rs`

**Usage:**
```bash
cd C:\Dev\Lab\BASS\bass-aes67
cargo build
cargo run --example test_stream
```

**What it does:**
1. Initializes BASS audio library
2. Loads bass_aes67.dll plugin
3. Configures network interface and PTP domain
4. Creates stream from `aes67://239.192.76.52:5004`
5. Starts playback
6. Displays PTP stats every second for 60 seconds

---

## Current Project Structure

```
bass-aes67/
├── Cargo.toml
├── src/
│   ├── lib.rs                 # Plugin entry, BASSplugin export, config handlers
│   ├── ffi/
│   │   ├── mod.rs
│   │   ├── bass.rs            # BASS type bindings
│   │   └── addon.rs           # Add-on API bindings
│   ├── input/
│   │   ├── mod.rs
│   │   ├── stream.rs          # Aes67Stream, UDP receive, STREAMPROC
│   │   ├── rtp.rs             # RTP packet parsing
│   │   └── jitter.rs          # Jitter buffer
│   └── ptp/
│       ├── mod.rs             # PtpClient, global instance
│       ├── messages.rs        # PTPv2 message parsing
│       ├── servo.rs           # PI controller
│       ├── stats.rs           # Statistics formatting
│       └── platform.rs        # Platform timestamps
├── examples/
│   └── test_stream.rs         # Test application
../
└── DEVELOPMENT_STEPS_1.md     # This file
```

---

## Next Steps (Phase 3.5: Clock Drift Compensation)

### Problem Statement
For 24/7 broadcast operation, clock drift between the AES67 source and local audio output will cause the jitter buffer to eventually overflow or underflow.

At 100ppm drift:
- ~8.6 seconds drift per day
- Buffer issues within hours of operation

### Proposed Solution
Use `BASS_ChannelSetAttribute` with `BASS_ATTRIB_FREQ` to dynamically adjust playback rate based on PTP frequency offset.

**Concept:**
```rust
// If PTP servo says source is +10ppm faster:
let freq_ppm = ptp_servo.frequency_ppm();  // e.g., 10.0
let base_rate = 48000.0;
let adjusted_rate = base_rate * (1.0 + freq_ppm / 1_000_000.0);  // 48000.48 Hz
BASS_ChannelSetAttribute(handle, BASS_ATTRIB_FREQ, adjusted_rate);
```

**Why this works:**
- BASS consumes samples slightly faster/slower
- Jitter buffer drains at same rate it fills
- No manual PCM manipulation needed
- Pitch shift at 100ppm is ~0.017 cents (inaudible)

**Implementation needed:**
1. Store BASS channel handle when stream is created
2. Add FFI binding for `BASS_ChannelSetAttribute`
3. Periodically update frequency based on PTP servo output
4. Optionally use jitter buffer fill level as additional feedback

---

## Future Phases

### Phase 4: AES67 Output Library
- Separate utility library (not add-on)
- Pull audio via `BASS_ChannelGetData()`
- RTP packetization and multicast transmit
- PTP-synchronized timestamps
- SAP announcements with SDP

### Phase 5: Enhancements
- Linux hardware PTP timestamps via `SO_TIMESTAMPING` (sub-microsecond precision)
- SAP/SDP discovery for input streams
- Multiple simultaneous stream support
- Stream quality monitoring and statistics

---

## Build Commands

```bash
# Build the plugin
cd C:\Dev\Lab\BASS\bass-aes67
cargo build

# Build release version
cargo build --release

# Run test application
cargo run --example test_stream

# Output locations
# Debug:   target\debug\bass_aes67.dll
# Release: target\release\bass_aes67.dll
# Test:    target\debug\test_stream.exe
```

---

## Test Environment

- **OS:** Windows
- **Network Interface:** 192.168.60.102
- **AES67 Source:** 239.192.76.52:5004
- **PTP Domain:** 10 (GPS-controlled grandmaster)
- **Grandmaster ID:** dca600fffeff2eea:2

---

## Key Dependencies

From `Cargo.toml`:
- `socket2` - UDP socket handling with multicast support
- `parking_lot` - Fast mutex implementation
- Standard library only for PTP (no external PTP crates)

---

## Session Notes

- PTP implementation is custom/embedded (not using external libraries like `statime`)
- Software timestamps achieve ~100-500µs precision on Windows
- Lock threshold set to 1ms for realistic Windows software timestamp performance
- The PI servo tracks frequency drift but doesn't yet apply compensation to audio
- Next session should implement `BASS_ATTRIB_FREQ` compensation for 24/7 operation
