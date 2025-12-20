# Development Steps - Clock Abstraction Layer (Session 8)

## Session Summary
Added Livewire Clock support as an alternative to PTP. Created unified clock abstraction layer allowing runtime selection between PTP (IEEE 1588v2) and Axia Livewire Clock synchronization.

## What Was Done

### New Clock Mode Config Option
- `BASS_CONFIG_AES67_CLOCK_MODE` (0x20019)
  - `0` = PTP (default, IEEE 1588v2)
  - `1` = Livewire Clock (Axia proprietary)

### New File: `bass-aes67/src/clock_bindings.rs`
Unified clock abstraction that:
- Dynamically loads both `bass_ptp.dll` and `bass_livewire_clock.dll`
- Provides unified API regardless of which clock is active:
  - `clock_start(interface, domain, mode)` - Start selected clock
  - `clock_stop()` - Stop active clock
  - `clock_is_locked()` - Check if locked
  - `clock_get_frequency_ppm()` - Get frequency adjustment
  - `clock_get_offset()` - Get offset in ns
  - `clock_get_state()` - Get state (0-3)
  - `clock_get_stats_string()` - Get formatted status
- Maintains backward-compatible `ptp_*` functions for existing code

### Removed: `bass-aes67/src/ptp_bindings.rs`
Replaced by `clock_bindings.rs`

### Modified Files

#### `bass-aes67/src/lib.rs`
- Added `BASS_CONFIG_AES67_CLOCK_MODE` constant
- Added `CONFIG_CLOCK_MODE` static variable
- Updated config handler to support new clock mode option
- Changed `ptp_bindings` → `clock_bindings` module
- Stream creation now starts correct clock based on mode

#### `bass-aes67/src/input/stream.rs`
- Changed `ptp_is_locked()` → `clock_is_locked()`
- Changed `ptp_get_frequency_ppm()` → `clock_get_frequency_ppm()`
- Renamed variable `ptp_feedforward` → `clock_feedforward`

#### `bass-aes67/src/output/stream.rs`
- Changed import from `ptp_bindings` → `clock_bindings`
- Changed `init_ptp_bindings()` → `init_clock_bindings()`
- Changed `ptp_get_frequency_ppm()` → `clock_get_frequency_ppm()`

## Usage

### Select Clock Mode Before Stream Creation
```c
// Use Livewire Clock instead of PTP
BASS_SetConfig(BASS_CONFIG_AES67_CLOCK_MODE, 1);

// Set interface
BASS_SetConfigPtr(BASS_CONFIG_AES67_INTERFACE, "192.168.60.102");

// Create stream - will use Livewire clock
HSTREAM stream = BASS_StreamCreateURL("aes67://239.192.76.52:5004", ...);
```

### Stats Will Show Clock Type
- PTP mode: `"Slave to: PTP/00:1D:C1:... (domain=1)"`
- Livewire mode: `"Slave to: LW/00:1D:C1:... (prio=8)"`

## DLL Requirements

### For PTP Mode (default)
- `bass_ptp.dll` must be in same directory as `bass_aes67.dll`

### For Livewire Mode
- `bass_livewire_clock.dll` must be in same directory as `bass_aes67.dll`

### Both Modes Available
- Place both DLLs alongside `bass_aes67.dll`
- Switch modes at runtime via `BASS_CONFIG_AES67_CLOCK_MODE`

## Architecture

```
Application
    │
    ▼ BASS_SetConfig(CLOCK_MODE, 0 or 1)
┌───────────────────────────────────────────┐
│            bass_aes67.dll                  │
│  ┌─────────────────────────────────────┐  │
│  │       clock_bindings.rs             │  │
│  │  ┌─────────────┬───────────────┐   │  │
│  │  │  PTP API    │  Livewire API │   │  │
│  │  └──────┬──────┴───────┬───────┘   │  │
│  └─────────┼──────────────┼───────────┘  │
└────────────┼──────────────┼───────────────┘
             │              │
             ▼              ▼
     bass_ptp.dll   bass_livewire_clock.dll
```

