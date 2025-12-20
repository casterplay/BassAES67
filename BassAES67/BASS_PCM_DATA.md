# BASS PCM Data Flow & Clock Synchronization

This document describes how the `bass_aes67.dll` plugin handles PCM data flow with BASS and synchronizes timing using PTP/Livewire/System clocks. This will serve as a reference for implementing similar plugins (e.g., SRT).

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                         bass_aes67.dll                               │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌─────────────────────┐              ┌─────────────────────┐       │
│  │   INPUT (Receive)   │              │   OUTPUT (Transmit) │       │
│  │   Aes67Stream       │              │   Aes67OutputStream │       │
│  └──────────┬──────────┘              └──────────┬──────────┘       │
│             │                                    │                   │
│             ▼                                    ▼                   │
│  ┌─────────────────────┐              ┌─────────────────────┐       │
│  │  Receiver Thread    │              │  Transmitter Thread │       │
│  │  (UDP multicast RX) │              │  (UDP multicast TX) │       │
│  └──────────┬──────────┘              └──────────┬──────────┘       │
│             │                                    │                   │
│             ▼                                    ▼                   │
│  ┌─────────────────────┐              ┌─────────────────────┐       │
│  │  Lock-Free Ring     │              │  BASS_ChannelGetData│       │
│  │  Buffer (ringbuf)   │              │  (pull from BASS)   │       │
│  └──────────┬──────────┘              └─────────────────────┘       │
│             │                                                        │
│             ▼                                                        │
│  ┌─────────────────────┐                                            │
│  │  BASS STREAMPROC    │◄──── BASS calls this to get samples        │
│  │  Callback           │                                            │
│  └─────────────────────┘                                            │
│                                                                      │
├─────────────────────────────────────────────────────────────────────┤
│                      Clock Synchronization                           │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐              │
│  │ bass_ptp.dll │  │ bass_lw.dll  │  │ bass_sys.dll │              │
│  │ (PTP clock)  │  │ (Livewire)   │  │ (fallback)   │              │
│  └──────────────┘  └──────────────┘  └──────────────┘              │
└─────────────────────────────────────────────────────────────────────┘
```

---

## PART 1: PULLING PCM FROM BASS (OUTPUT)

### The Problem
BASS provides audio through `BASS_ChannelGetData()`. We need to:
1. Pull samples at precise intervals (1ms, 5ms, etc.)
2. Synchronize timing to PTP/Livewire master clock
3. Send RTP packets at exact intervals

### Implementation: `Aes67OutputStream`

**File:** `bass-aes67/src/output/stream.rs`

#### 1. High-Priority Transmitter Thread

```rust
// Set thread priority for precise timing (Windows)
#[cfg(windows)]
{
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
    };
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
    }
}
```

#### 2. Pulling Samples from BASS

```rust
// FFI import
#[link(name = "bass")]
extern "system" {
    fn BASS_ChannelGetData(handle: DWORD, buffer: *mut c_void, length: DWORD) -> DWORD;
}

const BASS_DATA_FLOAT: DWORD = 0x40000000;

// In transmitter loop:
let bytes_read = unsafe {
    BASS_ChannelGetData(
        source_channel,
        audio_buffer.as_mut_ptr() as *mut c_void,
        bytes_needed | BASS_DATA_FLOAT,  // Request float samples
    )
};

// Handle underrun
if bytes_read == 0xFFFFFFFF {
    audio_buffer.fill(0.0);  // Send silence
    stats.underruns.fetch_add(1, Ordering::Relaxed);
}
```

**Key Points:**
- `BASS_DATA_FLOAT` flag requests 32-bit float samples
- Returns `0xFFFFFFFF` on error/end-of-stream
- Partial reads: fill remaining with silence
- **NO MUTEX** - direct call, atomic stats only

#### 3. Precision Timing with Clock Correction

```rust
let base_interval_us = interval_us as f64;  // e.g., 1000 for 1ms packets
let mut next_tx = Instant::now() + Duration::from_micros(interval_us);

