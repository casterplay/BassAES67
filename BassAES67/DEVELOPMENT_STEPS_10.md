# Development Steps - C# AES67 Loopback Example (Session 10)

## Session Goal
Create a C# (.NET 10) version of the `aes67_loopback.rs` Rust example. This demonstrates the production use case: receiving AES67 audio via BASS, routing through BASS (decode/mixer/effects), and transmitting via AES67.

## Background

### What Was Done in Session 9
- Created `bass-system-clock` crate for free-running fallback clock
- Added System Clock mode (`BASS_AES67_CLOCK_SYSTEM = 2`)
- Implemented automatic fallback when PTP/Livewire loses lock
- Added `BASS_CONFIG_AES67_CLOCK_FALLBACK_TIMEOUT` option
- Fixed clock stats display for all modes (PTP, Livewire, System)

### The Rust Example (`aes67_loopback.rs`)
The Rust example demonstrates:
1. Initialize BASS in no-soundcard mode (`device=0`)
2. Load `bass_aes67.dll` plugin
3. Configure clock mode (PTP, Livewire, or System)
4. Create AES67 INPUT stream (receives multicast audio)
5. Create AES67 OUTPUT stream (transmits to different multicast)
6. Monitor buffer levels, clock stats, and transmission stats
7. Clean shutdown with Ctrl+C handler

### Flow
```
AES67 INPUT (239.192.76.49:5004)
        │
        ▼
   BASS Decoder
   (can add effects/mixing)
        │
        ▼
AES67 OUTPUT (239.192.1.100:5004)
```

## Required DLLs

For the C# example to work, these DLLs must be in the output directory:
```
bass.dll                  # BASS core library
bass_aes67.dll            # AES67 plugin (input streams)
bass_ptp.dll              # PTP clock (for PTP mode)
bass_livewire_clock.dll   # Livewire clock (for Livewire mode)
bass_system_clock.dll     # System clock (for System mode + fallback)
```

## C# P/Invoke Declarations

### BASS Core Functions
```csharp
// bass.dll imports
[DllImport("bass.dll", CharSet = CharSet.Ansi)]
public static extern uint BASS_GetVersion();

[DllImport("bass.dll")]
public static extern int BASS_ErrorGetCode();

[DllImport("bass.dll")]
public static extern bool BASS_Init(int device, uint freq, uint flags, IntPtr win, IntPtr clsid);

[DllImport("bass.dll")]
public static extern bool BASS_Free();

[DllImport("bass.dll", CharSet = CharSet.Ansi)]
public static extern uint BASS_PluginLoad(string file, uint flags);

[DllImport("bass.dll")]
public static extern bool BASS_PluginFree(uint handle);

[DllImport("bass.dll", CharSet = CharSet.Ansi)]
public static extern uint BASS_StreamCreateURL(string url, uint offset, uint flags, IntPtr proc, IntPtr user);

[DllImport("bass.dll")]
public static extern bool BASS_StreamFree(uint handle);

[DllImport("bass.dll")]
public static extern uint BASS_ChannelIsActive(uint handle);

[DllImport("bass.dll")]
public static extern int BASS_ChannelGetData(uint handle, IntPtr buffer, uint length);

[DllImport("bass.dll")]
public static extern bool BASS_SetConfig(uint option, uint value);

[DllImport("bass.dll")]
public static extern uint BASS_GetConfig(uint option);

[DllImport("bass.dll", CharSet = CharSet.Ansi)]
public static extern bool BASS_SetConfigPtr(uint option, string value);

[DllImport("bass.dll")]
public static extern IntPtr BASS_GetConfigPtr(uint option);
```

### BASS Constants
```csharp
// Flags
public const uint BASS_STREAM_DECODE = 0x200000;
public const uint BASS_DATA_FLOAT = 0x40000000;

// Config options
public const uint BASS_CONFIG_BUFFER = 0;
public const uint BASS_CONFIG_UPDATEPERIOD = 6;

// Channel states
public const uint BASS_ACTIVE_STOPPED = 0;
public const uint BASS_ACTIVE_PLAYING = 1;
public const uint BASS_ACTIVE_STALLED = 2;
public const uint BASS_ACTIVE_PAUSED = 3;
```

