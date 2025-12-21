//! Raw FFI bindings to libsrt (Haivision SRT library).
//! Based on srt.h header file.

use std::ffi::{c_char, c_int, c_void};

// SRT socket type (int32_t in C)
pub type SRTSOCKET = i32;

// Invalid socket constant
pub const SRT_INVALID_SOCK: SRTSOCKET = -1;
pub const SRT_ERROR: c_int = -1;

// Socket status enum
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SrtSockStatus {
    Init = 1,
    Opened,
    Listening,
    Connecting,
    Connected,
    Broken,
    Closing,
    Closed,
    NonExist,
}

// Socket options enum (commonly used ones)
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SrtSockOpt {
    Mss = 0,              // Maximum Segment Size
    SndSyn = 1,           // Sending is blocking
    RcvSyn = 2,           // Receiving is blocking
    Isn = 3,              // Initial Sequence Number
    Fc = 4,               // Flight flag size (window size)
    SndBuf = 5,           // Send buffer size
    RcvBuf = 6,           // Receive buffer size
    Linger = 7,           // Linger on close
    UdpSndBuf = 8,        // UDP send buffer
    UdpRcvBuf = 9,        // UDP receive buffer
    Rendezvous = 12,      // Rendezvous mode
    SndTimeo = 13,        // Send timeout
    RcvTimeo = 14,        // Receive timeout
    ReuseAddr = 15,       // Reuse address
    MaxBw = 16,           // Maximum bandwidth
    State = 17,           // Socket state (read-only)
    Event = 18,           // Pending events
    SndData = 19,         // Data in send buffer
    RcvData = 20,         // Data in receive buffer
    Sender = 21,          // Sender mode
    TsbPdMode = 22,       // Timestamp-based packet delivery
    Latency = 23,         // Latency (ms)
    InputBw = 24,         // Input bandwidth
    OHeadBw = 25,         // Overhead bandwidth
    Passphrase = 26,      // Encryption passphrase
    PbKeyLen = 27,        // Passphrase key length
    IpTtl = 29,           // IP TTL
    IpTos = 30,           // IP TOS
    TlPktDrop = 31,       // Too-late packet drop
    SndNakReport = 32,    // Send periodic NAK reports
    Version = 33,         // SRT version
    RcvLatency = 34,      // Receiver latency
    PeerLatency = 35,     // Peer latency
    MessageApi = 36,      // Message API
    PayloadSize = 37,     // Payload size
    KmRefreshRate = 39,   // Key refresh rate
    KmPreAnnounce = 40,   // Key pre-announce
    EnforcedEncryption = 41, // Enforced encryption
    IpV6Only = 42,        // IPv6 only
    PeerIdleTimeo = 43,   // Peer idle timeout
    BindToDevice = 44,    // Bind to device
    Transtype = 50,       // Transmission type (replaces old incorrect value 38)
    PacketFilter = 60,    // Packet filter
    Retransmitalgo = 61,  // Retransmit algorithm
    StreamId = 46,        // Stream ID
}

// Transmission type
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SrtTranstype {
    Live = 0,
    File = 1,
    Invalid = 2,
}

