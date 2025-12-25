# Development Session 6 - Failed Attempts at Fixing RTP Output Late-Start Pops

## Problem Statement

**WORKS:**
- Starting `rtp_output_test` FIRST, then ZipOne connects - ALL codecs work, reconnection works
- Return audio TO ZipOne works in ALL cases

**FAILS:**
- Starting ZipOne FIRST (already sending RTP), then starting `rtp_output_test` - crazy pops (many per second) in incoming audio
- This affects ALL codecs equally: PCM16, G711, G722, MP2, AAC

## What I Did Wrong This Session

### 1. Wrong Initial Assumption
I incorrectly assumed the issue was codec-specific (MP2 decoder mid-stream sync issue). The user explicitly told me: "It is NOT a codec issue. I get the exact same issue on PCM, G711, G722, MP2-AAC."

### 2. Implemented Wrong Fix
Added `consecutive_empty_decodes` tracking to reset stuck decoders. This was completely wrong because:
- The issue affects ALL codecs equally
- It's not a decoder problem

### 3. Reverted Wrong Fix
Removed the `consecutive_empty_decodes` code after user correction.

### 4. Added Debug Instrumentation
Added debug prints to track:
- When generation changes
- Buffer level at generation change
- When buffering completes

### 5. Misread Debug Output
When user provided debug output showing both cases, I incorrectly concluded the fix was working. The user had to correct me: "NO!!!! NO CODEC IS WORKING IN ZipOne FIRST. All codecs are Working in 'rtp_output_test' First!!!"

### 6. Removed Debug Prints Prematurely
Removed debug prints thinking the issue was fixed, when it was NOT fixed.

## Current State of Code

The code in `src/output_new/stream.rs` has:
- Generation tracking for connection changes (lines 860-874)
- Buffering mechanism that waits for `target_samples` before outputting (lines 878-885)
- Adaptive resampling with PI controller (lines 887-962)
- Debug prints have been removed

## Debug Output Analysis (What It Actually Shows)

### ZipOne First (FAILS - has pops):
```
[DEBUG] Generation changed: 0 -> 1, buffer: 1152, target: 9600
[DEBUG] Buffering complete: available=9792, target=9600
Buf: 4032  (lower than target after running)
```

### Test First (WORKS - no pops):
```
[DEBUG] Generation changed: 0 -> 1, buffer: 1728, target: 9600
[DEBUG] Buffering complete: available=11520, target=9600
Buf: 8240-11706  (healthy, above target after running)
```

## Key Observation
- Buffer level stays LOWER (4032-4608) in failing case vs HIGHER (8240-11706) in working case
- Despite buffering completing successfully, audio still has pops in "ZipOne first" case
- The pops are NOT caused by initial buffering - something else is wrong

## Files Modified This Session

1. `src/output_new/stream.rs` - Added/removed wrong codec fix, added/removed debug prints
2. `docs/debug.txt` - User's debug output (created by user)

## What Needs Investigation in Next Session

1. Why does buffer level stay lower in "ZipOne first" case?
2. What causes the pops if buffering completes successfully?
3. Is there a timing/race condition specific to the "ZipOne first" scenario?
4. The resampler loop outputs zeros when `resample_init = false` - could this be causing pops?
5. Is consumption rate somehow faster in the "ZipOne first" case?

## Key Code Locations

- `receiver_loop()`: Lines 576-646 - receives RTP, decodes, pushes to ring buffer
- `read_samples()`: Lines 845-999 - BASS callback, reads from ring buffer with resampling
- Generation tracking: Lines 860-874
- Buffering logic: Lines 878-885
- Resampler output loop: Lines 920-947
