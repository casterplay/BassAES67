# Development Steps - C# AES67 Integration Complete (Session 11)

## Session Goal
Complete the C# AES67 integration with BassMixer support and independent clock control.

## Background

### What Was Done in Session 10
- Created C# (.NET 10) console app `aes67_dotnet`
- Added `AudioEngine.cs` with basic BASS initialization using Bass.NET wrapper
- Planned the implementation of AES67 output stream in C#

## What Was Accomplished in Session 11

### 1. Created C# AES67 Infrastructure

**Files Created in `aes67_dotnet/`:**

| File | Purpose |
|------|---------|
| `Aes67Native.cs` | All AES67 constants from `bass_aes67.h` + P/Invoke for string configs |
| `Aes67OutputConfig.cs` | Config class with all parameters (multicast, port, channels, etc.) |
| `RtpPacketBuilder.cs` | RTP packet construction with L24 float-to-24bit conversion |
| `Aes67OutputStream.cs` | Output stream with high-priority TX thread, PPM correction, lock-free stats |
| `Program.cs` | Loopback example using BassMixer |

### 2. Added Independent Clock Control to Rust Plugin

**Problem:** Clock (PTP/Livewire/System) was only started when creating an AES67 input stream. Users wanted output-only mode with regular BASS sources (web radio, files).

**Solution:** Added two new exported functions to `bass_aes67.dll`:

```rust
// In bass-aes67/src/lib.rs
#[no_mangle]
pub unsafe extern "system" fn BASS_AES67_ClockStart() -> i32

#[no_mangle]
pub unsafe extern "system" fn BASS_AES67_ClockStop() -> i32
```

**Files Modified:**
- `bass-aes67/src/lib.rs` - Added `BASS_AES67_ClockStart()` and `BASS_AES67_ClockStop()` functions
- `bass-aes67/bass_aes67.h` - Added function declarations
- `aes67_dotnet/Aes67Native.cs` - Added P/Invoke for new functions

**Also fixed:** Added `clock_is_running()` check before starting clock in `stream_create_url()` to prevent conflicts when both `BASS_AES67_ClockStart()` and AES67 input streams are used.

### 3. Fixed BASS_STREAM_DECODE for BassMixer Compatibility

**Problem:** When adding AES67 input stream to BassMixer, it returned `BASS_ERROR_DECODE` because BassMixer checks `BASS_ChannelGetInfo` to verify the stream has `BASS_STREAM_DECODE` flag.

**Root Cause:** The `addon_get_info` callback in `stream.rs` was only reporting `BASS_SAMPLE_FLOAT` as flags, not including `BASS_STREAM_DECODE`.

**Solution:**
1. Added `stream_flags` field to `Aes67Stream` struct to store creation flags
2. In `lib.rs`, store the flags after stream creation: `(*stream_ptr).stream_flags = stream_flags`
3. In `addon_get_info`, report the stored flags: `(*info).flags = stream.stream_flags | BASS_SAMPLE_FLOAT`

**Files Modified:**
- `bass-aes67/src/input/stream.rs` - Added `stream_flags` field and updated `addon_get_info`
- `bass-aes67/src/lib.rs` - Store flags in stream after creation

## Current Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        C# Application                        │
├─────────────────────────────────────────────────────────────┤
│  AudioEngine.cs     │  Aes67Native.cs    │  Program.cs      │
│  (BASS init)        │  (P/Invoke)        │  (Loopback)      │
├─────────────────────┼────────────────────┼──────────────────┤
│  Aes67OutputConfig  │  RtpPacketBuilder  │ Aes67OutputStream│
│  (Config class)     │  (RTP packets)     │  (TX thread)     │
└─────────────────────┴────────────────────┴──────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                     Native Libraries                         │
├─────────────────────────────────────────────────────────────┤
│  bass.dll           │  bass_aes67.dll   │  bass_ptp.dll    │
│  (BASS core)        │  (AES67 input)    │  (PTP clock)     │
├─────────────────────┼───────────────────┼──────────────────┤
│  bass_aac.dll       │  bass_mix.dll     │  bass_lw_clock   │
│  (AAC decoder)      │  (BassMixer)      │  bass_sys_clock  │
└─────────────────────┴───────────────────┴──────────────────┘
```

## Data Flow

```
┌──────────────────┐     ┌──────────────────┐
│  AES67 Input     │     │  Web Radio       │
│  (multicast)     │     │  (HTTP stream)   │
└────────┬─────────┘     └────────┬─────────┘
         │                        │
         ▼                        ▼