// SRT statistics structure (simplified - commonly used fields)
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct SrtTraceStats {
    // Global measurements
    pub ms_time_stamp: i64,           // Time since SRT started (ms)
    pub pkt_sent_total: i64,          // Total packets sent
    pub pkt_recv_total: i64,          // Total packets received
    pub pkt_sent_loss_total: i32,     // Total lost packets (sender)
    pub pkt_recv_loss_total: i32,     // Total lost packets (receiver)
    pub pkt_retrans_total: i32,       // Total retransmitted packets
    pub pkt_sent_ack_total: i32,      // Total ACK packets sent
    pub pkt_recv_ack_total: i32,      // Total ACK packets received
    pub pkt_sent_nak_total: i32,      // Total NAK packets sent
    pub pkt_recv_nak_total: i32,      // Total NAK packets received
    pub us_snd_duration_total: i64,   // Total time spent sending (us)

    // Instant measurements
    pub pkt_sent: i64,                // Packets sent
    pub pkt_recv: i64,                // Packets received
    pub pkt_sent_loss: i32,           // Lost packets (sender)
    pub pkt_recv_loss: i32,           // Lost packets (receiver)
    pub pkt_retrans: i32,             // Retransmitted packets
    pub pkt_recv_retrans: i32,        // Received retransmitted
    pub pkt_sent_ack: i32,            // ACK packets sent
    pub pkt_recv_ack: i32,            // ACK packets received
    pub pkt_sent_nak: i32,            // NAK packets sent
    pub pkt_recv_nak: i32,            // NAK packets received
    pub mbs_send_rate: f64,           // Send rate (Mbps)
    pub mbs_recv_rate: f64,           // Receive rate (Mbps)
    pub us_snd_duration: i64,         // Time spent sending (us)
    pub pkt_reorder_distance: i32,    // Reorder distance
    pub pkt_recv_avg_belated_time: f64, // Average belated time
    pub pkt_recv_belated: i64,        // Belated packets

    // Sender side
    pub pkt_snd_filter_extra: i32,    // Filter overhead (sender)
    pub pkt_rcv_filter_extra: i32,    // Filter overhead (receiver)
    pub pkt_rcv_filter_supply: i32,   // Filter recovery (receiver)
    pub pkt_rcv_filter_loss: i32,     // Filter loss (receiver)

    // Buffer info
    pub pkt_snd_buf: i32,             // Packets in send buffer
    pub byte_snd_buf: i32,            // Bytes in send buffer
    pub ms_snd_buf: i32,              // Send buffer delay (ms)
    pub ms_snd_tsbpd_delay: i32,      // Send TSBPD delay
    pub pkt_rcv_buf: i32,             // Packets in receive buffer
    pub byte_rcv_buf: i32,            // Bytes in receive buffer
    pub ms_rcv_buf: i32,              // Receive buffer delay (ms)
    pub ms_rcv_tsbpd_delay: i32,      // Receive TSBPD delay

    // Connection info
    pub pkt_flight_size: i32,         // Packets in flight
    pub ms_rtt: f64,                  // Round-trip time (ms)
    pub mbs_bandwidth: f64,           // Estimated bandwidth (Mbps)
    pub byte_avail_snd_buf: i32,      // Available send buffer (bytes)
    pub byte_avail_rcv_buf: i32,      // Available receive buffer (bytes)
    pub mbs_max_bw: f64,              // Maximum bandwidth (Mbps)
    pub byte_mss: i32,                // MSS (bytes)
    pub pkt_snd_period: f64,          // Send period (us)
    pub pkt_flow_window: i32,         // Flow window
    pub pkt_congestion_window: i32,   // Congestion window
    pub pkt_recv_undecrypt: i32,      // Undecrypted packets
    pub byte_recv_undecrypt: i64,     // Undecrypted bytes
    pub byte_recv_undecrypt_total: i64, // Total undecrypted bytes
}

// Sockaddr structures for network addressing
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SockaddrIn {
    pub sin_family: u16,
    pub sin_port: u16,
    pub sin_addr: u32,
    pub sin_zero: [u8; 8],
}

impl SockaddrIn {
    pub fn new(addr: u32, port: u16) -> Self {
        Self {
            sin_family: 2, // AF_INET
            sin_port: port.to_be(),
            sin_addr: addr.to_be(),
            sin_zero: [0; 8],
        }
    }

    pub fn from_parts(a: u8, b: u8, c: u8, d: u8, port: u16) -> Self {
        let addr = u32::from_be_bytes([a, b, c, d]);
        Self::new(addr, port)
    }
}

// Generic sockaddr (for function signatures)
#[repr(C)]
pub struct Sockaddr {
    pub sa_family: u16,
    pub sa_data: [u8; 14],
}

