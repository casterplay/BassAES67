# Broadcast Audio Processor Implementation Plan

## Overview

This document outlines the design and implementation of a professional broadcast audio processor in Rust, similar to systems like Omnia 9/11 and Orban Optimod. The processor is designed for streaming and digital broadcast (DAB) applications, processing stereo PCM audio at 48kHz sample rate.

## Signal Flow Architecture

```
Input (48kHz Stereo PCM)
         ↓
    Input Gain
         ↓
  Pre-emphasis Filter (optional)
         ↓
    Wideband AGC
         ↓
  Multi-band Split (5 bands typical)
         ↓
    Per-band Processing:
    - Parametric EQ
    - Compressor
    - Limiter
         ↓
   Band Recombination
         ↓
   Final Limiter
         ↓
   Look-ahead Clipper
         ↓
   Loudness Metering
         ↓
   De-emphasis Filter (optional)
         ↓
    Output Gain
         ↓
Output (48kHz Stereo PCM)
```

## Processing Stages

### 1. Input Stage

**Purpose:** Normalize input levels and prepare signal for processing

**Components:**
- Input gain control (-20dB to +20dB)
- DC blocker (high-pass filter at 20Hz)
- Input metering (peak, RMS, LUFS)

**Implementation Notes:**
- Use double precision (f64) for gain calculations to avoid rounding errors
- DC blocker: 1st or 2nd order Butterworth high-pass at 20Hz
- Implement true peak metering according to ITU-R BS.1770

### 2. Pre-emphasis Filter (Optional)

**Purpose:** Emphasize high frequencies before processing to improve SNR

**Specifications:**
- Standard FM pre-emphasis curve (50µs or 75µs time constant)
- Only if targeting FM transmission
- For streaming/DAB, typically skipped

### 3. Wideband AGC (Automatic Gain Control)

**Purpose:** Establish consistent average loudness, handling large level variations

**Parameters:**
- Target level: -20 dBFS to -12 dBFS
- Attack time: 10-100 ms
- Release time: 100-2000 ms
- Ratio: 2:1 to 4:1 (gentle compression)
- Knee: soft (10-20 dB)

**Algorithm:**
- RMS or program-level detection
- Smooth gain reduction with exponential envelope followers
- Look-ahead buffer (5-10ms) to catch fast transients

**Implementation Notes:**
```rust
// Pseudo-structure
struct WidebandAGC {
    target_level: f32,
    attack_coeff: f32,
    release_coeff: f32,
    gain_reduction: f32,
    envelope: f32,
}
```

### 4. Multi-band Split

**Purpose:** Divide audio spectrum into frequency bands for independent processing

**Typical Band Configuration (5-band):**
1. **Sub-bass:** 20 Hz - 100 Hz
2. **Bass:** 100 Hz - 400 Hz
3. **Midrange:** 400 Hz - 2 kHz
4. **Presence:** 2 kHz - 8 kHz
5. **Brilliance:** 8 kHz - 20 kHz

**Crossover Design:**
- Use Linkwitz-Riley (4th order) or Linear Phase FIR filters
- Linkwitz-Riley ensures perfect reconstruction when bands are summed
- 24 dB/octave slopes typical

**Alternative: 8-band Configuration:**
- Provides more granular control
- Common split points: 50, 100, 250, 500, 1k, 2k, 4k, 8k Hz

**Implementation Considerations:**
- Linear phase FIR: no phase distortion, higher latency (128-512 taps per band)
- Linkwitz-Riley IIR: minimal latency, slight phase shifts (acceptable for broadcast)
- Process L and R channels independently or use M/S (Mid-Side) encoding

### 5. Per-band Parametric EQ

**Purpose:** Shape frequency response of each band before dynamics processing

**Parameters per Band:**
- Gain: -12 dB to +12 dB
- Q (bandwidth): 0.5 to 4.0
- Frequency: band-specific center frequency

