# Development Steps 3: Configurable Buffer Management

## Session Summary

This session focused on implementing configurable buffer management for the bass-rtp plugin, inspired by the Telos Z/IP ONE's "Buffer Range" settings.

## What Was Accomplished

### 1. Added BufferMode Enum
- **File**: `src/stream/input.rs`
- Two buffer modes:
  - `Simple { buffer_ms }` - Single buffer value with automatic 3x headroom
  - `MinMax { min_ms, max_ms }` - Separate min (target) and max (ceiling) values

### 2. Updated Configuration Structures

#### FFI Config (`src/lib.rs`)
Added new fields to `RtpStreamConfigFFI`:
```rust
pub min_buffer_ms: u32,      // Min/Max mode: target buffer
pub max_buffer_ms: u32,      // Min/Max mode: ceiling buffer
pub buffer_mode: u8,         // 0 = simple, 1 = min/max
```

New constants:
- `BASS_CONFIG_RTP_MIN_BUFFER`
- `BASS_CONFIG_RTP_MAX_BUFFER`
- `BASS_CONFIG_RTP_BUFFER_MODE`
- `BASS_RTP_BUFFER_MODE_SIMPLE` (0)
- `BASS_RTP_BUFFER_MODE_MINMAX` (1)

#### Stats Structure (`RtpStatsFFI`)
Added buffer monitoring fields:
```rust
pub buffer_level_ms: u32,    // Current buffer level in milliseconds
pub target_buffer_ms: u32,   // Target buffer (min in min/max mode)
pub max_buffer_ms: u32,      // Max buffer setting
pub is_minmax_mode: u8,      // 1 if min/max mode, 0 if simple
```

### 3. Updated PI Controller for Min/Max Mode
- **File**: `src/stream/input.rs`
- Normal operation: aims for target (min_samples)
- When buffer > max: aggressive correction (3x error amplification, up to 100 PPM)
- Normal mode uses 50 PPM max adjustment

### 4. Updated Example with CLI Args
- **File**: `examples/rtp_loopback.rs`
- New arguments:
  - `--buffer <ms>` - Simple mode buffer size (default: 100ms)
  - `--min-buffer <ms>` - Min/Max mode: minimum buffer
  - `--max-buffer <ms>` - Min/Max mode: maximum buffer
- Status display shows: `Buf:85ms/100ms` (simple) or `Buf:75ms/50-200ms` (min/max)

## Files Modified

| File | Changes |
|------|---------|
| `src/lib.rs` | New config constants, updated `RtpStreamConfigFFI`, `RtpStatsFFI`, config handler |
| `src/stream/input.rs` | Added `BufferMode` enum, updated `RtpInputConfig`, buffer logic, PI controller, stats methods |
| `src/stream/bidirectional.rs` | Updated `BidirectionalConfig`, `BidirectionalStats`, config passing |
| `examples/rtp_loopback.rs` | CLI arg parsing for buffer options, updated display |

## Default Values

| Setting | Default | Range |
|---------|---------|-------|
| `jitter_ms` (simple) | 100ms | 20-500ms |
| `min_buffer_ms` | 50ms | 20-500ms |
| `max_buffer_ms` | 200ms | 50-1000ms |
| `buffer_mode` | 0 (simple) | 0-1 |

## Usage Examples

```bash
# Simple mode (default 100ms buffer)
cargo run --release --example rtp_loopback -- 192.168.50.155 9152 0

# Simple mode with custom buffer
cargo run --release --example rtp_loopback -- 192.168.50.155 9152 0 5004 --buffer 150

# Min/Max mode (target 50ms, ceiling 200ms)
cargo run --release --example rtp_loopback -- 192.168.50.155 9152 2 5004 --min-buffer 50 --max-buffer 200
```

## Buffer Behavior

### Simple Mode
- Target = `buffer_ms`
- Ring buffer size = target * 3 (headroom)
- PI controller aims to maintain target level

### Min/Max Mode
- Target = `min_ms` (system aims for this - lowest latency)
- Ceiling = `max_ms` (speeds up playback if exceeded)
- Ring buffer size = max * 2
- PI controller becomes more aggressive when above max

## Current Decoder Support

| Codec | Decoder | Status |
|-------|---------|--------|
| PCM-16 | Native | Working |
| PCM-24 | Native | Working |
| MP2 | mpg123 | Working |
| G.711 | - | Not implemented |
| G.722 | - | Not implemented |
| AAC | - | Not implemented |
| OPUS | libopus | Encoder only |
| FLAC | libFLAC | Encoder only |

## Next Session: Add Decoder Codecs

### Priority Decoders to Add

1. **G.711 (mu-law/A-law)** - PT 0 (mu-law), PT 8 (A-law)
   - Simple codec, can implement natively in Rust
   - Used by Z/IP ONE for basic compatibility

2. **G.722** - PT 9
   - Z/IP ONE uses this for port 9151 replies
   - Need to find/use a G.722 decoder library
   - Options: implement from spec, or use existing C library

3. **AAC Decoder**
   - Dynamic payload type
   - Options: libfdk-aac, faad2
   - May need to handle LATM/LOAS framing

### Implementation Approach

1. Add decoder variants to `DecoderType` enum in `input.rs`
2. Update `create_decoder_for_pt()` to instantiate new decoders
3. Add codec modules to `src/codec/`:
   - `g711.rs` - Native implementation
   - `g722.rs` - Using library or native
   - `aac.rs` - Using libfdk-aac or faad2

### G.711 Reference (mu-law)
```rust
fn ulaw_decode(input: u8) -> i16 {
    let mut input = !input;
    let sign = (input & 0x80) != 0;
    let exponent = ((input >> 4) & 0x07) as i32;
    let mantissa = (input & 0x0F) as i32;
    let mut sample = ((mantissa << 3) + 0x84) << exponent;
    sample -= 0x84;
    if sign { -sample } else { sample }
}
```

### G.722 Notes
- 7kHz wideband codec
- 64 kbps bitrate
- Sub-band ADPCM
- Libraries: ITU reference code, or ports like libg722

### AAC Notes
- Need to handle RTP payload format (RFC 3640)
- May have AU headers before actual AAC data
- libfdk-aac is high quality but has licensing considerations
- faad2 is LGPL alternative

## Build Status

All changes compile successfully:
```bash
cargo build --release
cargo build --release --example rtp_loopback
```

## Architecture Reference

```
┌─────────────────────────────────────────────────────────────────┐
│                        bass-rtp Plugin                          │
├─────────────────────────────────────────────────────────────────┤
│  BidirectionalStream                                            │
│  ├── RtpInputStream (receive)                                   │
│  │   ├── BufferMode (Simple or MinMax)                         │
│  │   ├── DecoderType                                            │
│  │   │   ├── PCM16/PCM24 (native)                              │
│  │   │   ├── MP2 (mpg123)                                       │
│  │   │   ├── G.711 (TODO)                                       │
│  │   │   ├── G.722 (TODO)                                       │
│  │   │   └── AAC (TODO)                                         │
│  │   ├── Ring Buffer (lock-free)                                │
│  │   └── PI Controller (adaptive resampling)                    │
│  │                                                              │
│  └── RtpOutputStream (send)                                     │
│      ├── EncoderType (PCM16/PCM24/MP2/OPUS/FLAC)               │
│      └── RTP Packet Builder                                     │
└─────────────────────────────────────────────────────────────────┘
```
