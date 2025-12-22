# Development Steps - SRT Windows Support (Part 4)

This document covers porting bass-srt from Linux to Windows x64 while maintaining full Linux compatibility.

## Goals

1. Make bass-srt work on Windows x64
2. Never break Linux compatibility
3. Reduce native library dependencies where possible using pure Rust alternatives

## Changes Made

### 1. Windows Thread Priority (output/stream.rs)

Added Windows equivalent of Linux `nice(-20)` for audio thread priority:

```rust
#[cfg(target_os = "linux")]
unsafe {
    libc::nice(-20);
}

#[cfg(target_os = "windows")]
unsafe {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
    };
    SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
}
```

### 2. Windows Target Configuration (.cargo/config.toml)

Added Windows MSVC target:

```toml
[target.x86_64-pc-windows-msvc]
# No special flags needed for Windows
```

### 3. Symphonia Integration (Cargo.toml)

Replaced native mpg123 and FLAC decoder with pure Rust Symphonia library:

```toml
[dependencies]
symphonia = { version = "0.5", default-features = false, features = ["mpa", "flac"] }
```

Benefits:
- No mpg123.dll needed on any platform
- FLAC decoding works without native library
- Cross-platform without compilation hassles

### 4. Codec Changes

#### mpg123.rs - Complete Rewrite
Replaced native mpg123 bindings with Symphonia-based MP2/MP3 decoder.
- Pure Rust implementation
- No DLL dependencies for decoding
- Same public API maintained

#### flac.rs - Hybrid Approach
- **Encoder**: Kept native libFLAC (needed for streaming output)
- **Decoder**: Replaced with Symphonia (pure Rust)

#### twolame.rs - Conditional Library Naming
Windows uses different library naming convention:

```rust
#[cfg_attr(target_os = "windows", link(name = "libtwolame_dll"))]
#[cfg_attr(not(target_os = "windows"), link(name = "twolame"))]
extern "C" { ... }
```

### 5. Build Script (build.rs)

Added Windows library search paths:

```rust
#[cfg(target_os = "windows")]
{
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let base_path = std::path::Path::new(&manifest_dir).parent().unwrap();

    // BASS library
    let bass_path = base_path.join("bass24/c/x64");
    println!("cargo:rustc-link-search=native={}", bass_path.display());

    // Windows_need_builds folder with native libraries
    let libs_path = base_path.join("Windows_need_builds");

    // SRT
    let srt_path = libs_path.join("srt/srt-1.5.4/build/Release");
    println!("cargo:rustc-link-search=native={}", srt_path.display());

    // OPUS
    let opus_path = libs_path.join("opus-1.6/build/Release");
    println!("cargo:rustc-link-search=native={}", opus_path.display());

    // TwoLame
    let twolame_path = libs_path.join("twolame-main");
    println!("cargo:rustc-link-search=native={}", twolame_path.display());

    // FLAC
    let flac_lib_path = libs_path.join("flac-master/build/src/libFLAC/Release");
    println!("cargo:rustc-link-search=native={}", flac_lib_path.display());

    println!("cargo:rustc-link-lib=dylib=srt");
}
```

## Native Libraries Built from Source

All built using CMake on Windows with Visual Studio 2022:

### OPUS 1.6
```cmd
cd Windows_need_builds/opus-1.6
mkdir build && cd build
cmake .. -DCMAKE_BUILD_TYPE=Release
cmake --build . --config Release
```
Output: `opus.dll`, `opus.lib`

### SRT 1.5.4 (with OpenSSL encryption)
```cmd
cd Windows_need_builds/srt/srt-1.5.4
mkdir build && cd build
cmake .. -DCMAKE_BUILD_TYPE=Release -DENABLE_ENCRYPTION=ON
cmake --build . --config Release
```
Requires OpenSSL installed to `C:\Program Files\OpenSSL-Win64` (full version, not "Light").
Output: `srt.dll`, `srt.lib`

### FLAC (latest master)
```cmd
cd Windows_need_builds/flac-master
mkdir build && cd build
cmake .. -DCMAKE_BUILD_TYPE=Release -DWITH_OGG=OFF -DBUILD_PROGRAMS=OFF -DBUILD_EXAMPLES=OFF -DBUILD_TESTING=OFF -DBUILD_DOCS=OFF -DINSTALL_MANPAGES=OFF
cmake --build . --config Release
```
Output: `FLAC.dll` (in `objs/Release`), `FLAC.lib` (in `src/libFLAC/Release`)

### TwoLame
Pre-built binaries used from `Windows_need_builds/twolame-main/`.

## Required DLLs at Runtime

All DLLs must be in the same folder as the executable:

| DLL | Purpose | Source |
|-----|---------|--------|
| `bass.dll` | BASS audio library | bass24/c/x64/ |
| `bass_srt.dll` | This plugin | target/release/ |
| `srt.dll` | SRT protocol | Windows_need_builds/srt/.../Release/ |
| `opus.dll` | Opus codec | Windows_need_builds/opus-1.6/build/Release/ |
| `FLAC.dll` | FLAC encoder | Windows_need_builds/flac-master/build/objs/Release/ |
| `libtwolame_dll.dll` | MP2 encoder (import lib) | Windows_need_builds/twolame-main/ |
| `twolame.dll` | MP2 encoder (runtime) | Windows_need_builds/twolame-main/ |
| `libcrypto-3-x64.dll` | OpenSSL crypto | Windows_need_builds/openssl_dlls/ |
| `libssl-3-x64.dll` | OpenSSL SSL | Windows_need_builds/openssl_dlls/ |

**Note**: Both `libtwolame_dll.dll` AND `twolame.dll` are needed due to how the library was built.

## Building on Windows

```cmd
cd bass-srt
cargo build --release
```

Output: `target/release/bass_srt.dll`

## Verification

Successfully tested:
- Loading bass_srt.dll from C# .NET application
- Connecting to Linux SRT server
- Receiving and playing audio stream

## Key Lessons Learned

1. **DLL naming matters**: Windows import libraries (.lib) encode the expected DLL name. The runtime DLL must match exactly.

2. **Symphonia is excellent**: Pure Rust audio decoding eliminates many cross-platform headaches.

3. **Keep native encoders**: For real-time streaming output, native encoders (FLAC, TwoLame) are still needed.

4. **OpenSSL versions**: SRT encryption requires the full OpenSSL installation, not the "Light" version.

5. **Conditional compilation**: Use `#[cfg(target_os = "...")]` and `#[cfg_attr(...)]` to handle platform differences cleanly.