// Link to libsrt
#[cfg(target_os = "linux")]
#[link(name = "srt-gnutls")]
extern "C" {
    // Library initialization
    pub fn srt_startup() -> c_int;
    pub fn srt_cleanup() -> c_int;

    // Socket creation and lifecycle
    pub fn srt_create_socket() -> SRTSOCKET;
    pub fn srt_close(sock: SRTSOCKET) -> c_int;

    // Connection functions
    pub fn srt_bind(sock: SRTSOCKET, name: *const Sockaddr, namelen: c_int) -> c_int;
    pub fn srt_listen(sock: SRTSOCKET, backlog: c_int) -> c_int;
    pub fn srt_accept(sock: SRTSOCKET, addr: *mut Sockaddr, addrlen: *mut c_int) -> SRTSOCKET;
    pub fn srt_connect(sock: SRTSOCKET, name: *const Sockaddr, namelen: c_int) -> c_int;

    // Data transfer
    pub fn srt_send(sock: SRTSOCKET, buf: *const c_char, len: c_int) -> c_int;
    pub fn srt_recv(sock: SRTSOCKET, buf: *mut c_char, len: c_int) -> c_int;
    pub fn srt_sendmsg(sock: SRTSOCKET, buf: *const c_char, len: c_int, ttl: c_int, inorder: c_int) -> c_int;
    pub fn srt_recvmsg(sock: SRTSOCKET, buf: *mut c_char, len: c_int) -> c_int;

    // Socket options
    pub fn srt_setsockopt(sock: SRTSOCKET, level: c_int, optname: SrtSockOpt, optval: *const c_void, optlen: c_int) -> c_int;
    pub fn srt_getsockopt(sock: SRTSOCKET, level: c_int, optname: SrtSockOpt, optval: *mut c_void, optlen: *mut c_int) -> c_int;
    pub fn srt_setsockflag(sock: SRTSOCKET, opt: SrtSockOpt, optval: *const c_void, optlen: c_int) -> c_int;
    pub fn srt_getsockflag(sock: SRTSOCKET, opt: SrtSockOpt, optval: *mut c_void, optlen: *mut c_int) -> c_int;

    // Socket state
    pub fn srt_getsockstate(sock: SRTSOCKET) -> SrtSockStatus;
    pub fn srt_getpeername(sock: SRTSOCKET, name: *mut Sockaddr, namelen: *mut c_int) -> c_int;
    pub fn srt_getsockname(sock: SRTSOCKET, name: *mut Sockaddr, namelen: *mut c_int) -> c_int;

    // Statistics
    pub fn srt_bstats(sock: SRTSOCKET, perf: *mut SrtTraceStats, clear: c_int) -> c_int;
    pub fn srt_bistats(sock: SRTSOCKET, perf: *mut SrtTraceStats, clear: c_int, instantaneous: c_int) -> c_int;

    // Error handling
    pub fn srt_getlasterror(errno_loc: *mut c_int) -> c_int;
    pub fn srt_getlasterror_str() -> *const c_char;
    pub fn srt_strerror(code: c_int, errnoval: c_int) -> *const c_char;
    pub fn srt_clearlasterror();
}

// Windows uses different library name
#[cfg(target_os = "windows")]
#[link(name = "srt")]
extern "C" {
    pub fn srt_startup() -> c_int;
    pub fn srt_cleanup() -> c_int;
    pub fn srt_create_socket() -> SRTSOCKET;
    pub fn srt_close(sock: SRTSOCKET) -> c_int;
    pub fn srt_bind(sock: SRTSOCKET, name: *const Sockaddr, namelen: c_int) -> c_int;
    pub fn srt_listen(sock: SRTSOCKET, backlog: c_int) -> c_int;
    pub fn srt_accept(sock: SRTSOCKET, addr: *mut Sockaddr, addrlen: *mut c_int) -> SRTSOCKET;
    pub fn srt_connect(sock: SRTSOCKET, name: *const Sockaddr, namelen: c_int) -> c_int;
    pub fn srt_send(sock: SRTSOCKET, buf: *const c_char, len: c_int) -> c_int;
    pub fn srt_recv(sock: SRTSOCKET, buf: *mut c_char, len: c_int) -> c_int;
    pub fn srt_sendmsg(sock: SRTSOCKET, buf: *const c_char, len: c_int, ttl: c_int, inorder: c_int) -> c_int;
    pub fn srt_recvmsg(sock: SRTSOCKET, buf: *mut c_char, len: c_int) -> c_int;
    pub fn srt_setsockopt(sock: SRTSOCKET, level: c_int, optname: SrtSockOpt, optval: *const c_void, optlen: c_int) -> c_int;
    pub fn srt_getsockopt(sock: SRTSOCKET, level: c_int, optname: SrtSockOpt, optval: *mut c_void, optlen: *mut c_int) -> c_int;
    pub fn srt_setsockflag(sock: SRTSOCKET, opt: SrtSockOpt, optval: *const c_void, optlen: c_int) -> c_int;
    pub fn srt_getsockflag(sock: SRTSOCKET, opt: SrtSockOpt, optval: *mut c_void, optlen: *mut c_int) -> c_int;
    pub fn srt_getsockstate(sock: SRTSOCKET) -> SrtSockStatus;
    pub fn srt_getpeername(sock: SRTSOCKET, name: *mut Sockaddr, namelen: *mut c_int) -> c_int;
    pub fn srt_getsockname(sock: SRTSOCKET, name: *mut Sockaddr, namelen: *mut c_int) -> c_int;
    pub fn srt_bstats(sock: SRTSOCKET, perf: *mut SrtTraceStats, clear: c_int) -> c_int;
    pub fn srt_bistats(sock: SRTSOCKET, perf: *mut SrtTraceStats, clear: c_int, instantaneous: c_int) -> c_int;
    pub fn srt_getlasterror(errno_loc: *mut c_int) -> c_int;
    pub fn srt_getlasterror_str() -> *const c_char;
    pub fn srt_strerror(code: c_int, errnoval: c_int) -> *const c_char;
    pub fn srt_clearlasterror();
}

