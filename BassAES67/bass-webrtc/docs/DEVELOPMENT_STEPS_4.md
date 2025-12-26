# BASS WebRTC Development - Step 4: Statistics & Events Complete

## Session Summary

This session completed the WebRTC statistics feature with callback-based delivery, following the same architecture as the Connected/Disconnected/Error events.

---

## Completed Features

### 1. Event-Based Callbacks (Previous Session)
- `Connected` event - fires when WebRTC connection established
- `Disconnected` event - fires when connection lost (with double-fire prevention)
- `Error` event - fires on errors with code and message

### 2. WebRTC Statistics (This Session)
- `StatsUpdated` event - fires periodically with connection stats
- `EnableStats(intervalMs)` method to start stats collection

---

## Statistics Available

| Metric | Source | Description |
|--------|--------|-------------|
| RTT | RemoteInboundRTP | Round-trip time in milliseconds |
| Packet Loss % | RemoteInboundRTP | Fraction of packets lost |
| Packets Lost | RemoteInboundRTP | Total packets lost (negative = duplicates) |
| Jitter | InboundRTP | Variation in packet arrival time |
| Packets Sent | OutboundRTP | Total packets sent |
| Packets Received | InboundRTP | Total packets received |
| Bytes Sent | OutboundRTP | Total bytes sent |
| Bytes Received | InboundRTP | Total bytes received |
| NACK Count | InboundRTP | Retransmission requests |
| Bitrate (calculated) | C# wrapper | Send/receive bitrate in kbps |

---

## Files Modified

### Rust Side

**bass-webrtc/src/signaling/ws_peer.rs**
```rust
// Expanded stats struct
pub struct WebRtcPeerStats {
    pub packets_sent: AtomicU64,
    pub packets_received: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,
    pub round_trip_time_ms: AtomicU32,  // NEW
    pub packets_lost: AtomicI64,         // NEW
    pub fraction_lost: AtomicU32,        // NEW
    pub jitter_ms: AtomicU32,            // NEW
    pub nack_count: AtomicU64,           // NEW
}

// FFI-safe snapshot
pub struct WebRtcPeerStatsSnapshot { ... }

// Stats callback type
pub type OnStatsCallback = unsafe extern "C" fn(
    stats: *const WebRtcPeerStatsSnapshot,
    user: *mut c_void
);
```

**bass-webrtc/src/lib.rs**
```rust
// FFI stats struct
#[repr(C)]
pub struct WebRtcPeerStatsFFI {
    pub packets_sent: u64,
    pub packets_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub round_trip_time_ms: u32,
    pub packets_lost: i64,
    pub fraction_lost_percent: f32,
    pub jitter_ms: u32,
    pub nack_count: u64,
}

// New FFI function
BASS_WEBRTC_PeerSetStatsCallback(handle, callback, interval_ms, user) -> i32

// WebRtcPeerWrapper additions
on_stats: Option<OnStatsCallback>,
stats_user: *mut c_void,
stats_interval_ms: u32,
stats_running: Arc<AtomicBool>,

// Stats loop spawned in BASS_WEBRTC_PeerSetupStreams
// Uses pc.get_stats() to collect WebRTC stats
```

### C# Side

**webrtc_dotnet/BassWebRtcNative.cs**
```csharp
[StructLayout(LayoutKind.Sequential)]
public struct WebRtcPeerStatsFFI { ... }

[UnmanagedFunctionPointer(CallingConvention.StdCall)]
public delegate void OnStatsCallback(ref WebRtcPeerStatsFFI stats, IntPtr user);

[DllImport("bass_webrtc")]
public static extern int BASS_WEBRTC_PeerSetStatsCallback(...);
```

**webrtc_dotnet/BassWebRtcPeer.cs**
```csharp
public class WebRtcStats {
    public ulong PacketsSent { get; init; }
    public ulong PacketsReceived { get; init; }
    public ulong BytesSent { get; init; }
    public ulong BytesReceived { get; init; }
    public TimeSpan RoundTripTime { get; init; }
    public long PacketsLost { get; init; }
    public float PacketLossPercent { get; init; }
    public TimeSpan Jitter { get; init; }
    public ulong NackCount { get; init; }
    public double SendBitrateKbps { get; internal set; }
    public double ReceiveBitrateKbps { get; internal set; }
}

public event Action<WebRtcStats>? StatsUpdated;
public bool EnableStats(uint intervalMs = 1000);
```

### Browser Side

**bass-webrtc/examples/test_client_websocket.html**
- Added stats panel with 8 metrics (RTT, jitter, loss, packets, bytes, bitrate)
- Color-coded indicators (green/yellow/red based on quality thresholds)
- Auto-updates every 1 second when connected
- Uses browser's `pc.getStats()` API

---

## Usage Example (C#)

```csharp
var peer = new BassWebRtcPeer(
    signalingUrl: "ws://localhost:8080",
    roomId: "studio-1",
    sourceChannel: toBrowserChan,
    decodeStream: true
);

peer.Connected += () =>
{
    Console.WriteLine("Connected!");
    peer.SetupStreams();
    peer.EnableStats(1000); // Stats every 1 second

    int fromBrowserChan = peer.InputStreamHandle;
    BassMix.BASS_Mixer_StreamAddChannel(mixer, fromBrowserChan, BASSFlag.BASS_STREAM_AUTOFREE);
};

peer.StatsUpdated += stats =>
{
    Console.WriteLine($"RTT: {stats.RoundTripTime.TotalMilliseconds:F1}ms");
    Console.WriteLine($"Loss: {stats.PacketLossPercent:F2}%");
    Console.WriteLine($"Bitrate: {stats.SendBitrateKbps:F0} kbps");
};

peer.Disconnected += () =>
{
    Console.WriteLine("Disconnected - recreating peer...");
    peer.Dispose();
    Thread.Sleep(500);
    DoPeerConnect(); // Reconnect
};

peer.Error += (code, msg) => Console.WriteLine($"Error {code}: {msg}");

peer.Connect();
```

