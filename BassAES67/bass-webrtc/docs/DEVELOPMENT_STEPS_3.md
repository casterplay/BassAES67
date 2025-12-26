# BASS WebRTC Development Steps - Part 3

## Session Summary (December 26, 2024)

This session focused on implementing **room-based signaling** for the WebSocket signaling server, enabling multiple independent WebRTC sessions to share the same signaling infrastructure.

---

## What Was Accomplished

### 1. Room-Based Signaling (Rust)

**Problem:** The original signaling server broadcast messages to ALL connected clients, breaking when:
- Multiple browser clients connected simultaneously
- Multiple Rust peers used the same signaling server

**Solution:** Room-based message routing where:
- Room ID is extracted from WebSocket URL path: `ws://server:port/{room_id}`
- Messages only relay to clients in the SAME room
- Empty rooms are automatically cleaned up

**Files Modified:**

#### `src/signaling/ws_signaling_server.rs`
```rust
// Changed from flat client list to room-based structure
struct Room {
    clients: HashMap<ClientId, ClientSender>,
}

pub struct SignalingServer {
    port: u16,
    rooms: Arc<Mutex<HashMap<RoomId, Room>>>,  // room_id -> Room
    next_client_id: AtomicU64,
    running: Arc<AtomicBool>,
}

// Key methods:
pub fn room_count(&self) -> usize;
pub fn client_count(&self) -> usize;  // Total across all rooms

// Room extraction via tokio_tungstenite::accept_hdr_async()
// Parses room ID from request.uri().path()
```

#### `src/signaling/ws_peer.rs`
```rust
impl WebRtcPeer {
    pub fn new(
        signaling_url: &str,  // Base URL: "ws://127.0.0.1:8080"
        room_id: &str,        // Room: "studio-1"
        ice_servers: Vec<IceServerConfig>,
        sample_rate: u32,
        channels: u16,
        buffer_samples: usize,
    ) -> Self;

    pub fn room_id(&self) -> &str;
}

// connect() builds full URL: {signaling_url}/{room_id}
```

#### `src/lib.rs` (FFI)
```rust
#[no_mangle]
pub unsafe extern "system" fn BASS_WEBRTC_CreatePeer(
    signaling_url: *const c_char,
    room_id: *const c_char,        // NEW parameter
    source_channel: DWORD,
    sample_rate: u32,
    channels: u16,
    opus_bitrate: u32,
    buffer_ms: u32,
    decode_stream: u8,
) -> *mut c_void;
```

### 2. Room-Based Signaling (C#)

**File:** `bass-webrtc-dotnet/SignalingServer.cs`

Updated to match Rust implementation:
```csharp
public class SignalingServer : IDisposable
{
    // room_id -> (client_id -> RoomClient)
    private readonly ConcurrentDictionary<string, ConcurrentDictionary<Guid, RoomClient>> _rooms;

    public int RoomCount { get; }
    public int ClientCount { get; }  // Total across all rooms
    public int GetRoomClientCount(string roomId);

    // Events now include roomId:
    public event Action<Guid, string>? OnClientConnected;     // clientId, roomId
    public event Action<Guid, string>? OnClientDisconnected;  // clientId, roomId
    public event Action<Guid, string, string>? OnMessageReceived;  // clientId, roomId, message
}
```

Room ID extracted from `context.Request.Url.AbsolutePath`.

### 3. Example & HTML Client Updates

#### `examples/webrtc_bidirectional.rs`
```bash
# New CLI argument
webrtc_bidirectional --port 8080 --room studio-1

# Prints instructions showing room ID
```

#### `examples/test_client_websocket.html`
- Added "Room ID" input field in UI
- WebSocket URL constructed as: `{baseUrl}/{roomId}`
- Logs show room connection info

---

## Current Architecture

```
┌─────────────────┐         WebSocket          ┌─────────────────┐
│     Browser     │◄─────────────────────────►│   Signaling     │
│  (room: abc)    │     ws://.../abc          │    Server       │
├─────────────────┤                            │  (Rust or C#)   │
│  WebRTC Peer    │                            │                 │
└────────┬────────┘                            └────────┬────────┘
         │                                              │
         │              Direct P2P (RTP)                │
         │◄════════════════════════════════════════════►│
         │                                              │
┌────────┴────────┐                            ┌────────┴────────┐
│   bass-webrtc   │◄─────────────────────────►│     Browser     │
│   (Rust peer)   │     ws://.../abc          │  (room: abc)    │
│  (room: abc)    │                            │                 │
└─────────────────┘                            └─────────────────┘

Multiple sessions use different rooms:
- Browser A + Rust Peer A → room "session1"
- Browser B + Rust Peer B → room "session2"
Messages stay isolated within each room.
```

---

## Files Structure

