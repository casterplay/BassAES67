//! PTP (IEEE 1588v2) client for AES67 clock synchronization.
//!
//! Provides a lightweight PTP slave implementation that tracks offset
//! and frequency from a network grandmaster clock.

use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::messages::*;
use crate::servo::PtpServo;
use crate::stats::{PtpState, PtpStats};
use crate::{platform, stats};

/// PTP multicast address
const PTP_MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 1, 129);
/// PTP event port (Sync, Delay_Req)
const PTP_EVENT_PORT: u16 = 319;
/// PTP general port (Announce, Follow_Up, Delay_Resp)
const PTP_GENERAL_PORT: u16 = 320;

/// Global PTP client instance
static PTP_CLIENT: OnceLock<Mutex<Option<PtpClientHandle>>> = OnceLock::new();

/// Reference count for Start/Stop calls
static REFERENCE_COUNT: AtomicU32 = AtomicU32::new(0);

/// Handle to a running PTP client
struct PtpClientHandle {
    running: Arc<AtomicBool>,
    stats: Arc<Mutex<PtpStats>>,
    event_thread: Option<JoinHandle<()>>,
    general_thread: Option<JoinHandle<()>>,
}

/// Shared state between PTP threads
struct PtpSharedState {
    /// Current grandmaster info
    grandmaster: Option<PortIdentity>,
    /// PI servo for offset/frequency
    servo: PtpServo,
    /// Current statistics
    stats: PtpStats,
    /// Pending Sync data (waiting for Follow_Up)
    pending_sync: Option<PendingSyncData>,
    /// Pending Delay_Req data (waiting for Delay_Resp)
    pending_delay: Option<PendingDelayData>,
    /// Our local port identity
    local_port: PortIdentity,
    /// Delay request sequence counter
    delay_req_seq: u16,
    /// Socket for sending Delay_Req
    event_socket: Option<UdpSocket>,
    /// Initial offset baseline (for relative tracking)
    /// PTP grandmasters may use TAI while local clock uses UTC
    initial_offset_ns: Option<i64>,
    /// Last (t2 - t1) measurement for path delay calculation
    last_sync_diff_ns: i64,
}

/// Data from a Sync message waiting for Follow_Up
struct PendingSyncData {
    sequence_id: u16,
    receive_time_ns: i64,
}

/// Data from a Delay_Req waiting for Delay_Resp
struct PendingDelayData {
    sequence_id: u16,
    send_time_ns: i64,
}

/// Start the global PTP client (with reference counting)
pub fn start_ptp_client(interface: Ipv4Addr, domain: u8) -> Result<(), String> {
    // Increment reference count
    let prev_count = REFERENCE_COUNT.fetch_add(1, Ordering::SeqCst);

    // If already running, just return success
    if prev_count > 0 {
        return Ok(());
    }

    let client_mutex = PTP_CLIENT.get_or_init(|| Mutex::new(None));
    let mut client_guard = client_mutex
        .lock()
        .map_err(|_| "Failed to lock PTP client mutex")?;

    // Check if already running (shouldn't happen with ref counting, but be safe)
    if client_guard.is_some() {
        return Ok(());
    }

    // Create shared state
    let stats = Arc::new(Mutex::new(PtpStats {
        state: PtpState::Listening,
        domain,
        ..Default::default()
    }));
    let state = Arc::new(Mutex::new(PtpSharedState {
        grandmaster: None,
        servo: PtpServo::new(),
        stats: PtpStats {
            state: PtpState::Listening,
            domain,
            ..Default::default()
        },
        pending_sync: None,
        pending_delay: None,
        local_port: generate_local_port_identity(),
        delay_req_seq: 0,
        event_socket: None,
        initial_offset_ns: None,
        last_sync_diff_ns: 0,
    }));

    let running = Arc::new(AtomicBool::new(true));

    // Create event socket (port 319)
    let event_socket = create_ptp_socket(interface, PTP_EVENT_PORT)?;

    // Store socket in shared state for sending Delay_Req
    if let Ok(mut s) = state.lock() {
        s.event_socket = Some(event_socket.try_clone().map_err(|e| e.to_string())?);
    }

    // Create general socket (port 320)
    let general_socket = create_ptp_socket(interface, PTP_GENERAL_PORT)?;

    // Start event thread (Sync messages)
    let event_running = running.clone();
    let event_state = state.clone();
    let event_stats = stats.clone();
    let event_domain = domain;
    let event_thread = thread::spawn(move || {
        run_event_thread(event_socket, event_running, event_state, event_stats, event_domain);
    });

    // Start general thread (Announce, Follow_Up, Delay_Resp)
    let general_running = running.clone();
    let general_state = state.clone();
    let general_stats = stats.clone();
    let general_domain = domain;
    let general_thread = thread::spawn(move || {
        run_general_thread(general_socket, general_running, general_state, general_stats, general_domain);
    });

    *client_guard = Some(PtpClientHandle {
        running,
        stats,
        event_thread: Some(event_thread),
        general_thread: Some(general_thread),
    });

    Ok(())
}

