//! PTP statistics tracking and formatting.

use crate::messages::ClockIdentity;

/// PTP client state machine states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PtpState {
    /// Not started
    Disabled = 0,
    /// Listening for Announce messages
    Listening = 1,
    /// Received Announce, waiting for Sync
    Uncalibrated = 2,
    /// Synchronized to grandmaster
    Slave = 3,
}

impl PtpState {
    pub fn as_str(&self) -> &'static str {
        match self {
            PtpState::Disabled => "DISABLED",
            PtpState::Listening => "LISTENING",
            PtpState::Uncalibrated => "UNCALIBRATED",
            PtpState::Slave => "SLAVE",
        }
    }
}

impl Default for PtpState {
    fn default() -> Self {
        PtpState::Disabled
    }
}

/// PTP statistics for display
#[derive(Debug, Clone, Default)]
pub struct PtpStats {
    /// Current PTP state
    pub state: PtpState,
    /// Grandmaster clock identity
    pub grandmaster_id: ClockIdentity,
    /// Grandmaster port number
    pub grandmaster_port: u16,
    /// Current offset from master in nanoseconds
    pub offset_ns: i64,
    /// Frequency adjustment in ppm
    pub frequency_ppm: f64,
    /// Mean path delay in nanoseconds
    pub mean_path_delay_ns: i64,
    /// Number of Sync messages received
    pub sync_count: u64,
    /// Number of Announce messages received
    pub announce_count: u64,
    /// Number of Follow_Up messages received
    pub follow_up_count: u64,
    /// Number of Delay_Resp messages received
    pub delay_resp_count: u64,
    /// Whether the servo is locked (stable tracking)
    pub locked: bool,
    /// PTP domain number
    pub domain: u8,
    /// Grandmaster clock class
    pub clock_class: u8,
}

impl PtpStats {
    /// Format statistics for display.
    ///
    /// Returns a string like:
    /// "Slave to: PTP/2ccf67fffe55b29a:0, δ 0.9µs, Freq: +0.00ppm"
    pub fn format_display(&self) -> String {
        match self.state {
            PtpState::Disabled => "PTP: Disabled".to_string(),
            PtpState::Listening => "PTP: Listening for grandmaster...".to_string(),
            PtpState::Uncalibrated => {
                format!(
                    "PTP: Uncalibrated - GM: {}:{}",
                    self.grandmaster_id.to_hex_string(),
                    self.grandmaster_port
                )
            }
            PtpState::Slave => {
                let lock_indicator = if self.locked { " [LOCKED]" } else { " [UNLOCKED]" };
                format!(
                    "Slave to: PTP/{}:{}, δ {:.1}µs, Delay: {:.1}µs, Freq: {:+.2}ppm{}",
                    self.grandmaster_id.to_hex_string(),
                    self.grandmaster_port,
                    self.offset_ns as f64 / 1_000.0,
                    self.mean_path_delay_ns as f64 / 1_000.0,
                    self.frequency_ppm,
                    lock_indicator
                )
            }
        }
    }

    /// Format detailed statistics for debugging
    pub fn format_detailed(&self) -> String {
        format!(
            "PTP Status:\n\
             State: {}\n\
             Grandmaster: {}:{}\n\
             Clock Class: {}\n\
             Domain: {}\n\
             Offset: {:.3}µs\n\
             Frequency: {:+.3}ppm\n\
             Path Delay: {:.3}µs\n\
             Locked: {}\n\
             Messages: Sync={}, FollowUp={}, Announce={}, DelayResp={}",
            self.state.as_str(),
            self.grandmaster_id.to_hex_string(),
            self.grandmaster_port,
            self.clock_class,
            self.domain,
            self.offset_ns as f64 / 1_000.0,
            self.frequency_ppm,
            self.mean_path_delay_ns as f64 / 1_000.0,
            if self.locked { "Yes" } else { "No" },
            self.sync_count,
            self.follow_up_count,
            self.announce_count,
            self.delay_resp_count
        )
    }
}

// Thread-safe statistics storage using static buffer
use std::sync::Mutex;
use std::sync::OnceLock;

static STATS_STRING: OnceLock<Mutex<String>> = OnceLock::new();

/// Update the global stats string
pub fn update_stats_string(stats: &PtpStats) {
    let formatted = stats.format_display();
    let mutex = STATS_STRING.get_or_init(|| Mutex::new(String::new()));
    if let Ok(mut s) = mutex.lock() {
        *s = formatted;
    }
}

/// Get the current stats string
pub fn get_stats_string() -> String {
    let mutex = STATS_STRING.get_or_init(|| Mutex::new(String::from("PTP: Not initialized")));
    if let Ok(s) = mutex.lock() {
        s.clone()
    } else {
        String::from("PTP: Error")
    }
}
