# Bass Broadcast Processor - Development Session 1

## Session Summary

This session implemented a **flexible N-band multiband audio processor** for the BASS audio library. The processor sits in the pipeline: `BASS-AES67_IN → AUDIO_PROCESSOR → BASS-AES67_OUT`.

---

## What Was Built

### Phase 1: 2-Band MVP (Completed Previously)
- Basic 2-band LR4 crossover processor
- Direct sample-by-sample processing (no ring buffer)
- Bypass mode with `BASS_Processor_SetBypass()`
- Processing time measurement in stats
- Lock-free atomic statistics for real-time metering

### Phase 2: Flexible N-Band Processor (This Session)
- Expanded architecture to support any number of bands (2, 5, 8, etc.)
- Uses `Vec`-based dynamic storage for crossovers and compressors
- New FFI API: `BASS_MultibandProcessor_*` functions
- Backward compatible: original 2-band API unchanged

---

## Project Structure

```
bass_broadcast_processor/
├── Cargo.toml
├── src/
│   ├── lib.rs                    # FFI exports for both 2-band and N-band
│   ├── ffi/
│   │   ├── mod.rs
│   │   └── bass.rs               # BASS types and imports
│   ├── processor/
│   │   ├── mod.rs                # BroadcastProcessor (2-band)
│   │   ├── multiband.rs          # MultibandProcessor (N-band) [NEW]
│   │   ├── config.rs             # ProcessorConfig, MultibandConfig [UPDATED]
│   │   └── stats.rs              # AtomicStats, MultibandAtomicStats [UPDATED]
│   └── dsp/
│       ├── mod.rs
│       ├── biquad.rs             # Butterworth biquad filter
│       ├── crossover.rs          # LR4 2-band crossover
│       ├── multiband.rs          # N-band crossover [NEW]
│       ├── compressor.rs         # Envelope follower + compression
│       └── gain.rs               # dB/linear conversions
├── examples/
│   ├── file_to_speakers.rs       # 2-band test
│   └── file_to_speakers_multiband.rs  # 5-band test [NEW]
└── docs/
    ├── PLAN.md                   # Full implementation plan (13-stage broadcast processor)
    └── DEVELOPMENT_STEPS_1.md    # This file
```

---

## Key Technical Decisions

### 1. No Ring Buffer - Direct Processing
**Problem**: Initial ring buffer approach caused choppy/crackled audio.
**Solution**: Removed ring buffer, process samples directly in STREAMPROC callback.
**Result**: Clean audio, simpler architecture.

### 2. Zero-Latency Sample-by-Sample IIR Processing
All filters (crossover, compressor envelope) use IIR design:
- Linkwitz-Riley 4th order crossover = two cascaded Butterworth biquads
- Peak envelope follower for compressor dynamics
- No look-ahead buffers (latency is only BASS internal buffers)

### 3. Lock-Free Statistics
All metering uses `AtomicU64`/`AtomicI32` with `Ordering::Relaxed`:
- Input/output peak levels (scaled to i32 for atomic ops)
- Per-band gain reduction
- Processing time in microseconds
- Underrun count

### 4. Flexible N-Band Architecture
- `MultibandCrossover` uses `Vec<LR4Crossover>` (N-1 crossovers for N bands)
- `MultibandProcessor` uses `Vec<Compressor>` (one per band)
- FFI uses fixed-size header + pointers to variable-length arrays

---

## FFI API Reference

### Original 2-Band API (Backward Compatible)
```c
void* BASS_Processor_Create(DWORD source, ProcessorConfig* config);
HSTREAM BASS_Processor_GetOutput(void* handle);
BOOL BASS_Processor_GetStats(void* handle, ProcessorStats* stats);
BOOL BASS_Processor_SetLowBand(void* handle, CompressorConfig* config);
BOOL BASS_Processor_SetHighBand(void* handle, CompressorConfig* config);
BOOL BASS_Processor_SetGains(void* handle, float input_db, float output_db);
BOOL BASS_Processor_SetBypass(void* handle, BOOL bypass);
BOOL BASS_Processor_Reset(void* handle);
BOOL BASS_Processor_Prefill(void* handle);
BOOL BASS_Processor_Free(void* handle);
BOOL BASS_Processor_GetDefaultConfig(ProcessorConfig* config);
```

### New N-Band API
```c
void* BASS_MultibandProcessor_Create(
    DWORD source,
    MultibandConfigHeader* header,
    float* crossover_freqs,       // num_bands - 1 elements
    CompressorConfig* bands       // num_bands elements
);
HSTREAM BASS_MultibandProcessor_GetOutput(void* handle);
BOOL BASS_MultibandProcessor_GetStats(
    void* handle,
    MultibandStatsHeader* header_out,
    float* band_gr_out            // num_bands elements
);
BOOL BASS_MultibandProcessor_SetBand(void* handle, DWORD band, CompressorConfig* config);
BOOL BASS_MultibandProcessor_SetBypass(void* handle, BOOL bypass);
BOOL BASS_MultibandProcessor_SetGains(void* handle, float input_db, float output_db);
BOOL BASS_MultibandProcessor_Reset(void* handle);
BOOL BASS_MultibandProcessor_Prefill(void* handle);
BOOL BASS_MultibandProcessor_Free(void* handle);
DWORD BASS_MultibandProcessor_GetNumBands(void* handle);
```

---

