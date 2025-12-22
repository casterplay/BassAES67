# BASS AES67 Development - Phase 2: PTP Sync Adjustments

## Summary

The `cpal_output.rs` example now plays AES67 audio successfully, but periodic crackling occurs every 10-15 seconds due to clock drift between the PTP-synchronized network stream and the local soundcard.

---

## What Was Accomplished

### Working Architecture: "Direct Mode"

```
AES67 Network (PTP clock) → bass_aes67 plugin → BASS decode stream
                                                        ↓
                                              cpal audio callback
                                                        ↓
                                              Soundcard (local clock)
```

**Key insight**: cpal's audio callback directly calls `BASS_ChannelGetData()` instead of using a timer + ring buffer. This ensures the read rate exactly matches the playback rate.

### Previous Failed Approaches

1. **Timer → Ring Buffer → cpal**: Timer ran faster than network delivered, draining buffer
2. **Larger buffers**: Didn't help - still drained eventually
3. **Loop drain**: Filled to 100% but data was zeros (jitter buffer pads with silence)

### Why Direct Mode Works

- cpal's audio thread requests exactly what it needs for playback
- No rate mismatch between reading and playing
- BASS_ChannelGetData returns the actual bytes (jitter buffer may pad with silence)

---

## Current Configuration

```rust
// BASS settings
BASS_Init(0, 48000, 0, ...)           // No soundcard mode
BASS_CONFIG_BUFFER = 100              // 100ms internal buffer
BASS_CONFIG_UPDATEPERIOD = 0          // Disabled (we drive manually)

// AES67 settings
BASS_CONFIG_AES67_INTERFACE = "192.168.60.102"
BASS_CONFIG_AES67_JITTER = 50         // 50ms jitter buffer
BASS_CONFIG_AES67_PTP_DOMAIN = 10

// cpal settings
sample_rate = 48000
channels = 2
buffer_size = 960 samples (20ms)

// Stream URL
aes67://239.192.76.52:5004
```

---

## The Remaining Problem: Clock Drift

### Symptoms
- Audio plays perfectly for 10-15 seconds
- Brief crackling/glitches occur
- Returns to good audio
- Cycle repeats

### Root Cause
Two independent clocks are involved:
1. **Network clock (PTP)**: Drives the AES67 stream at exactly 48000 Hz
2. **Soundcard clock**: Local oscillator, approximately 48000 Hz but not exact

Over time, even a tiny frequency difference accumulates:
- If soundcard is 0.01% fast: consumes 4.8 extra samples/second
- After 10 seconds: 48 samples deficit = 1ms glitch

### Visualized

```
Time →
Network:   |----48000 samples----|----48000 samples----|
Soundcard: |---48005 samples---|---48005 samples---|    ← runs slightly fast
                                                   ↑
                                            Buffer underrun!
```

---

## Proposed Solution: Adaptive Resampling

### Option A: BASS_ATTRIB_FREQ Adjustment

Adjust the decode stream's sample rate based on buffer fill level:

```rust
// In monitoring loop or separate thread
let fill_percent = ring_buffer.len() as f32 / ring_buffer.capacity() as f32;

// Target 50% fill
let error = fill_percent - 0.5;

// Proportional adjustment (very small - 0.1% max)
let freq_adjustment = 1.0 + (error * 0.001);
let new_freq = (48000.0 * freq_adjustment) as u32;

BASS_ChannelSetAttribute(stream, BASS_ATTRIB_FREQ, new_freq);
```

**Pros**: Simple, uses existing BASS functionality
**Cons**: May introduce slight pitch variation

### Option B: Reintroduce Ring Buffer with Rate Control

Use a ring buffer between timer and cpal, but with adaptive fill control:

```rust
// Timer callback (PTP-synchronized)
fn timer_callback() {
    // Read from BASS at network rate
    BASS_ChannelGetData(stream, buffer, ...);
    ring_buffer.push(buffer);
}

// cpal callback
fn cpal_callback(output) {
    // Read from ring buffer at soundcard rate
    ring_buffer.pop(output);

    // Report fill level for rate adjustment
    FILL_LEVEL.store(ring_buffer.len(), Ordering::Relaxed);
}

// Control loop
fn rate_control() {
    let fill = FILL_LEVEL.load(Ordering::Relaxed);
    // Adjust timer interval or BASS_ATTRIB_FREQ
}
```

