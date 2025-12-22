# SRT Crash Fix & Setup Guide - Development Notes

This document captures the resolution of the SIGSEGV crash issue and provides setup instructions for Linux Docker and Windows x64.

## Problem Statement

The bass-srt plugin was crashing with **SIGSEGV (exit code 139)** when the SRT sender disconnected. Both the Rust test app (`test_srt_input`) and C# app (`srt_dotnet`) were affected.

Previous debugging attempts (documented in `DEVELOPMENT_STEPS_SRT_2.md`) tried:
- SIGPIPE handling
- `catch_unwind` around receive functions
- Explicit Drop ordering
- Custom Drop for AudioDecoder

None of these fixed the crash because **the issue was in the SRT library itself**.

## Root Cause

The **apt-get version of libsrt (1.4.4 with GnuTLS)** doesn't handle disconnection gracefully. When the sender stops, `srt_recv()` causes a segmentation fault instead of returning an error code.

## Solution: Upgrade to SRT 1.5.4 (OpenSSL)

Building SRT 1.5.4 from source with OpenSSL resolved the crash. The library now returns error code 2001 on disconnect instead of crashing.

### Building SRT 1.5.4 from Source

```bash
cd /tmp
wget https://github.com/Haivision/srt/archive/refs/tags/v1.5.4.tar.gz
tar xzf v1.5.4.tar.gz
cd srt-1.5.4
mkdir build && cd build
cmake .. -DCMAKE_INSTALL_PREFIX=$HOME/local/srt-1.5.4 \
         -DENABLE_ENCRYPTION=ON \
         -DUSE_OPENSSL=ON
make -j$(nproc)
make install
```

### Code Changes Required

**`build.rs`** - Update library path:
```rust
#[cfg(target_os = "linux")]
{
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/kennet".to_string());
    let srt_path = format!("{}/local/srt-1.5.4/lib", home);
    println!("cargo:rustc-link-search=native={}", srt_path);
    println!("cargo:rustc-link-lib=dylib=srt");
}
```

**`srt_bindings.rs`** - Change library name from `srt-gnutls` to `srt`:
```rust
#[link(name = "srt")]
extern "C" {
    // ... FFI functions
}
```

**Add version check** (optional but recommended):
```rust
pub fn srt_getversion() -> u32;

pub fn get_version_string() -> String {
    let v = unsafe { srt_getversion() };
    format!("{}.{}.{}", (v >> 16) & 0xFF, (v >> 8) & 0xFF, v & 0xFF)
}
```

## Additional Features Implemented

### Connection State Callback System

Added callback mechanism for C# to receive connection state changes from Rust:

**`stream.rs`**:
```rust
pub type ConnectionStateCallback = extern "C" fn(state: u32, user: *mut c_void);

static CONNECTION_STATE_CALLBACK: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static CONNECTION_STATE_USER: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

pub fn set_connection_state_callback(callback: ConnectionStateCallback, user: *mut c_void);
pub fn clear_connection_state_callback();
fn notify_connection_state(state: u32);
fn set_connection_state(stats: &StreamStats, state: u32);
```

**`lib.rs`** - C API exports:
```rust
#[no_mangle]
pub unsafe extern "C" fn BASS_SRT_SetConnectionStateCallback(
    callback: ConnectionStateCallback,
    user: *mut c_void,
);

#[no_mangle]
pub unsafe extern "C" fn BASS_SRT_ClearConnectionStateCallback();
```

**`BassSrtNative.cs`** - C# bindings:
```csharp
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate void ConnectionStateCallback(uint state, IntPtr user);

[DllImport("bass_srt")]
public static extern void BASS_SRT_SetConnectionStateCallback(
    ConnectionStateCallback callback, IntPtr user);

[DllImport("bass_srt")]
public static extern void BASS_SRT_ClearConnectionStateCallback();
```

### C# Refactoring (Idiomatic Patterns)

Replaced `while (running)` loop with proper C# patterns:

1. **Status Timer** - `System.Timers.Timer` for periodic updates
2. **ManualResetEventSlim** - Clean Ctrl+C handling
3. **Auto-reconnect Timer** - One-shot timer for reconnection attempts

