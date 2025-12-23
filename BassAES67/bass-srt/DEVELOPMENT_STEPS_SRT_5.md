# bass_srt Development Steps - Session 5: Windows Port Complete

## Overview

This session completed the Windows port of bass_srt by fixing the MP2 and FLAC decoders. The Symphonia (pure Rust) approach failed for streaming audio, so we restored the native library FFI bindings.

## Problem Statement

After porting bass_srt to Windows using Symphonia for MP2/FLAC decoding:
- **PCM**: Working ✓
- **OPUS**: Working ✓
- **MP2**: Choppy/oscillating audio ✗
- **FLAC**: Silent (no audio output) ✗

**Root Cause**: Symphonia's probe-based architecture requires file metadata/headers. For streaming individual audio frames over SRT without container metadata, Symphonia cannot decode properly.

## Solution: Restore Native Library FFI

Reverted to the original native library approach from git commit `0755083`:
- **mpg123** for MP2 decoding (streaming-friendly `mpg123_decode()` API)
- **libFLAC** for FLAC decoding (callback-based streaming API)

## Files Modified

| File | Changes |
|------|---------|
| `src/codec/mpg123.rs` | Restored native mpg123 FFI from git `0755083` |
| `src/codec/flac.rs` | Restored native libFLAC decoder FFI from git `0755083` |
| `Cargo.toml` | Removed Symphonia dependency |
| `build.rs` | Added mpg123 library path for Windows |

## Windows DLL Setup

### Required DLLs (all in `target/release/`)

| DLL | Source | Purpose |
|-----|--------|---------|
| `bass_srt.dll` | Cargo build | Main SRT plugin |
| `srt.dll` | `Windows_need_builds/srt/srt-1.5.4/build/Release/` | SRT transport |
| `opus.dll` | `Windows_need_builds/opus-1.6/build/Release/` | OPUS codec |
| `FLAC.dll` | `Windows_need_builds/flac-master/build/objs/Release/` | FLAC encoder/decoder |
| `mpg123.dll` | Downloaded from mpg123.de (see below) | MP2/MP3 decoder |
| `libmpg123-0.dll` | Downloaded from mpg123.de | MP2/MP3 decoder (original name) |
| `twolame.dll` | `Windows_need_builds/twolame-main/` | MP2 encoder |
| `libtwolame.dll` | `Windows_need_builds/twolame-main/` | MP2 encoder |
| `libtwolame _dll.dll` | `Windows_need_builds/twolame-main/` | MP2 encoder |

### mpg123 Setup (Windows x64)

The old VS2010 solution in `Windows_need_builds/libmpg123-master/` only had Win32 configurations. Instead, we downloaded pre-built x64 binaries:

1. Downloaded from: https://www.mpg123.de/download/win64/1.32.10/mpg123-1.32.10-x86-64.zip
2. Extracted to: `Windows_need_builds/mpg123-1.32.10/mpg123-1.32.10-x86-64/`
3. Created import library:
   ```cmd
   cd Windows_need_builds\mpg123-1.32.10\mpg123-1.32.10-x86-64
   "C:\Program Files\Microsoft Visual Studio\...\lib.exe" /def:libmpg123-0.def /out:mpg123.lib /machine:x64
   ```
4. Copied DLLs to `target/release/`:
   - `libmpg123-0.dll` (original)
   - `mpg123.dll` (copy, for Rust linking)

### build.rs Library Paths (Windows)

```rust
#[cfg(target_os = "windows")]
{
    // BASS library
    let bass_path = base_path.join("bass24/c/x64");

    // SRT
    let srt_path = libs_path.join("srt/srt-1.5.4/build/Release");

    // OPUS
    let opus_path = libs_path.join("opus-1.6/build/Release");

    // TwoLame
    let twolame_path = libs_path.join("twolame-main");

    // FLAC
    let flac_lib_path = libs_path.join("flac-master/build/src/libFLAC/Release");

    // mpg123 (pre-built x64 binaries)
    let mpg123_path = libs_path.join("mpg123-1.32.10/mpg123-1.32.10-x86-64");
}
```

## Build Commands

### Windows
```cmd
cd BassAES67\bass-srt
cargo build --release
```