┌─────────────────────────────────────────┐
│           BassMixer (decode mode)        │
│    BASS_STREAM_DECODE | BASS_MIXER_NONSTOP
└────────────────────┬────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────┐
│         Aes67OutputStream (C#)           │
│  - High-priority TX thread               │
│  - RTP packet building (L24)             │
│  - PPM frequency correction              │
│  - Lock-free statistics                  │
└────────────────────┬────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────┐
│         AES67 Output (multicast)         │
│         UDP → 239.192.x.x:5004          │
└─────────────────────────────────────────┘
```

## Clock Synchronization

```
┌─────────────────────────────────────────┐
│           BASS_AES67_ClockStart()        │
│  - Can be called independently           │
│  - Or auto-started by aes67:// stream   │
└────────────────────┬────────────────────┘
                     │
         ┌───────────┼───────────┐
         ▼           ▼           ▼
┌─────────────┐ ┌─────────────┐ ┌─────────────┐
│  PTP Clock  │ │  Livewire   │ │  System     │
│  (Domain 0) │ │  (Domain 1) │ │  (Fallback) │
└─────────────┘ └─────────────┘ └─────────────┘
```

## Usage Examples

### Output-Only Mode (No AES67 Input)
```csharp
// Configure and start clock WITHOUT input stream
Aes67Native.BASS_SetConfigPtr(Aes67Native.BASS_CONFIG_AES67_INTERFACE, "192.168.60.102");
Bass.BASS_SetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_CLOCK_MODE,
    Aes67Native.BASS_AES67_CLOCK_PTP);
Aes67Native.BASS_AES67_ClockStart();

// Wait for lock
while (Bass.BASS_GetConfig((BASSConfig)Aes67Native.BASS_CONFIG_AES67_PTP_LOCKED) == 0)
    Thread.Sleep(100);

// Create mixer with any BASS sources
int mixer = BassMix.BASS_Mixer_StreamCreate(48000, 2, BASSFlag.BASS_STREAM_DECODE);
BassMix.BASS_Mixer_StreamAddChannel(mixer, webRadioStream, BASSFlag.BASS_DEFAULT);

// Output to AES67 multicast
var output = new Aes67OutputStream(config);
output.Start(mixer);
```

### Full Loopback Mode (AES67 In → Mixer → AES67 Out)
```csharp
// Clock starts automatically with input stream
int inputStream = Bass.BASS_StreamCreateURL("aes67://239.192.76.49:5004", 0,
    BASSFlag.BASS_STREAM_DECODE, null, IntPtr.Zero);

// Add to mixer (now works with BASS_STREAM_DECODE fix!)
BassMix.BASS_Mixer_StreamAddChannel(mixer, inputStream, BASSFlag.BASS_DEFAULT);

// Output
var output = new Aes67OutputStream(config);
output.Start(mixer);
```

## Required DLLs

Copy to output directory:
```
bass.dll                  # BASS core library
bass_aes67.dll            # AES67 plugin (rebuilt with fixes)
bass_ptp.dll              # PTP clock
bass_livewire_clock.dll   # Livewire clock
bass_system_clock.dll     # System clock (fallback)
bass_aac.dll              # AAC decoder (for web streams)
bass_mix.dll              # BassMixer
```

## Test Configuration

```
Interface: 192.168.60.102 (AoIP network)
Input:     aes67://239.192.76.49:5004 (Livewire source)
Output:    239.192.1.100:5004 (5ms packets for Livewire)
Jitter:    10ms
PTP Domain: 1
```

## Future Sessions

### Session 12: Linux Testing
- Test all Rust crates on Linux
- Verify clock libraries work (bass_ptp, bass_livewire_clock, bass_system_clock)
- Test C# with .NET on Linux
- Address any platform-specific issues

### Session 13+: SRT Plugin
- Create new BASS plugin for SRT (Secure Reliable Transport)
- Use Haivision's libsrt: https://github.com/Haivision/srt
- Implement both input and output streams
- Support caller/listener/rendezvous modes
- Handle encryption (AES-128/256)

**SRT Plugin Structure:**
```
bass-srt/
├── Cargo.toml
├── src/
│   ├── lib.rs          # Plugin entry point
│   ├── input/          # SRT input stream
│   │   ├── mod.rs
│   │   └── stream.rs
│   ├── output/         # SRT output stream
│   │   ├── mod.rs
│   │   └── stream.rs
│   └── ffi/            # libsrt bindings
│       └── mod.rs
└── bass_srt.h          # C header
```

## Key Files Reference

### Rust Plugin
- `bass-aes67/src/lib.rs` - Main plugin with clock start/stop
- `bass-aes67/src/input/stream.rs` - Input stream with `stream_flags` fix
- `bass-aes67/bass_aes67.h` - C header with all constants

### C# Application
- `aes67_dotnet/Aes67Native.cs` - All P/Invoke and constants
- `aes67_dotnet/Aes67OutputStream.cs` - RTP output with TX thread
- `aes67_dotnet/Program.cs` - Working loopback example

### Clock Libraries
- `bass-ptp/` - IEEE 1588v2 PTP clock
- `bass-livewire-clock/` - Axia Livewire clock
- `bass-system-clock/` - Free-running fallback clock