**Common EQ Strategies:**
- Bass boost: +2 to +4 dB (increases perceived loudness)
- Midrange dip: -1 to -3 dB around 800Hz (reduces "boxiness")
- Presence lift: +2 to +4 dB around 3-5 kHz (increases clarity)
- High-frequency roll-off: gentle reduction above 10 kHz if needed

**Implementation:**
- Biquad filters (efficient, stable)
- Consider using bell/peaking filters for each band

### 6. Multi-band Compression

**Purpose:** Control dynamics independently per frequency band

**Parameters per Band:**
- Threshold: -30 dBFS to -10 dBFS
- Ratio: 2:1 to 10:1
- Attack: 0.5-10 ms (faster for higher frequencies)
- Release: 20-500 ms (slower for lower frequencies)
- Knee: soft or hard

**Band-specific Tuning Guidelines:**

**Sub-bass (20-100 Hz):**
- Ratio: 3:1 to 5:1
- Attack: 5-10 ms
- Release: 100-300 ms
- Purpose: Control rumble and prevent excessive bass energy

**Bass (100-400 Hz):**
- Ratio: 3:1 to 6:1
- Attack: 3-7 ms
- Release: 80-200 ms
- Purpose: Add punch while controlling muddiness

**Midrange (400-2000 Hz):**
- Ratio: 2:1 to 4:1
- Attack: 2-5 ms
- Release: 50-150 ms
- Purpose: Control vocal and instrument dynamics

**Presence (2-8 kHz):**
- Ratio: 3:1 to 6:1
- Attack: 0.5-3 ms
- Release: 30-100 ms
- Purpose: Enhance clarity and definition

**Brilliance (8-20 kHz):**
- Ratio: 3:1 to 8:1
- Attack: 0.3-1 ms
- Release: 20-80 ms
- Purpose: Control sibilance and add air

**Detection Methods:**
- RMS: smoother, more musical (50-100 ms window)
- Peak: faster, catches transients
- Program: hybrid approach, common in broadcast

**Implementation Notes:**
```rust
struct BandCompressor {
    threshold: f32,
    ratio: f32,
    attack_time: f32,
    release_time: f32,
    knee: f32,
    envelope_follower: f32,
    gain_reduction: f32,
}
```

### 7. Multi-band Limiting

**Purpose:** Prevent any band from exceeding maximum level

**Parameters per Band:**
- Threshold: -6 dBFS to -2 dBFS
- Ratio: 10:1 to ∞:1 (brick wall)
- Attack: 0.1-1 ms
- Release: 10-100 ms
- Look-ahead: 1-5 ms

**Implementation:**
- Very fast attack times (< 1 ms)
- Automatic makeup gain to compensate for limiting
- Look-ahead buffer prevents overshoots

### 8. Band Recombination

**Purpose:** Sum all processed frequency bands back into full spectrum stereo signal

**Considerations:**
- Phase coherence must be maintained (Linkwitz-Riley guarantees this)
- Gain compensation if using FIR filters
- Monitor for intersample peaks

### 9. Final Limiter (Wideband)

**Purpose:** Ensure combined signal doesn't exceed maximum level

**Parameters:**
- Threshold: -1 dBFS to -0.3 dBFS
- Ratio: ∞:1 (brick wall)
- Attack: 0.1-0.5 ms
- Release: 50-200 ms
- Look-ahead: 2-5 ms

**Implementation:**
- True peak limiting (4x oversampled detection)
- Soft knee to reduce distortion
- Program-dependent release

### 10. Look-ahead Clipper

**Purpose:** Hard limit to prevent any samples exceeding 0 dBFS

**Parameters:**
- Ceiling: -0.1 dBFS to 0 dBFS
- Look-ahead time: 1-3 ms
- Overshoot protection: enabled

**Clipping Strategies:**
- Hard clipping: simple but harsh
- Soft clipping: polynomial curve (tanh, atan)
- Multi-stage clipping: cascade of gentle clippers

