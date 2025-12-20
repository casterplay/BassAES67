# Development Steps - AES67 Input Adaptive Resampling (Session 5)

## Problem Summary
Audio has "crackling and gaps" in `aes67_loopback` example after 10-15 seconds of playback.
The input stream now uses a lock-free ring buffer, but buffer level drifts over time causing underruns.

## Critical Constraints (UNCHANGED)
- **DO NOT** modify `bass-aes67/src/output/` - output is finalized and working
- **DO NOT** modify `bass-ptp/` - PTP mechanism is finalized
- **ONLY** modify `bass-aes67/src/input/stream.rs` and examples

## Current Architecture

### Data Flow
1. **Input receiver thread** → receives UDP multicast packets → pushes to ring buffer (lock-free)
2. **Output transmitter thread** → calls `BASS_ChannelGetData()` every 5ms (PTP-adjusted interval)
3. BASS invokes our `stream_proc` callback → reads from ring buffer via `read_samples()`
4. Output sends RTP packet

### Key Files
- `bass-aes67/src/input/stream.rs` - Input stream with adaptive resampling (MAIN FILE TO MODIFY)
- `bass-aes67/src/output/stream.rs` - Output stream (DO NOT MODIFY) - has PTP steering
- `bass-aes67/examples/aes67_loopback.rs` - Test example

## What We Tried This Session

### 1. PI Controller (Buffer Level Feedback Only)
```rust
const KP: f64 = 0.0001;  // Proportional gain
const KI: f64 = 0.00001; // Integral gain
const MAX_TRIM_PPM: f64 = 10.0;

let error = (available - target) / target;
self.integral_error += error;
let trim = KP * error + KI * self.integral_error;
let resample_ratio = 1.0 + trim.clamp(-MAX_PPM/1e6, MAX_PPM/1e6);
```
**Result**: Buffer stable at ~245/500, only 2 underruns in 35 seconds. "Very good for a while, then crackling."

### 2. PTP-Only Resampling
Applied PTP frequency correction directly to consumption rate:
```rust
let ptp_ppm = ptp_get_frequency_ppm();
let resample_ratio = 1.0 + (ptp_ppm / 1_000_000.0);
```
**Result**: Buffer still drained. Problem: sign was wrong, and PTP ppm swings during calibration caused wild ratio changes. "Really BAD pitch changes!"

### 3. Smoothed Ratio (EMA)
Added exponential moving average to smooth ratio changes:
```rust
const SMOOTH_ALPHA: f64 = 0.01;
self.smoothed_ratio = smoothed * (1.0 - SMOOTH_ALPHA) + target * SMOOTH_ALPHA;
```
**Result**: WORSE - 17 underruns. Smoothing was too slow to respond to buffer level changes.

### 4. PTP Compensation with PI Trim
Theory: BASS pulls samples at local clock rate, input arrives at PTP rate.
When local clock is +5ppm fast, we need to slow down consumption:
```rust
let ptp_ppm = ptp_get_frequency_ppm();
let ptp_correction = 1.0 - (ptp_ppm / 1_000_000.0);  // Compensate for local clock drift
let resample_ratio = ptp_correction + PI_trim;
```
**Result**: Only 2 underruns but still had periodic crackling after 10-15 seconds.

## Key Insights

### The Core Problem
- **Input packets** arrive at PTP-synchronized rate (from Axia hardware)
- **Output** applies PTP steering: `interval = base_interval * (1.0 - ppm/1e6)`
- **BASS** pulls samples based on how often output calls `BASS_ChannelGetData()`
- The output's PTP steering means it sends at PTP-synced rate
- BUT input consumption rate isn't matching due to subtle timing issues

### PTP Frequency Meaning
When `ptp_get_frequency_ppm()` returns +5.0:
- Local clock is 5ppm FASTER than PTP master
- Output compensates by shortening interval (sends faster relative to local clock)
- Net effect: output sends at PTP-correct rate

### Why Simple PI Worked Best
The simple PI controller (KP=0.0001, KI=0.00001) achieved the most stable results because:
1. It directly responds to buffer level (the actual problem symptom)
2. No dependency on PTP ppm which swings during calibration
3. Very gentle adjustment prevents oscillation

### Remaining Issue
After 10-15 seconds, the buffer still drifts to critical level causing an underrun.
This suggests there's a small systematic drift that the PI controller can't fully compensate.

## Current Code State (stream.rs read_samples)

```rust
pub fn read_samples(&mut self, buffer: &mut [f32]) -> usize {
    let available = self.consumer.occupied_len();
    let is_buffering = self.buffering.load(Ordering::Relaxed);

    // Critical buffer protection thresholds
    let critical_threshold = self.target_samples / 20;  // 5%
    let recovery_threshold = self.target_samples / 2;   // 50%

    // Buffering/recovery mode
    if is_buffering {
        if available >= recovery_threshold {
            self.buffering.store(false, Ordering::Relaxed);
            self.integral_error = 0.0;
        } else {
            buffer.fill(0.0);
            return buffer.len();
        }
    }

    if available < critical_threshold {
        self.buffering.store(true, Ordering::Relaxed);
        self.integral_error = 0.0;
        buffer.fill(0.0);
        self.stats.underruns.fetch_add(1, Ordering::Relaxed);
        return buffer.len();
    }

    // Buffer error calculation
    let target = self.target_samples as f64;
    let error = (available as f64 - target) / target;

    // PTP compensation + PI trim (CURRENT IMPLEMENTATION)
    let ptp_ppm = crate::ptp_bindings::ptp_get_frequency_ppm();
    let ptp_correction = 1.0 - (ptp_ppm / 1_000_000.0);

    const KP: f64 = 0.00005;
    const KI: f64 = 0.000005;
    const MAX_TRIM_PPM: f64 = 5.0;

    self.integral_error += error;
    let max_integral = MAX_TRIM_PPM / KI / 1e6;
    self.integral_error = self.integral_error.clamp(-max_integral, max_integral);

    let trim = KP * error + KI * self.integral_error;
    let trim_clamped = trim.clamp(-MAX_TRIM_PPM / 1e6, MAX_TRIM_PPM / 1e6);

    let resample_ratio = ptp_correction + trim_clamped;

    // Linear interpolation resampling...
}
```

