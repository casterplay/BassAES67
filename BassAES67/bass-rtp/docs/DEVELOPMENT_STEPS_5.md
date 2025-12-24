# Development Steps 5: Output Module (We Connect TO Z/IP ONE)

## Session Summary

This session focused on creating a new "output" module for bass-rtp where **WE initiate the connection** to Z/IP ONE, as opposed to the existing "input" module where Z/IP ONE connects to us.

## Background

The existing bass-rtp implementation had an "input" module where:
- Z/IP ONE connects TO us
- We receive their audio and send return audio on the same socket

The new "output" module reverses this:
- WE connect TO Z/IP ONE
- We send our audio and receive return audio on the same socket

## What Was Accomplished

### 1. Added G.711 u-Law Encoder
- **File**: `src/codec/g711.rs`
- Added `G711UlawEncoder` struct
- Implements 48kHz stereo → 8kHz mono downsampling (6:1 ratio)
- mu-law compression algorithm for encoding

### 2. Added G.722 Encoder
- **File**: `src/codec/g722.rs`
- Added `G722Encoder` struct
- Implements 48kHz stereo → 16kHz mono downsampling (3:1 ratio)
- Sub-band ADPCM encoding (reverse of existing decoder)

### 3. Created Output Module
- **File**: `src/output/mod.rs` - Module exports
- **File**: `src/output/stream.rs` - Main implementation

#### RtpOutputBidirectional Structure
- TX thread: reads from BASS channel, encodes, sends RTP packets
- RX thread: receives return audio, decodes, pushes to ring buffer
- Hybrid sleep-spin timing for precise packet scheduling
- PPM clock correction from PTP/Livewire/System clocks
- Adaptive resampling with PI controller for return audio

#### SendEncoderType Enum
- Pcm16, Pcm20, Pcm24 (native)
- Mp2 (TwoLAME)
- G711Ulaw (NEW)
- G722 (NEW)

#### ReturnDecoderType Enum
- Pcm16, Pcm20, Pcm24, Mp2, G711Ulaw, G722, Aac

### 4. Added FFI Exports
- **File**: `src/lib.rs`

```c
// FFI API
void*   BASS_RTP_OutputCreate(HSTREAM source_channel, RtpOutputConfigFFI* config);
int     BASS_RTP_OutputStart(void* handle);
int     BASS_RTP_OutputStop(void* handle);
HSTREAM BASS_RTP_OutputGetReturnStream(void* handle);
int     BASS_RTP_OutputGetStats(void* handle, RtpOutputStatsFFI* stats);
int     BASS_RTP_OutputIsRunning(void* handle);
int     BASS_RTP_OutputFree(void* handle);
```

#### RtpOutputConfigFFI Structure
```c
struct RtpOutputConfigFFI {
    uint8_t  remote_addr[4];      // Z/IP ONE IP
    uint16_t remote_port;         // 9150-9153 or custom
    uint16_t local_port;          // 0 = auto-assign
    uint8_t  interface_addr[4];   // Network interface
    uint32_t sample_rate;         // 48000
    uint16_t channels;            // 1 or 2
    uint8_t  send_codec;          // BASS_RTP_CODEC_*
    uint32_t send_bitrate;        // For MP2/OPUS
    uint32_t frame_duration_ms;   // Frame size
    uint8_t  clock_mode;          // 0=PTP, 1=Livewire, 2=System
    uint8_t  ptp_domain;          // PTP domain 0-127
    uint8_t  return_buffer_mode;  // 0=simple, 1=min/max
    uint32_t return_buffer_ms;    // Return audio buffer
    uint32_t return_max_buffer_ms;// Max buffer (min/max mode)
};
```

#### RtpOutputStatsFFI Structure
```c
struct RtpOutputStatsFFI {
    uint64_t tx_packets;
    uint64_t tx_bytes;
    uint64_t tx_encode_errors;
    uint64_t tx_underruns;
    uint64_t rx_packets;
    uint64_t rx_bytes;
    uint64_t rx_decode_errors;
    uint64_t rx_dropped;
    uint32_t buffer_level;
    uint8_t  detected_return_pt;
    int32_t  current_ppm_x1000;
};
```

### 5. Created Test Example
- **File**: `examples/rtp_output_test.rs`
- Tests connecting TO Z/IP ONE
- Command-line options for codec, bitrate, clock mode, buffer settings
- Real-time display of TX/RX stats, audio meters, detected return codec

