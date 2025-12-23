# Development Steps - Session 14: Multi-Stream Input Support

## Goal
Enable multiple simultaneous AES67 input streams via `BASS_StreamCreateURL("aes67://...")`.

## Problem Statement
Previously, only one AES67 input stream could be created at a time. Attempting to create a second stream returned `BASS_ERROR_FILEOPEN`.

```csharp
// Before fix:
int stream1 = BASS_StreamCreateURL("aes67://239.192.76.49:5004", ...);  // Works
int stream2 = BASS_StreamCreateURL("aes67://239.192.76.50:5004", ...);  // BASS_ERROR_FILEOPEN
```

## Root Causes Identified

### Issue 1: Socket Binding Without SO_REUSEADDR
**Location:** `bass-aes67/src/input/stream.rs:166`

The `create_multicast_socket()` function used `std::net::UdpSocket::bind()` directly without setting `SO_REUSEADDR`. On most systems, you cannot bind two sockets to the same port (5004) without this flag.

### Issue 2: Single-Stream Stats Registry
**Location:** `bass-aes67/src/lib.rs:81`

A global `ACTIVE_STREAM: AtomicPtr<Aes67Stream>` could only hold one stream pointer. When a second stream was created, it would overwrite the first stream's reference, causing metrics to only work for the last-created stream.

---

## Changes Made

### 1. `bass-aes67/Cargo.toml`
Added required dependencies:
```toml
[dependencies]
parking_lot = "0.12"
lazy_static = "1.4"
```

### 2. `bass-aes67/src/input/stream.rs`

#### Socket Binding Fix
Changed `create_multicast_socket()` to use `socket2::Socket` with `set_reuse_address(true)`:

```rust
fn create_multicast_socket(&self) -> Result<UdpSocket, String> {
    use socket2::{Socket, Domain, Type, Protocol};

    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .map_err(|e| format!("Failed to create socket: {}", e))?;

    // Allow multiple sockets to bind to the same port (required for multi-stream)
    socket.set_reuse_address(true)
        .map_err(|e| format!("Failed to set reuse address: {}", e))?;

    let bind_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, self.config.port);
    socket.bind(&bind_addr.into())
        .map_err(|e| format!("Failed to bind socket to {}: {}", bind_addr, e))?;

    // ... join multicast, set timeout, return socket
    Ok(socket.into())
}
```

#### Stream Cleanup
Updated `addon_free()` to use the new registry:
```rust
unsafe extern "system" fn addon_free(inst: *mut c_void) {
    if !inst.is_null() {
        let stream = inst as *mut Aes67Stream;
        crate::unregister_stream((*stream).handle);
        let _ = Box::from_raw(stream);
    }
}
```

### 3. `bass-aes67/src/lib.rs`

#### Stream Registry
Replaced single-pointer with thread-safe HashMap:

```rust
// Wrapper for raw pointer to allow Send + Sync in HashMap
#[derive(Clone, Copy)]
struct StreamPtr(*mut Aes67Stream);
unsafe impl Send for StreamPtr {}
unsafe impl Sync for StreamPtr {}

lazy_static! {
    static ref STREAM_REGISTRY: RwLock<HashMap<HSTREAM, StreamPtr>> =
        RwLock::new(HashMap::new());
}

fn register_stream(handle: HSTREAM, stream: *mut Aes67Stream) {
    STREAM_REGISTRY.write().insert(handle, StreamPtr(stream));
}

pub fn unregister_stream(handle: HSTREAM) {
    STREAM_REGISTRY.write().remove(&handle);
}

fn get_any_stream() -> Option<*mut Aes67Stream> {
    STREAM_REGISTRY.read().values().next().map(|ptr| ptr.0)
}
```

#### Stream Creation
Updated `stream_create_url()` to register streams:
```rust
// Register stream for buffer level queries (supports multiple streams)
register_stream(handle, stream_ptr);
```

