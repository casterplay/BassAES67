# Development Steps - AES67 Plugin Complete (Session 7)

## Session Summary
Audio loopback working with minimal latency. Stats API exposed via BASS_GetConfig. Ready for C# bindings.

## Current Working Configuration

### Audio Settings
- **Jitter buffer:** 10ms (minimal latency, 0-1 underruns typical)
- **Packet rate:** 200 packets/sec (5ms Livewire standard)
- **Sample rate:** 48kHz stereo
- **PTP domain:** 1 (Livewire)

### PI Controller (stream.rs)
```rust
const KP: f64 = 0.0001;      // Proportional gain
const KI: f64 = 0.00005;     // Integral gain
const MAX_TRIM_PPM: f64 = 20.0;  // ±20 ppm adjustment range
```

### PTP Feedforward
When PTP is locked, input applies same frequency correction as output:
```rust
let ptp_feedforward = if ptp_is_locked() {
    ptp_get_frequency_ppm() / 1_000_000.0
} else {
    0.0  // No feedforward during calibration
};
let resample_ratio = 1.0 + ptp_feedforward + trim_clamped;
```

### BASS Configuration (for decode mode)
```rust
BASS_SetConfig(BASS_CONFIG_BUFFER, 20);      // 20ms buffer
BASS_SetConfig(BASS_CONFIG_UPDATEPERIOD, 0); // Disable auto-update
BASS_Init(0, 48000, 0, ...);                 // No soundcard mode
```

## Architecture

### Data Flow
```
Axia/Livewire (PTP Domain 1)
        │
        ▼  239.192.76.49:5004 @ 48kHz stereo, 5ms packets
┌───────────────────┐
│  bass_aes67.dll   │
│  INPUT STREAM     │  ← Ring buffer + PI controller + PTP feedforward
└────────┬──────────┘
         │ BASS decode channel
         ▼
┌───────────────────┐
│  OUTPUT STREAM    │  ← PTP-corrected send intervals
└────────┬──────────┘
         │
         ▼  239.192.1.100:5004 @ 48kHz stereo, 5ms packets
   xNode/Destination
```

### PTP Singleton Architecture
- `bass_ptp.dll` loaded once, shared by all streams
- First stream creation starts PTP client
- All input/output streams read shared `ptp_get_frequency_ppm()`
- PTP stops only when plugin unloads

## Files Created/Modified

### New File: `bass-aes67/bass_aes67.h`
C/C++ header with all config constants for application integration.

### Modified: `bass-aes67/src/lib.rs`
Added config options:
- `BASS_CONFIG_AES67_PTP_LOCKED` (0x20017) - PTP lock status
- `BASS_CONFIG_AES67_PTP_FREQ` (0x20018) - PTP frequency × 1000

## Stats API Reference

### Configuration (BASS_SetConfig)
| Option | Value | Description |
|--------|-------|-------------|
| `BASS_CONFIG_AES67_PT` | 0x20000 | RTP payload type (default 96) |
| `BASS_CONFIG_AES67_INTERFACE` | 0x20001 | Network interface IP (string ptr) |
| `BASS_CONFIG_AES67_JITTER` | 0x20002 | Jitter buffer depth in ms |
| `BASS_CONFIG_AES67_PTP_DOMAIN` | 0x20003 | PTP domain (default 0) |
| `BASS_CONFIG_AES67_PTP_ENABLED` | 0x20007 | Enable/disable PTP (default 1) |

### Statistics (BASS_GetConfig, read-only)
| Option | Value | Description |
|--------|-------|-------------|
| `BASS_CONFIG_AES67_BUFFER_LEVEL` | 0x20010 | Buffer fill % (0-200, 100=target) |
| `BASS_CONFIG_AES67_JITTER_UNDERRUNS` | 0x20011 | Underrun count |
| `BASS_CONFIG_AES67_PACKETS_RECEIVED` | 0x20012 | Total packets received |
| `BASS_CONFIG_AES67_PACKETS_LATE` | 0x20013 | Late/dropped packets |
| `BASS_CONFIG_AES67_BUFFER_PACKETS` | 0x20014 | Current buffer (packets) |
| `BASS_CONFIG_AES67_TARGET_PACKETS` | 0x20015 | Target buffer (packets) |
| `BASS_CONFIG_AES67_PACKET_TIME` | 0x20016 | Packet time in µs |
| `BASS_CONFIG_AES67_PTP_LOCKED` | 0x20017 | PTP locked (0/1) |
| `BASS_CONFIG_AES67_PTP_FREQ` | 0x20018 | PTP freq PPM × 1000 |
| `BASS_CONFIG_AES67_PTP_STATE` | 0x20006 | PTP state (0-3) |
| `BASS_CONFIG_AES67_PTP_OFFSET` | 0x20005 | PTP offset (ns, i64) |
| `BASS_CONFIG_AES67_PTP_STATS` | 0x20004 | PTP stats string (ptr) |

### PTP State Values
```c
#define BASS_AES67_PTP_DISABLED     0  // Not running
#define BASS_AES67_PTP_LISTENING    1  // Waiting for master
#define BASS_AES67_PTP_UNCALIBRATED 2  // Syncing with master
#define BASS_AES67_PTP_SLAVE        3  // Locked to master
```

## Build Commands

### Windows
```bash
cd "c:/Dev/Lab/BASS/bass-aes67"
cargo build --release
# DLL at: target/release/bass_aes67.dll

# Build example
cargo build --release --example aes67_loopback
cd target/release/examples
./aes67_loopback.exe
```

### Linux (untested)
```bash
cd bass-aes67
cargo build --release
# .so at: target/release/libbass_aes67.so
```

## Test Results

### Loopback Performance (10ms jitter buffer)
- 0-1 underruns over 2+ minutes
- Buffer stable at 10-30 packets (target ~20)
- No audible latency between input and output
- PTP locks within 3-4 seconds

## Next Session Goals

### 1. C# P/Invoke Bindings (.NET 10)
Create `BassAes67.cs` with:
- BASS_GetConfig/SetConfig wrappers
- All config constants
- Stats struct for easy polling
- Example console app

### 2. Linux Testing
- Build on Linux
- Test with ALSA/PulseAudio
- Verify PTP works (may need `bass_ptp.so`)

### 3. Optional Improvements
- Multiple stream support (currently ACTIVE_STREAM is single)
- SDP file generation for stream discovery
- SAP announcement support

## Key Files

| File | Purpose |
|------|---------|
| `bass-aes67/src/lib.rs` | Plugin entry, config handler |
| `bass-aes67/src/input/stream.rs` | Input with PI controller |
| `bass-aes67/src/output/stream.rs` | Output (DO NOT MODIFY) |
| `bass-aes67/src/ptp_bindings.rs` | PTP DLL bindings |
| `bass-aes67/bass_aes67.h` | C/C++ header |
| `bass-aes67/examples/aes67_loopback.rs` | Test example |

## Critical Constraints
- **DO NOT** modify `bass-aes67/src/output/stream.rs` - proven working
- **DO NOT** modify `bass-ptp/` - PTP mechanism finalized
- **DO NOT** use Mutex in audio path - use atomics only
