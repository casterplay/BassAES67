# Development Steps 8: C# Bindings Fixes & Input Module

## Session Summary

This session focused on fixing issues in the C# bindings and adding support for the "we call Z/IP ONE" scenario.

## Issues Fixed

### 1. DECODE Flag for Mixer Compatibility

**Problem**: `BASS_RTP_OutputGetInputStream` and `BASS_RTP_InputGetReturnStream` returned streams without `BASS_STREAM_DECODE` flag, causing `BASS_ERROR_DECODE` when adding to a mixer.

**Solution**: Added `decode_stream` configuration option to both Input and Output modules.

**Rust Changes**:
- `lib.rs`: Added `decode_stream: u8` to `RtpOutputConfigFFI` and `RtpInputConfigFFI`
- `output_new/stream.rs`: Added `decode_stream: bool` to `RtpOutputConfig`
- `input/stream.rs`: Added `decode_stream: bool` to `RtpInputConfig`
- `lib.rs`: Modified `BASS_RTP_OutputStart` and `BASS_RTP_InputStart` to use the flag:
  ```rust
  let flags = if stream.config.decode_stream {
      BASS_SAMPLE_FLOAT | BASS_STREAM_DECODE
  } else {
      BASS_SAMPLE_FLOAT
  };
  ```

**C# Changes**:
- Added `DecodeStream` field to `RtpOutputConfigFFI` struct
- Added `DecodeStream` field to `RtpInputConfigFFI` struct

### 2. Proper C# Callback Pattern

**Problem**: Using `GC.KeepAlive` immediately after creating the delegate doesn't prevent GC collection - the delegate could be collected and cause a crash when native code calls it.

**Solution**:
- Store callback delegate as a variable that lives for the program's duration
- Use `Marshal.GetFunctionPointerForDelegate()` to pass to native code
- Place `GC.KeepAlive()` at the END of the program (after `Console.ReadLine()`)

```csharp
// Store callback - lives for program duration
BassRtpNative.ConnectionStateCallback connectionCallback = (state, user) => { ... };

// Pass to config
config.ConnectionCallback = Marshal.GetFunctionPointerForDelegate(connectionCallback);

// ... program runs ...

Console.ReadLine();

// Keep alive until here
GC.KeepAlive(connectionCallback);
```

## New C# Classes

### BassRtpInput.cs

Complete C# bindings for the Input module (we call Z/IP ONE):

- `BassRtpInput` - Wrapper class with `IDisposable` pattern
- `BassRtpInputNative` - P/Invoke declarations
- `RtpInputConfigFFI` - Configuration struct matching Rust FFI
- `RtpInputStatsFFI` - Statistics struct matching Rust FFI

**Usage**:
```csharp
var config = BassRtpInputNative.RtpInputConfigFFI.CreateDefault(
    "192.168.1.100",  // Z/IP ONE IP
    9152              // Port (9152 = same codec reply)
);
config.DecodeStream = 1;  // For mixer compatibility

var rtpInput = new BassRtpInput(sourceChannel, config);
rtpInput.Start();

int returnStream = rtpInput.ReturnStreamHandle;
// Add returnStream to mixer or play directly
```

### ExampleCallZipOne.cs

Example code demonstrating how to use `BassRtpInput` to call Z/IP ONE.

## Module Comparison

| Feature | **Output Module** | **Input Module** |
|---------|-------------------|------------------|
| Scenario | Z/IP ONE calls US | WE call Z/IP ONE |
| Config key | `LocalPort` (we listen) | `RemoteAddr` + `RemotePort` |
| Send audio | `BackfeedCodec` | `SendCodec` |
| Receive audio | `GetInputStream()` | `GetReturnStream()` |
| C# class | `BassRtpNative` | `BassRtpInput` / `BassRtpInputNative` |

## Z/IP ONE Reciprocal Ports

| Port | Description |
|------|-------------|
| 9150 | Codec negotiation / lowest bitrate |
| 9151 | Lowest bitrate reply |
| 9152 | Same codec reply (recommended) |
| 9153 | Highest quality reply |

## Files Modified

### Rust (bass-rtp)
- `src/lib.rs` - Added `decode_stream` to FFI configs, used in Start functions
- `src/output_new/stream.rs` - Added `decode_stream` to `RtpOutputConfig`
- `src/input/stream.rs` - Added `decode_stream` to `RtpInputConfig`

### C# (rtp_dotnet)
- `BassRtpNative.cs` - Added `DecodeStream` field
- `Program.cs` - Fixed callback pattern, added `DecodeStream = 1`
- `BassRtpInput.cs` - **NEW** - Input module bindings
- `ExampleCallZipOne.cs` - **NEW** - Example usage

## Important Notes

1. **Callback lifetime**: When using callbacks with P/Invoke, the delegate must be kept alive for the entire duration it might be called. Store as a class field or use `GC.KeepAlive()` at program end.

2. **`using` with long-lived streams**: Don't use `using var` for streams that need to persist. The `using` statement disposes immediately when the variable goes out of scope:
   ```csharp
   // BAD - disposes immediately
   using var rtpInput = new BassRtpInput(...);

   // GOOD - control disposal manually
   var rtpInput = new BassRtpInput(...);
   // ... use rtpInput ...
   rtpInput.Dispose();  // when done
   ```

3. **Struct field order**: C# struct field order must exactly match Rust `#[repr(C)]` struct order for P/Invoke to work correctly.

## Project Status

The bass-rtp project now has complete C# bindings for both scenarios:
- **Output module**: Z/IP ONE connects to us (existing `Program.cs`)
- **Input module**: We connect to Z/IP ONE (new `BassRtpInput` class)

Both modules support:
- All codecs: PCM16/20/24, MP2, G.711, G.722
- Mixer compatibility via `DecodeStream` option
- Connection state callbacks
- Lock-free statistics
