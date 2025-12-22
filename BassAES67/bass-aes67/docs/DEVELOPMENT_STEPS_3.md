# Development Steps 3: AES67 Input PTP Synchronization

## Context

This continues from DEVELOPMENT_STEPS_2.md where we successfully implemented PTP synchronization for **AES67 OUTPUT**. Now we need to apply similar synchronization to the **AES67 INPUT** side.

## What Was Accomplished (Output Side)

### Problem Solved
- PTP offset calculation was producing wrong values due to TAI vs Unix epoch mismatch
- PI controller was causing frequency to grow unboundedly
- Needed stable frequency compensation for packet timing

### Solution Implemented

1. **Relative Offset Tracking** (`bass-ptp/src/client.rs:500-507`)
   - First measurement becomes baseline (includes epoch diff + path delay)
   - All subsequent offsets are relative to baseline
   - This cancels out the ~37 second TAI/Unix epoch difference

2. **Drift Rate Servo** (`bass-ptp/src/servo.rs`)
   - Uses linear regression over 32-sample sliding window
   - Calculates drift rate (ns/s = ppb) from offset history
   - Low-pass filters result (alpha=0.1)
   - Output frequency = negated drift rate (to compensate)
   - Lock state based on drift rate being < 50 ppm

3. **Output Stream Timing** (`bass-aes67/src/output/stream.rs:281-299`)
   - Reads frequency from PTP servo via `ptp_get_frequency_ppm()`
   - Adjusts packet interval: `interval_factor = 1.0 - (ppm / 1_000_000.0)`
   - Positive ppm = send faster, Negative ppm = send slower

### Test Results
- `file_to_aes67` stays LOCKED
- Frequency stabilizes around +5-7 ppm
- Offset bounded in ±500µs range (jitter, not drift)

## The Input Side Challenge

### Current Architecture
```
AES67 Network → JitterBuffer → BASS Stream → cpal Soundcard
     ^                                            ^
     |                                            |
  PTP clock                              Free-running clock
  (48000 Hz exact)                       (48000 Hz ± drift)
```

### The Problem
- Network audio arrives at exact 48000 Hz (PTP synchronized)
- Local soundcard runs at approximately 48000 Hz (free-running crystal)
- Over time, soundcard consumes samples faster/slower than they arrive
- Result: Buffer underrun (soundcard faster) or overflow (soundcard slower)

### Where Compensation is Needed

**Option A: Jitter Buffer Level Feedback**
- Monitor jitter buffer fill level
- If buffer growing → soundcard is slow → need to consume faster
- If buffer shrinking → soundcard is fast → need to consume slower
- Requires resampling at playback side

**Option B: Use PTP Frequency for Resampling**
- PTP servo already knows the drift rate
- Apply same frequency adjustment to resampler
- Simpler but assumes local clock drift is consistent

**Option C: Combined Approach**
- Use PTP frequency as feed-forward term
- Use buffer level as feedback correction
- Most robust but more complex

## Key Files to Examine

### Input Module
- `bass-aes67/src/input/mod.rs` - Module structure
- `bass-aes67/src/input/stream.rs` - AES67 input stream
- `bass-aes67/src/input/jitter.rs` - Jitter buffer implementation
- `bass-aes67/src/input/rtp.rs` - RTP packet parsing

### Examples
- `bass-aes67/examples/aes67_loopback.rs` - AES67 IN → BASS → AES67 OUT
- `bass-aes67/examples/aes67_to_soundcard.rs` - AES67 IN → cpal output (if exists)

### PTP Module (Already Working)
- `bass-ptp/src/servo.rs` - Drift rate measurement
- `bass-ptp/src/client.rs` - PTP message handling
- `bass-aes67/src/ptp_bindings.rs` - FFI to bass_ptp.dll

## Implementation Considerations

### For aes67_loopback (AES67 IN → AES67 OUT)
- Both input and output use same PTP clock
- In theory, NO resampling needed
- Just need proper jitter buffer management
- May still need small compensation for processing delays

### For Soundcard Output (aes67_to_soundcard)
- This is where real resampling is needed
- Need to measure soundcard actual rate vs PTP rate
- Apply variable-rate resampling

### Resampling Options
1. **rubato** crate - High quality, used by many audio apps
2. **libsamplerate** FFI - Battle-tested C library
3. **Simple linear interpolation** - Low quality but simple
4. **Custom polyphase** - Best quality, most work

## Questions to Clarify

1. Which use case to focus on first?
   - `aes67_loopback` (simpler, no resampling)
   - Soundcard output (requires resampling)

2. For soundcard output, what quality level?
   - Broadcast quality (rubato/libsamplerate)
   - Acceptable quality (simpler algorithm)

3. Current state of `aes67_loopback.rs`?
   - Does it work at all currently?
   - What problems does it exhibit?

## API Already Available

```rust
// From bass_ptp.dll (already implemented)
pub fn ptp_get_frequency_ppm() -> f64;  // Returns servo frequency output
pub fn ptp_is_locked() -> bool;          // Returns lock state

// These can be used by input side for compensation
```

## Suggested Implementation Order

1. **Test current aes67_loopback** - See what happens without any changes
2. **Add monitoring** - Display buffer levels, packet timing, PTP stats
3. **Identify the drift** - Is buffer growing or shrinking over time?
4. **Implement compensation** - Based on what we observe

## Notes

- The PTP servo outputs frequency in ppm (parts per million)
- At 48000 Hz, 1 ppm = 0.048 samples/second drift
- At 100 ppm, that's 4.8 samples/second or ~288 samples/minute
- Over 10 minutes = 2880 samples = 60ms of drift

## Files to Read at Session Start

Tell Claude to read these files:
1. `DEVELOPMENT_STEPS_3.md` (this file)
2. `bass-aes67/examples/aes67_loopback.rs`
3. `bass-aes67/src/input/stream.rs`
4. `bass-aes67/src/input/jitter.rs`
5. `bass-ptp/src/servo.rs` (for reference on how servo works)