```csharp
// Keep delegate reference to prevent GC!
BassSrtNative.ConnectionStateCallback connectionCallback = (state, user) =>
{
    if (state == BassSrtNative.CONNECTION_STATE_DISCONNECTED)
        ScheduleReconnect(3000);
};
BassSrtNative.BASS_SRT_SetConnectionStateCallback(connectionCallback, IntPtr.Zero);

void ScheduleReconnect(int delayMs)
{
    reconnectTimer = new System.Timers.Timer(delayMs);
    reconnectTimer.AutoReset = false; // One-shot
    reconnectTimer.Elapsed += (s, e) => {
        if (!CreateStreamAndPlay())
            ScheduleReconnect(3000); // Retry
    };
    reconnectTimer.Start();
}
```

## Lessons Learned

1. **SRT library version matters** - The apt-get version (1.4.4) has bugs; use 1.5.4
2. **GnuTLS vs OpenSSL** - Use OpenSSL build for better stability
3. **Check version at runtime** - Use `srt_getversion()` to confirm correct library
4. **C# delegate references** - Keep references to prevent garbage collection
5. **Use timers, not sleep** - For reconnection and periodic tasks in C#
6. **Thread safety** - Callbacks are called from receiver thread

---

# Linux Docker Setup

## Dependencies from apt-get (OK to use)

```bash
sudo apt-get update
sudo apt-get install -y \
    build-essential \
    cmake \
    git \
    wget \
    curl \
    pkg-config \
    libssl-dev \
    libopus-dev \
    libtwolame-dev \
    libmpg123-dev \
    libflac-dev
```

## Dependencies that MUST be built from source

### 1. SRT 1.5.4 (Required)

The apt-get version is too old (1.4.x with GnuTLS) and causes crashes.

```bash
cd /tmp
wget https://github.com/Haivision/srt/archive/refs/tags/v1.5.4.tar.gz
tar xzf v1.5.4.tar.gz
cd srt-1.5.4
mkdir build && cd build
cmake .. \
    -DCMAKE_INSTALL_PREFIX=/usr/local \
    -DENABLE_ENCRYPTION=ON \
    -DUSE_OPENSSL=ON \
    -DENABLE_STATIC=OFF \
    -DENABLE_SHARED=ON
make -j$(nproc)
sudo make install
sudo ldconfig
```

### 2. OPUS 1.6 (Optional)

Only needed if apt-get version is too old. Ubuntu 22.04 has 1.3.1 which works fine.

```bash
cd /tmp
wget https://github.com/xiph/opus/releases/download/v1.6/opus-1.6.tar.gz
tar xzf opus-1.6.tar.gz
cd opus-1.6
./configure --prefix=/usr/local
make -j$(nproc)
sudo make install
sudo ldconfig
```

## Runtime Dependencies

For deployment without build tools:

```bash
# Ubuntu/Debian
apt-get install -y libopus0 libmpg123-0 libflac12 libtwolame0 libssl3

# The SRT library must be copied from build or installed system-wide
```

## Complete Dockerfile Example

```dockerfile
FROM ubuntu:22.04

# Prevent interactive prompts
ENV DEBIAN_FRONTEND=noninteractive

# Install build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    cmake \
    git \
    wget \
    curl \
    pkg-config \
    libssl-dev \
    libopus-dev \
    libtwolame-dev \
    libmpg123-dev \
    libflac-dev \
    && rm -rf /var/lib/apt/lists/*

# Build SRT 1.5.4 from source (required - apt version crashes)
RUN cd /tmp && \
    wget https://github.com/Haivision/srt/archive/refs/tags/v1.5.4.tar.gz && \
    tar xzf v1.5.4.tar.gz && \
    cd srt-1.5.4 && \
    mkdir build && cd build && \
    cmake .. \
        -DCMAKE_INSTALL_PREFIX=/usr/local \
        -DENABLE_ENCRYPTION=ON \
        -DUSE_OPENSSL=ON \
        -DENABLE_STATIC=OFF \
        -DENABLE_SHARED=ON && \
    make -j$(nproc) && \
    make install && \
    ldconfig && \
    rm -rf /tmp/*

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

# Set library path
ENV LD_LIBRARY_PATH=/usr/local/lib:${LD_LIBRARY_PATH}

# Copy project and build
WORKDIR /app
COPY . .
RUN cd BassAES67/bass-srt && cargo build --release
```

## Environment Variables