## Build Commands

```bash
# Build bass-aes67 (main plugin)
cd "c:/Dev/CasterPlay2025/BassAES67/BassAES67/bass-aes67"
cargo build --release

# Build bass-livewire-clock
cd "c:/Dev/CasterPlay2025/BassAES67/BassAES67/bass-livewire-clock"
cargo build --release

# Output DLLs:
# - bass-aes67/target/release/bass_aes67.dll
# - bass-livewire-clock/target/release/bass_livewire_clock.dll
```

## Updated Stats API Reference

### Configuration (BASS_SetConfig)
| Option | Value | Description |
|--------|-------|-------------|
| `BASS_CONFIG_AES67_CLOCK_MODE` | 0x20019 | Clock mode: 0=PTP, 1=Livewire |

### Clock Mode Constants
```c
#define BASS_AES67_CLOCK_PTP      0  // IEEE 1588v2 PTP
#define BASS_AES67_CLOCK_LIVEWIRE 1  // Axia Livewire Clock
```

## Key Files

| File | Purpose |
|------|---------|
| `bass-aes67/src/clock_bindings.rs` | **NEW** Unified clock abstraction |
| `bass-aes67/src/lib.rs` | Plugin entry, config handler |
| `bass-aes67/src/input/stream.rs` | Input with PI controller |
| `bass-aes67/src/output/stream.rs` | Output (minimal changes) |
| `bass-livewire-clock/src/lib.rs` | Livewire clock C API |
| `bass-livewire-clock/src/client.rs` | Livewire UDP client |
| `bass-livewire-clock/src/servo.rs` | PI controller servo |

## Session 8 Update: Shutdown Hang Fix

### Problem
App was hanging on Ctrl+C during cleanup phase at `BASS_PluginFree()`.

### Root Cause
**Windows DllMain Loader Lock**: You cannot safely call `thread.join()` inside `DllMain(DLL_PROCESS_DETACH)`. Windows holds a "loader lock" during DLL unload, and joining threads that might be waiting on synchronization primitives causes a deadlock.

When `BASS_PluginFree()` unloads `bass_aes67.dll`:
1. `DllMain(DLL_PROCESS_DETACH)` is called
2. This calls `clock_stop()` which calls the clock DLL's `force_stop()`
3. `force_stop()` was trying to `thread.join()` - DEADLOCK

### Fix Applied
Modified `force_stop` functions to **NOT join threads** - just signal stop and drop handles:

```rust
// Fixed pattern - NO thread.join() in force_stop:
pub fn force_stop_ptp_client() {
    // Signal threads to stop
    let mut guard = client_mutex.lock();
    if let Some(ref handle) = *guard {
        handle.running.store(false, Ordering::SeqCst);
    }

    // Drop handles WITHOUT joining - threads terminate naturally
    if let Some(mut handle) = guard.take() {
        drop(handle.event_thread.take());  // Don't join!
        drop(handle.general_thread.take());
    }
}
```

The threads will:
1. See `running = false` on next loop iteration
2. Exit after their 100ms socket timeout
3. Be cleaned up by the OS when the process exits

### Files Modified
- `bass-ptp/src/client.rs` - `force_stop_ptp_client()` no longer joins threads
- `bass-livewire-clock/src/client.rs` - `force_stop_lw_client()` no longer joins threads
- `bass-aes67/src/clock_bindings.rs` - `clock_stop()` now calls `force_stop` instead of `stop`

## Next Session Goals

### 1. C# P/Invoke Bindings (.NET 10)
- Add `BASS_CONFIG_AES67_CLOCK_MODE` constant
- Example showing clock mode selection

### 2. Testing
- Test with actual Axia Livewire network
- Verify Livewire clock locks correctly
- Compare sync quality between PTP and Livewire

### 3. Linux Support
- Implement dlopen loading for .so files
- Test on Linux with both clock modes

## Critical Constraints
- **DO NOT** modify `bass-aes67/src/output/stream.rs` beyond import changes
- **Use atomics only** in audio path - no mutex
