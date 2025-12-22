# Development Steps - Session 12: Linux Testing

## Goal
Make `aes67_loopback.rs` (Rust example) work on Linux without breaking Windows.

## Environment
- OS: Pop!_OS (Linux)
- AoIP Network: `enp86s0` at `192.168.60.104`

---

## What Worked

### 1. System Clock
- Worked immediately after fixing the library loading issue.

### 2. Livewire Clock
- Port 7000 (non-privileged) - works without sudo.
- Uses `socket2::Socket` with `set_reuse_address(true)`.
- Locked successfully to master `00:50:C2:90:11:31`.

### 3. PTP Clock
- Ports 319/320 (privileged) - requires sudo on Linux.
- Command: `sudo LD_LIBRARY_PATH=./target/release ./target/release/examples/aes67_loopback ptp`
- Locked successfully to master `0050c2fffe901131:1`.

---

## Key Fixes Applied

### Fix 1: RTLD_NOLOAD for dlopen
**File:** `bass-aes67/examples/aes67_loopback.rs`

**Problem:** On Linux, using `dlopen` with `RTLD_LOCAL` was creating a second library instance with its own static variables. The example couldn't access the plugin's internal state.

**Solution:** Use `RTLD_NOLOAD` to get a handle to the already-loaded library (loaded by BASS) instead of loading a new instance.

```rust
const RTLD_NOW: i32 = 2;
const RTLD_NOLOAD: i32 = 4;  // Get handle to already-loaded library

let handle = dlopen(c_name.as_ptr(), RTLD_NOW | RTLD_NOLOAD);
```

### Fix 2: Direct Function Exports for Linux
**Files:** `bass-aes67/src/lib.rs`, `bass-aes67/examples/aes67_loopback.rs`

**Problem:** On Linux, `BASS_GetConfigPtr` doesn't route to plugin config handlers like it does on Windows. The stats string was always "(stats unavailable)".

**Solution:** Added direct exported functions that can be called via dlsym:
- `BASS_AES67_ClockIsLocked()` - returns 1 if locked, 0 if not
- `BASS_AES67_GetClockStats()` - returns pointer to stats string

The example loads these via dlsym and falls back to BASS_GetConfig* if unavailable.

---

## What Went Wrong (Learn From This!)

### 1. Overcomplicating Things
I spent too much time analyzing code paths, launching agents, and writing elaborate plans instead of just testing the simple obvious things first.

**Lesson:** When debugging, try the simple solution first (like running with sudo for privileged ports).

### 2. Not Recognizing Privileged Ports
PTP uses ports 319/320 which are below 1024. On Linux, this requires root. This is standard knowledge - I should have recognized it immediately instead of searching for code bugs.

**Lesson:** Know your protocols. PTP = ports 319/320 = needs root on Linux.

### 3. Guessing Instead of Reading
I made assumptions about what might be wrong instead of carefully reading the existing working code and understanding the architecture.

**Lesson:** The clock modules (bass-ptp, bass-livewire-clock, bass-system-clock) work 100% on Windows. The issue was only in the abstraction layer's interaction with Linux, not in the modules themselves.

### 4. Not Listening to User Feedback
The user repeatedly told me:
- "ALL needed functions ARE ALREADY there. IT works 100% on Windows"
- "THE PTP, Livewire and SYSTEM clock modules ARE working 100% on Windows"
- "You are overcomplicating things!"

I should have listened earlier.

**Lesson:** When the user says "it works on Windows", focus on platform-specific differences, not rewriting working code.

---

## Test Commands

```bash
# Build
cd /home/kennet/dev/BassAES67/BassAES67/bass-aes67
cargo build --release --example aes67_loopback

# System clock (no network, no sudo)
LD_LIBRARY_PATH=./target/release ./target/release/examples/aes67_loopback sys

# Livewire (port 7000, no sudo needed)
LD_LIBRARY_PATH=./target/release ./target/release/examples/aes67_loopback lw

# PTP (ports 319/320, needs sudo)
sudo LD_LIBRARY_PATH=./target/release ./target/release/examples/aes67_loopback ptp
```

---

## Files Modified

1. `bass-aes67/examples/aes67_loopback.rs` - Added RTLD_NOLOAD fix and plugin_loader module
2. `bass-aes67/src/lib.rs` - Added `BASS_AES67_ClockIsLocked()` and `BASS_AES67_GetClockStats()` exports
3. `bass-aes67/src/clock_bindings.rs` - Removed debug eprintln (cleanup)

---

## Next Steps (Stage 2)

Test C# app on Linux. The C# wrapper will need similar considerations:
- Linux library names (`libbass_aes67.so` vs `bass_aes67.dll`)
- P/Invoke calling conventions may differ
- May need to add the direct function exports to the C# bindings

---

## Summary

**Simple truth:** The code was already correct. The issues were:
1. Linux dlopen semantics (RTLD_NOLOAD fix)
2. Linux doesn't route BASS config to plugins (added direct exports)
3. PTP needs sudo on Linux (privileged ports)

Don't overcomplicate. Test first. Listen to user feedback.
