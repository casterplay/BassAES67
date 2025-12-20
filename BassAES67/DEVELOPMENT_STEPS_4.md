# Development Steps - AES67 Input PTP Synchronization (Session 4)

## Problem Summary
Audio "cracks" every 3-5 seconds in `aes67_loopback` example (AES67 IN → BASS → AES67 OUT).

## Critical Constraints
- **DO NOT** modify `bass-aes67/src/output/` - output is finalized and working
- **DO NOT** modify `bass-ptp/` - PTP mechanism is finalized
- **ONLY** modify bass-aes67 input side and aes67_loopback.rs example

## Current Architecture

### Data Flow
1. **Input receiver thread** → receives UDP multicast packets → pushes to ring buffer
2. **Output transmitter thread** → calls `BASS_ChannelGetData()` every 5ms (PTP-adjusted)
3. BASS invokes our `stream_proc` callback → reads from ring buffer
4. Output sends RTP packet

### Key Files
- `bass-aes67/src/input/stream.rs` - Input stream with lock-free ring buffer (MODIFIED)
- `bass-aes67/src/input/jitter.rs` - Old jitter buffer (now unused, can be removed)
- `bass-aes67/src/output/stream.rs` - Output stream (DO NOT MODIFY)
- `bass-aes67/examples/aes67_loopback.rs` - Test example

## What We Accomplished

### 1. Removed Mutex from Network Receiver
User pointed out mutex was blocking network receiver. Changed from:
```rust
// OLD - BAD
if let Ok(mut state) = state.lock() {
    state.jitter.push(&packet);
}
```
To using a **lock-free ring buffer** (`ringbuf` crate).

### 2. Implemented Lock-Free Ring Buffer
- Network thread writes to ring buffer (single producer)
- BASS callback reads from ring buffer (single consumer)
- No mutex contention

### 3. Tried PTP-Based Resampling
Attempted linear interpolation resampling based on PTP frequency correction:
```rust
let step = 1.0 - (ppm / 1_000_000.0);
// Interpolate between prev and curr samples
```
This did NOT solve the problem.

## Current Symptom
**Output consumes samples ~24% faster than input provides them.**

Test logs show:
```
rcv=3074 packets received (input)
pkt=3703 packets sent (output)
```
After ~15 seconds, that's ~205 pkt/sec input vs ~247 pkt/sec output.
This is impossible if both are 200 pkt/sec - something is fundamentally wrong.

## Theories

### Theory 1: Output Timing Bug
The output uses `std::time::Instant` for timing, which is based on local system clock.
PTP frequency correction adjusts the interval, but maybe there's accumulation error.
However, PTP shows only ±4 ppm correction, which can't explain 24% drift.

### Theory 2: BASS Decode Mode Behavior
BASS is initialized with `BASS_Init(0, ...)` (no soundcard) and streams use `BASS_STREAM_DECODE`.
In this mode, BASS doesn't have a "clock" - it only processes data when `BASS_ChannelGetData()` is called.
Maybe something about how BASS processes decode streams is causing extra callbacks?

### Theory 3: Use bass-ptp Timer
User mentioned bass-ptp has a **PTP-synchronized timer** (`ptp_timer_start`, etc.).
Perhaps the correct architecture is to use this timer to drive both input consumption and output transmission at exactly PTP time.

## bass-ptp Timer API (from ptp_bindings.rs)
```rust
pub type PtpTimerCallback = unsafe extern "C" fn(*mut c_void);

pub fn ptp_timer_start(
    interval_ms: u32,
    callback: Option<PtpTimerCallback>,
    user: *mut c_void,
) -> Result<(), i32>

pub fn ptp_timer_stop()
pub fn ptp_timer_is_running() -> bool
pub fn ptp_timer_set_interval(interval_ms: u32) -> Result<(), i32>
pub fn ptp_timer_set_pll(enabled: bool)
```

## Key Configuration
- Interface: 192.168.60.102 (AoIP network)
- Input: 239.192.76.52:5004 (Livewire source)
- Output: 239.192.1.100:5004
- Packet rate: 200 packets/sec (5ms intervals)
- Sample rate: 48kHz
- Channels: 2 (stereo)
- Format: 24-bit audio (converted to float internally)
- PTP domain: 1 (Livewire standard)
- Jitter buffer: 150ms target

## Current Code State

### stream.rs (input)
Uses lock-free ring buffer:
- `ringbuf::HeapRb` for SPSC sample transfer
- Receiver thread pushes samples (no mutex)
- `read_samples()` uses simple `pop_slice()` (no resampling)
- Statistics tracked with `AtomicU64`

### Cargo.toml
Added `ringbuf = "0.4"` as main dependency.

## Next Steps to Try

1. **Debug the packet rate mismatch**
   - Add timing instrumentation to output loop to verify actual packet rate
   - Check if `BASS_ChannelGetData` is being called more than once per output packet

2. **Try using bass-ptp timer**
   - Restructure example to use `ptp_timer_start()` for timing
   - Have timer callback drive both input read and output transmission

3. **Investigate BASS decode mode**
   - Verify stream_proc is called exactly once per `BASS_ChannelGetData` call
   - Check if there's any buffering/caching behavior we're not accounting for

4. **Consider alternative architecture**
   - Maybe bypass BASS entirely for the loopback case?
   - Direct ring buffer from input receiver to output transmitter?

## Build Commands
```bash
cd "c:/Dev/Lab/BASS/bass-aes67"
cargo build --release --example aes67_loopback
cp "c:/Dev/Lab/BASS/bass-aes67/target/release/bass_aes67.dll" "c:/Dev/Lab/BASS/bass-aes67/target/release/examples/bass_aes67.dll"
cd "c:/Dev/Lab/BASS/bass-aes67/target/release/examples"
./aes67_loopback.exe
```

## Test Environment
- Livewire network device sending AES67 on 239.192.76.52:5004
- xNode or similar receiving output on 239.192.1.100:5004
- PTP grandmaster: 0050c2fffe901131 domain 1

## User's Key Insights
1. "Mutex in network receiver is a NO-GO" - Led to lock-free implementation
2. "BASS has NO soundcard, BASS is in DECODER mode - it can't pull data" - Output drives timing
3. "bass-ptp is our creation, not done by BASS" - Custom PTP library with timer support
4. "bass-ptp has a time event that you use" - Suggests using ptp_timer_start for synchronization

## Files to Reference
- `bass-aes67/src/ptp_bindings.rs` - PTP timer API definitions (lines 380-460)
- `bass-aes67/src/output/stream.rs` - Output timing loop (lines 254-340)
- `bass-aes67/src/input/stream.rs` - Input implementation (current working version)
