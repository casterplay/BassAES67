# Development Steps - Session 13: C# Linux Testing & Bass.NET Workaround

## Goal
Make `aes67_dotnet` C# application work on Linux.

## Environment
- OS: Pop!_OS (Linux)
- AoIP Network: `enp86s0` at `192.168.60.104`
- .NET 10.0

---

## What Worked

### Final Result
The C# app now works on Linux with full AES67 input/output:
```
sudo dotnet run
OK - InitBass: BASS_OK
OK - BASS_PluginLoad libbass_aes67.so: BASS_OK
mixer: -2147483647
Clock mode set to: PTP
AES67 configured (interface=192.168.60.104, jitter=10ms, domain=1)

Waiting for PTP lock...
PTP locked!

Creating AES67 input stream... aes67://239.192.76.49:5004
Using direct P/Invoke (bypassing Bass.NET)...
BASS_StreamCreateURL (aes67 plugin): BASS_OK, handle=-2147483646
BASS_Mixer_StreamAddChannel: BASS_OK
Buffer ready (300%)

Output stream created (dest: 239.192.1.100:5004, 5ms/200pkt/s)
IN: 246/10 rcv=2826 late=18 und=0 | OUT: pkt=2803 und=0 | PTP LOCKED
```

---

## The Problem

| Scenario | Works? |
|----------|--------|
| Rust example with `aes67://` on Linux | YES |
| C# with HTTP icecast URL on Linux | YES |
| C# with `aes67://` on Windows | YES |
| C# with `aes67://` on Linux | HANGS |

The C# app hung indefinitely when calling `Bass.BASS_StreamCreateURL("aes67://...")` on Linux.

---

## Root Cause

**Bass.NET wrapper issue on Linux** - The Bass.NET managed wrapper (`Bass.BASS_StreamCreateURL`) has platform-specific string marshalling that doesn't correctly handle custom URL schemes (`aes67://`) on Linux.

The Bass.NET wrapper worked fine for:
- HTTP/HTTPS URLs on Linux
- All URLs on Windows
- But NOT custom plugin URL schemes on Linux

---

## The Fix

### Solution: Bypass Bass.NET with Direct P/Invoke

**File:** `aes67_dotnet/Aes67Native.cs`

Added direct P/Invoke declaration:
```csharp
// Direct P/Invoke for BASS_StreamCreateURL - bypasses Bass.NET
[DllImport("bass", EntryPoint = "BASS_StreamCreateURL", CharSet = CharSet.Ansi)]
public static extern int BASS_StreamCreateURL_Direct(
    [MarshalAs(UnmanagedType.LPStr)] string url,
    int offset,
    int flags,
    IntPtr proc,
    IntPtr user);

public const int BASS_STREAM_DECODE = 0x200000;
```

**File:** `aes67_dotnet/Program.cs`

Changed to use direct call:
```csharp
Console.WriteLine("Using direct P/Invoke (bypassing Bass.NET)...");
int inputStream = Aes67Native.BASS_StreamCreateURL_Direct(
    inputUrl, 0, Aes67Native.BASS_STREAM_DECODE, IntPtr.Zero, IntPtr.Zero);
```

---

## Other Fixes Applied

### Fix 1: BASS_CTYPE_STREAM_AES67 Constant Mismatch
**File:** `bass-aes67/src/ffi/bass.rs`

**Problem:** Had `0x1f000` but should be `0x1f200`

**Fix:**
```rust
pub const BASS_CTYPE_STREAM_AES67: DWORD = 0x1f200;
```

### Fix 2: Plugin Extension Registration
**File:** `bass-aes67/src/lib.rs`

**Change:** Changed `exts` from `"*.aes67"` to `"aes67://"` for URL scheme registration:
```rust
static PLUGIN_FORMATS: [BassPluginForm; 1] = [
    BassPluginForm {
        ctype: BASS_CTYPE_STREAM_AES67,
        name: b"AES67 Network Audio\0".as_ptr() as *const i8,
        exts: b"aes67://\0".as_ptr() as *const i8,  // URL scheme, not file ext
    },
];
```

---

## What Went Wrong (Lessons Learned!)

### 1. Excessive Speculation Instead of Facts
I kept guessing about potential causes:
- "Maybe it's the library naming convention"
- "Maybe it's the DllImport paths"
- "Maybe it's thread safety"
- "Maybe it's the clock initialization order"

**Reality:** The Rust example worked perfectly. This meant the plugin code was correct. The issue was in the C# wrapper layer, not the Rust plugin.

### 2. Not Listening to User Feedback
The user explicitly said:
- "Stop Guessing, read the code! and listen to ME!"
- "Again the 'aes67_loopback.rs' Works!"
- "Why? It works on Windows and it works when I'm using an icecast URL! Stop guessing, stop assuming! Analyze, use Facts!"

**Lesson:** When the user gives you facts, USE THEM. The fact that:
- Rust example works = plugin is fine
- Icecast URL works = BASS is fine
- Windows works = code is fine
- Only `aes67://` on Linux fails = it's the intersection point (Bass.NET wrapper)

### 3. Adding Debug Prints Was Actually Useful
Adding `eprintln!` statements to the Rust code revealed that:
- `BASSplugin` was called and returned the `stream_create_url` function pointer
- But `stream_create_url` was NEVER called

