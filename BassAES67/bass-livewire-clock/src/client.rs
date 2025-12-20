//! Livewire clock client implementation.
//!
//! Receives multicast clock packets and calculates offset/frequency
//! using the same algorithm as the Axia reference implementation.

use std::mem::MaybeUninit;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Instant;
use parking_lot::Mutex;
use socket2::{Domain, Protocol, Socket, Type};

use crate::servo::ClockServo;
use crate::stats::{update_stats_string, LwState, LwStats, MasterIdentity};

// Livewire clock constants
const LWCLK_PACKET_TYPE_SYNC: u32 = 0x0C00CABA;
const LWCLK_MAGIC: u8 = 0xAC;
const LWCLK_RTP_EXT_PROFILE: u16 = 0xFA1A;
const FRAME_DURATION_NS: u64 = 250_000; // 250µs in nanoseconds
const NS_PER_MICROTICK: f64 = 81.380; // ~81.38 nanoseconds per microtick

// Multicast address for Livewire clock (standard clock used by all devices)
const MULTICAST_CLOCK: Ipv4Addr = Ipv4Addr::new(239, 192, 255, 2);
const LIVEWIRE_PORT: u16 = 7000;

/// Parsed Livewire clock packet
#[derive(Debug, Clone)]
struct LwClockPacket {
    /// Frame number (250µs units)
    frame: u32,
    /// Packet type (should be 0x0C00CABA for sync)
    packet_type: u32,
    /// Microticks within frame (0-3071)
    microticks: u16,
    /// Magic byte (should be 0xAC)
    magic: u8,
    /// Priority (0-15)
    priority: u8,
    /// Hardware ID (lower 15 bits of IP)
    hardware_id: u16,
    /// Master MAC address
    mac_address: [u8; 6],
    /// RTP extension profile (should be 0xFA1A)
    ext_profile: u16,
}

impl LwClockPacket {
    /// Parse a raw UDP packet
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 36 {
            return None;
        }

        // RTP extension header (bytes 12-15)
        let ext_profile = u16::from_be_bytes([data[12], data[13]]);

        // Livewire clock data (bytes 16-35)
        let frame = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let packet_type = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        let microticks = u16::from_be_bytes([data[24], data[25]]);
        let magic = data[26];
        let priority = data[27];
        let hardware_id = u16::from_be_bytes([data[28], data[29]]);
        let mac_address = [data[30], data[31], data[32], data[33], data[34], data[35]];

        Some(LwClockPacket {
            frame,
            packet_type,
            microticks,
            magic,
            priority,
            hardware_id,
            mac_address,
            ext_profile,
        })
    }

    /// Check if this is a valid Livewire clock sync packet
    fn is_valid_sync(&self) -> bool {
        self.magic == LWCLK_MAGIC
            && self.packet_type == LWCLK_PACKET_TYPE_SYNC
            && self.ext_profile == LWCLK_RTP_EXT_PROFILE
    }

    /// Get master identity
    fn master_identity(&self) -> MasterIdentity {
        MasterIdentity {
            mac_address: self.mac_address,
            priority: self.priority,
            hardware_id: self.hardware_id,
        }
    }
}

/// Global client instance
static CLIENT: OnceLock<Mutex<Option<LwClientHandle>>> = OnceLock::new();
static REF_COUNT: AtomicU32 = AtomicU32::new(0);

/// Shared stats accessible from outside
static SHARED_STATS: OnceLock<Mutex<LwStats>> = OnceLock::new();

