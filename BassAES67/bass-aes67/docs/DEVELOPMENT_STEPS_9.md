# Development Steps - System Clock Fallback (Session 9)

## Session Summary
Added System Clock support as a fallback when PTP or Livewire clock loses lock. The system clock is a free-running clock that ensures audio playback continues even without network synchronization.

## What Was Done

### New Crate: `bass-system-clock`
Created a new crate providing a system clock (free-running) that:
- Always reports "locked" when running
- Returns 0.0 ppm frequency correction (nominal rate)
- Returns 0 ns offset (no time correction)
- Works on both Windows and Linux

**Files Created:**
- `bass-system-clock/Cargo.toml` - Crate configuration
- `bass-system-clock/src/lib.rs` - C API exports matching PTP/Livewire pattern
- `bass-system-clock/src/timer.rs` - High-precision timer (Windows: WaitableTimer, Linux: nanosleep)

**C API Functions:**
- `BASS_SYS_Start(interface_ip)` - Start system clock
- `BASS_SYS_Stop()` - Stop system clock
- `BASS_SYS_ForceStop()` - Force stop
- `BASS_SYS_IsRunning()` - Check if running
- `BASS_SYS_GetOffset()` - Returns 0 (no offset)
- `BASS_SYS_GetFrequencyPPM()` - Returns 0.0 (nominal rate)
- `BASS_SYS_GetStatsString()` - "System Clock (free-running)"
- `BASS_SYS_GetState()` - Returns 3 (Slave) when running
- `BASS_SYS_IsLocked()` - Returns 1 when running (always "locked")
- Timer functions for precision timing

### New Config Option
- `BASS_CONFIG_AES67_CLOCK_FALLBACK_TIMEOUT` (0x2001A)
  - Timeout in seconds before falling back to system clock (default: 5)
  - `0` = Disable automatic fallback

### Clock Mode Values Updated
```c
#define BASS_AES67_CLOCK_PTP        0  // IEEE 1588v2 PTP (default)
#define BASS_AES67_CLOCK_LIVEWIRE   1  // Axia Livewire Clock
#define BASS_AES67_CLOCK_SYSTEM     2  // System Clock (free-running)
```

### Modified Files

#### `bass-aes67/src/clock_bindings.rs`
- Added `ClockMode::System = 2`
- Added fallback state tracking atomics:
  - `FALLBACK_ACTIVE` - Is fallback currently active?
  - `LAST_LOCK_TIME_MS` - Timestamp of last lock
  - `FALLBACK_TIMEOUT_SECS` - Configurable timeout (default 5s)
- Added `SysFunctions` struct and `SYS_LIB` for system clock DLL loading
- Added Windows/Unix loaders for `bass_system_clock.dll/.so`
- Updated `init_clock_bindings()` to load all three clock libraries
- Updated `clock_start()` to:
  - Handle `ClockMode::System`
  - Preload system clock when using PTP/Livewire for fallback
- Modified `clock_is_locked()` with fallback logic:
  - If primary clock is locked, update last lock time
  - If primary loses lock and timeout expires, activate fallback
  - Return true when fallback is active (system clock is "locked")
- Modified `clock_get_frequency_ppm()` to return 0.0 in fallback mode
- Modified `clock_get_stats_string()` to show fallback status:
  - `"FALLBACK: System Clock (free-running) - PTP lost lock 7s ago"`
- Added helper functions:
  - `is_fallback_active()` - Check if fallback is active
  - `set_fallback_timeout()` - Configure timeout
  - `get_fallback_timeout()` - Get current timeout

#### `bass-aes67/src/lib.rs`
- Added `BASS_CONFIG_AES67_CLOCK_FALLBACK_TIMEOUT` constant
- Added `BASS_AES67_CLOCK_SYSTEM` constant
- Added `CONFIG_FALLBACK_TIMEOUT` static variable (default 5s)
- Added config handler for fallback timeout

#### `bass-aes67/bass_aes67.h`
- Added `BASS_CONFIG_AES67_CLOCK_MODE` constant
- Added `BASS_CONFIG_AES67_CLOCK_FALLBACK_TIMEOUT` constant
- Added clock mode constants (PTP, Livewire, System)