while running.load(Ordering::SeqCst) {
    // Get clock frequency correction every 100 packets
    if ppm_update_counter >= 100 {
        current_ppm = clock_get_frequency_ppm();
    }

    // Apply PPM correction to interval
    // If clock is +10 ppm faster, we send packets slightly faster
    let interval_factor = 1.0 - (current_ppm / 1_000_000.0);
    let adjusted_interval_us = (base_interval_us * interval_factor) as u64;

    // Sleep with margin, then spin-wait for precision
    let now = Instant::now();
    if next_tx > now {
        let sleep_time = next_tx - now;
        if sleep_time > Duration::from_millis(2) {
            thread::sleep(sleep_time - Duration::from_millis(1));
        }
        // Spin-wait for final precision
        while Instant::now() < next_tx {
            std::hint::spin_loop();
        }
    }

    // ... read samples, build packet, send ...

    // Schedule next packet
    next_tx = target_time + interval;

    // Reset if fallen too far behind (> 1 packet)
    if Instant::now() > next_tx + interval {
        next_tx = Instant::now() + interval;
    }
}
```

#### 4. RTP Packet Format

```rust
// RTP Header (12 bytes)
// Byte 0: V=2, P=0, X=0, CC=0 = 0x80
// Byte 1: M=0, PT (payload type, typically 96)
// Bytes 2-3: Sequence number (big-endian, wraps at 65535)
// Bytes 4-7: Timestamp (big-endian, increments by samples/packet)
// Bytes 8-11: SSRC (random, constant for stream)

// Payload: L24 PCM (24-bit big-endian per sample)
fn convert_float_to_l24(sample: f32) -> [u8; 3] {
    let value = (sample * 8388607.0) as i32;  // 2^23 - 1
    let value = value.clamp(-8388608, 8388607);
    [
        ((value >> 16) & 0xFF) as u8,  // MSB
        ((value >> 8) & 0xFF) as u8,
        (value & 0xFF) as u8,          // LSB
    ]
}
```

---

## PART 2: PUSHING PCM INTO BASS (INPUT)

### The Problem
RTP packets arrive asynchronously via UDP. We need to:
1. Receive packets in a dedicated thread
2. Buffer them for jitter absorption
3. Deliver samples to BASS via STREAMPROC callback
4. Match BASS's consumption rate to network arrival rate

### Implementation: `Aes67Stream`

**File:** `bass-aes67/src/input/stream.rs`

#### 1. Lock-Free Ring Buffer Architecture

```rust
use ringbuf::{HeapRb, traits::{Producer, Consumer, Split}};

// Create ring buffer (3x target for headroom)
let buffer_size = target_samples * 3;
let rb = HeapRb::<f32>::new(buffer_size);
let (producer, consumer) = rb.split();