## Struct Fields
```rust
pub struct Aes67Stream {
    consumer: ringbuf::HeapCons<f32>,
    running: Arc<AtomicBool>,
    ended: Arc<AtomicBool>,
    receiver_thread: Option<JoinHandle<()>>,
    pub handle: HSTREAM,
    config: Aes67Url,
    stats: Arc<StreamStats>,
    target_samples: usize,
    buffering: AtomicBool,
    channels: usize,
    resample_pos: f64,
    prev_samples: Vec<f32>,
    curr_samples: Vec<f32>,
    resample_init: bool,
    integral_error: f64,
    smoothed_ratio: f64,  // Currently unused
}
```

## Things to Try Next

### 1. Wait for PTP to Stabilize Before Starting Audio
The PTP frequency correction swings wildly during the first 5-10 seconds of calibration (from 0 to +15ppm).
Perhaps wait until PTP offset is stable (<10µs variation) before starting output.

### 2. Increase PI Gains Slightly
The best result was with KP=0.0001, KI=0.00001. Try:
- KP=0.0002, KI=0.00002
- Or switch to pure PI without PTP correction

### 3. Use Different Approach: Bypass BASS for Loopback
Since BASS is only used as a passthrough (no effects/mixing), could potentially:
- Have input receiver push directly to a buffer
- Have output transmitter read from that buffer
- Remove BASS from the audio path entirely

### 4. Investigate the Output Timing
The output applies `interval_factor = 1.0 - (ppm / 1_000_000.0)` to shorten/lengthen intervals.
When PTP ppm is positive, intervals get shorter. Maybe the cumulative effect over time causes drift?

### 5. Consider a Simple Holdover Mode
Once PTP is locked and stable, record the current ppm and use that fixed value.
Don't follow real-time PTP ppm changes which may introduce instability.

## Build Commands
```bash
cd "c:/Dev/Lab/BASS/bass-aes67"
cargo build --release
powershell -Command "Copy-Item 'target/release/bass_aes67.dll' 'target/release/examples/' -Force"
cargo build --release --example aes67_loopback
cd target/release/examples
./aes67_loopback.exe
```

## Test Configuration
- Interface: 192.168.60.102 (AoIP network)
- Input: 239.192.76.52:5004 (Livewire source)
- Output: 239.192.1.100:5004
- Packet rate: 200 packets/sec (5ms intervals)
- Sample rate: 48kHz
- Channels: 2 (stereo)
- Format: 24-bit audio (converted to float)
- PTP domain: 1 (Livewire standard)
- Jitter buffer: 500ms (can reduce to 150ms once working)

## Key Observations from Logs

### Best Run (PI only, no PTP correction)
```
KIN: 245/500 rcv=2650 late=0 und=0 | OUT: pkt=2601 | Freq: +0.20ppm [LOCKED] | STABLE
```
Buffer stayed at ~245/500 (near target of 250), only 2 underruns after 35 seconds.

### With PTP Correction (worse)
Buffer oscillated between 20-295, 17 underruns. PTP ppm swung from +4 to +14 during calibration.

## Files for Reference
- `bass-aes67/src/input/stream.rs` - Main file to modify (lines 278-390 for read_samples)
- `bass-aes67/src/output/stream.rs` - Output timing (lines 264-276 for PTP adjustment)
- `bass-aes67/src/ptp_bindings.rs` - PTP API including `ptp_get_frequency_ppm()`
- `bass-aes67/examples/aes67_loopback.rs` - Test example

## User Feedback Summary
- "Better but small crackling" - First PI attempt
- "Still a lot of stumbling, gaps and crackling, when crackling I hear heavy pitch changes! Really BAD!" - PTP-only approach
- "That's way worse" - Smoothed ratio approach
- "Very good for a while, then crackling" - Simple PI without PTP
- "Crackles and gaps after 10-15 sec playback" - Final state with PTP correction + PI

## Critical Finding from Latest Run
The latest test log shows the ROOT CAUSE clearly:
- PTP frequency swings from 0 → +21 ppm during the first 10 seconds
- At +20 ppm, the input consumption is 20ppm slower than it should be
- This causes buffer to drain from 485 → 0 in about 10 seconds
- Once buffer hits 0, underruns climb rapidly (17 → 2146 over the run)

**The PTP correction sign is CORRECT** (`1.0 - ppm/1e6` slows consumption when local clock is fast), but the magnitude of change during calibration is too extreme.

## Suggested Fix for Next Session
**Wait for PTP to stabilize before applying correction:**
```rust
// Only apply PTP correction after frequency is stable
let ptp_ppm = ptp_get_frequency_ppm();
let ptp_stable = /* PTP locked for >10 seconds with low variance */;

let ptp_correction = if ptp_stable {
    1.0 - (ptp_ppm / 1_000_000.0)
} else {
    1.0  // No correction during calibration
};
```

Or better: Use only PI controller (no PTP correction) which worked best.

## Known Issues
1. **Ctrl+C hang**: Process hangs on termination, likely thread cleanup issue
2. **PTP calibration instability**: First 10 seconds have wild ppm swings (0 → +21 ppm)
3. **Buffer drain during calibration**: PTP correction causes massive buffer drain before stabilizing