/// Handle to a running client
struct LwClientHandle {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// Start the Livewire clock client.
///
/// Multiple calls are reference-counted - only first call actually starts.
pub fn start_lw_client(interface_ip: Ipv4Addr) -> Result<(), String> {
    let count = REF_COUNT.fetch_add(1, Ordering::SeqCst);
    if count > 0 {
        // Already running, just increment ref count
        return Ok(());
    }

    let client_mutex = CLIENT.get_or_init(|| Mutex::new(None));
    let mut client_guard = client_mutex.lock();

    if client_guard.is_some() {
        return Ok(());
    }

    // Initialize shared stats
    let stats_mutex = SHARED_STATS.get_or_init(|| Mutex::new(LwStats::default()));
    {
        let mut stats = stats_mutex.lock();
        *stats = LwStats {
            state: LwState::Listening,
            ..Default::default()
        };
        update_stats_string(&stats);
    }

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    let thread = thread::Builder::new()
        .name("lw-clock-client".to_string())
        .spawn(move || {
            client_thread(running_clone, interface_ip);
        })
        .map_err(|e| format!("Failed to spawn thread: {}", e))?;

    *client_guard = Some(LwClientHandle {
        running,
        thread: Some(thread),
    });

    Ok(())
}

/// Stop the Livewire clock client (decrements ref count)
pub fn stop_lw_client() {
    let prev = REF_COUNT.fetch_sub(1, Ordering::SeqCst);
    if prev > 1 {
        // Other references remain
        return;
    }

    // Last reference, actually stop
    force_stop_lw_client();
}

/// Force stop regardless of reference count
pub fn force_stop_lw_client() {
    REF_COUNT.store(0, Ordering::SeqCst);

    let client_mutex = match CLIENT.get() {
        Some(m) => m,
        None => return,
    };

    let mut client_guard = client_mutex.lock();

    if let Some(mut handle) = client_guard.take() {
        handle.running.store(false, Ordering::SeqCst);
        if let Some(thread) = handle.thread.take() {
            let _ = thread.join();
        }
    }

    // Update stats to disabled
    if let Some(stats_mutex) = SHARED_STATS.get() {
        let mut stats = stats_mutex.lock();
        stats.state = LwState::Disabled;
        update_stats_string(&stats);
    }
}

/// Check if client is running
pub fn is_lw_running() -> bool {
    REF_COUNT.load(Ordering::SeqCst) > 0
}

/// Get current stats
pub fn get_lw_stats() -> Option<LwStats> {
    SHARED_STATS.get().map(|m| m.lock().clone())
}

/// Get current offset in nanoseconds
pub fn get_offset_ns() -> i64 {
    SHARED_STATS
        .get()
        .map(|m| m.lock().offset_ns)
        .unwrap_or(0)
}

/// Get current frequency in ppm
pub fn get_frequency_ppm() -> f64 {
    SHARED_STATS
        .get()
        .map(|m| m.lock().frequency_ppm)
        .unwrap_or(0.0)
}

/// Client thread that receives and processes clock packets.
/// Uses the same algorithm as the Axia reference implementation.
fn client_thread(running: Arc<AtomicBool>, interface_ip: Ipv4Addr) {
    // Create socket
    let socket = match create_multicast_socket(interface_ip, MULTICAST_CLOCK) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("bass-livewire-clock: Failed to create socket: {}", e);
            return;
        }
    };

    // Set non-blocking with timeout for clean shutdown
    let _ = socket.set_read_timeout(Some(std::time::Duration::from_millis(100)));

    let mut servo = ClockServo::new();
    let start_time = Instant::now();
    let mut current_master: Option<MasterIdentity> = None;

    let mut buf: [MaybeUninit<u8>; 128] = unsafe { MaybeUninit::uninit().assume_init() };

    while running.load(Ordering::SeqCst) {
        // Receive packet
        let len = match socket.recv(&mut buf) {
            Ok(n) => n,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut
                {
                    continue;
                }
                eprintln!("bass-livewire-clock: Receive error: {}", e);
                continue;
            }
        };

        // Get local timestamp immediately (in nanoseconds since start)
        let local_ns = start_time.elapsed().as_nanos() as u64;

        // Copy to regular buffer for parsing
        let data: Vec<u8> = (0..len).map(|i| unsafe { buf[i].assume_init() }).collect();

        // Parse packet
        let packet = match LwClockPacket::parse(&data) {
            Some(p) if p.is_valid_sync() => p,
            _ => continue,
        };

        // Master selection: highest priority wins
        let packet_master = packet.master_identity();
        let accept_packet = match &current_master {
            None => true,
            Some(master) => {
                // Accept if higher priority or same master
                packet_master.priority > master.priority
                    || (packet_master.mac_address == master.mac_address
                        && packet_master.priority == master.priority)
            }
        };

        if !accept_packet {
            continue;
        }

        // Update current master if changed
        if current_master.as_ref() != Some(&packet_master) {
            current_master = Some(packet_master.clone());
            // Reset servo when master changes
            servo.reset();
        }

        // Convert local timestamp to frame/ticks format (as per reference)
        // local_ns is nanoseconds since start
        // Convert to frame (250µs units) and microticks (0-3071)
        let local_frame = (local_ns / FRAME_DURATION_NS) as u32;
        let local_remainder_ns = local_ns % FRAME_DURATION_NS;
        let local_ticks = (local_remainder_ns as f64 / NS_PER_MICROTICK) as u16;

        // Update stats mutex
        let stats_mutex = match SHARED_STATS.get() {
            Some(m) => m,
            None => continue,
        };

        let mut stats = stats_mutex.lock();
        stats.packet_count += 1;
        stats.master = packet_master;

        // Feed remote and local frame/ticks to servo
        let batch_complete = servo.update(
            packet.frame,
            packet.microticks,
            local_frame,
            local_ticks,
        );

        // Update stats
        stats.offset_ns = servo.offset_ns();
        stats.frequency_ppm = servo.frequency_ppm();
        stats.locked = servo.is_locked();

        // State machine based on sample count
        if servo.sample_count() < 10 {
            stats.state = LwState::Uncalibrated;
        } else if batch_complete || stats.state == LwState::Listening {
            stats.state = LwState::Slave;
        }

        update_stats_string(&stats);
    }

    // Leave multicast group on exit
    let _ = socket.leave_multicast_v4(&MULTICAST_CLOCK, &interface_ip);
}

/// Create and configure a multicast UDP socket
fn create_multicast_socket(interface_ip: Ipv4Addr, multicast_addr: Ipv4Addr) -> Result<Socket, String> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .map_err(|e| format!("Socket creation failed: {}", e))?;

    socket
        .set_reuse_address(true)
        .map_err(|e| format!("set_reuse_address failed: {}", e))?;

    // Bind to the port on all interfaces (required for Windows multicast)
    let bind_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, LIVEWIRE_PORT);
    socket
        .bind(&bind_addr.into())
        .map_err(|e| format!("Bind failed: {}", e))?;

    // Join multicast group on specific interface
    socket
        .join_multicast_v4(&multicast_addr, &interface_ip)
        .map_err(|e| format!("Join multicast failed: {}", e))?;

    Ok(socket)
}
