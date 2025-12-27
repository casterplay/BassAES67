# Bass Broadcast Processor - Development Session 2

## Session Summary

This session implemented **C# P/Invoke bindings** for the bass_broadcast_processor Rust library, with **callback-based event notifications** for real-time stats delivery. Also added stats callback infrastructure to the Rust FFI layer.

---

## What Was Built

### Phase 1: Rust Callback Infrastructure
- Added `ProcessorStatsCallbackData` struct for FFI-safe stats delivery
- Created `MultibandProcessorWrapper` to hold processor + callback state
- Implemented background `std::thread` for periodic stats pushing
- Added `BASS_MultibandProcessor_SetStatsCallback()` FFI function
- Updated ALL existing FFI functions to use wrapper pattern

### Phase 2: C# P/Invoke Bindings
- Created `bass_broadcast_dotnet` project (net10.0)
- `BassProcessorNative.cs` - Low-level P/Invoke declarations (~35 functions)
- `BroadcastProcessor.cs` - High-level wrapper with C# events
- All config structs with `IsEnabled` bool properties for convenience

---

## Project Structure (Updated)

```
bass_broadcast_processor/
├── Cargo.toml
├── src/
│   ├── lib.rs                    # FFI exports + stats callback infrastructure [UPDATED]
│   ├── ffi/
│   │   ├── mod.rs
│   │   └── bass.rs
│   ├── processor/
│   │   ├── mod.rs
│   │   ├── multiband.rs
│   │   ├── config.rs
│   │   └── stats.rs
│   └── dsp/
│       ├── mod.rs
│       ├── biquad.rs
│       ├── crossover.rs
│       ├── multiband.rs
│       ├── compressor.rs
│       └── gain.rs
├── examples/
│   └── ...
└── docs/
    ├── PLAN.md
    ├── DEVELOPMENT_STEPS_1.md
    └── DEVELOPMENT_STEPS_2.md    # This file

bass_broadcast_dotnet/                # [NEW - C# bindings]
├── bass_broadcast_dotnet.csproj
├── BassProcessorNative.cs           # P/Invoke declarations
└── BroadcastProcessor.cs            # High-level wrapper with events
```

---

## Key Technical Decisions

### 1. Callback-Based Stats (Not Polling)
**Decision**: Push stats via callback instead of requiring C# to poll.
**Reason**: More efficient, matches the bass-webrtc pattern, enables real-time UI updates.
**Implementation**: Background `std::thread` reads lock-free atomics and fires callback.

### 2. MultibandProcessorWrapper Pattern
**Problem**: Need to store callback state alongside processor.
**Solution**: Wrap `MultibandProcessor` in `MultibandProcessorWrapper` struct.
**Impact**: All existing FFI functions updated to use `wrapper.processor.method()`.

### 3. Fixed 8-Element Array for Band GR
**Problem**: Variable-length per-band gain reduction is complex for FFI.
**Solution**: Use fixed `[f32; 8]` array with `num_bands` count field.
**Trade-off**: Wastes a few bytes, but vastly simplifies C# marshalling.

### 4. SendablePtr for Thread Safety
**Problem**: Raw pointers can't be sent across thread boundaries in Rust.
**Solution**: Created `SendablePtr(usize)` wrapper that converts pointers to usize.
**Why It Works**: `usize` is inherently `Send`, so we wrap pointers before spawning thread.

### 5. FFI Struct Types (byte, ushort, uint)
**Problem**: User found `byte` instead of `bool` confusing.
**Solution**: Added `IsEnabled` bool properties to all config structs.
**Why Not Just Use bool**: FFI marshalling requires exact type sizes to match Rust layout.

### 6. No Unsafe Code in C#
**Constraint**: User's CLAUDE.md prohibits `unsafe` code in C#.
**Solution**: Used `[MarshalAs(UnmanagedType.ByValArray)]` instead of `fixed` arrays.
**Example**: `public float[] BandGrDb` with `SizeConst = 8` instead of `fixed float BandGrDb[8]`.