---

## Architecture Highlights

### Stats Collection Flow
1. C# calls `EnableStats(1000)` after connection
2. `BASS_WEBRTC_PeerSetStatsCallback` stores callback and interval
3. `start_stats_loop()` spawns async task on Tokio runtime
4. Task calls `pc.get_stats()` every interval
5. Stats extracted from `StatsReportType` variants
6. `WebRtcPeerStatsFFI` struct passed to callback
7. C# delegate converts to `WebRtcStats` and fires event

### Send Safety for Callbacks
- Raw `*mut c_void` pointers are not `Send`
- Solution: Cast to `usize` before spawning async task, cast back in callback
```rust
let user_usize = wrapper.stats_user as usize;
// In async block:
cb(&ffi_stats, user_usize as *mut c_void);
```

### Lifecycle Management
- Stats loop controlled by `AtomicBool` flag
- Stopped in `BASS_WEBRTC_PeerDisconnect` and `BASS_WEBRTC_PeerFree`
- Automatically started if callback registered before connection

---

## Current State

### Working
- Bidirectional WebRTC audio (browser <-> C#)
- Event-based callbacks (Connected, Disconnected, Error, StatsUpdated)
- Auto-reconnection on browser refresh
- Room-based signaling isolation
- WebRTC statistics collection and display

### Project Structure
```
bass-webrtc/
├── src/
│   ├── lib.rs                    # FFI exports, WebRtcPeerWrapper
│   ├── signaling/
│   │   ├── ws_peer.rs            # WebRTC peer with stats, callbacks
│   │   ├── ws_signaling_server.rs # Rust signaling server
│   │   └── ...
│   └── stream/
│       ├── input.rs              # Browser -> BASS
│       └── output.rs             # BASS -> Browser
├── examples/
│   └── test_client_websocket.html # Browser test client with stats
└── docs/
    └── DEVELOPMENT_STEPS_4.md    # This file

webrtc_dotnet/
├── BassWebRtcNative.cs           # P/Invoke declarations
├── BassWebRtcPeer.cs             # High-level wrapper with events
├── BassWebRtcSignalingServer.cs  # Signaling server wrapper
└── Program.cs                    # Test program
```

---

## Bug Fixes

### Half-Speed Audio When Using Mono (`channels: 1`)

**Problem:** When setting `channels: 1` in `BassWebRtcPeer`, audio played at half speed in the browser.

**Root Cause:** The BASS source channel is always stereo (2 channels), but when `channels: 1` is passed, the OPUS encoder was configured for mono. The code read samples based on the mono encoder's frame size, but BASS returned stereo samples (2x the expected data), causing half-speed playback.

**File Modified:** `bass-webrtc/src/stream/output.rs`

**Fix:** Always read stereo from BASS source, then downmix to mono when needed:

```rust
// IMPORTANT: Source BASS channel is always stereo (2 channels)
// We need to read stereo samples and downmix to mono if channels == 1
let source_channels: usize = 2; // BASS source is always stereo
let source_samples_per_frame = (sample_rate as usize * FRAME_DURATION_MS as usize / 1000) * source_channels;
let bytes_per_frame = source_samples_per_frame * 4; // 4 bytes per float sample

// Audio buffer for BASS_ChannelGetData (always stereo)
let mut source_buffer = vec![0.0f32; source_samples_per_frame];
// Audio buffer for encoder (mono or stereo depending on channels)
let mut audio_buffer = vec![0.0f32; samples_per_frame];

// ... after reading from BASS ...

// Convert from stereo source to encoder format
if channels == 1 {
    // Downmix stereo to mono: average L+R
    let mono_samples = source_samples_per_frame / 2;
    for i in 0..mono_samples {
        let left = source_buffer[i * 2];
        let right = source_buffer[i * 2 + 1];
        audio_buffer[i] = (left + right) * 0.5;
    }
} else {
    // Stereo: copy directly
    audio_buffer.copy_from_slice(&source_buffer);
}
```

**Usage:** Now works correctly with mono encoding:

```csharp
peer = new BassWebRtcPeer(
    signalingUrl: "ws://localhost:8080",
    roomId: "studio-1",
    sourceChannel: toBrowserChan,
    decodeStream: true,
    channels: 1,        // ✓ Now works correctly - proper stereo-to-mono downmix
    opusBitrate: 64     // 64 kbps (good for voice/talk radio)
);
```

---

## Next Steps: Video Support

The user mentioned "crazy video idea" for the next session. Potential directions:

1. **Add video track support** to WebRTC peer
2. **Screen sharing** from browser to C#
3. **Video encoding/decoding** integration
4. **MediaMTX integration** for video streaming

The current architecture with callbacks and stats provides a solid foundation for adding video support.

---

## Build Commands

```bash
# Build Rust library
cd bass-webrtc
cargo build --release

# Build C# project
cd webrtc_dotnet
dotnet build

# Run test
dotnet run
# Open test_client_websocket.html in browser
```

---

## Key Files for Next Session

| File | Purpose |
|------|---------|
| `bass-webrtc/src/lib.rs` | FFI exports, add video functions here |
| `bass-webrtc/src/signaling/ws_peer.rs` | WebRTC peer, add video tracks here |
| `webrtc_dotnet/BassWebRtcPeer.cs` | C# wrapper, add video events/methods |
| `webrtc_dotnet/BassWebRtcNative.cs` | P/Invoke, add video FFI declarations |
| `examples/test_client_websocket.html` | Browser client, add video elements |