## Usage

### Configure Fallback Timeout
```c
// Set fallback timeout to 10 seconds
BASS_SetConfig(BASS_CONFIG_AES67_CLOCK_FALLBACK_TIMEOUT, 10);

// Disable fallback (never fall back to system clock)
BASS_SetConfig(BASS_CONFIG_AES67_CLOCK_FALLBACK_TIMEOUT, 0);
```

### Use System Clock Directly
```c
// Use system clock instead of PTP/Livewire
BASS_SetConfig(BASS_CONFIG_AES67_CLOCK_MODE, BASS_AES67_CLOCK_SYSTEM);
```

### Stats String Shows Fallback Status
When fallback is active:
```
FALLBACK: System Clock (free-running) - PTP lost lock 7s ago
```

When normal operation resumes:
```
Slave to: PTP/00:1D:C1:... (domain=1)
```

## Architecture Flow

```
User selects: CLOCK_MODE = PTP (0)
              FALLBACK_TIMEOUT = 5 seconds
                    │
                    ▼
        ┌───────────────────────┐
        │  clock_start(PTP)     │
        │  Load bass_ptp.dll    │
        │  Load bass_system.dll │  ← Preload for fallback
        └───────────┬───────────┘
                    │
        ┌───────────▼───────────┐
        │  PTP Running          │
        │  clock_is_locked()=✓  │
        │  last_lock_time=now   │
        └───────────┬───────────┘
                    │
            PTP loses lock
                    │
        ┌───────────▼───────────┐
        │  Timeout counting...  │
        │  clock_is_locked()=✗  │
        │  5...4...3...2...1    │
        └───────────┬───────────┘
                    │
            5 seconds elapsed
                    │
        ┌───────────▼───────────┐
        │  FALLBACK ACTIVE      │
        │  clock_is_locked()=✓  │  ← Returns true (system clock)
        │  frequency_ppm()=0.0  │  ← Nominal rate
        │  Stats: "FALLBACK..." │
        └───────────┬───────────┘
                    │
            PTP regains lock
                    │
        ┌───────────▼───────────┐
        │  Resume PTP           │
        │  FALLBACK_ACTIVE=false│
        │  last_lock_time=now   │
        │  Stats: "Slave to..." │
        └───────────────────────┘
```

## DLL Deployment

For full fallback support, deploy all three clock DLLs:
```
bass_aes67.dll
bass_ptp.dll              (for PTP mode)
bass_livewire_clock.dll   (for Livewire mode)
bass_system_clock.dll     (for fallback + System mode)
```

## Build Commands

```bash
# Build bass-system-clock
cd "c:/Dev/CasterPlay2025/BassAES67/BassAES67/bass-system-clock"
cargo build --release

# Build bass-aes67 (main plugin)
cd "c:/Dev/CasterPlay2025/BassAES67/BassAES67/bass-aes67"
cargo build --release

# Output DLLs:
# - bass-system-clock/target/release/bass_system_clock.dll
# - bass-aes67/target/release/bass_aes67.dll
```

## Key Files

| File | Purpose |
|------|---------|
| `bass-system-clock/src/lib.rs` | **NEW** System clock C API |
| `bass-system-clock/src/timer.rs` | **NEW** High-precision timer |
| `bass-aes67/src/clock_bindings.rs` | Unified clock abstraction with fallback |
| `bass-aes67/src/lib.rs` | Plugin entry, config handler |
| `bass-aes67/bass_aes67.h` | C/C++ header file |

## Critical Constraints
- **NO mutex in audio path** - Use atomics only for fallback state
- **Cross-platform** - Works on Windows and Linux
- **Low overhead** - Fallback check is called frequently
- **Automatic recovery** - Returns to primary clock when it regains lock

## Next Session Goals

### 1. Linux Support
- Implement dlopen loading for .so files
- Test on Linux with all clock modes

### 2. C# P/Invoke Bindings
- Add `BASS_CONFIG_AES67_CLOCK_FALLBACK_TIMEOUT` constant
- Example showing fallback configuration

### 3. Testing
- Test fallback behavior with real PTP network
- Verify timeout countdown works correctly
- Test automatic recovery when clock regains lock