```bash
# For local SRT installation
export LD_LIBRARY_PATH=$HOME/local/srt-1.5.4/lib:$LD_LIBRARY_PATH
export PKG_CONFIG_PATH=$HOME/local/srt-1.5.4/lib/pkgconfig:$PKG_CONFIG_PATH

# For system-wide installation
export LD_LIBRARY_PATH=/usr/local/lib:$LD_LIBRARY_PATH
```

---

# Windows x64 Setup

## Required Changes to build.rs

```rust
#[cfg(target_os = "windows")]
{
    // Link against SRT DLL
    println!("cargo:rustc-link-lib=dylib=srt");

    // Add search paths for libraries
    if let Ok(srt_path) = std::env::var("SRT_LIB_PATH") {
        println!("cargo:rustc-link-search=native={}", srt_path);
    }

    // Default locations
    println!("cargo:rustc-link-search=native=C:/libs/srt/lib");
}
```

## Required DLLs

Place these in the same directory as your executable:

| DLL | Source | Purpose |
|-----|--------|---------|
| `srt.dll` | Build from source | SRT library |
| `libcrypto-3-x64.dll` | OpenSSL | Encryption |
| `libssl-3-x64.dll` | OpenSSL | SSL/TLS |
| `opus.dll` | xiph.org | OPUS codec |
| `twolame.dll` | twolame.org | MP2 encoder |
| `libmpg123-0.dll` | mpg123.org | MP2 decoder |
| `FLAC.dll` | xiph.org | FLAC codec |
| `bass.dll` | un4seen.com | BASS audio |

## Building SRT on Windows