// Producer → Receiver thread (writes samples)
// Consumer → STREAMPROC callback (reads samples)
```

**Why Lock-Free?**
- STREAMPROC callback runs in BASS's audio thread
- Any blocking (mutex) causes audio glitches
- `ringbuf` crate provides SPSC (single-producer single-consumer) lock-free queue

#### 2. Receiver Thread

```rust
fn receiver_loop(
    socket: UdpSocket,
    running: Arc<AtomicBool>,
    mut producer: ringbuf::HeapProd<f32>,
    ...
) {
    while running.load(Ordering::SeqCst) {
        match socket.recv(&mut buf) {
            Ok(len) => {
                // Parse RTP packet
                if let Some(packet) = RtpPacket::parse(&buf[..len]) {
                    // Convert 24-bit BE to float
                    convert_24bit_be_to_float(packet.payload, &mut sample_buf, channels);

                    // Push to ring buffer (atomic, lock-free)
                    // CRITICAL: Only push if room for ENTIRE packet
                    if producer.vacant_len() >= total_samples {
                        producer.push_slice(&sample_buf[..total_samples]);
                    } else {
                        // Buffer full - drop packet (better than corrupting alignment)
                        stats.packets_dropped.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(_) => break,
        }
    }
}
```

#### 3. BASS STREAMPROC Callback

```rust
pub unsafe extern "system" fn stream_proc(
    _handle: HSTREAM,
    buffer: *mut c_void,
    length: DWORD,
    user: *mut c_void,
) -> DWORD {
    let stream = &mut *(user as *mut Aes67Stream);
    let samples = length as usize / 4;  // 4 bytes per float
    let float_buffer = std::slice::from_raw_parts_mut(buffer as *mut f32, samples);

    // Read with adaptive resampling (clock synchronization)
    let written = stream.read_samples(float_buffer);

    if stream.is_ended() {
        (written * 4) as DWORD | BASS_STREAMPROC_END
    } else {
        (written * 4) as DWORD
    }
}
```

#### 4. Adaptive Resampling (Clock Matching)

The core challenge: BASS pulls samples at its own rate, but packets arrive at the sender's rate.

```rust
pub fn read_samples(&mut self, buffer: &mut [f32]) -> usize {
    let available = self.consumer.occupied_len();

    // Buffering mode - wait until we have enough samples
    if is_buffering {
        if available >= recovery_threshold {
            self.buffering.store(false, Ordering::Relaxed);
        } else {
            buffer.fill(0.0);  // Output silence
            return buffer.len();
        }
    }

    // PI Controller: adjust consumption rate based on buffer level
    let target = self.target_samples as f64;
    let error = (available as f64 - target) / target;  // Normalized: -1 to +1

    // PI gains (tuned for stability)
    const KP: f64 = 0.0001;   // Proportional
    const KI: f64 = 0.00005;  // Integral
    const MAX_TRIM_PPM: f64 = 20.0;

    self.integral_error += error;
    self.integral_error = self.integral_error.clamp(-max_integral, max_integral);

    let trim = KP * error + KI * self.integral_error;
    let trim_clamped = trim.clamp(-MAX_TRIM_PPM / 1e6, MAX_TRIM_PPM / 1e6);

    // Clock feedforward: match output's rate when clock is locked
    let clock_feedforward = if clock_is_locked() {
        clock_get_frequency_ppm() / 1_000_000.0
    } else {
        0.0
    };

    let resample_ratio = 1.0 + clock_feedforward + trim_clamped;

    // Linear interpolation between frames
    for _ in 0..frames_requested {
        let t = self.resample_pos;
        for ch in 0..self.channels {
            buffer[out_idx + ch] = prev[ch] + (curr[ch] - prev[ch]) * t as f32;
        }

        // Advance by resample ratio
        self.resample_pos += resample_ratio;

        // Load new frames as needed
        while self.resample_pos >= 1.0 {
            self.resample_pos -= 1.0;
            self.load_next_frame();
        }
    }
}
```

**How It Works:**
1. **Buffer Level Monitoring**: Track samples in ring buffer vs target
2. **PI Controller**: Adjust consumption rate to maintain target level
   - Buffer high → consume faster (ratio > 1.0)
   - Buffer low → consume slower (ratio < 1.0)
3. **Clock Feedforward**: When clock is locked, add PPM correction directly
4. **Linear Interpolation**: Smoothly resample between frames

---

## PART 3: CLOCK SYNCHRONIZATION

### Clock DLLs

| DLL | Purpose | Function Prefix |
|-----|---------|-----------------|
| `bass_ptp.dll` | IEEE 1588v2 PTP | `BASS_PTP_*` |
| `bass_livewire_clock.dll` | Axia Livewire | `BASS_LW_*` |
| `bass_system_clock.dll` | Free-running | `BASS_SYS_*` |

### Key Functions

```c
// Start clock (interface IP, domain for PTP)
int clock_start(const char* interface, uint8_t domain);

// Stop clock
int clock_stop();

// Is clock running?
int clock_is_running();

// Is clock locked to master?
int clock_is_locked();

// Get frequency correction in PPM (parts per million)
// Positive = local clock is faster than master
// Negative = local clock is slower than master
double clock_get_frequency_ppm();

// Get offset from master in nanoseconds
int64_t clock_get_offset();

// Get state: 0=Disabled, 1=Listening, 2=Uncalibrated, 3=Slave(locked)
uint8_t clock_get_state();
```

### Dynamic Loading

**File:** `bass-aes67/src/clock_bindings.rs`

```rust
// Load at runtime based on clock mode
static PTP_LIB: OnceLock<Option<PtpLibrary>> = OnceLock::new();
static LW_LIB: OnceLock<Option<LwLibrary>> = OnceLock::new();
static SYS_LIB: OnceLock<Option<SysLibrary>> = OnceLock::new();

pub fn clock_start(interface: Ipv4Addr, domain: u8, mode: ClockMode) -> Result<(), i32> {
    match mode {
        ClockMode::Ptp => {
            if let Some(lib) = PTP_LIB.get().and_then(|o| o.as_ref()) {
                let iface = CString::new(interface.to_string()).unwrap();
                let result = unsafe { (lib.functions.start)(iface.as_ptr(), domain) };
                // ...
            }
        }
        ClockMode::Livewire => { /* similar */ }
        ClockMode::System => { /* similar */ }
    }
}
```

### Automatic Fallback

```rust
// If primary clock loses lock for > timeout seconds, fall back to system clock
static FALLBACK_TIMEOUT_SECS: AtomicU32 = AtomicU32::new(5);
static FALLBACK_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn clock_is_locked() -> bool {
    let primary_locked = /* check primary clock */;

    if primary_locked {
        LAST_LOCK_TIME_MS.store(elapsed_ms(), Ordering::Relaxed);
        if FALLBACK_ACTIVE.load(Ordering::Relaxed) {
            // Restore primary clock
            FALLBACK_ACTIVE.store(false, Ordering::Relaxed);
        }
        return true;
    }

    // Check fallback timeout
    let timeout_ms = FALLBACK_TIMEOUT_SECS.load(Ordering::Relaxed) as u64 * 1000;
    if timeout_ms > 0 {
        let last_lock = LAST_LOCK_TIME_MS.load(Ordering::Relaxed);
        if elapsed_ms() - last_lock > timeout_ms {
            // Activate fallback
            FALLBACK_ACTIVE.store(true, Ordering::Relaxed);
            return true;  // System clock is always "locked"
        }
    }

    false
}
```

---

## PART 4: DATA FLOW SUMMARY

### Output (BASS → Network)

```
BASS Channel (mixer, file, web stream, etc.)
    │
    ▼
BASS_ChannelGetData(handle, buffer, length | BASS_DATA_FLOAT)
    │
    ▼
Float samples in buffer
    │
    ▼
Convert float → L24 (24-bit big-endian)
    │
    ▼
Build RTP packet (header + payload)
    │
    ▼
UDP socket.send_to(multicast_addr:port)
    │
    ▼
Network (multicast)
```

**Timing:** TX thread wakes every `packet_time` (1ms/5ms), adjusted by `clock_get_frequency_ppm()`

### Input (Network → BASS)

```
Network (multicast)
    │
    ▼
UDP socket.recv()
    │
    ▼
Parse RTP packet
    │
    ▼
Convert L24 → float
    │
    ▼
Push to ring buffer (lock-free)
    │
    ▼
BASS calls STREAMPROC callback
    │
    ▼
read_samples() with adaptive resampling
    │
    ▼
Float samples to BASS
```

**Timing:** BASS controls when STREAMPROC is called. We adapt via resampling.

---

## PART 5: CRITICAL DESIGN RULES

### 1. NO MUTEX IN AUDIO PATH
- Use atomics for statistics
- Use lock-free ring buffers for sample transfer
- Any blocking = audio glitches

### 2. HIGH-PRIORITY THREADS
```rust
#[cfg(windows)]
SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
```

### 3. PRECISION TIMING
```rust
// Sleep with margin, spin for final precision
if sleep_time > Duration::from_millis(2) {
    thread::sleep(sleep_time - Duration::from_millis(1));
}
while Instant::now() < next_tx {
    std::hint::spin_loop();
}
```

### 4. HANDLE UNDERRUNS GRACEFULLY
- Fill with silence, don't crash
- Track statistics for monitoring

### 5. ATOMIC STATISTICS
```rust
struct AtomicStats {
    packets_sent: AtomicU64,
    underruns: AtomicU64,
    // ...
}

// Increment without locking
stats.packets_sent.fetch_add(1, Ordering::Relaxed);
```

### 6. SEPARATE CLOCK DLLs
- Clock logic is complex, keep it isolated
- Dynamic loading allows optional features
- Fallback support for robustness

---

## PART 6: APPLYING TO SRT PLUGIN

For an SRT (Secure Reliable Transport) plugin, the patterns are similar:

### SRT Input
1. Create SRT socket, connect to sender
2. Receiver thread: `srt_recv()` → ring buffer
3. STREAMPROC callback: read from ring buffer
4. **Difference**: SRT handles retransmission, no jitter buffer needed
5. **Clock**: Use buffer level for adaptive resampling (no external clock)

### SRT Output
1. TX thread: `BASS_ChannelGetData()` → encode → `srt_send()`
2. **Difference**: SRT handles pacing internally
3. **Clock**: May not need PPM correction (SRT handles timing)

### Key SRT Functions
```c
SRTSOCKET srt_create_socket();
int srt_connect(SRTSOCKET sock, const struct sockaddr* addr, int len);
int srt_listen(SRTSOCKET sock, int backlog);
SRTSOCKET srt_accept(SRTSOCKET sock, struct sockaddr* addr, int* len);
int srt_send(SRTSOCKET sock, const char* buf, int len);
int srt_recv(SRTSOCKET sock, char* buf, int len);
int srt_close(SRTSOCKET sock);
```

---

## Files Reference

| File | Purpose |
|------|---------|
| `bass-aes67/src/output/stream.rs` | Output stream (BASS → network) |
| `bass-aes67/src/output/rtp.rs` | RTP packet building |
| `bass-aes67/src/input/stream.rs` | Input stream (network → BASS) |
| `bass-aes67/src/input/rtp.rs` | RTP packet parsing |
| `bass-aes67/src/clock_bindings.rs` | Clock DLL dynamic loading |
| `bass-aes67/src/lib.rs` | Plugin entry point + FFI exports |
| `bass-aes67/bass_aes67.h` | C header for external use |
