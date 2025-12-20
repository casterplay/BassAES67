//! Platform-specific high-resolution timestamps.
//!
//! For PTP we need wall clock time (epoch-based) to compare with PTP timestamps.
//! Using GetSystemTimePreciseAsFileTime on Windows for better precision.

#[cfg(windows)]
mod windows_time {
    /// FILETIME structure (two 32-bit values representing 100ns intervals since Jan 1, 1601)
    #[repr(C)]
    struct FILETIME {
        dw_low_date_time: u32,
        dw_high_date_time: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetSystemTimePreciseAsFileTime(lpSystemTimeAsFileTime: *mut FILETIME);
    }

    // Windows FILETIME epoch is Jan 1, 1601
    // Unix epoch is Jan 1, 1970
    // Difference in 100ns intervals: 116444736000000000
    const FILETIME_TO_UNIX_EPOCH: i64 = 116_444_736_000_000_000;

    pub fn get_timestamp_ns() -> i64 {
        let mut ft = FILETIME {
            dw_low_date_time: 0,
            dw_high_date_time: 0,
        };

        unsafe {
            GetSystemTimePreciseAsFileTime(&mut ft);
        }

        // Combine into 64-bit value (100ns intervals since 1601)
        let filetime = (ft.dw_high_date_time as i64) << 32 | ft.dw_low_date_time as i64;

        // Convert to nanoseconds since Unix epoch
        // 1. Subtract the epoch difference to get 100ns intervals since 1970
        // 2. Multiply by 100 to get nanoseconds
        (filetime - FILETIME_TO_UNIX_EPOCH) * 100
    }
}

/// Get current timestamp in nanoseconds since Unix epoch.
/// Used for PTP timing measurements.
#[cfg(windows)]
pub fn get_timestamp_ns() -> i64 {
    windows_time::get_timestamp_ns()
}

#[cfg(unix)]
pub fn get_timestamp_ns() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Use SystemTime for Unix - gives wall clock time
    // For PTP we need consistent timestamps across the system
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos() as i64,
        Err(_) => 0,
    }
}

#[cfg(not(any(windows, unix)))]
pub fn get_timestamp_ns() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos() as i64,
        Err(_) => 0,
    }
}

/// Convert nanoseconds to PTP timestamp format (seconds + nanoseconds)
pub fn ns_to_ptp_timestamp(ns: i64) -> (u64, u32) {
    let secs = (ns / 1_000_000_000) as u64;
    let nanos = (ns % 1_000_000_000) as u32;
    (secs, nanos)
}

/// Convert PTP timestamp to nanoseconds
pub fn ptp_timestamp_to_ns(secs: u64, nanos: u32) -> i64 {
    secs as i64 * 1_000_000_000 + nanos as i64
}