---

## Files Created/Modified

### Rust Changes (lib.rs)

```rust
// Added at top of lib.rs:
pub const MAX_BANDS: usize = 8;

pub type ProcessorStatsCallback = unsafe extern "system" fn(
    stats: *const ProcessorStatsCallbackData,
    user: *mut c_void,
);

#[repr(C)]
pub struct ProcessorStatsCallbackData {
    pub lufs_momentary: f32,
    pub lufs_short_term: f32,
    pub lufs_integrated: f32,
    pub input_peak: f32,
    pub output_peak: f32,
    pub agc_gr_db: f32,
    pub band_gr_db: [f32; MAX_BANDS],
    pub num_bands: u32,
    pub clipper_activity: f32,
    pub samples_processed: u64,
    pub underruns: u64,
    pub process_time_us: u64,
}

struct MultibandProcessorWrapper {
    processor: MultibandProcessor,
    stats_callback: Option<ProcessorStatsCallback>,
    stats_user: *mut c_void,
    stats_interval_ms: u32,
    stats_running: Arc<AtomicBool>,
    stats_thread: Option<JoinHandle<()>>,
}
```

### C# Files Created

| File | Purpose |
|------|---------|
| `bass_broadcast_dotnet.csproj` | Project file (net10.0, nullable, radio42.Bass.Net.core) |
| `BassProcessorNative.cs` | All P/Invoke declarations and structs |
| `BroadcastProcessor.cs` | High-level wrapper with `StatsUpdated` event |

---

## C# API Usage

```csharp
// Create 5-band processor using factory method
var processor = BroadcastProcessor.Create5Band(sourceChannel);

// Subscribe to stats events (callback-based, not polling)
processor.StatsUpdated += stats =>
{
    Console.WriteLine($"LUFS: {stats.LufsMomentary:F1}");
    Console.WriteLine($"Peak: {stats.OutputPeakDbfs:F1} dBFS");
    Console.WriteLine($"AGC GR: {stats.AgcGrDb:F1} dB");
};

// Enable stats with 100ms interval
processor.EnableStats(100);

// Configure using bool properties
var agc = AgcConfig.Default;
agc.IsEnabled = true;  // Uses bool, not byte!
processor.SetAgc(agc);

// Or use Create() helper with bool parameters
var agc2 = AgcConfig.Create(targetLevelDb: -16f, enabled: true);

// Play
Bass.BASS_ChannelPlay(processor.OutputHandle, false);

// Cleanup
processor.Dispose();
```

---

## Errors Encountered and Fixes

### 1. Thread Safety - Raw Pointers Not Send
**Error**: `*const MultibandProcessor cannot be sent between threads safely`
**Attempts**:
1. Created `SendPtr<T>` wrapper with `unsafe impl Send` - Still failed
2. Created `StatsThreadContext` struct - Closure still captured raw pointers
3. **Final Fix**: `SendablePtr(usize)` - Convert pointers to usize BEFORE closure

```rust
// This works because usize is inherently Send
#[derive(Clone, Copy)]
struct SendablePtr(usize);

impl SendablePtr {
    fn from_const<T>(ptr: *const T) -> Self { Self(ptr as usize) }
    unsafe fn as_const<T>(&self) -> *const T { self.0 as *const T }
}

unsafe impl Send for SendablePtr {}
```

### 2. Unsafe Code in C#
**Error**: User rejected `AllowUnsafeBlocks` in .csproj
**Fix**: Changed from `fixed` arrays to `[MarshalAs(UnmanagedType.ByValArray)]`

```csharp
// Before (requires unsafe):
public fixed float BandGrDb[8];

// After (safe):
[MarshalAs(UnmanagedType.ByValArray, SizeConst = 8)]
public float[] BandGrDb;
```

### 3. Confusing byte for bool
**Complaint**: User found `byte Enabled` confusing
**Fix**: Added `IsEnabled` bool properties to all config structs

