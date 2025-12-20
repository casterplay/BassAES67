# bass-livewire-clock

A Rust library providing Livewire clock synchronization for AES67 audio applications. Designed as a drop-in alternative to `bass-ptp`, allowing applications to use either PTP (IEEE 1588v2) or Axia Livewire clock synchronization.

## Overview

Axia Livewire is a proprietary AoIP (Audio over IP) protocol used in broadcast environments. Unlike PTP which uses a two-way delay measurement, Livewire broadcasts clock packets from a master to all slaves on the network.

This library receives Livewire clock multicast packets, calculates the offset between local and remote clocks, and provides frequency drift estimation for audio synchronization.

## Features

- **Same API as bass-ptp** - Drop-in replacement, same function signatures
- **Standard Livewire clock** - Joins 239.192.255.2:7000 multicast group
- **Master selection** - Automatically follows highest priority master
- **PI controller** - Uses PI control algorithm matching Axia reference implementation
- **Lock detection** - Reports when synchronization is stable
- **Precision timer** - Optional PLL-adjusted timer for audio scheduling

## Livewire Clock Protocol

### Packet Format (36 bytes)

```
Offset  Size  Field               Description
------  ----  ------------------  ------------------------------------------
0-11    12    RTP Header          Standard RTP header
12-13   2     Extension Profile   0xFA1A (Livewire identifier)
14-15   2     Extension Length    Length of extension data
16-19   4     Frame Number        Clock frame (250µs units, big-endian)
20-23   4     Packet Type         0x0C00CABA = sync packet
24-25   2     Microticks          Sub-frame time (0-3071)
26      1     Magic               0xAC (Livewire identifier)
27      1     Priority            Master priority (0-15, higher wins)
28-29   2     Hardware ID         Lower 15 bits of master IP
30-35   6     MAC Address         Master's MAC address
```

### Timing

- 1 frame = 250 microseconds
- 3072 microticks per frame
- 1 microtick ≈ 81.38 nanoseconds
- Slow-rate packets arrive every ~30-40ms

## C API Reference

### Lifecycle Functions

```c
// Start the Livewire clock client
// interface_ip: Network interface IP as C string (e.g., "192.168.60.102")
// Joins multicast group 239.192.255.2:7000 (standard Livewire clock)
// Returns: BASS_LW_OK (0) on success
int BASS_LW_Start(const char* interface_ip);

// Stop the client (reference counted)
int BASS_LW_Stop();

// Force stop regardless of reference count
int BASS_LW_ForceStop();

// Check if client is running
// Returns: 1 if running, 0 if not
int BASS_LW_IsRunning();
```

### Status Functions

```c
// Get current state
// Returns: 0=Disabled, 1=Listening, 2=Uncalibrated, 3=Slave
uint8_t BASS_LW_GetState();

// Check if locked (stable synchronization)
// Returns: 1 if locked, 0 if not
int BASS_LW_IsLocked();

// Get current clock offset in nanoseconds
int64_t BASS_LW_GetOffset();

// Get frequency adjustment in parts per million
double BASS_LW_GetFrequencyPPM();

// Get formatted status string
// Returns: Length of string written
int BASS_LW_GetStatsString(char* buffer, int buffer_size);

// Get library version (0xMMNN format)
uint32_t BASS_LW_GetVersion();
```

### Timer Functions

```c
// Timer callback type
typedef void (*BASS_LW_TimerProc)(void* user);

// Start precision timer
// interval_ms: Timer period (1-1000 ms)
// callback: Function called on each tick (can be NULL)
// user: User data passed to callback
int BASS_LW_TimerStart(uint32_t interval_ms, BASS_LW_TimerProc callback, void* user);

// Stop timer
int BASS_LW_TimerStop();

// Check if timer is running
int BASS_LW_TimerIsRunning();

// Set/get timer interval (can change while running)
int BASS_LW_TimerSetInterval(uint32_t interval_ms);
uint32_t BASS_LW_TimerGetInterval();

// Enable/disable PLL frequency adjustment
// When enabled, timer period is adjusted based on clock drift
int BASS_LW_TimerSetPLL(int enabled);
int BASS_LW_TimerIsPLLEnabled();
```

