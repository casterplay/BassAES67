//! Stream module for bidirectional RTP audio.
//!
//! Provides input stream (network → BASS), output stream (BASS → network),
//! and combined bidirectional stream.

pub mod input;
pub mod output;
pub mod bidirectional;

pub use input::*;
pub use output::*;
pub use bidirectional::*;

use std::sync::atomic::{AtomicU64, Ordering};

/// Atomic statistics for lock-free updates.
#[derive(Default)]
pub struct AtomicStats {
    pub packets_received: AtomicU64,
    pub packets_sent: AtomicU64,
    pub packets_dropped: AtomicU64,
    pub decode_errors: AtomicU64,
    pub encode_errors: AtomicU64,
    pub send_errors: AtomicU64,
    pub underruns: AtomicU64,
}

impl AtomicStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            packets_received: self.packets_received.load(Ordering::Relaxed),
            packets_sent: self.packets_sent.load(Ordering::Relaxed),
            packets_dropped: self.packets_dropped.load(Ordering::Relaxed),
            decode_errors: self.decode_errors.load(Ordering::Relaxed),
            encode_errors: self.encode_errors.load(Ordering::Relaxed),
            send_errors: self.send_errors.load(Ordering::Relaxed),
            underruns: self.underruns.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of statistics (for non-atomic access).
#[derive(Debug, Clone, Default)]
pub struct StatsSnapshot {
    pub packets_received: u64,
    pub packets_sent: u64,
    pub packets_dropped: u64,
    pub decode_errors: u64,
    pub encode_errors: u64,
    pub send_errors: u64,
    pub underruns: u64,
}