// Helper functions for safe Rust usage

// Initialize the SRT library (call once at startup)
pub fn startup() -> Result<(), i32> {
    let result = unsafe { srt_startup() };
    if result == 0 {
        Ok(())
    } else {
        Err(result)
    }
}

// Cleanup the SRT library (call once at shutdown)
pub fn cleanup() -> Result<(), i32> {
    let result = unsafe { srt_cleanup() };
    if result == 0 {
        Ok(())
    } else {
        Err(result)
    }
}

// Create a new SRT socket
pub fn create_socket() -> Result<SRTSOCKET, i32> {
    let sock = unsafe { srt_create_socket() };
    if sock == SRT_INVALID_SOCK {
        Err(get_last_error())
    } else {
        Ok(sock)
    }
}

// Close an SRT socket
pub fn close(sock: SRTSOCKET) -> Result<(), i32> {
    let result = unsafe { srt_close(sock) };
    if result == SRT_ERROR {
        Err(get_last_error())
    } else {
        Ok(())
    }
}

// Connect to an SRT server (caller mode)
pub fn connect(sock: SRTSOCKET, addr: &SockaddrIn) -> Result<(), i32> {
    let result = unsafe {
        srt_connect(
            sock,
            addr as *const SockaddrIn as *const Sockaddr,
            std::mem::size_of::<SockaddrIn>() as c_int,
        )
    };
    if result == SRT_ERROR {
        Err(get_last_error())
    } else {
        Ok(())
    }
}

// Receive data from SRT socket
pub fn recv(sock: SRTSOCKET, buf: &mut [u8]) -> Result<usize, i32> {
    let result = unsafe { srt_recv(sock, buf.as_mut_ptr() as *mut c_char, buf.len() as c_int) };
    if result == SRT_ERROR {
        Err(get_last_error())
    } else {
        Ok(result as usize)
    }
}

// Send data to SRT socket
pub fn send(sock: SRTSOCKET, buf: &[u8]) -> Result<usize, i32> {
    let result = unsafe { srt_send(sock, buf.as_ptr() as *const c_char, buf.len() as c_int) };
    if result == SRT_ERROR {
        Err(get_last_error())
    } else {
        Ok(result as usize)
    }
}

// Get socket state
pub fn get_sock_state(sock: SRTSOCKET) -> SrtSockStatus {
    unsafe { srt_getsockstate(sock) }
}

// Get last error code
pub fn get_last_error() -> i32 {
    let mut errno = 0;
    unsafe { srt_getlasterror(&mut errno) }
}

// Get last error as string
pub fn get_last_error_str() -> String {
    unsafe {
        let ptr = srt_getlasterror_str();
        if ptr.is_null() {
            "Unknown error".to_string()
        } else {
            std::ffi::CStr::from_ptr(ptr)
                .to_string_lossy()
                .into_owned()
        }
    }
}