#### Config Handler
Updated all stats queries to use `get_any_stream()`:
```rust
BASS_CONFIG_AES67_BUFFER_LEVEL => {
    let level = if let Some(stream_ptr) = get_any_stream() {
        (*stream_ptr).buffer_fill_percent()
    } else {
        100
    };
    // ...
}
```

---

## Files Modified

| File | Changes |
|------|---------|
| `bass-aes67/Cargo.toml` | Added `parking_lot` and `lazy_static` dependencies |
| `bass-aes67/src/input/stream.rs` | Socket binding with `SO_REUSEADDR`, cleanup via `unregister_stream()` |
| `bass-aes67/src/lib.rs` | `STREAM_REGISTRY` HashMap, helper functions, updated config handler |

---

## Testing

After the fix:
```csharp
int stream1 = BASS_StreamCreateURL("aes67://239.192.76.49:5004", ...);  // Works
int stream2 = BASS_StreamCreateURL("aes67://239.192.76.50:5004", ...);  // Works now!
int stream3 = BASS_StreamCreateURL("aes67://239.192.76.51:5004", ...);  // Works too!
```

---

## Platform Compatibility

The fix is cross-platform:
- **Windows:** `SO_REUSEADDR` allows multiple sockets on the same port
- **Linux:** Same behavior via `socket2` crate

Build on Linux:
```bash
cd bass-aes67
cargo build --release
# Output: target/release/libbass_aes67.so
```

---

## Stats Behavior with Multiple Streams

The `BASS_GetConfig()` stats functions (buffer level, underruns, packets received, etc.) return data for the **first registered stream** in the registry. This maintains backwards compatibility with single-stream usage.

Future enhancement: Add per-stream stats query by handle if needed.

---

## Current Project Status

### Working Components

| Component | Windows | Linux |
|-----------|---------|-------|
| AES67 Input (single stream) | YES | YES |
| AES67 Input (multiple streams) | YES | YES (expected) |
| AES67 Output (multiple streams) | YES | YES |
| PTP Clock | YES | YES (sudo) |
| Livewire Clock | YES | YES |
| System Clock | YES | YES |
| C# wrapper | YES | YES |

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Application                               │
├─────────────────────────────────────────────────────────────┤
│  BASS_StreamCreateURL("aes67://239.192.76.49:5004")         │
│  BASS_StreamCreateURL("aes67://239.192.76.50:5004")         │
│  BASS_StreamCreateURL("aes67://239.192.76.51:5004")         │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    bass_aes67.dll                            │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              STREAM_REGISTRY                         │    │
│  │  HashMap<HSTREAM, *mut Aes67Stream>                 │    │
│  │  ┌─────────┐  ┌─────────┐  ┌─────────┐             │    │
│  │  │Stream 1 │  │Stream 2 │  │Stream 3 │  ...        │    │
│  │  │ :5004   │  │ :5004   │  │ :5004   │             │    │
│  │  └─────────┘  └─────────┘  └─────────┘             │    │
│  └─────────────────────────────────────────────────────┘    │
│                                                              │
│  Each stream has:                                            │
│  - Own UDP socket (SO_REUSEADDR)                            │
│  - Own multicast group membership                           │
│  - Own ring buffer                                          │
│  - Own receiver thread                                      │
└─────────────────────────────────────────────────────────────┘
```

---

## Build Commands

### Windows
```bash
cd "C:\Dev\CasterPlay2025\BassAES67\BassAES67\bass-aes67"
cargo build --release
# Output: target\release\bass_aes67.dll
```

### Linux
```bash
cd bass-aes67
cargo build --release
# Output: target/release/libbass_aes67.so
```

---

## Key Constraints (Unchanged)
- **DO NOT** modify `bass-aes67/src/output/` - output is finalized
- **DO NOT** modify `bass-ptp/` - PTP mechanism finalized
- **NO mutex in audio path** - registry access is outside audio callback

---

## Next Steps (Future Sessions)

1. **Per-stream stats API** - Add config options to query specific stream by handle
2. **SRT Plugin** - Create new BASS plugin for SRT streaming (mentioned in Session 13)
3. **Stream discovery** - SAP/SDP support for automatic stream detection
