# AES67 Loopback Audio Crackling - Development Session 6

## Problem Statement
AES67 loopback example has audio crackling and silent gaps (0.1-0.5 sec intervals).

## System Architecture
```
Axia/Livewire (PTP Domain 1)
        │
        ▼  239.192.76.52:5004 @ 48kHz stereo, 5ms packets (200 pkt/sec)
┌───────────────────┐
│  bass_aes67.dll   │
│  INPUT STREAM     │  ← Receives AES67, stores in ring buffer
│  (stream.rs)      │    Uses PI controller for adaptive resampling
└────────┬──────────┘
         │ BASS decode channel (handle)
         ▼
┌───────────────────┐
│  OUTPUT STREAM    │  ← Pulls via BASS_ChannelGetData
│  (output/stream.rs)│   Sends at PTP-corrected intervals
└────────┬──────────┘
         │
         ▼  239.192.1.100:5004 @ 48kHz stereo, 5ms packets
   xNode/Destination
```

## Key Files
- `bass-aes67/src/input/stream.rs` - Input stream with PI controller (MODIFY THIS)
- `bass-aes67/src/output/stream.rs` - Output stream (DO NOT MODIFY - proven working)
- `bass-aes67/examples/aes67_loopback.rs` - Test example

## What Works
- `file_to_aes67.rs` example works perfectly (output is proven)
- PTP synchronization locks correctly
- Packet reception works
- Basic audio passes through

## Current Issue
Buffer level swings wildly and occasionally hits critical low, causing underruns:
- Target buffer: ~250 packets (500ms × 48kHz / 1000)
- Buffer drops to 48-58 before underrun
- Recovery mode kicks in but cycle repeats

## Root Cause Analysis
The output stream pulls samples at PTP-corrected rate. When PTP frequency is +4 ppm (local clock slow), output sends packets 4 ppm FASTER = consumes more samples from input buffer.

The input PI controller must adjust resampling ratio to match actual consumption rate.

## Changes Applied So Far

### 1. Removed PTP Correction from Input (✓ Done)
Input packets arrive at constant 48kHz from Axia (PTP-locked at source).
No need to apply local PTP correction to consumption.
```rust
// Before: let resample_ratio = ptp_base + trim_clamped;
// After:
let resample_ratio = 1.0 + trim_clamped;  // Pure PI control, no PTP
```

### 2. Preserved Integral Error Across Recovery (✓ Done)
Was resetting integral_error to 0.0 when exiting recovery mode, causing oscillation.
```rust
// Line 297: DON'T reset integral_error
// Line 310: DON'T reset integral_error
```

### 3. Increased PI Gains (✓ Just Applied)
```rust
// Current values in stream.rs lines 328-330:
const KP: f64 = 0.0001;   // P: -50 ppm at half-empty buffer
const KI: f64 = 0.0002;   // I: 4x stronger than original
const MAX_TRIM_PPM: f64 = 100.0;  // ±100 ppm headroom
```

## PI Controller Theory

### How It Works (stream.rs lines 319-337)
```rust
let error = (available - target) / target;  // Normalized: -1 to +1
let trim = KP * error + KI * integral_error;
let resample_ratio = 1.0 + trim;
```

### Direction (Verified Correct)
- Buffer LOW → error negative → trim negative → ratio < 1.0 → consume SLOWER → buffer FILLS
- Buffer HIGH → error positive → trim positive → ratio > 1.0 → consume FASTER → buffer DRAINS

### Integral Accumulation (Verified Correct)
- `integral_error += error` on each call (accumulates, not replaced)
- Only reset on stream `start()` (correct)
- NOT reset in recovery mode (fixed)

## Test Results History
1. Original: 13+ underruns in 30 seconds
2. After integral-reset fix: 7 underruns in 85 seconds (improved)
3. After KI=0.0002, MAX_TRIM=100ppm: **26 underruns in ~125 seconds** (WORSE!)
   - Test 1: 26 underruns after 24000+ packets sent
   - Test 2: 20 underruns after 25600+ packets sent
   - Buffer stabilizes around 166-236/500 when PTP is stable (+3 ppm)
   - BUT buffer still swings wildly during PTP frequency changes
   - Pattern: PTP frequency varies from -6 to +7 ppm during calibration

## Key Observations from Latest Tests
- When PTP frequency is STABLE (around +3-4 ppm), buffer stays stable at ~170-230/500
- Problems occur when PTP frequency CHANGES rapidly (e.g., from +7 to +3 ppm)
- The PI controller CAN track steady-state drift, but responds too slowly to rapid changes
- Underruns cluster when PTP is actively calibrating/adjusting

## Next Steps
1. The PI controller is fundamentally fighting the output's PTP corrections
2. Consider: Should the INPUT also apply PTP correction in the SAME direction as output?
   - Output: `interval_factor = 1.0 - (ppm / 1e6)` → sends FASTER when ppm > 0
   - Input should: consume FASTER when ppm > 0 to match output demand
   - Current: Input uses pure PI without PTP, so it's always catching up
3. Alternative: Increase PI gains even more (but risk oscillation)
4. Alternative: Increase jitter buffer to 1000ms for more headroom

## Build Commands
```bash
cd "c:/Dev/Lab/BASS/bass-aes67"
cargo build --release
powershell -Command "Copy-Item 'target/release/bass_aes67.dll' 'target/release/examples/' -Force"
cargo build --release --example aes67_loopback
cd target/release/examples
./aes67_loopback.exe
```

## Success Criteria
- Buffer stays above 100/500 (20% of target)
- Underruns < 3 over 60 seconds after initial stabilization
- No audible crackling or gaps

## User Instructions (CRITICAL)
- "NEVER TOUCH the OUTPUT code!!!!" - Output stream works perfectly
- "stop overcomplicate things, stop Guessing!" - Focus on simple, targeted changes
- File `file_to_aes67.rs` proves output works - problem is input side

## Configuration (aes67_loopback.rs)
- Interface: 192.168.60.102
- PTP Domain: 1 (Livewire)
- Jitter buffer: 500ms
- Input: 239.192.76.52:5004
- Output: 239.192.1.100:5004, 5ms packets (200/sec)
