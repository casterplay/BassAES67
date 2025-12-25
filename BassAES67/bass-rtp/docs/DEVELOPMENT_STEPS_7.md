# Development Session 7 - C# P/Invoke Bindings for bass-rtp

## Summary

Created C# P/Invoke bindings for the bass-rtp native library, following the same pattern used in srt_dotnet.

## What Was Done

### 1. Fixed "ZipOne First" Audio Pops Issue (from Session 6)

The fix from Session 6 is now complete and working:
- **Root cause**: When Z/IP ONE was started first, BASS's internal buffers were empty and consumed samples too rapidly after buffering completed
- **Fix**: Modified resampler loop to output zeros on buffer underrun instead of popping (lines 972-987 in `stream.rs`)
- **Debug prints removed**: The `[DEBUG]` eprintln! statements were removed after confirming the fix works

### 2. Created rtp_dotnet C# Project

**Location**: `C:\Dev\CasterPlay2025\BassAES67\BassAES67\rtp_dotnet\`

**Files created**:

#### rtp_dotnet.csproj
- .NET 10.0 console application
- References `radio42.Bass.Net.core` v2.4.17.8

#### BassRtpNative.cs
Complete P/Invoke bindings matching `bass-rtp/src/lib.rs`:

**Constants**:
- `BASS_RTP_CODEC_*` (PCM16=0, PCM20=1, PCM24=2, MP2=3, G711=4, G722=5)
- `BASS_RTP_BUFFER_MODE_*` (SIMPLE=0, MINMAX=1)
- `BASS_RTP_CLOCK_*` (PTP=0, LIVEWIRE=1, SYSTEM=2)

**Structs**:
- `RtpOutputConfigFFI` - matches Rust struct exactly
- `RtpOutputStatsFFI` - matches Rust struct exactly

**DllImports** (Output module - Z/IP ONE connects TO us):
- `BASS_RTP_OutputCreate`
- `BASS_RTP_OutputStart`
- `BASS_RTP_OutputStop`
- `BASS_RTP_OutputGetInputStream`
- `BASS_RTP_OutputGetStats`
- `BASS_RTP_OutputIsRunning`
- `BASS_RTP_OutputFree`

**Helper methods**:
- `GetCodecName()`
- `GetClockModeName()`
- `GetPpm()`
- `GetConnectionStateName()`
- `GetPayloadTypeName()`

#### Program.cs
Test application similar to `rtp_output_test.rs`:
- Initializes BASS
- Creates mixer for backfeed (NONSTOP - outputs silence)
- Creates RTP Output stream listening on specified port
- Displays status line: RX/TX packets, buffer level, codec, PPM, level meters
- Handles Ctrl+C for graceful shutdown

**Usage**:
```bash
cd rtp_dotnet
dotnet run                    # Listen on port 6004, G.711 backfeed
dotnet run 5004               # Listen on port 5004
dotnet run 6004 3             # Port 6004, MP2 backfeed codec
```

## Current Status

- **C# bindings**: Created but NOT YET TESTED
- **Need to test**: Run `dotnet run` with Z/IP ONE to verify P/Invoke bindings work correctly

## Files Modified This Session

1. `src/output_new/stream.rs` - Removed debug eprintln! statements
2. `rtp_dotnet/rtp_dotnet.csproj` - Created
3. `rtp_dotnet/BassRtpNative.cs` - Created
4. `rtp_dotnet/Program.cs` - Created

## Required DLLs for Testing

The following DLLs must be in the bin folder or PATH:
- `bass.dll` (BASS core)
- `bassmix.dll` (BASS mixer)
- `bass_rtp.dll` (our Rust plugin)

## Next Steps

1. Build bass-rtp: `cargo build --release`
2. Copy `bass_rtp.dll` to rtp_dotnet bin folder
3. Run: `cd rtp_dotnet && dotnet run`
4. Connect Z/IP ONE and verify:
   - RX packets increasing
   - Buffer level stable
   - Audio plays without pops
   - TX packets sending (backfeed)

## Potential Issues to Watch For

1. **Struct alignment**: C# struct must match Rust `#[repr(C)]` exactly - field order matters
2. **Calling convention**: Using `CallingConvention.StdCall` to match Rust `extern "system"`
3. **Connection callback**: Currently set to `IntPtr.Zero` in config - callback in struct needs special handling if we want to use it

## Reference Files

- Rust FFI: `bass-rtp/src/lib.rs` (lines 420-729)
- Pattern: `srt_dotnet/BassSrtNative.cs`
- Pattern: `srt_dotnet/Program.cs`