### AES67 Config Constants
```csharp
// General settings
public const uint BASS_CONFIG_AES67_PT = 0x20000;           // Payload type
public const uint BASS_CONFIG_AES67_INTERFACE = 0x20001;    // Network interface IP
public const uint BASS_CONFIG_AES67_JITTER = 0x20002;       // Jitter buffer depth (ms)

// PTP/Clock settings
public const uint BASS_CONFIG_AES67_PTP_DOMAIN = 0x20003;   // PTP domain
public const uint BASS_CONFIG_AES67_PTP_STATS = 0x20004;    // Stats string (ptr)
public const uint BASS_CONFIG_AES67_PTP_OFFSET = 0x20005;   // Offset in ns
public const uint BASS_CONFIG_AES67_PTP_STATE = 0x20006;    // Clock state
public const uint BASS_CONFIG_AES67_PTP_ENABLED = 0x20007;  // Enable/disable

// Stream statistics
public const uint BASS_CONFIG_AES67_BUFFER_LEVEL = 0x20010;
public const uint BASS_CONFIG_AES67_JITTER_UNDERRUNS = 0x20011;
public const uint BASS_CONFIG_AES67_PACKETS_RECEIVED = 0x20012;
public const uint BASS_CONFIG_AES67_PACKETS_LATE = 0x20013;
public const uint BASS_CONFIG_AES67_BUFFER_PACKETS = 0x20014;
public const uint BASS_CONFIG_AES67_TARGET_PACKETS = 0x20015;
public const uint BASS_CONFIG_AES67_PACKET_TIME = 0x20016;

// Clock status
public const uint BASS_CONFIG_AES67_PTP_LOCKED = 0x20017;
public const uint BASS_CONFIG_AES67_PTP_FREQ = 0x20018;

// Clock mode settings
public const uint BASS_CONFIG_AES67_CLOCK_MODE = 0x20019;
public const uint BASS_CONFIG_AES67_CLOCK_FALLBACK_TIMEOUT = 0x2001A;

// Clock mode values
public const uint BASS_AES67_CLOCK_PTP = 0;       // IEEE 1588v2 PTP
public const uint BASS_AES67_CLOCK_LIVEWIRE = 1;  // Axia Livewire
public const uint BASS_AES67_CLOCK_SYSTEM = 2;    // System clock (free-running)

// Clock state values
public const uint BASS_AES67_PTP_DISABLED = 0;
public const uint BASS_AES67_PTP_LISTENING = 1;
public const uint BASS_AES67_PTP_UNCALIBRATED = 2;
public const uint BASS_AES67_PTP_SLAVE = 3;
```

## AES67 Output Implementation

The Rust example uses `Aes67OutputStream` from the bass-aes67 crate. For C#, we need to implement equivalent functionality:

### Aes67OutputConfig
```csharp
public class Aes67OutputConfig
{
    public IPAddress MulticastAddr { get; set; } = IPAddress.Parse("239.192.1.100");
    public ushort Port { get; set; } = 5004;
    public IPAddress? Interface { get; set; }
    public byte PayloadType { get; set; } = 96;
    public ushort Channels { get; set; } = 2;
    public uint SampleRate { get; set; } = 48000;
    public uint PacketTimeUs { get; set; } = 5000;  // 5ms for Livewire
}
```

### Aes67OutputStream
Needs to implement:
- UDP multicast socket setup
- RTP packet building (24-bit PCM with proper headers)
- BASS_ChannelGetData to pull samples
- Precision timing loop (with PTP frequency correction)
- Thread-safe statistics

### RTP Packet Format
```
| RTP Header (12 bytes) | Payload (L24 PCM samples) |

RTP Header:
  - Version (2), Padding (0), Extension (0), CSRC Count (0): 1 byte
  - Marker (0), Payload Type (96): 1 byte
  - Sequence Number: 2 bytes (big-endian)
  - Timestamp: 4 bytes (big-endian, increments by samples/packet)
  - SSRC: 4 bytes (random, constant for stream)

Payload:
  - L24 PCM: 3 bytes per sample, big-endian, interleaved channels
  - For 5ms @ 48kHz stereo: 240 samples × 2 channels × 3 bytes = 1440 bytes
```

### Float to L24 Conversion
```csharp
// Convert 32-bit float [-1.0, 1.0] to 24-bit signed integer
// Then store as 3 bytes big-endian
void ConvertFloatToL24(float sample, Span<byte> dest)
{
    int value = (int)(sample * 8388607.0f);  // 2^23 - 1
    value = Math.Clamp(value, -8388608, 8388607);
    dest[0] = (byte)((value >> 16) & 0xFF);  // MSB
    dest[1] = (byte)((value >> 8) & 0xFF);
    dest[2] = (byte)(value & 0xFF);          // LSB
}
```

## Project Structure