### Prerequisites
1. Visual Studio 2022 with C++ Desktop Development workload
2. CMake (from cmake.org or Visual Studio)
3. OpenSSL (from https://slproweb.com/products/Win32OpenSSL.html)
   - Install Win64 OpenSSL v3.x
   - Choose "Copy OpenSSL DLLs to: The Windows system directory"

### Build Steps

```powershell
# Clone SRT
git clone https://github.com/Haivision/srt.git
cd srt
git checkout v1.5.4

# Create build directory
mkdir build
cd build

# Configure with CMake
cmake .. -G "Visual Studio 17 2022" -A x64 `
    -DENABLE_ENCRYPTION=ON `
    -DUSE_OPENSSL=ON `
    -DOPENSSL_ROOT_DIR="C:/Program Files/OpenSSL-Win64"

# Build
cmake --build . --config Release

# Output: Release/srt.dll, Release/srt.lib
```

## Cross-Compilation from Linux (Alternative)

```bash
# Add Windows target
rustup target add x86_64-pc-windows-msvc

# Build (requires MSVC linker or use MinGW)
cargo build --release --target x86_64-pc-windows-msvc
```

**Note**: Cross-compilation requires either:
- MSVC toolchain via Wine/xwin
- MinGW-w64 (`x86_64-pc-windows-gnu` target)

For MinGW:
```bash
# Install MinGW
sudo apt-get install mingw-w64

# Add target
rustup target add x86_64-pc-windows-gnu

# Build
cargo build --release --target x86_64-pc-windows-gnu
```

---

# Files Modified in This Session

| File | Changes |
|------|---------|
| `bass-srt/build.rs` | Updated SRT library path to `~/local/srt-1.5.4/lib` |
| `bass-srt/src/srt_bindings.rs` | Changed from `srt-gnutls` to `srt`, added `srt_getversion()` |
| `bass-srt/src/input/stream.rs` | Added `ConnectionStateCallback`, `set_connection_state()`, `notify_connection_state()` |
| `bass-srt/src/lib.rs` | Exported `BASS_SRT_SetConnectionStateCallback`, `BASS_SRT_ClearConnectionStateCallback` |
| `srt_dotnet/BassSrtNative.cs` | Added `ConnectionStateCallback` delegate and P/Invoke bindings |
| `srt_dotnet/Program.cs` | Timer-based refactor, auto-reconnect with one-shot timer |

---

# Testing

## Start Sender
```bash
cd BassAES67/bass-srt
./run_sender.sh opus   # Options: opus, pcm, mp2, flac
```

## Start Receiver (Rust)
```bash
cd BassAES67/bass-srt
./run_receiver.sh
```

## Start Receiver (C#)
```bash
cd BassAES67/srt_dotnet
dotnet run
```

## Test Reconnection
1. Start receiver
2. Start sender - should connect and play
3. Stop sender (Ctrl+C)
4. Observe: receiver shows "Disconnected", schedules reconnect
5. Restart sender - should automatically reconnect

---

# Summary

The SIGSEGV crash was caused by the old SRT 1.4.4 library from apt-get. Upgrading to SRT 1.5.4 built from source with OpenSSL fixed the issue. Additional improvements include a callback system for connection state changes and proper C# patterns for timer-based operations.

---

# SRT Output Module Implementation

## Overview

The SRT output module enables sending audio from BASS channels/mixers via SRT protocol. This complements the existing input (receiver) module.

## Architecture

```
BASS Channel/Mixer
       │
       ▼ (BASS_ChannelGetData - float samples)
  Transmitter Thread
       │
       ▼ (encode)
   Audio Encoder (PCM/OPUS/MP2/FLAC)
       │
       ▼ (frame with 4-byte protocol header)
  Protocol Framing
       │
       ▼ (srt_send)
   SRT Socket
```

## Supported Features

| Feature | Description |
|---------|-------------|
| **Codecs** | PCM L16, OPUS, MP2, FLAC |
| **SRT Modes** | Caller (connects to remote) and Listener (accepts connections) |
| **Connection Callback** | Separate callback system for output state changes |
| **Lock-free Design** | Uses AtomicBool, AtomicU64 - no Mutex in audio thread |

## C API Functions

### Core Functions

```c
// Create SRT output stream from a BASS channel
void* BASS_SRT_OutputCreate(DWORD bass_channel, SrtOutputConfigFFI* config);

// Start transmitting
BOOL BASS_SRT_OutputStart(void* handle);

// Stop transmitting
BOOL BASS_SRT_OutputStop(void* handle);

// Get statistics
BOOL BASS_SRT_OutputGetStats(void* handle, SrtOutputStatsFFI* stats);

// Check if running
BOOL BASS_SRT_OutputIsRunning(void* handle);

// Free resources
BOOL BASS_SRT_OutputFree(void* handle);
```

### Callback Functions

```c
// Set callback for connection state changes (0=disconnected, 1=connecting, 2=connected, 3=reconnecting)
void BASS_SRT_SetOutputConnectionStateCallback(
    void (*callback)(uint32_t state, void* user),
    void* user
);

// Clear the callback
void BASS_SRT_ClearOutputConnectionStateCallback();
```

## FFI Structures

### SrtOutputConfigFFI

```rust
#[repr(C)]
pub struct SrtOutputConfigFFI {
    pub host_addr: [u8; 4],      // IP address bytes (e.g., [192, 168, 1, 100])
    pub port: u16,               // Port number
    pub mode: u8,                // 0=Caller, 1=Listener
    pub latency_ms: u32,         // SRT latency in milliseconds
    pub passphrase: *const c_char, // Optional passphrase (null for none)
    pub stream_id: *const c_char,  // Optional stream ID (null for none)
    pub channels: u16,           // 1=mono, 2=stereo
    pub sample_rate: u32,        // e.g., 48000
    pub codec: u8,               // 0=PCM, 1=OPUS, 2=MP2, 3=FLAC
    pub bitrate_kbps: u32,       // For OPUS/MP2 (e.g., 192)
    pub flac_level: u8,          // FLAC compression 0-8
}
```

### SrtOutputStatsFFI

```rust
#[repr(C)]
pub struct SrtOutputStatsFFI {
    pub packets_sent: u64,
    pub bytes_sent: u64,
    pub send_errors: u64,
    pub underruns: u64,
    pub connection_state: u32,
}
```

## C# Bindings

### Constants

```csharp
// Codec constants
public const int OUTPUT_CODEC_PCM = 0;
public const int OUTPUT_CODEC_OPUS = 1;
public const int OUTPUT_CODEC_MP2 = 2;
public const int OUTPUT_CODEC_FLAC = 3;

// Connection mode constants
public const int OUTPUT_MODE_CALLER = 0;
public const int OUTPUT_MODE_LISTENER = 1;
```

### P/Invoke Declarations

```csharp
[DllImport("bass_srt")]
public static extern IntPtr BASS_SRT_OutputCreate(int bassChannel, ref SrtOutputConfigFFI config);

[DllImport("bass_srt")]
public static extern bool BASS_SRT_OutputStart(IntPtr handle);

[DllImport("bass_srt")]
public static extern bool BASS_SRT_OutputStop(IntPtr handle);

[DllImport("bass_srt")]
public static extern bool BASS_SRT_OutputGetStats(IntPtr handle, out SrtOutputStatsFFI stats);

[DllImport("bass_srt")]
public static extern bool BASS_SRT_OutputIsRunning(IntPtr handle);

[DllImport("bass_srt")]
public static extern bool BASS_SRT_OutputFree(IntPtr handle);

// Callback
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate void OutputConnectionStateCallback(uint state, IntPtr user);

[DllImport("bass_srt")]
public static extern void BASS_SRT_SetOutputConnectionStateCallback(
    OutputConnectionStateCallback callback, IntPtr user);

[DllImport("bass_srt")]
public static extern void BASS_SRT_ClearOutputConnectionStateCallback();
```

### C# Usage Example

```csharp
// Initialize BASS
BassSrtNative.BASS_Init(-1, 48000, 0, IntPtr.Zero, IntPtr.Zero);

// Load bass_srt plugin
int plugin = BassSrtNative.BASS_PluginLoad("bass_srt", 0);

// Create a mixer or stream as audio source
int mixer = /* your BASS mixer/channel */;

// Configure output
var config = SrtOutputConfigFFI.CreateDefault("192.168.1.100", 5000);
config.Codec = BassSrtNative.OUTPUT_CODEC_OPUS;
config.BitrateKbps = 192;
config.Mode = BassSrtNative.OUTPUT_MODE_CALLER;

// Keep delegate reference to prevent GC!
BassSrtNative.OutputConnectionStateCallback outputCallback = (state, user) =>
{
    Console.WriteLine($"Output state: {BassSrtNative.GetConnectionStateName((int)state)}");
};
BassSrtNative.BASS_SRT_SetOutputConnectionStateCallback(outputCallback, IntPtr.Zero);

// Create and start output
IntPtr outputHandle = BassSrtNative.BASS_SRT_OutputCreate(mixer, ref config);
if (outputHandle != IntPtr.Zero)
{
    BassSrtNative.BASS_SRT_OutputStart(outputHandle);

    // Monitor stats
    if (BassSrtNative.BASS_SRT_OutputGetStats(outputHandle, out var stats))
    {
        Console.WriteLine($"Packets sent: {stats.PacketsSent}");
    }

    // Cleanup
    BassSrtNative.BASS_SRT_OutputStop(outputHandle);
    BassSrtNative.BASS_SRT_OutputFree(outputHandle);
}

BassSrtNative.BASS_SRT_ClearOutputConnectionStateCallback();
BassSrtNative.BASS_Free();
```

## Codec Frame Sizes

| Codec | Frame Duration | Samples at 48kHz |
|-------|----------------|------------------|
| PCM   | 5ms            | 240 samples      |
| OPUS  | 5ms            | 240 samples      |
| MP2   | 24ms           | 1152 samples     |
| FLAC  | 24ms           | 1152 samples     |

## Caller vs Listener Mode

### Caller Mode (OUTPUT_MODE_CALLER)
- Connects to a remote SRT listener
- Use when sending to a fixed server/receiver
- Automatically reconnects on disconnect

### Listener Mode (OUTPUT_MODE_LISTENER)
- Binds to local port and waits for connections
- Use when receivers connect to you
- Accepts one connection at a time (disconnects existing on new connection)

## Files Created/Modified for Output Module

| File | Changes |
|------|---------|
| `bass-srt/src/output/stream.rs` | Complete rewrite - SrtOutputStream, transmitter threads, connection callbacks |
| `bass-srt/src/output/encoder.rs` | New file - AudioEncoder trait, PCM/OPUS/MP2/FLAC encoder wrappers |
| `bass-srt/src/output/mod.rs` | Updated exports |
| `bass-srt/src/lib.rs` | Added C API exports (BASS_SRT_Output*) |
| `srt_dotnet/BassSrtNative.cs` | Added C# P/Invoke bindings for output |

## Protocol Interoperability

The output module uses the same 4-byte framing protocol as the input module:

```
[Format: 1 byte] [Reserved: 3 bytes] [Audio payload: N bytes]
```

Format byte values:
- `0x01` = PCM L16
- `0x02` = OPUS
- `0x03` = MP2
- `0x04` = FLAC

This ensures compatibility between bass-srt senders and receivers