**Implementation:**
```rust
fn soft_clip(sample: f32, threshold: f32) -> f32 {
    if sample.abs() <= threshold {
        sample
    } else {
        threshold * sample.signum() * (1.0 - (-((sample.abs() - threshold) / (1.0 - threshold)).abs()).exp())
    }
}
```

### 11. Loudness Metering & Target

**Purpose:** Monitor and control final loudness to meet broadcast standards

**Standards:**
- EBU R128: -23 LUFS (European broadcast)
- ATSC A/85: -24 LKFS (US broadcast)
- Streaming: -14 to -16 LUFS (Spotify, Apple Music)

**Measurements:**
- Integrated loudness (LUFS/LKFS)
- Momentary loudness (400 ms)
- Short-term loudness (3 seconds)
- True peak level (dBTP)
- Loudness range (LRA)

**Implementation:**
- ITU-R BS.1770-4 algorithm
- K-weighting filter
- Gating algorithm (absolute gate: -70 LUFS, relative gate: -10 LU)

### 12. De-emphasis Filter (Optional)

**Purpose:** Reverse pre-emphasis applied earlier

**Specifications:**
- Mirror of pre-emphasis curve
- Only if pre-emphasis was used

### 13. Output Stage

**Purpose:** Final gain adjustment and metering

**Components:**
- Output gain control
- Peak/RMS metering
- Dithering (if reducing bit depth)

## DSP Fundamentals Required

### Filters
- **Biquad filters:** 2nd-order IIR for EQ and simple crossovers
- **FIR filters:** Linear phase crossovers and brick-wall filtering
- **Butterworth/Chebyshev:** High-pass, low-pass, band-pass designs
- **Linkwitz-Riley:** 4th-order crossovers for perfect reconstruction

### Envelope Followers
- **Attack/Release coefficients:**
  ```rust
  attack_coeff = 1.0 - exp(-1.0 / (attack_time_ms * sample_rate / 1000.0))
  release_coeff = 1.0 - exp(-1.0 / (release_time_ms * sample_rate / 1000.0))
  ```

### Gain Calculation
- **dB to linear:** `gain_linear = 10.0^(gain_db / 20.0)`
- **Linear to dB:** `gain_db = 20.0 * log10(gain_linear)`

### Oversampling
- Required for true peak detection
- 4x oversampling typical
- Use polyphase FIR filters for upsampling/downsampling

### Look-ahead Buffering
- Circular buffer implementation
- Allows zero-latency response to transients
- Size: attack_time * sample_rate

## Rust Implementation Structure

### Recommended Crate Dependencies

```toml
[dependencies]
# DSP fundamentals
biquad = "0.4"          # Biquad filter implementations
rubato = "0.14"         # Sample rate conversion/oversampling
realfft = "3.3"         # FFT operations if needed

# Audio I/O (for testing)
cpal = "0.15"           # Cross-platform audio I/O
hound = "3.5"           # WAV file I/O

# Utilities
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

### Module Structure

```rust
src/
├── main.rs                      // CLI or application entry
├── processor.rs                 // Main processor orchestration
├── stages/
│   ├── mod.rs
│   ├── input.rs                 // Input stage with DC blocker
│   ├── wideband_agc.rs         // AGC implementation
│   ├── multiband_split.rs      // Crossover filters
│   ├── band_processor.rs       // Per-band EQ/comp/limit
│   ├── final_limiter.rs        // Wideband limiter
│   └── clipper.rs              // Look-ahead clipper
├── dsp/
│   ├── mod.rs
│   ├── filters.rs              // Filter implementations
│   ├── compressor.rs           // Compressor/limiter dynamics
│   ├── envelope.rs             // Envelope followers
│   └── metering.rs             // Level and loudness metering
├── config/
│   ├── mod.rs
│   └── presets.rs              // Preset configurations
└── utils/
    ├── mod.rs
    ├── buffer.rs               // Circular buffers, look-ahead
    └── conversion.rs           // dB conversion utilities