/// Stop the global PTP client (with reference counting)
pub fn stop_ptp_client() {
    // Decrement reference count
    let prev_count = REFERENCE_COUNT.fetch_sub(1, Ordering::SeqCst);

    // Only stop if this was the last reference
    if prev_count != 1 {
        return;
    }

    let client_mutex = match PTP_CLIENT.get() {
        Some(m) => m,
        None => return,
    };

    let mut client_guard = match client_mutex.lock() {
        Ok(g) => g,
        Err(_) => return,
    };

    if let Some(mut handle) = client_guard.take() {
        handle.running.store(false, Ordering::SeqCst);

        if let Some(thread) = handle.event_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = handle.general_thread.take() {
            let _ = thread.join();
        }
    }
}

/// Force stop the PTP client regardless of reference count
pub fn force_stop_ptp_client() {
    REFERENCE_COUNT.store(0, Ordering::SeqCst);

    let client_mutex = match PTP_CLIENT.get() {
        Some(m) => m,
        None => return,
    };

    let mut client_guard = match client_mutex.lock() {
        Ok(g) => g,
        Err(_) => return,
    };

    if let Some(mut handle) = client_guard.take() {
        handle.running.store(false, Ordering::SeqCst);

        if let Some(thread) = handle.event_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = handle.general_thread.take() {
            let _ = thread.join();
        }
    }
}

/// Get current PTP statistics
pub fn get_ptp_stats() -> Option<PtpStats> {
    let client_mutex = PTP_CLIENT.get()?;
    let client_guard = client_mutex.lock().ok()?;
    let handle = client_guard.as_ref()?;
    let stats = handle.stats.lock().ok()?;
    Some(stats.clone())
}

/// Check if PTP client is running
pub fn is_ptp_running() -> bool {
    let client_mutex = match PTP_CLIENT.get() {
        Some(m) => m,
        None => return false,
    };
    let client_guard = match client_mutex.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    client_guard.is_some()
}

/// Get current offset in nanoseconds
pub fn get_offset_ns() -> i64 {
    get_ptp_stats().map(|s| s.offset_ns).unwrap_or(0)
}

/// Get current frequency adjustment in ppm
pub fn get_frequency_ppm() -> f64 {
    get_ptp_stats().map(|s| s.frequency_ppm).unwrap_or(0.0)
}

/// Create a PTP multicast socket
fn create_ptp_socket(interface: Ipv4Addr, port: u16) -> Result<UdpSocket, String> {
    // Bind to INADDR_ANY with the port
    let socket_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port);
    let socket = UdpSocket::bind(socket_addr)
        .map_err(|e| format!("Failed to bind PTP socket on port {}: {}", port, e))?;

    // Join PTP multicast group
    socket
        .join_multicast_v4(&PTP_MULTICAST_ADDR, &interface)
        .map_err(|e| format!("Failed to join PTP multicast group: {}", e))?;

    // Set read timeout for clean shutdown
    socket
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|e| format!("Failed to set socket timeout: {}", e))?;

    Ok(socket)
}

/// Generate a local port identity based on system info
fn generate_local_port_identity() -> PortIdentity {
    // Use a simple hash of current time as a pseudo-random ID
    // In production, this should use MAC address
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    let mut id = [0u8; 8];
    id[0..8].copy_from_slice(&(now as u64).to_be_bytes());

    PortIdentity {
        clock_identity: ClockIdentity(id),
        port_number: 1,
    }
}