**Pros**: More control, can buffer network jitter
**Cons**: More complex, reintroduces synchronization challenges

### Option C: PTP-Aware cpal (Ideal but Complex)

Use bass_ptp's PLL-adjusted timer to drive a custom audio callback that compensates for drift:

```rust
// Timer set to slightly variable interval based on PTP offset
BASS_PTP_TimerSetPLL(1);  // Already enabled

// Timer callback feeds cpal-compatible buffer
// PLL adjustment keeps timer in sync with PTP master
```

---

## Key Files

| File | Purpose |
|------|---------|
| `bass-aes67/examples/cpal_output.rs` | Working example (direct mode) |
| `bass-aes67/src/input/jitter.rs` | Jitter buffer - always returns requested amount |
| `bass-aes67/src/input/stream.rs` | BASS stream integration |
| `bass-aes67/src/lib.rs` | Plugin entry points |
| `bass-ptp/` | PTP client with timer and PLL |

---

## Important Code Details

### cpal Direct Mode (current working solution)

```rust
// Global state
static STREAM_HANDLE: AtomicU64 = AtomicU64::new(0);
static DIRECT_MODE: AtomicBool = AtomicBool::new(false);

// cpal callback
move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
    if DIRECT_MODE.load(Ordering::Relaxed) {
        let stream = STREAM_HANDLE.load(Ordering::Relaxed) as DWORD;
        if stream != 0 {
            let bytes = unsafe {
                BASS_ChannelGetData(
                    stream,
                    data.as_mut_ptr() as *mut c_void,
                    (data.len() * 4) as DWORD | BASS_DATA_FLOAT,
                )
            };
            // bytes is always data.len() * 4 (BASS pads with zeros if needed)
        }
    }
}
```

### Jitter Buffer Behavior

The jitter buffer (`jitter.rs`) ALWAYS returns the requested number of samples:
- If packets available: returns audio data
- If packets missing: returns silence (zeros)
- This means `BASS_ChannelGetData` always reports "full" return

### Livewire Specifics

- Standard AES67: 1000 packets/sec (1ms intervals)
- Livewire: 200 packets/sec (5ms intervals)
- The jitter buffer handles both automatically
- 5ms packets = 240 samples per packet at 48kHz

---

## Next Steps

1. **Add buffer monitoring to cpal_output.rs**
   - Track actual jitter buffer fill level
   - Display in status output

2. **Implement BASS_ATTRIB_FREQ adjustment**
   - Start with Option A (simplest)
   - Add gradual frequency adjustment based on buffer level

3. **Test and tune**
   - Find optimal adjustment rate (too fast = oscillation, too slow = underruns)
   - Target: maintain 40-60% buffer fill

4. **Consider ring buffer approach if needed**
   - Option B provides more isolation
   - May be necessary if direct adjustment causes artifacts

---

## Build & Run

```bash
cd c:\Dev\Lab\BASS\bass-aes67

# Build
cargo build --example cpal_output

# Copy DLLs (if not already done)
copy ..\bass24\x64\bass.dll target\debug\examples\
copy ..\bass-ptp\target\debug\bass_ptp.dll target\debug\examples\
copy target\debug\bass_aes67.dll target\debug\examples\

# Run
cargo run --example cpal_output
```

---

## References

- BASS documentation: https://www.un4seen.com/doc/
- `BASS_ChannelGetData`: Returns requested bytes, pads with zeros if insufficient data
- `BASS_ATTRIB_FREQ`: Can adjust playback rate for decode streams
- PTP: Precision Time Protocol for network synchronization
- Livewire: Axia's AES67-compatible audio over IP protocol


## Issue (user report)

When running "cpal_output" example, the PTP status switches to "unlock" after a few minues. When that happens the audio starts crackeling a lot for 10-20 seconds, it recovers but have acational small cracks. PLL: do Not swich back to LOCKED! Audio is down to 91%, Under fixed on 0.
