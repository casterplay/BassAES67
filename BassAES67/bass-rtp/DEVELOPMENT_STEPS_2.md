# Development Steps 2: RTP Test Example & MP2 Codec Integration

## Session Summary

This session focused on creating a test example for the bass-rtp plugin and getting bidirectional audio working with a Telos Z/IP ONE codec, including MP2 encode/decode support.

## What Was Accomplished

### 1. Created RTP Loopback Test Example
- **File**: `examples/rtp_loopback.rs`
- Dynamic loading of bass_rtp.dll via LoadLibraryA/GetProcAddress
- Supports multiple modes:
  - Loopback mode (default): sends RTP to itself
  - Z/IP ONE mode: bidirectional communication with Telos Z/IP ONE
- Command-line arguments: `[remote_ip] [remote_port] [codec] [local_port]`
- Z/IP ONE reciprocal RTP ports:
  - 9150 = Receive only (no reply)
  - 9151 = Reply with G.722
  - 9152 = Reply with same codec as sent
  - 9153 = Reply with current codec setting (often MP2)
- Real-time display showing TX/RX packets, buffer level, codec detection, audio meters

### 2. Wired Up MP2 Encoder (TwoLAME)
- **File**: `src/stream/output.rs`
- Added `Mp2(twolame::Encoder)` to `EncoderType` enum
- Added `encode_float()` call for MP2 encoding
- Fixed frame duration calculation for MP2's fixed 1152 samples per frame
- MP2 RTP payload type: 14

### 3. Wired Up MP2 Decoder (mpg123)
- **File**: `src/stream/input.rs`
- Added `Mp2(mpg123::Decoder)` to `DecoderType` enum
- **Key Discovery**: RFC 2250 MPEG Audio RTP header
  - Z/IP ONE sends MP2 with a 4-byte RFC 2250 header before the audio data
  - Bytes 0-1: MBZ (must be zero) - always `00 00`
  - Bytes 2-3: Fragment offset - typically `00 00`
  - Byte 4+: Actual MPEG audio frame starting with sync word `FF Fx`
  - Solution: Skip first 4 bytes when sync word is found at offset 4

### 4. Fixed Input Stream Playback
- **File**: `src/lib.rs`
- Removed `BASS_STREAM_DECODE` flag from input stream creation
- Now the input stream can be played directly with `BASS_ChannelPlay()`

### 5. Improved Jitter Buffer Thresholds
- **File**: `src/stream/input.rs`
- Old thresholds caused dropouts:
  - Critical: 5% of target (too small for MP2 frames)
  - Recovery: 50% of target
- New thresholds:
  - Critical: max(25% of target, 4608 samples) - ensures at least 48ms buffer
  - Recovery: 100% of target - more conservative
- Increased default jitter buffer in example from 40ms to 100ms

## Key Technical Learnings

### MP2 Frame Characteristics
- Fixed 1152 samples per frame at all sample rates
- At 48kHz: 1152/48000 = 24ms per frame
- At 48kHz stereo: 1152 * 2 = 2304 samples per frame
- Packet rate: 48000/1152 ≈ 41.67 packets/second (~42 pps)

### RFC 2250 MPEG Audio RTP Payload Format
```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|             MBZ               |          Frag_offset          |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                    MPEG Audio Frame Data...                   |
```
- MBZ: Must be zero
- Frag_offset: Fragment offset for fragmented frames (usually 0)
- Detection: Check if byte 4 is 0xFF and (byte 5 & 0xE0) == 0xE0

### mpg123 Decoder Behavior
- Feed mode: Use `mpg123_feed()` to push data, `mpg123_read()` to get output
- Returns `MPG123_NEED_MORE` (-10) until enough data accumulated
- Returns `MPG123_NEW_FORMAT` (-11) on first successful format detection
- After `NEW_FORMAT`, must call `read()` again to get actual samples
- Outputs signed 16-bit samples, need conversion to f32: `sample / 32768.0`