```
CasterPlay2025/
├── BassAES67/
│   ├── BassAES67/           # Rust crates (existing)
│   └── BassAES67.CSharp/    # NEW: C# solution
│       ├── BassAES67.CSharp.sln
│       ├── Bass.Aes67/              # Library project
│       │   ├── Bass.Aes67.csproj
│       │   ├── BassNative.cs        # P/Invoke declarations
│       │   ├── Aes67Config.cs       # Config constants
│       │   ├── Aes67OutputStream.cs # Output stream implementation
│       │   └── RtpPacketBuilder.cs  # RTP packet building
│       └── Aes67Loopback/           # Console app
│           ├── Aes67Loopback.csproj
│           └── Program.cs           # Main loopback example
```

## Implementation Steps

### Step 1: Create C# Solution and Projects
```bash
mkdir BassAES67.CSharp
cd BassAES67.CSharp
dotnet new sln -n BassAES67.CSharp
dotnet new classlib -n Bass.Aes67 -f net10.0
dotnet new console -n Aes67Loopback -f net10.0
dotnet sln add Bass.Aes67/Bass.Aes67.csproj
dotnet sln add Aes67Loopback/Aes67Loopback.csproj
cd Aes67Loopback && dotnet add reference ../Bass.Aes67/Bass.Aes67.csproj
```

### Step 2: Implement Bass.Aes67 Library
1. **BassNative.cs**: All P/Invoke declarations
2. **Aes67Config.cs**: Config option constants
3. **RtpPacketBuilder.cs**: RTP packet construction
4. **Aes67OutputStream.cs**: Output stream with timing loop

### Step 3: Implement Aes67Loopback Console App
1. Parse command line (ptp/lw/sys)
2. Initialize BASS (no soundcard mode)
3. Load bass_aes67.dll plugin
4. Configure clock mode and interface
5. Create input stream (decode mode)
6. Wait for clock lock
7. Create output stream
8. Monitor and display stats
9. Handle Ctrl+C for clean shutdown

### Step 4: Copy DLLs and Test
Copy required DLLs to output directory and run with each clock mode.

## Key Differences from Rust Version

| Aspect | Rust | C# |
|--------|------|-----|
| Memory management | Manual with unsafe | GC with pinned buffers |
| Thread priority | Windows-sys crate | Thread.CurrentThread.Priority |
| Timing | std::thread::sleep + spin loop | Thread.Sleep + SpinWait |
| Atomics | std::sync::atomic | System.Threading.Interlocked |
| Ctrl+C | SetConsoleCtrlHandler | Console.CancelKeyPress |

## Test Configuration

```
Interface: 192.168.60.102 (AoIP network)
Input:  239.192.76.49:5004 (Livewire source)
Output: 239.192.1.100:5004 (5ms/200pkt/s)
Jitter: 10ms
PTP Domain: 1
```

## Expected Output

```
BASS AES67 Loopback Example (C#)
================================

Clock Mode: System

BASS version: 2.4.18.3
Initializing BASS (no soundcard mode)...
  BASS initialized (device=0, no soundcard)
  bass_aes67.dll loaded
  Clock mode set to: System (2)
  AES67 configured (interface=192.168.60.102, jitter=10ms, domain=1)

Creating AES67 input stream...
  Input stream created (source: 239.192.76.49:5004)

Waiting for System lock...
  System locked!

Creating AES67 output stream...
  Output stream created (dest: 239.192.1.100:5004, 5ms/200pkt/s)

==========================================
Loopback running (System sync):
  INPUT:  239.192.76.49:5004
  OUTPUT: 239.192.1.100:5004
==========================================
Press Ctrl+C to stop

IN: 24/10 rcv=1000 late=5 und=0 | OUT: pkt=1000 und=0 | System Clock (free-running) | STABLE
```

## Files to Create

| File | Purpose |
|------|---------|
| `Bass.Aes67/BassNative.cs` | BASS P/Invoke declarations |
| `Bass.Aes67/Aes67Config.cs` | AES67 config constants |
| `Bass.Aes67/RtpPacketBuilder.cs` | RTP packet construction |
| `Bass.Aes67/Aes67OutputStream.cs` | Output stream implementation |
| `Bass.Aes67/OutputStats.cs` | Statistics struct |
| `Aes67Loopback/Program.cs` | Main loopback example |

## Critical Constraints

1. **No Mutex in Audio Path**: Use lock-free atomics for stats
2. **High-Priority Thread**: Set thread priority for precise timing
3. **PTP Frequency Correction**: Apply PPM adjustment to send interval
4. **Native Memory**: Pin buffers for P/Invoke calls
5. **Clean Shutdown**: Handle Ctrl+C and free all resources

## References

- Rust example: `bass-aes67/examples/aes67_loopback.rs`
- Output stream: `bass-aes67/src/output/stream.rs`
- RTP builder: `bass-aes67/src/output/rtp.rs`
- Header file: `bass-aes67/bass_aes67.h`