```

### Core Processor Structure

```rust
pub struct AudioProcessor {
    sample_rate: f32,
    
    // Processing stages
    input_stage: InputStage,
    wideband_agc: WidebandAGC,
    multiband_split: MultibandSplit,
    band_processors: Vec<BandProcessor>,
    final_limiter: FinalLimiter,
    clipper: Clipper,
    
    // Metering
    loudness_meter: LoudnessMeter,
    
    // Buffers
    processing_buffer: Vec<[f32; 2]>,  // Stereo buffer
}

impl AudioProcessor {
    pub fn new(sample_rate: f32, config: ProcessorConfig) -> Self {
        // Initialize all stages
    }
    
    pub fn process_block(&mut self, input: &[[f32; 2]], output: &mut [[f32; 2]]) {
        // 1. Input stage
        // 2. Wideband AGC
        // 3. Split into bands
        // 4. Process each band
        // 5. Recombine bands
        // 6. Final limiter
        // 7. Clipper
        // 8. Output
    }
    
    pub fn get_meters(&self) -> MeterData {
        // Return current metering values
    }
    
    pub fn set_preset(&mut self, preset: Preset) {
        // Apply preset configuration
    }
}
```

### Preset System

Create presets for different broadcast formats:

1. **Streaming Loud:** Aggressive compression, high loudness (-14 LUFS)
2. **Streaming Balanced:** Moderate processing, natural dynamics (-16 LUFS)
3. **FM Classic:** Traditional broadcast sound with presence peak
4. **DAB Clear:** Clean, transparent with focus on intelligibility
5. **Talk Radio:** Heavy compression, midrange focus
6. **Classical:** Minimal processing, wide dynamics

### Configuration Structure

```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct ProcessorConfig {
    pub wideband_agc: AGCConfig,
    pub num_bands: usize,
    pub band_frequencies: Vec<f32>,
    pub band_processors: Vec<BandConfig>,
    pub final_limiter: LimiterConfig,
    pub clipper: ClipperConfig,
    pub target_loudness: f32,  // LUFS
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BandConfig {
    pub eq_gain: f32,
    pub compressor: CompressorConfig,
    pub limiter: LimiterConfig,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CompressorConfig {
    pub threshold: f32,      // dBFS
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub knee_db: f32,
    pub makeup_gain: f32,
}
```

## Performance Considerations

### Real-time Processing
- Target latency: < 50 ms total
- Process in blocks of 64-512 samples for efficiency
- Use SIMD instructions where possible (consider `packed_simd` crate)

### Optimization Strategies
1. **Pre-calculate coefficients:** Don't recalculate filter coefficients every sample
2. **Use fixed-point math** for gain calculations if needed
3. **Profile:** Use `cargo flamegraph` to identify bottlenecks
4. **Parallel processing:** Consider processing bands in parallel with `rayon`
5. **Buffer management:** Minimize allocations, reuse buffers

### Memory Management
- Pre-allocate all buffers at initialization
- Use ring buffers for look-ahead
- Avoid heap allocations in processing loop

## Testing & Calibration

### Test Signals
1. **Sine waves:** Verify frequency response and THD
2. **White/pink noise:** Test spectral balance
3. **Impulse response:** Measure latency and transient response
4. **Music samples:** Varied genres for subjective evaluation
5. **Speech:** Especially for talk radio presets

### Measurements
- Frequency response (± 0.5 dB target)
- THD+N (< 0.1% typical)
- IMD (< 0.05%)
- Latency (measure and report)
- Peak vs. RMS levels
- LUFS conformance

### Calibration Procedure
1. Feed 1 kHz tone at -20 dBFS
2. Adjust AGC to maintain level
3. Verify band crossovers sum flat
4. Adjust per-band makeup gain
5. Set final limiter threshold
6. Verify loudness target is met with various program material

## Advanced Features (Future Enhancements)

### Stereo Enhancement
- M/S (Mid-Side) processing
- Stereo width control per band
- Phase correlation monitoring

### Dynamic EQ
- Frequency-dependent dynamic processing
- De-esser for vocal content
- Bass enhancer with dynamic frequency shift

### Multi-path Processing
- Separate paths for different content (speech vs. music detection)
- Automatic preset switching

### Metering & Monitoring
- Real-time visualization of:
  - Multi-band gain reduction
  - Spectrum analyzer (FFT)
  - Loudness history graph
  - Correlation meter

### Intelligent Features
- Program-dependent attack/release
- Adaptive threshold based on content
- Genre detection and optimization

## References

### Standards
- ITU-R BS.1770-4: Loudness measurement
- EBU R128: Loudness normalization and permitted maximum level
- AES17: Digital audio measurement
- ATSC A/85: Techniques for establishing and maintaining audio loudness

### Books & Papers
- "Audio Processes" by Udo Zölzer
- "Digital Audio Signal Processing" by Udo Zölzer
- "DAFX: Digital Audio Effects" by Udo Zölzer
- Orban white papers on broadcast processing
- Julius O. Smith III's online DSP books

### Tools for Testing
- ffmpeg with `ebur128` filter for loudness analysis
- Audacity for visual inspection
- Room EQ Wizard for frequency response
- REAPER with SWS extensions for metering

## Implementation Roadmap

### Phase 1: Core DSP (Week 1-2)
- [ ] Basic filter implementations (biquad, crossover)
- [ ] Envelope followers and gain smoothing
- [ ] Simple compressor/limiter
- [ ] Level metering

### Phase 2: Multi-band Processing (Week 3-4)
- [ ] Linkwitz-Riley crossover implementation
- [ ] Per-band EQ
- [ ] Per-band compression
- [ ] Band recombination

### Phase 3: Advanced Processing (Week 5-6)
- [ ] Wideband AGC
- [ ] Final limiter with look-ahead
- [ ] Soft clipping with oversampling
- [ ] LUFS metering (ITU-R BS.1770)

### Phase 4: Integration & Presets (Week 7-8)
- [ ] Complete signal chain integration
- [ ] Preset system
- [ ] Configuration file format
- [ ] Real-time parameter adjustment

### Phase 5: Testing & Optimization (Week 9-10)
- [ ] Performance profiling and optimization
- [ ] Extensive testing with various audio content
- [ ] Calibration and tuning
- [ ] Documentation

### Phase 6: Polish (Week 11-12)
- [ ] CLI interface or GUI
- [ ] File processing mode
- [ ] Real-time processing mode
- [ ] Export metering data

## Getting Started

### Minimal Working Example

Start with this basic structure and build up:

```rust
// Simple 2-band processor as proof of concept
struct SimpleProcessor {
    sample_rate: f32,
    lowpass: BiquadFilter,
    highpass: BiquadFilter,
    bass_comp: Compressor,
    treble_comp: Compressor,
}

impl SimpleProcessor {
    pub fn process_sample(&mut self, input: f32) -> f32 {
        // Split into 2 bands
        let low = self.lowpass.process(input);
        let high = self.highpass.process(input);
        
        // Compress each band
        let low_processed = self.bass_comp.process(low);
        let high_processed = self.treble_comp.process(high);
        
        // Sum
        low_processed + high_processed
    }
}
```

Once this works, expand to 5+ bands, add EQ, limiters, AGC, etc.

## Conclusion

Building a broadcast audio processor is a complex but rewarding project that combines DSP theory, real-time programming, and audio engineering. Start simple with a 2-band proof of concept, then gradually add complexity. Focus on one stage at a time, test thoroughly, and tune carefully for the best sound quality.

The key to a great-sounding processor is not just implementing the algorithms correctly, but carefully tuning the parameters for musical and transparent processing. Listen critically and compare to reference processors like Omnia and Optimod.

Good luck with your implementation!