## Files Created/Modified

| File | Action | Description |
|------|--------|-------------|
| `src/codec/g711.rs` | MODIFIED | Added G711UlawEncoder |
| `src/codec/g722.rs` | MODIFIED | Added G722Encoder |
| `src/output/mod.rs` | CREATED | Module exports |
| `src/output/stream.rs` | CREATED | RtpOutputBidirectional implementation |
| `src/lib.rs` | MODIFIED | Added output module, FFI exports |
| `examples/rtp_output_test.rs` | CREATED | Test example |
| `Cargo.toml` | MODIFIED | Added rtp_output_test example |

## Supported Codecs

### Sending (TX) - For sending TO Z/IP ONE:
| Codec ID | Codec | Payload Type |
|----------|-------|--------------|
| 0 | PCM-16 | 21 |
| 1 | PCM-20 | 116 |
| 2 | PCM-24 | 22 |
| 3 | MP2 | 14 |
| 4 | G.711 u-Law | 0 |
| 5 | G.722 | 9 |

### Receiving (RX) - Return audio FROM Z/IP ONE:
| Codec | Payload Type | Status |
|-------|--------------|--------|
| G.711 u-Law | 0 | Working |
| G.722 | 9 | Working |
| MP2 | 14, 96 | Working |
| PCM-16 | 21 | Working |
| PCM-20 | 116 | Working |
| PCM-24 | 22 | Working |
| AAC (ADTS/Xstream) | 99 | Working |

## Build Status

```bash
cargo build --release
cargo build --release --example rtp_output_test
```

Both build successfully with only minor warnings (unused variables, dead code).

## Test Commands

```bash
# Connect to Z/IP ONE port 9152 (returns same codec as sent)
cargo run --release --example rtp_output_test -- 192.168.50.155 9152

# Send MP2 codec
cargo run --release --example rtp_output_test -- 192.168.50.155 9152 --codec 3

# Send G.722
cargo run --release --example rtp_output_test -- 192.168.50.155 9151 --codec 5

# With custom buffer
cargo run --release --example rtp_output_test -- 192.168.50.155 9152 --buffer 150
```

## Known Issues / TODO

1. **Naming confusion**: The "input" vs "output" naming may need clarification:
   - Current "input" module = Z/IP ONE connects to us (we receive first)
   - New "output" module = We connect to Z/IP ONE (we send first)
   - Both are bidirectional - the name refers to who initiates

2. **Not yet tested with hardware**: The output module compiles but hasn't been tested with actual Z/IP ONE hardware.

3. **Clock integration**: PPM correction is implemented but clock DLL loading needs verification.

## Architecture Reference

```
┌─────────────────────────────────────────────────────────────────┐
│                        bass-rtp Plugin                          │
├─────────────────────────────────────────────────────────────────┤
│  "INPUT" Module (Z/IP ONE connects TO us)                       │
│  └── BidirectionalStream                                        │
│      ├── RtpInputStream (receive their audio)                   │
│      └── RtpOutputStream (send our return audio)                │
│                                                                 │
│  "OUTPUT" Module (WE connect TO Z/IP ONE) [NEW]                 │
│  └── RtpOutputBidirectional                                     │
│      ├── TX Thread (send our audio)                             │
│      │   ├── BASS_ChannelGetData()                              │
│      │   ├── Encode (PCM/MP2/G.711/G.722)                       │
│      │   ├── Hybrid sleep-spin timing                           │
│      │   └── PPM clock correction                               │
│      │                                                          │
│      └── RX Thread (receive return audio)                       │
│          ├── Decode (auto-detect from PT)                       │
│          ├── Ring buffer (lock-free)                            │
│          └── Adaptive resampling (PI controller)                │
├─────────────────────────────────────────────────────────────────┤
│  Encoders                                                       │
│  ├── PCM16/20/24 (native)                                       │
│  ├── MP2 (TwoLAME)                                              │
│  ├── G.711 u-Law (native) [NEW]                                 │
│  └── G.722 (native) [NEW]                                       │
└─────────────────────────────────────────────────────────────────┘
```

## Next Session Focus

1. **Clarify input/output naming** - Review and potentially rename modules for clarity
2. **Test with Z/IP ONE hardware** - Verify the output module works correctly
3. **Debug any issues** - Fix problems discovered during testing