### Error Codes

```c
#define BASS_LW_OK            0  // Success
#define BASS_LW_ERROR_ALREADY 1  // Already running
#define BASS_LW_ERROR_NOT_INIT 2 // Not initialized
#define BASS_LW_ERROR_SOCKET  3  // Socket error
#define BASS_LW_ERROR_INVALID 4  // Invalid parameter
```

## Rust API

```rust
use bass_livewire_clock::{
    start_lw_client, stop_lw_client, force_stop_lw_client,
    is_lw_running, get_lw_stats, get_offset_ns, get_frequency_ppm,
    LwState, LwStats,
};

// Start client (joins 239.192.255.2:7000)
start_lw_client(Ipv4Addr::new(192, 168, 60, 102))?;

// Get stats
if let Some(stats) = get_lw_stats() {
    println!("State: {:?}", stats.state);
    println!("Offset: {} ns", stats.offset_ns);
    println!("Frequency: {} ppm", stats.frequency_ppm);
    println!("Locked: {}", stats.locked);
}

// Stop client
stop_lw_client();
```

## State Machine

```
DISABLED → LISTENING → UNCALIBRATED → SLAVE
              ↑                          |
              └──────────────────────────┘
                    (master change)
```

1. **Disabled** - Client not started
2. **Listening** - Waiting for first clock packet
3. **Uncalibrated** - Received packets, building baseline (10 packets)
4. **Slave** - Actively tracking master clock

## Offset Calculation

Since Livewire is one-way (no delay request/response), we use relative offset tracking:

1. **Baseline**: First packet establishes reference point
   - `baseline_local_ns` = local receive timestamp
   - `baseline_remote_ns` = (frame × 250µs) + (microticks × 81.38ns)

2. **Subsequent packets**: Calculate drift from baseline
   - `local_elapsed` = current_local - baseline_local
   - `remote_elapsed` = current_remote - baseline_remote
   - `offset_ns` = local_elapsed - remote_elapsed

3. **Servo**: PI controller with minimum filter (28 samples) provides stable frequency estimate

## Comparison with bass-ptp

| Aspect | bass-ptp (PTP) | bass-livewire-clock |
|--------|----------------|---------------------|
| Protocol | IEEE 1588v2 | Axia Livewire |
| Delay measurement | Two-way (DelayReq/Resp) | One-way (broadcast) |
| Master ID | EUI-64 Clock Identity | MAC + Priority + HW ID |
| Packet rate | ~8/sec | ~30/sec (slow-rate) |
| Path delay | Measured | Not available |
| API | BASS_PTP_* | BASS_LW_* |

## Building

```bash
cargo build --release
```

Output: `target/release/bass_livewire_clock.dll` (Windows)

## Dependencies

- `socket2` - Multicast socket support
- `parking_lot` - Fast mutex implementation
- `windows-sys` (Windows only) - Waitable timer for precision timing

## Usage Example

```c
#include "bass_livewire_clock.h"

int main() {
    // Start on specific network interface
    if (BASS_LW_Start("192.168.60.102") != BASS_LW_OK) {
        printf("Failed to start\n");
        return 1;
    }

    // Wait for lock
    while (!BASS_LW_IsLocked()) {
        Sleep(100);
    }

    // Read clock data
    int64_t offset = BASS_LW_GetOffset();
    double freq = BASS_LW_GetFrequencyPPM();
    printf("Offset: %lld ns, Freq: %.3f ppm\n", offset, freq);

    // Cleanup
    BASS_LW_Stop();
    return 0;
}
```

## Integration with AES67

To use Livewire clock instead of PTP in an AES67 application:

1. Replace `BASS_PTP_Start()` with `BASS_LW_Start(ip)`
2. Replace all `BASS_PTP_*` calls with equivalent `BASS_LW_*` calls
3. The frequency_ppm and offset_ns values can be used identically for audio timing

The timer API works the same way - when PLL is enabled, the timer period is adjusted based on the measured clock drift to track the network clock rate.