// Set socket option (generic)
pub fn set_sock_opt<T>(sock: SRTSOCKET, opt: SrtSockOpt, value: &T) -> Result<(), i32> {
    let result = unsafe {
        srt_setsockflag(
            sock,
            opt,
            value as *const T as *const c_void,
            std::mem::size_of::<T>() as c_int,
        )
    };
    if result == SRT_ERROR {
        Err(get_last_error())
    } else {
        Ok(())
    }
}

// Set latency option (common operation)
pub fn set_latency(sock: SRTSOCKET, latency_ms: i32) -> Result<(), i32> {
    set_sock_opt(sock, SrtSockOpt::Latency, &latency_ms)
}

// Set transmission type
pub fn set_transtype(sock: SRTSOCKET, transtype: SrtTranstype) -> Result<(), i32> {
    // SRT expects an i32 value, not the enum directly
    let value = transtype as i32;
    set_sock_opt(sock, SrtSockOpt::Transtype, &value)
}

// Set receive buffer size
pub fn set_rcvbuf(sock: SRTSOCKET, size: i32) -> Result<(), i32> {
    set_sock_opt(sock, SrtSockOpt::RcvBuf, &size)
}

// Set send buffer size
pub fn set_sndbuf(sock: SRTSOCKET, size: i32) -> Result<(), i32> {
    set_sock_opt(sock, SrtSockOpt::SndBuf, &size)
}

// Get statistics
pub fn get_stats(sock: SRTSOCKET, clear: bool) -> Result<SrtTraceStats, i32> {
    let mut stats = SrtTraceStats::default();
    let result = unsafe { srt_bstats(sock, &mut stats, if clear { 1 } else { 0 }) };
    if result == SRT_ERROR {
        Err(get_last_error())
    } else {
        Ok(stats)
    }
}

// Set string socket option (for passphrase, streamid)
pub fn set_sock_opt_str(sock: SRTSOCKET, opt: SrtSockOpt, value: &str) -> Result<(), i32> {
    let cstr = match std::ffi::CString::new(value) {
        Ok(s) => s,
        Err(_) => return Err(-1),
    };
    let result = unsafe {
        srt_setsockflag(sock, opt, cstr.as_ptr() as *const c_void, value.len() as c_int)
    };
    if result == SRT_ERROR {
        Err(get_last_error())
    } else {
        Ok(())
    }
}

// Set passphrase for encryption
pub fn set_passphrase(sock: SRTSOCKET, passphrase: &str) -> Result<(), i32> {
    set_sock_opt_str(sock, SrtSockOpt::Passphrase, passphrase)
}

// Set stream ID
pub fn set_streamid(sock: SRTSOCKET, streamid: &str) -> Result<(), i32> {
    set_sock_opt_str(sock, SrtSockOpt::StreamId, streamid)
}

// Set rendezvous mode
pub fn set_rendezvous(sock: SRTSOCKET, enabled: bool) -> Result<(), i32> {
    let value: i32 = if enabled { 1 } else { 0 };
    set_sock_opt(sock, SrtSockOpt::Rendezvous, &value)
}

// Bind socket to local address (for listener/rendezvous)
pub fn bind(sock: SRTSOCKET, addr: &SockaddrIn) -> Result<(), i32> {
    let result = unsafe {
        srt_bind(
            sock,
            addr as *const SockaddrIn as *const Sockaddr,
            std::mem::size_of::<SockaddrIn>() as c_int,
        )
    };
    if result == SRT_ERROR {
        Err(get_last_error())
    } else {
        Ok(())
    }
}

// Listen for incoming connections
pub fn listen(sock: SRTSOCKET, backlog: i32) -> Result<(), i32> {
    let result = unsafe { srt_listen(sock, backlog) };
    if result == SRT_ERROR {
        Err(get_last_error())
    } else {
        Ok(())
    }
}

// Accept incoming connection (returns new socket for the client)
pub fn accept(sock: SRTSOCKET) -> Result<SRTSOCKET, i32> {
    let mut client_addr: Sockaddr = unsafe { std::mem::zeroed() };
    let mut addr_len: c_int = std::mem::size_of::<Sockaddr>() as c_int;

    let client = unsafe { srt_accept(sock, &mut client_addr, &mut addr_len) };
    if client == SRT_INVALID_SOCK {
        Err(get_last_error())
    } else {
        Ok(client)
    }
}