/// Event thread - handles Sync messages on port 319
fn run_event_thread(
    socket: UdpSocket,
    running: Arc<AtomicBool>,
    state: Arc<Mutex<PtpSharedState>>,
    stats: Arc<Mutex<PtpStats>>,
    domain: u8,
) {
    let mut buf = [0u8; 1024];

    while running.load(Ordering::SeqCst) {
        match socket.recv(&mut buf) {
            Ok(len) => {
                let receive_time = platform::get_timestamp_ns();

                if let Some(header) = PtpHeader::parse(&buf[..len]) {
                    // Filter by domain
                    if header.domain_number != domain {
                        continue;
                    }

                    match header.message_type {
                        PtpMessageType::Sync => {
                            if let Some(sync) = SyncMessage::parse(&buf[..len]) {
                                handle_sync(&state, &stats, &sync, receive_time);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(_) => break,
        }
    }
}

/// General thread - handles Announce, Follow_Up, Delay_Resp on port 320
fn run_general_thread(
    socket: UdpSocket,
    running: Arc<AtomicBool>,
    state: Arc<Mutex<PtpSharedState>>,
    stats: Arc<Mutex<PtpStats>>,
    domain: u8,
) {
    let mut buf = [0u8; 1024];

    while running.load(Ordering::SeqCst) {
        match socket.recv(&mut buf) {
            Ok(len) => {
                if let Some(header) = PtpHeader::parse(&buf[..len]) {
                    // Filter by domain
                    if header.domain_number != domain {
                        continue;
                    }

                    match header.message_type {
                        PtpMessageType::Announce => {
                            if let Some(announce) = AnnounceMessage::parse(&buf[..len]) {
                                handle_announce(&state, &stats, &announce);
                            }
                        }
                        PtpMessageType::FollowUp => {
                            if let Some(follow_up) = FollowUpMessage::parse(&buf[..len]) {
                                handle_follow_up(&state, &stats, &follow_up);
                            }
                        }
                        PtpMessageType::DelayResp => {
                            if let Some(delay_resp) = DelayRespMessage::parse(&buf[..len]) {
                                handle_delay_resp(&state, &stats, &delay_resp);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(_) => break,
        }
    }
}

/// Handle Announce message - select grandmaster
fn handle_announce(
    state: &Arc<Mutex<PtpSharedState>>,
    stats: &Arc<Mutex<PtpStats>>,
    announce: &AnnounceMessage,
) {
    let mut s = match state.lock() {
        Ok(s) => s,
        Err(_) => return,
    };

    // Update grandmaster (simple: just accept first/any announce)
    // A full implementation would run BMCA here
    let gm_port = announce.header.source_port_identity;

    s.grandmaster = Some(gm_port);
    s.stats.grandmaster_id = announce.grandmaster_identity;
    s.stats.grandmaster_port = gm_port.port_number;
    s.stats.clock_class = announce.grandmaster_clock_quality.clock_class;
    s.stats.announce_count += 1;

    if s.stats.state == PtpState::Listening {
        s.stats.state = PtpState::Uncalibrated;
    }

    // Update external stats
    if let Ok(mut ext_stats) = stats.lock() {
        ext_stats.grandmaster_id = s.stats.grandmaster_id;
        ext_stats.grandmaster_port = s.stats.grandmaster_port;
        ext_stats.clock_class = s.stats.clock_class;
        ext_stats.announce_count = s.stats.announce_count;
        ext_stats.state = s.stats.state;
    }

    // Update stats string
    stats::update_stats_string(&s.stats);
}

/// Handle Sync message - record receive time
fn handle_sync(
    state: &Arc<Mutex<PtpSharedState>>,
    stats: &Arc<Mutex<PtpStats>>,
    sync: &SyncMessage,
    receive_time_ns: i64,
) {
    let mut s = match state.lock() {
        Ok(s) => s,
        Err(_) => return,
    };

    // Check if from our grandmaster
    if let Some(ref gm) = s.grandmaster {
        if sync.header.source_port_identity != *gm {
            return;
        }
    } else {
        return;
    }

    // Store pending sync data for Follow_Up
    s.pending_sync = Some(PendingSyncData {
        sequence_id: sync.header.sequence_id,
        receive_time_ns,
    });

    s.stats.sync_count += 1;

    if let Ok(mut ext_stats) = stats.lock() {
        ext_stats.sync_count = s.stats.sync_count;
    }
}

/// Handle Follow_Up message - calculate offset
fn handle_follow_up(
    state: &Arc<Mutex<PtpSharedState>>,
    stats: &Arc<Mutex<PtpStats>>,
    follow_up: &FollowUpMessage,
) {
    let mut s = match state.lock() {
        Ok(s) => s,
        Err(_) => return,
    };

    // Check if from our grandmaster
    if let Some(ref gm) = s.grandmaster {
        if follow_up.header.source_port_identity != *gm {
            return;
        }
    } else {
        return;
    }

    // Match with pending Sync
    let pending = match s.pending_sync.take() {
        Some(p) if p.sequence_id == follow_up.header.sequence_id => p,
        other => {
            s.pending_sync = other;
            return;
        }
    };

    s.stats.follow_up_count += 1;

    // t1 = master send time (from Follow_Up)
    let t1_ns = follow_up.precise_origin_timestamp.to_ns();
    // t2 = slave receive time (recorded when Sync arrived)
    let t2_ns = pending.receive_time_ns;

    // Calculate raw (t2 - t1) for path delay calculation
    // This is: forward_delay + clock_offset
    let raw_sync_diff_ns = t2_ns - t1_ns;
    s.last_sync_diff_ns = raw_sync_diff_ns;

    // For software PTP (no system clock discipline), we track RELATIVE offset.
    //
    // The raw (t2 - t1) value contains:
    //   1. Epoch difference between TAI (grandmaster) and local clock (~37 leap seconds)
    //   2. Network path delay (~100-500µs on LAN)
    //   3. Actual clock offset (what we want to track)
    //
    // Since we can't sync the system clock, we only care about CHANGE in offset.
    // The epoch difference and path delay are constant, so they cancel out when
    // we take the difference between measurements.
    //
    // This is similar to how Omnia and other software PTP displays work.

    // Use first measurement as baseline (includes epoch diff + path delay)
    if s.initial_offset_ns.is_none() {
        s.initial_offset_ns = Some(raw_sync_diff_ns);
    }

    // Relative offset = change from baseline
    // This gives us just the clock drift component
    let offset_ns = raw_sync_diff_ns - s.initial_offset_ns.unwrap_or(raw_sync_diff_ns);

    // Path delay for display (from Delay_Req/Resp RTT measurement)
    let path_delay = s.stats.mean_path_delay_ns;

    // Update servo with the relative offset
    s.servo.update(offset_ns, path_delay);

    // Update stats
    s.stats.offset_ns = s.servo.offset_ns();
    s.stats.frequency_ppm = s.servo.frequency_ppm();
    s.stats.locked = s.servo.is_locked();

    if s.stats.state == PtpState::Uncalibrated && s.stats.sync_count > 5 {
        s.stats.state = PtpState::Slave;
    }

    // Send Delay_Req periodically (every 8 syncs)
    if s.stats.sync_count % 8 == 0 {
        send_delay_req(&mut s);
    }

    // Update external stats
    if let Ok(mut ext_stats) = stats.lock() {
        ext_stats.offset_ns = s.stats.offset_ns;
        ext_stats.frequency_ppm = s.stats.frequency_ppm;
        ext_stats.mean_path_delay_ns = s.stats.mean_path_delay_ns;
        ext_stats.locked = s.stats.locked;
        ext_stats.state = s.stats.state;
        ext_stats.follow_up_count = s.stats.follow_up_count;
    }

    // Update stats string
    stats::update_stats_string(&s.stats);
}

/// Send a Delay_Req message
fn send_delay_req(state: &mut PtpSharedState) {
    let socket = match &state.event_socket {
        Some(s) => s,
        None => return,
    };

    state.delay_req_seq = state.delay_req_seq.wrapping_add(1);

    let msg = DelayReqMessage::new(
        state.local_port,
        state.delay_req_seq,
        state.stats.domain,
    );

    let send_time = platform::get_timestamp_ns();
    let dest = SocketAddrV4::new(PTP_MULTICAST_ADDR, PTP_EVENT_PORT);

    if socket.send_to(&msg.to_bytes(), dest).is_ok() {
        state.pending_delay = Some(PendingDelayData {
            sequence_id: state.delay_req_seq,
            send_time_ns: send_time,
        });
    }
}

/// Handle Delay_Resp message - update delay count
/// Note: We can't calculate true path delay because master timestamps (TAI)
/// and local timestamps (Unix) use different epochs.
fn handle_delay_resp(
    state: &Arc<Mutex<PtpSharedState>>,
    stats: &Arc<Mutex<PtpStats>>,
    delay_resp: &DelayRespMessage,
) {
    let mut s = match state.lock() {
        Ok(s) => s,
        Err(_) => return,
    };

    // Check if this is for us
    if delay_resp.requesting_port_identity != s.local_port {
        return;
    }

    // Match with pending Delay_Req
    let pending = match s.pending_delay.take() {
        Some(p) if p.sequence_id == delay_resp.header.sequence_id => p,
        other => {
            s.pending_delay = other;
            return;
        }
    };

    s.stats.delay_resp_count += 1;

    // For software PTP, we use a fixed path delay estimate.
    // On a typical switched LAN, path delay is 100-500µs.
    // This is only used for display - the offset tracking doesn't depend on it.
    // We measure round-trip time from local timestamps only:
    let now_ns = platform::get_timestamp_ns();
    let rtt_ns = now_ns - pending.send_time_ns;
    // Estimate one-way delay as half of RTT
    s.stats.mean_path_delay_ns = rtt_ns / 2;

    // Update external stats
    if let Ok(mut ext_stats) = stats.lock() {
        ext_stats.mean_path_delay_ns = s.stats.mean_path_delay_ns;
        ext_stats.delay_resp_count = s.stats.delay_resp_count;
    }
}
