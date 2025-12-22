# SRT Reconnection Crash - Development Notes

## Problem Statement

The C# `srt_dotnet` application crashes with **SIGSEGV (exit code 139)** when the SRT sender is stopped. The application should survive sender disconnection and wait for reconnection.

## Current State

### Files Modified
- `/home/kennet/dev/BassAES67/BassAES67/bass-srt/src/input/stream.rs` - Main SRT stream handling
- `/home/kennet/dev/BassAES67/BassAES67/bass-srt/src/lib.rs` - Added SIGPIPE handling, connection state config
- `/home/kennet/dev/BassAES67/BassAES67/bass-srt/Cargo.toml` - Added `libc` dependency
- `/home/kennet/dev/BassAES67/BassAES67/srt_dotnet/Program.cs` - C# test app with reconnection handling
- `/home/kennet/dev/BassAES67/BassAES67/srt_dotnet/BassSrtNative.cs` - C# bindings with connection state

### Architecture

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  SRT Sender     │────▶│  bass-srt        │────▶│  srt_dotnet     │
│  (Listener)     │     │  (Rust plugin)   │     │  (C# app)       │
└─────────────────┘     └──────────────────┘     └─────────────────┘

Threads in bass-srt:
1. Receiver Thread - runs receiver_loop(), handles SRT, pushes to ring buffer
2. BASS Audio Thread - calls stream_proc(), reads from ring buffer
```

### Code Flow on Disconnect

1. Sender stops
2. `receive_from_socket()` gets error from `srt_recv()`, breaks loop
3. Function returns to caller loop in `receiver_loop()`
4. `srt_bindings::close(sock)` is called
5. `CONNECTION_STATE_RECONNECTING` is set
6. Sleep 500ms
7. Create new socket, try to connect again
8. **CRASH HAPPENS** somewhere in steps 2-4

## Failed Attempts (DO NOT REPEAT)

### Attempt 1: SIGPIPE Handling
**What was done:** Added `libc::signal(libc::SIGPIPE, libc::SIG_IGN)` in `init_plugin()`
**Result:** Did not fix the crash

### Attempt 2: catch_unwind around receive_from_socket
**What was done:** Wrapped the call in `std::panic::catch_unwind`
**Result:** Did not fix - SIGSEGV is not a Rust panic

### Attempt 3: Explicit Drop Ordering
**What was done:** Added explicit `drop(decoder)`, `drop(sample_buf)`, `drop(recv_buf)` at end of `receive_from_socket`
**Result:** Did not fix the crash

### Attempt 4: Custom Drop for AudioDecoder
**What was done:** Added `impl Drop for AudioDecoder` that explicitly drops inner decoder
**Result:** Did not fix the crash

### Attempt 5: Move Decoder Outside receive_from_socket
**What was done:**
- Moved `decoder`, `recv_buf`, `sample_buf` allocation to `receiver_loop()` (lines 453-458)
- Changed `receive_from_socket` signature to take `&mut` references instead of owning
- Decoder now lives for entire receiver_loop lifetime
**Result:** NOT TESTED PROPERLY - assistant falsely claimed success

## What We Know

1. **Crash type:** SIGSEGV (exit code 139) - segmentation fault, memory access violation
2. **Crash timing:** Intermittent (race condition), happens during/after `receive_from_socket` returns
3. **Codec used:** OPUS (user mentioned "opus" specifically)
4. **Debug output showed:** Crash happens between last line of `receive_from_socket` and return to caller

## What Needs Investigation

1. **Is it codec-related?** Test with PCM (no codec) to isolate
2. **Is it SRT library related?** Check if `srt_close()` or `srt_recv()` has issues
3. **Is it ring buffer related?** Unlikely but worth checking
4. **Is there stack corruption?** Something writing out of bounds

## Current Code State

### receive_from_socket signature (line 683-692)
```rust
fn receive_from_socket(
    sock: srt_bindings::SRTSOCKET,
    running: &Arc<AtomicBool>,
    _ended: &Arc<AtomicBool>,
    stats: &Arc<StreamStats>,
    producer: &mut ringbuf::HeapProd<f32>,
    config: &SrtUrl,
    decoder: &mut AudioDecoder,
    recv_buf: &mut Vec<u8>,
    sample_buf: &mut Vec<f32>,
)
```

### Caller loop in receiver_loop (lines 461-519)
- Creates socket, configures, connects
- Calls `receive_from_socket()` with borrowed decoder/buffers
- On return, closes socket, sets RECONNECTING state, sleeps, loops

### Connection states (lines 217-220)
```rust
pub const CONNECTION_STATE_DISCONNECTED: u32 = 0;
pub const CONNECTION_STATE_CONNECTING: u32 = 1;
pub const CONNECTION_STATE_CONNECTED: u32 = 2;
pub const CONNECTION_STATE_RECONNECTING: u32 = 3;
```

## How to Test

### Start receiver (C#)
```bash
cd /home/kennet/dev/BassAES67/BassAES67/srt_dotnet
dotnet run
```

### Start sender (Rust example)
```bash
cd /home/kennet/dev/BassAES67/BassAES67/bass-srt
./run_sender.sh opus   # or: pcm, mp2, flac
```

### Test reconnection
1. Start receiver
2. Start sender - should connect and play
3. Stop sender (Ctrl+C)
4. Observe: receiver should show "Reconnecting" and NOT exit
5. Restart sender - should reconnect

## Key Questions for New Session

1. Does crash happen with PCM codec (no libopus)?
2. What exact line/instruction causes the SIGSEGV?
3. Is memory being corrupted before the crash?
4. Are there thread safety issues between receiver thread and BASS audio thread?

## Lessons Learned

1. **DO NOT declare success without proper verification**
2. **Kill ALL sender processes before testing** - multiple senders cause confusion
3. **Timeout commands mask real test results**
4. **SIGSEGV requires memory debugging, not Rust-level fixes**

## Suggested Next Steps

1. Add proper debugging with GDB or valgrind
2. Test with PCM codec to isolate if libopus related
3. Check if issue is in SRT library during disconnect
4. Consider if ring buffer access during reconnection is safe
