# Development Steps 4: AAC Encoder Removal

## Session Summary

This session focused on removing AAC encoder support from bass-rtp after determining that FFmpeg's AAC encoder integration was too problematic to maintain reliably.

## Background

In the previous session, we attempted to add AAC encoding support using FFmpeg's libavcodec. The encoder consistently failed with "Invalid sample rate: 0" despite correctly passing 48000Hz through the configuration.

### Investigation Results

1. **Initial approach**: Used `av_opt_set_int()` to set codec context fields like sample_rate, bit_rate, etc.
   - Result: Fields not being set - `av_opt_set_int` doesn't work for core AVCodecContext fields

2. **Second approach**: Direct memory access via hardcoded structure offsets
   - Added `ctx_fields` module with offsets based on FFmpeg 7.x (avcodec-62.dll)
   - Added debug function `debug_find_sample_rate()` to scan memory for the value
   - Result: Found 48000 at offset 580, but FFmpeg still reported "Invalid sample rate: 0"

3. **Debug output confirmed**:
   ```
   DEBUG: Scanning for sample_rate 48000 in AVCodecContext
     Found 48000 at offset 580
   [aac @ ...] Invalid sample rate: 0
   AAC encoder open failed with error -22
   ```

### Root Cause Analysis

The FFmpeg AAC encoder requires proper initialization through specific API calls that we couldn't identify. The structure offsets vary between FFmpeg versions, and internal validation happens elsewhere in the codec initialization chain. The direct memory access approach was deemed too fragile and "hacky" for production use.

## Decision

User decision: "But, this seams like a 'hack' lets skip MP2-AAC on the encoder side."

Removed AAC encoder support entirely while keeping the decoder functional for receiving MP2-AAC Xstream (PT 99) from Z/IP ONE.

## Changes Made

### Files Modified

#### 1. `src/stream/output.rs`
- Removed `ffmpeg_aac` import
- Removed `AAC_PAYLOAD_TYPE` constant
- Removed `Aac(ffmpeg_aac::Encoder)` variant from `EncoderType` enum
- Removed AAC match arms from:
  - `encode()`
  - `total_samples_per_frame()`
  - `payload_type()`
- Removed AAC case from transmitter loop encoder creation
- Removed AAC buffer size case (was 4608 bytes)

#### 2. `examples/rtp_loopback.rs`
- Updated help text from:
  ```
  codec - Output codec: 0=PCM16, 1=PCM20, 2=PCM24, 3=MP2, 4=AAC
  ```
  to:
  ```
  codec - Output codec: 0=PCM16, 1=PCM20, 2=PCM24, 3=MP2
  ```

#### 3. `src/codec/ffmpeg_aac.rs`
- Updated module documentation to reflect decode-only support
- Removed `Encoder` struct and all its implementations
- Removed `impl Drop for Encoder`
- Removed `test_encoder_create` test
- Removed `ctx_fields` module (Windows and Linux versions)
- Removed `frame_fields` module
- Removed unused FFI functions:
  - `avcodec_find_encoder`
  - `avcodec_find_encoder_by_name`
  - `avcodec_send_frame`
  - `avcodec_receive_packet`
  - `av_opt_set_sample_fmt`
  - `av_opt_set_chlayout`
  - `av_frame_get_buffer`
  - `av_frame_make_writable`
  - `av_frame_get_nb_samples`
  - `av_channel_layout_default`
  - `av_channel_layout_uninit`
  - `av_malloc`
  - `av_free`
- Removed `AVChannelLayout` type

## Final Codec Support

### Output (Encoding) - For sending TO Z/IP ONE:
| Codec ID | Codec | Payload Type |
|----------|-------|--------------|
| 0 | PCM-16 | 21 |
| 1 | PCM-20 | 116 |
| 2 | PCM-24 | 22 |
| 3 | MP2 | 14 |

### Input (Decoding) - For receiving FROM Z/IP ONE:
| Codec | Payload Type | Status |
|-------|--------------|--------|
| G.711 u-Law | 0 | Working |
| G.722 | 9 | Working |
| MP2 | 14, 96 | Working |
| PCM-16 | 21 | Working |
| PCM-20 | 116 | Working |
| PCM-24 | 22 | Working |
| AAC (ADTS/Xstream) | 99 | Working |
| AAC-LATM | 122 | NOT SUPPORTED |

## What Went Wrong

1. **FFmpeg API complexity**: The FFmpeg codec context structure is complex and version-dependent. Fields like `sample_rate` cannot be set through the generic `av_opt_set_int()` API.

2. **Structure offset brittleness**: Hardcoding memory offsets for FFmpeg structures is fragile - offsets change between versions and platforms.

3. **Insufficient documentation**: FFmpeg's documentation doesn't clearly explain how to properly initialize an AAC encoder context in the newer send/receive API.

4. **Time investment**: Spent significant time debugging before realizing the approach was fundamentally flawed.

## What Went Right

1. **Quick decision**: User made a pragmatic decision to remove the problematic feature rather than continue with a hacky solution.

2. **Clean removal**: The encoder was cleanly separated, making removal straightforward.

3. **Decoder preserved**: The AAC decoder (for receiving) still works and was preserved.

4. **Code cleanup**: Removed all unused code, keeping the codebase clean.

## Lessons Learned

1. **FFmpeg encoding is harder than decoding**: Decoders are more forgiving about initialization; encoders require precise configuration.

2. **av_opt_set doesn't set everything**: Core codec context fields (sample_rate, channels, etc.) must be set differently - possibly through the newer AVCodecParameters API or by using avcodec_parameters_to_context().

3. **Test early with actual hardware**: Should have tested with Z/IP ONE immediately rather than assuming the encoder worked after compilation.

4. **Fallback codecs exist**: PCM and MP2 codecs work reliably for output to Z/IP ONE, so AAC encoding isn't critical.

## Future Considerations

If AAC encoding is needed in the future:
1. Consider using `avcodec_parameters_to_context()` approach
2. Look at FFmpeg's own examples for AAC encoding
3. Consider using a simpler AAC library like FDK-AAC directly
4. Test with actual Z/IP ONE hardware from the start

## Build Verification

```bash
cd BassAES67/bass-rtp && cargo build --release
```

Build succeeds with only existing warnings about unused `channels` fields in G.711 and G.722 decoders.