```csharp
public bool IsEnabled
{
    readonly get => Enabled != 0;
    set => Enabled = value ? (byte)1 : (byte)0;
}
```

---

## Current Project Status

### Completed Features
- [x] 2-Band MVP processor
- [x] N-Band flexible processor (2, 5, 8 bands)
- [x] Per-band compression with Linkwitz-Riley crossovers
- [x] Wideband AGC (single-stage and 3-stage cascaded)
- [x] Stereo Enhancer (Omnia 9 style M/S processing)
- [x] Per-band Parametric EQ
- [x] Soft Clipper with oversampling
- [x] LUFS Metering (ITU-R BS.1770)
- [x] Stats callback infrastructure (Rust)
- [x] C# P/Invoke bindings with events

### Not Yet Implemented
- [ ] Clipper activity tracking (currently returns 0.0)
- [ ] Test program for C# bindings
- [ ] Preset system (JSON serialization)
- [ ] Real-time parameter adjustment UI

---

## Build Commands

```bash
# Build Rust library
cd C:\Dev\CasterPlay2025\BassAES67\BassAES67\bass_broadcast_processor
cargo build --release

# Build C# bindings
cd C:\Dev\CasterPlay2025\BassAES67\BassAES67\bass_broadcast_dotnet
dotnet build

# Run Rust examples
cargo run --example file_to_speakers_multiband --release
```

---

## Dependencies

### Rust
- No external DSP crates (zero-latency sample-by-sample)
- `std::thread` for stats callback loop (no Tokio)
- BASS audio library (external DLL)

### C#
- `radio42.Bass.Net.core` (2.4.17.8) - BASS .NET wrapper
- .NET 10.0

---

## Architecture Notes

### Stats Flow
```
Audio Thread                    Stats Thread                   C# Event
     |                               |                              |
     | (writes atomics)              |                              |
     v                               |                              |
 AtomicStats <---- reads (Relaxed) --|                              |
                                     |                              |
                                     v                              |
                         ProcessorStatsCallbackData                 |
                                     |                              |
                                     |--- callback --->  OnStatsCallback
                                                                    |
                                                                    v
                                                          StatsUpdated event
```

### Wrapper Pattern
```
FFI Handle (void*) --> MultibandProcessorWrapper
                              |
                              |- processor: MultibandProcessor
                              |- stats_callback: Option<fn>
                              |- stats_user: *mut c_void
                              |- stats_running: Arc<AtomicBool>
                              |- stats_thread: Option<JoinHandle>
```

---

## Notes for Next Session

1. **DLL Location**: The `bass_broadcast_processor.dll` must be in the same directory as the C# executable or in PATH for the bindings to work.

2. **Stats Callback Thread**: The callback fires on a background thread. C# events will also fire on this thread - use `Dispatcher.Invoke` or similar for UI updates.

3. **Delegate Lifetime**: The `_statsDelegate` field in `BroadcastProcessor` prevents the delegate from being GC'd while the native code holds a reference.

4. **Clipper Activity**: Currently hardcoded to 0.0 in the stats callback. Need to add atomic tracking in the soft clipper to report actual clipping.

5. **Test Coverage**: No C# test program yet. Consider creating a simple console app to verify the bindings work end-to-end.

6. **Factory Methods**: `Create2Band()` and `Create5Band()` provide sensible defaults. Users can also use the full constructor for custom configurations.

---

## Reference Files

| What | Where |
|------|-------|
| Rust FFI | `bass_broadcast_processor/src/lib.rs` |
| Stats structs | `bass_broadcast_processor/src/processor/stats.rs` |
| C# P/Invoke | `bass_broadcast_dotnet/BassProcessorNative.cs` |
| C# Wrapper | `bass_broadcast_dotnet/BroadcastProcessor.cs` |
| WebRTC pattern reference | `bass-webrtc/src/lib.rs`, `webrtc_dotnet/BassWebRtcPeer.cs` |