### Jitter Buffer Design
- Z/IP ONE recommendation: min buffer = 2x jitter, max buffer = 5x jitter
- For typical 10-20ms network jitter, use 50-100ms buffer
- MP2 needs larger minimum buffer due to 24ms frame size
- Critical threshold should be at least 2 MP2 frames (48ms)

## Files Modified

| File | Changes |
|------|---------|
| `Cargo.toml` | Added example configuration |
| `examples/rtp_loopback.rs` | New test example (780+ lines) |
| `src/stream/output.rs` | MP2 encoder support via TwoLAME |
| `src/stream/input.rs` | MP2 decoder support, RFC 2250 header skip, improved thresholds |
| `src/codec/mpg123.rs` | Fixed NEW_FORMAT handling in read() |
| `src/lib.rs` | Removed BASS_STREAM_DECODE for playable input stream |

## Required DLLs (Windows)
All must be in PATH or same directory as executable:
- `bass.dll` - BASS audio library
- `bass_rtp.dll` - The RTP plugin
- `libtwolame_dll.dll` - MP2 encoder
- `libmpg123-0.dll` or `mpg123.dll` - MP2 decoder

## Current Status

### Working
- PCM16/PCM24 bidirectional streaming with Z/IP ONE
- MP2 encoding (send to Z/IP ONE)
- MP2 decoding (receive from Z/IP ONE)
- Direct playback of input stream
- Real-time statistics display

### Known Issues / TODO
1. **Buffering still needs tuning** - May still get dropouts in some conditions
2. **PCM return has some artifacts** - User reported "garbage" in PCM return audio
3. **Buffer thresholds** - May need adaptive thresholds based on codec
4. **No OPUS/FLAC support yet** - Decoders not wired up

## Test Commands

```bash
# Build
cd BassAES67/bass-rtp
cargo build --release

# Test with Z/IP ONE - send MP2, receive MP2 (port 9152 = same codec)
cargo run --release --example rtp_loopback -- 192.168.50.155 9152 2

# Test with Z/IP ONE - send PCM16, receive MP2 (port 9153 = MP2 reply)
cargo run --release --example rtp_loopback -- 192.168.50.155 9153 0

# Test with Z/IP ONE - send PCM16, receive PCM16 (port 9152 = same codec)
cargo run --release --example rtp_loopback -- 192.168.50.155 9152 0
```

## Next Session Focus

The next session should focus on:
1. **Buffering improvements** - Make the jitter buffer more robust
2. **Investigate PCM artifacts** - Debug why PCM return has garbage
3. **Consider adaptive buffering** - Different thresholds for different codecs
4. **Possibly add configurable buffer parameters** - Min/max buffer like Z/IP ONE

## Architecture Reference

```
┌─────────────────────────────────────────────────────────────────┐
│                        bass-rtp Plugin                          │
├─────────────────────────────────────────────────────────────────┤
│  BidirectionalStream                                            │
│  ├── RtpInputStream (receive)                                   │
│  │   ├── RtpSocket (UDP recv)                                   │
│  │   ├── DecoderType (PCM16/PCM24/MP2)                         │
│  │   ├── Ring Buffer (lock-free)                                │
│  │   └── Adaptive Resampler (PI controller)                     │
│  │                                                              │
│  └── RtpOutputStream (send)                                     │
│      ├── BASS channel source                                    │
│      ├── EncoderType (PCM16/PCM24/MP2)                         │
│      ├── RtpPacketBuilder                                       │
│      └── RtpSocket (UDP send)                                   │
├─────────────────────────────────────────────────────────────────┤
│  Codecs                                                         │
│  ├── PCM16/PCM24: src/codec/pcm.rs (native)                    │
│  ├── MP2 Encode: src/codec/twolame.rs (libtwolame_dll.dll)     │
│  └── MP2 Decode: src/codec/mpg123.rs (mpg123.dll)              │
└─────────────────────────────────────────────────────────────────┘
```