```
bass-webrtc/
├── src/
│   ├── lib.rs                    # FFI exports (BASS_WEBRTC_CreatePeer updated)
│   ├── signaling/
│   │   ├── mod.rs
│   │   ├── ws_signaling_server.rs  # Room-based signaling server
│   │   ├── ws_peer.rs              # WebRTC peer with room_id
│   │   ├── whip_client.rs          # WHIP client (unchanged)
│   │   ├── whep_client.rs          # WHEP client (unchanged)
│   │   └── callback.rs
│   ├── codec/
│   │   ├── mod.rs
│   │   └── opus.rs               # OPUS encoder/decoder
│   ├── peer/                     # WebRTC peer management
│   ├── stream/                   # Audio stream handling
│   └── ice.rs                    # ICE server configs
├── examples/
│   ├── webrtc_bidirectional.rs   # Main test example (--room support)
│   ├── test_client_websocket.html # Browser client (room ID input)
│   └── ...
└── docs/
    ├── DEVELOPMENT_STEPS.md
    ├── DEVELOPMENT_STEPS_2.md
    └── DEVELOPMENT_STEPS_3.md    # This file

bass-webrtc-dotnet/               # Rename pending → bass-webrtc-signaling-dotnet
├── SignalingServer.cs            # C# signaling server with room support
├── Program.cs                    # Example console app
└── bass-webrtc-dotnet.csproj
```

---

## Working Features

1. **Bidirectional WebRTC Audio**
   - Rust ↔ Browser audio streaming
   - 48kHz stereo OPUS codec
   - Works with browser reconnection

2. **Room-Based Signaling**
   - Multiple sessions on same server
   - Automatic room cleanup
   - URL-based room selection

3. **Reconnection Support**
   - Browser can disconnect/reconnect
   - Fresh WebRtcPeer created per connection
   - ICE candidate queuing for race conditions

4. **WHIP/WHEP Clients**
   - One-way streaming to/from media servers (MediaMTX)
   - 24/7 mode with auto-reconnection

---

## Next Steps (For Future Sessions)

### 1. C# Bindings for bass-webrtc

Need to create P/Invoke bindings for the Rust library:

```csharp
// Proposed API
public class BassWebRtc
{
    [DllImport("bass_webrtc")]
    public static extern IntPtr BASS_WEBRTC_CreatePeer(
        string signalingUrl,
        string roomId,
        uint sourceChannel,
        uint sampleRate,
        ushort channels,
        uint opusBitrate,
        uint bufferMs,
        byte decodeStream);

    [DllImport("bass_webrtc")]
    public static extern int BASS_WEBRTC_PeerConnect(IntPtr peer);

    [DllImport("bass_webrtc")]
    public static extern int BASS_WEBRTC_PeerDisconnect(IntPtr peer);

    [DllImport("bass_webrtc")]
    public static extern int BASS_WEBRTC_PeerIsConnected(IntPtr peer);

    [DllImport("bass_webrtc")]
    public static extern void BASS_WEBRTC_PeerFree(IntPtr peer);

    // DataChannel support
    [DllImport("bass_webrtc")]
    public static extern int BASS_WEBRTC_PeerSendData(
        IntPtr peer, string channel, byte[] data, uint len);
}
```

### 2. Video Support Ideas (User's "Crazy Ideas")

Potential video features to discuss:
- WebRTC video tracks alongside audio
- Screen sharing integration
- Video codec selection (VP8, VP9, H264)
- Video ↔ BASS integration (if applicable)
- NDI/SDI capture sources

### 3. Folder Rename

The folder `bass-webrtc-dotnet` should be renamed to `bass-webrtc-signaling-dotnet` to clarify it's a signaling-only server (no WebRTC library needed).

Currently locked - try:
```cmd
# Close VS Code first, then:
ren bass-webrtc-dotnet bass-webrtc-signaling-dotnet
```

---

## Build Commands

```bash
# Build bass-webrtc library
cd bass-webrtc
cargo build --release

# Build example
cargo build --release --example webrtc_bidirectional

# Run example
cargo run --release --example webrtc_bidirectional -- --port 8080 --room test

# Build C# signaling server
cd bass-webrtc-dotnet
dotnet build

# Run C# signaling server
dotnet run 8080
```

---

## Testing Workflow

1. Start the Rust example:
   ```bash
   cargo run --release --example webrtc_bidirectional -- --room myroom
   ```

2. Open `examples/test_client_websocket.html` in browser

3. Enter:
   - WebSocket URL: `ws://localhost:8080`
   - Room ID: `myroom`

4. Click Connect - audio should flow bidirectionally

5. Test reconnection: Refresh browser, reconnect - should work

---

## Key Lessons Learned

1. **ICE Candidate Queuing**: Candidates can arrive before remote description is set - must queue them

2. **Fresh Peers Per Connection**: Reusing WebRtcPeer after browser disconnects causes issues - create fresh peer each time

3. **tokio::select! for Non-Blocking**: Use `select!` with timeout to check connection state while waiting for WebSocket messages

4. **OPUS Application Mode**: Must use correct constant (2049 for OPUS_APPLICATION_AUDIO, not arbitrary values)

5. **Room-Based Signaling**: Essential for multiple simultaneous sessions - extract room from URL path