### Linux
```bash
cd ~/dev/BassAES67/BassAES67/bass-srt
cargo build --release
```

## Test Results

### Windows Receiver (with Linux sender)
- **PCM**: ✓ Working
- **OPUS**: ✓ Working
- **MP2**: ✓ Working (after native mpg123 restore)
- **FLAC**: ✓ Working (after native libFLAC restore)

### Pending Tests
- [ ] Linux receiver (rebuild needed after Symphonia removal)
- [ ] Windows C# sender

## Key Technical Details

### Why Symphonia Failed

Symphonia is designed for file-based decoding with:
1. File probing to detect format
2. Container metadata parsing
3. Seek support

For SRT streaming, we send individual compressed frames without container metadata. Symphonia's `probe()` function expects:
- FLAC: Stream info metadata block
- MP2/MP3: Full frame headers with sync patterns

The native libraries (mpg123, libFLAC) have streaming-friendly APIs:
- `mpg123_decode()` - Feed data incrementally, get PCM out
- `FLAC__stream_decoder_process_single()` - Callback-based frame decoding

### Native FFI Bindings

**mpg123.rs** key functions:
```rust
#[link(name = "mpg123")]
extern "C" {
    fn mpg123_new(...) -> *mut Mpg123Handle;
    fn mpg123_open_feed(...) -> c_int;
    fn mpg123_decode(...) -> c_int;  // Feed data, get PCM
    fn mpg123_read(...) -> c_int;
}
```

**flac.rs** key functions:
```rust
#[link(name = "FLAC")]
extern "C" {
    fn FLAC__stream_decoder_new() -> *mut FLAC__StreamDecoder;
    fn FLAC__stream_decoder_init_stream(...) -> c_int;  // Callback-based
    fn FLAC__stream_decoder_process_single(...) -> c_int;
}
```

## Directory Structure

```
bass-srt/
├── Cargo.toml                    # No Symphonia dependency
├── build.rs                      # Library paths for all platforms
├── src/
│   ├── lib.rs
│   ├── codec/
│   │   ├── mod.rs
│   │   ├── opus.rs              # Native opus FFI
│   │   ├── twolame.rs           # Native twolame FFI (encoder)
│   │   ├── mpg123.rs            # Native mpg123 FFI (decoder) ← RESTORED
│   │   └── flac.rs              # Native libFLAC FFI ← RESTORED
│   ├── input/
│   │   ├── mod.rs
│   │   ├── stream.rs            # SRT receiver + BASS STREAMPROC
│   │   └── url.rs               # srt:// URL parsing
│   └── ...
├── target/release/
│   ├── bass_srt.dll             # Main plugin
│   ├── srt.dll                  # SRT transport
│   ├── opus.dll                 # OPUS codec
│   ├── FLAC.dll                 # FLAC codec
│   ├── mpg123.dll               # MP2/MP3 decoder
│   ├── libmpg123-0.dll          # MP2/MP3 decoder (original)
│   ├── twolame.dll              # MP2 encoder
│   ├── libtwolame.dll           # MP2 encoder
│   └── libtwolame _dll.dll      # MP2 encoder
└── DEVELOPMENT_STEPS_SRT_*.md   # Development documentation
```

## Next Steps

1. **Test Linux receiver** - Rebuild and verify all codecs still work
2. **Test Windows C# sender** - Create or use existing C# sender
3. **C# bindings** - Create P/Invoke bindings for bass_srt.dll (like bass-aes67)

## Lessons Learned

1. **Symphonia is not suitable for streaming** - Great for file decoding, not for frame-by-frame streaming without metadata
2. **Native libraries are streaming-friendly** - mpg123 and libFLAC have APIs designed for incremental decoding
3. **Pre-built binaries save time** - Instead of building old VS2010 projects, use official pre-built binaries
4. **Keep git history** - Being able to restore from commit `0755083` saved significant time

## References

- mpg123 pre-built binaries: https://www.mpg123.de/download/win64/
- FLAC library: Already built in `Windows_need_builds/flac-master/`
- Previous sessions: `DEVELOPMENT_STEPS_SRT_1.md` through `DEVELOPMENT_STEPS_SRT_4.md`
