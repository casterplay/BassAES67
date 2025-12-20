//! Livewire clock statistics tracking and formatting.

use std::sync::OnceLock;
use parking_lot::Mutex;

/// Livewire client state machine states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LwState {
    /// Not started
    Disabled = 0,
    /// Listening for clock packets
    Listening = 1,
    /// Received packets, building baseline
    Uncalibrated = 2,
    /// Synchronized to master clock
    Slave = 3,
}

impl LwState {
    /// Returns human-readable state name
    pub fn as_str(&self) -> &'static str {
        match self {
            LwState::Disabled => "DISABLED",
            LwState::Listening => "LISTENING",
            LwState::Uncalibrated => "UNCALIBRATED",
            LwState::Slave => "SLAVE",
        }
    }
}

impl Default for LwState {
    fn default() -> Self {
        LwState::Disabled
    }
}

/// Master clock identity from Livewire packet
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MasterIdentity {
    /// MAC address of the master (6 bytes)
    pub mac_address: [u8; 6],
    /// Priority (0-15, higher = more preferred)
    pub priority: u8,
    /// Hardware ID (lower 15 bits of IP)
    pub hardware_id: u16,
}

impl MasterIdentity {
    /// Format MAC address as hex string
    pub fn mac_string(&self) -> String {
        format!(
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.mac_address[0], self.mac_address[1], self.mac_address[2],
            self.mac_address[3], self.mac_address[4], self.mac_address[5]
        )
    }
}

/// Livewire clock statistics for display
#[derive(Debug, Clone, Default)]
pub struct LwStats {
    /// Current state
    pub state: LwState,
    /// Master clock identity
    pub master: MasterIdentity,
    /// Current offset from master in nanoseconds (relative)
    pub offset_ns: i64,
    /// Frequency adjustment in ppm
    pub frequency_ppm: f64,
    /// Number of clock packets received
    pub packet_count: u64,
    /// Whether the servo is locked (stable tracking)
    pub locked: bool,
}

impl LwStats {
    /// Format statistics for display (similar format to bass-ptp)
    pub fn format_display(&self) -> String {
        match self.state {
            LwState::Disabled => "LW: Disabled".to_string(),
            LwState::Listening => "LW: Listening for clock...".to_string(),
            LwState::Uncalibrated => {
                format!(
                    "LW: Uncalibrated - Master: {} (prio={})",
                    self.master.mac_string(),
                    self.master.priority
                )
            }
            LwState::Slave => {
                let lock_indicator = if self.locked { " [LOCKED]" } else { " [UNLOCKED]" };
                format!(
                    "Slave to: LW/{} (prio={}), δ {:.1}µs, Freq: {:+.2}ppm{}",
                    self.master.mac_string(),
                    self.master.priority,
                    self.offset_ns as f64 / 1_000.0,
                    self.frequency_ppm,
                    lock_indicator
                )
            }
        }
    }

    /// Format detailed statistics for debugging
    pub fn format_detailed(&self) -> String {
        format!(
            "Livewire Clock Status:\n\
             State: {}\n\
             Master MAC: {}\n\
             Priority: {}\n\
             Hardware ID: 0x{:04X}\n\
             Offset: {:.3}µs\n\
             Frequency: {:+.3}ppm\n\
             Locked: {}\n\
             Packets: {}",
            self.state.as_str(),
            self.master.mac_string(),
            self.master.priority,
            self.master.hardware_id,
            self.offset_ns as f64 / 1_000.0,
            self.frequency_ppm,
            if self.locked { "Yes" } else { "No" },
            self.packet_count
        )
    }
}

// Thread-safe statistics storage using static buffer
static STATS_STRING: OnceLock<Mutex<String>> = OnceLock::new();

/// Update the global stats string
pub fn update_stats_string(stats: &LwStats) {
    let formatted = stats.format_display();
    let mutex = STATS_STRING.get_or_init(|| Mutex::new(String::new()));
    *mutex.lock() = formatted;
}

/// Get the current stats string
pub fn get_stats_string() -> String {
    let mutex = STATS_STRING.get_or_init(|| Mutex::new(String::from("LW: Not initialized")));
    mutex.lock().clone()
}