## Data Structures

### MultibandConfigHeader (FFI - Fixed Size)
```c
struct MultibandConfigHeader {
    uint32_t sample_rate;     // 48000
    uint16_t channels;        // 2
    uint16_t num_bands;       // 2, 5, 8, etc.
    uint8_t decode_output;    // 0=playable, 1=decode only
    uint8_t _pad[3];
    float input_gain_db;
    float output_gain_db;
};
```

### CompressorConfig (Per-Band)
```c
struct CompressorConfig {
    float threshold_db;       // -40 to 0
    float ratio;              // 1.0 to 10.0+
    float attack_ms;          // 0.5 to 100
    float release_ms;         // 10 to 1000
    float makeup_gain_db;     // 0 to 20
};
```

### MultibandStatsHeader (FFI - Fixed Size)
```c
struct MultibandStatsHeader {
    uint64_t samples_processed;
    float input_peak;         // linear, 0.0 to 1.0+
    float output_peak;
    uint32_t num_bands;
    uint64_t underruns;
    uint64_t process_time_us;
    // band_gr_db returned in separate array
};
```

---

## Standard Presets

### 5-Band Broadcast (Default)
| Band | Frequency | Threshold | Ratio | Attack | Release | Makeup |
|------|-----------|-----------|-------|--------|---------|--------|
| Sub-bass | < 100 Hz | -24 dB | 4:1 | 10ms | 200ms | +3 dB |
| Bass | 100-400 Hz | -20 dB | 5:1 | 5ms | 150ms | +4 dB |
| Midrange | 400-2000 Hz | -18 dB | 3:1 | 3ms | 100ms | +3 dB |
| Presence | 2000-8000 Hz | -16 dB | 4:1 | 1ms | 80ms | +4 dB |
| Brilliance | > 8000 Hz | -14 dB | 5:1 | 0.5ms | 50ms | +2 dB |

### Crossover Frequencies
- 2-band: 400 Hz
- 5-band: 100, 400, 2000, 8000 Hz
- 8-band: 60, 150, 400, 1000, 2500, 5000, 10000 Hz

---

## Performance Measurements

| Configuration | Processing Time | Bypass Time |
|---------------|-----------------|-------------|
| 2-band | ~0.5-1.0 ms | ~0.1 ms |
| 5-band | ~0.7-1.0 ms | ~0.1 ms |

Processing time is measured per STREAMPROC callback (~19200 samples at 48kHz stereo).

---

## Lessons Learned

1. **Ring buffers add complexity without benefit** for this use case. Direct processing in STREAMPROC is simpler and glitch-free.

2. **LR4 crossovers guarantee perfect reconstruction** when bands are summed. Essential for multiband processing.

3. **Atomic operations are sufficient** for real-time metering. No mutex needed in audio path.

4. **Vec-based dynamic storage** works well for flexible band counts. Slight allocation overhead at creation, but no runtime cost.

5. **Processing time scales sub-linearly** with band count (5-band is not 2.5x slower than 2-band).

---

## What's Next (From PLAN.md)

### Phase 3: Advanced Processing
- [ ] Wideband AGC (before multiband split)
- [ ] Final wideband limiter with look-ahead
- [ ] Soft clipping with oversampling
- [ ] LUFS metering (ITU-R BS.1770)

### Phase 4: Integration & Presets
- [ ] Complete signal chain integration
- [ ] Preset system (JSON serialization)
- [ ] Configuration file format
- [ ] Real-time parameter adjustment UI

### Future Enhancements
- Per-band parametric EQ
- Stereo enhancement (M/S processing)
- Dynamic EQ / de-esser
- Multi-path processing (speech vs music detection)

---

## Build & Test Commands

```bash
# Build release
cd C:\Dev\CasterPlay2025\BassAES67\BassAES67\bass_broadcast_processor
cargo build --release

# Run 2-band test
cargo run --example file_to_speakers --release

# Run 5-band test
cargo run --example file_to_speakers_multiband --release

# Run tests
cargo test
```

---

## Dependencies

- **No external crates** for DSP (zero-latency sample-by-sample)
- `windows-sys` for Windows threading (optional)
- BASS audio library (external DLL)

---

## Files Modified This Session

| File | Changes |
|------|---------|
| `src/dsp/multiband.rs` | Created - N-band crossover |
| `src/dsp/mod.rs` | Added multiband export |
| `src/processor/multiband.rs` | Created - MultibandProcessor |
| `src/processor/mod.rs` | Added multiband export |
| `src/processor/config.rs` | Added MultibandConfigHeader, MultibandConfig |
| `src/processor/stats.rs` | Added MultibandAtomicStats, MultibandStatsHeader |
| `src/lib.rs` | Added all BASS_MultibandProcessor_* FFI functions |
| `Cargo.toml` | Added multiband example |
| `examples/file_to_speakers_multiband.rs` | Created - 5-band demo |

---

## Notes for Continuation

1. The test file path `F:\Audio\GlobalNewsPodcast-20251215.mp3` is hardcoded in examples. May need adjustment.

2. Compiler warnings exist for unused fields/methods in `MultibandCrossover` (`sample_rate`, `num_bands`). These are intentional for potential future use.

3. The 2-band and N-band processors are separate implementations. Consider unifying them (N-band with n=2 could replace 2-band).

4. BASS library DLL must be in PATH or alongside executable for examples to run.