This proved BASS received our plugin but never invoked it for `aes67://` URLs when called through Bass.NET on Linux.

### 4. The Fix Was Simple
All that was needed: bypass Bass.NET wrapper with direct P/Invoke using explicit ANSI marshalling.

---

## Files Modified This Session

1. **`bass-aes67/src/lib.rs`**
   - Changed `exts` to `"aes67://"`
   - Added debug prints (later removed)

2. **`bass-aes67/src/input/stream.rs`**
   - Added debug prints (later removed)

3. **`bass-aes67/src/ffi/bass.rs`**
   - Fixed `BASS_CTYPE_STREAM_AES67` from `0x1f000` to `0x1f200`

4. **`aes67_dotnet/Aes67Native.cs`**
   - Added `BASS_StreamCreateURL_Direct` P/Invoke
   - Added `BASS_STREAM_DECODE` constant

5. **`aes67_dotnet/Program.cs`**
   - Changed to use `BASS_StreamCreateURL_Direct`

---

## Current Project Status

### Working Components

| Component | Windows | Linux |
|-----------|---------|-------|
| bass_aes67 plugin (Rust) | YES | YES |
| AES67 Input streams | YES | YES |
| AES67 Output streams | YES | YES |
| PTP Clock | YES | YES (sudo) |
| Livewire Clock | YES | YES |
| System Clock | YES | YES |
| C# wrapper | YES | YES (with direct P/Invoke) |
| Rust example (aes67_loopback) | YES | YES |

### Architecture Summary

```
C# Application (aes67_dotnet)
    |
    +-- Bass.NET wrapper (DON'T use for aes67:// on Linux!)
    |       |
    |       +-- Use BASS_StreamCreateURL_Direct instead
    |
    +-- Aes67Native.cs (P/Invoke declarations)
    +-- Aes67OutputStream.cs (managed output wrapper)
    +-- AudioEngine.cs (BASS initialization)
    |
    v
libbass.so / bass.dll (BASS audio library)
    |
    v
libbass_aes67.so / bass_aes67.dll (Our Rust plugin)
    |
    +-- Input: stream_create_url -> Aes67Stream -> UDP multicast receiver
    +-- Output: BASS_AES67_OutputCreate -> Aes67OutputStream -> UDP multicast sender
    +-- Clock: PTP / Livewire / System clock synchronization
```

---

## Test Commands (Linux)

```bash
# Build Rust library
cd /home/kennet/dev/BassAES67/BassAES67/bass-aes67
cargo build --release

# Copy to C# bin folder
cp target/release/libbass_aes67.so ../aes67_dotnet/bin/Debug/net10.0/

# Run C# app (needs sudo for PTP ports 319/320)
cd ../aes67_dotnet
sudo dotnet run
```

---

## Next Steps: SRT Plugin

Create a new BASS plugin for SRT (Secure Reliable Transport) based on what we learned here:

### Architecture (Following AES67 Pattern)

1. **bass-srt** (Rust crate)
   - Input: `srt://` URL scheme for receiving SRT streams
   - Output: SRT transmitter from BASS channels
   - Use `srt-rs` crate for SRT protocol

2. **Key Learnings to Apply**
   - Use `.init_array` / `DllMain` for initialization
   - Register URL scheme in `PLUGIN_FORMATS.exts` as `"srt://\0"`
   - Provide direct C exports for functions that BASS config routing doesn't reach on Linux
   - For C# on Linux: use direct P/Invoke, not Bass.NET wrapper for custom URLs

3. **SRT vs AES67 Differences**
   - SRT: TCP-based, encrypted, adaptive bitrate, handles NAT/firewall
   - AES67: UDP multicast, uncompressed PCM, PTP synchronized
   - SRT may carry compressed audio (AAC, Opus) - need codec handling
   - SRT has caller/listener/rendezvous modes

4. **Suggested File Structure**
   ```
   bass-srt/
   ├── Cargo.toml
   ├── src/
   │   ├── lib.rs           # Plugin entry, BASSplugin, stream_create_url
   │   ├── ffi/
   │   │   ├── mod.rs
   │   │   ├── bass.rs      # BASS types (copy from bass-aes67)
   │   │   └── addon.rs     # Addon functions (copy from bass-aes67)
   │   ├── input/
   │   │   ├── mod.rs
   │   │   ├── stream.rs    # SRT input stream
   │   │   └── url.rs       # srt:// URL parser
   │   └── output/
   │       ├── mod.rs
   │       └── stream.rs    # SRT output stream
   └── examples/
       └── srt_loopback.rs  # Test example
   ```

---

## Summary

**The actual problem:** Bass.NET wrapper doesn't correctly marshal custom URL schemes to BASS on Linux.

**The fix:** Use direct P/Invoke with explicit ANSI string marshalling.

**Time wasted:** Too much speculation, not enough fact-based debugging.

**Key takeaway:** When debugging cross-platform issues:
1. Identify what WORKS (Rust example, icecast URLs, Windows)
2. Identify what FAILS (only aes67:// on Linux through C#)
3. The bug is at the intersection - in this case, the Bass.NET wrapper

**Listen to the user. Use facts. Don't guess.**
