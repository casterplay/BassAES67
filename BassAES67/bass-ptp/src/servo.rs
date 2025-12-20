//! Drift rate servo for PTP clock offset tracking.
//!
//! For software PTP (no system clock adjustment), we measure the rate of change
//! of offset (drift rate) using linear regression and output that as frequency.

/// Drift rate estimator for software PTP
/// Uses windowed linear regression to estimate clock drift rate (ppb)
pub struct PtpServo {
    /// Current offset in nanoseconds
    offset_ns: i64,
    /// Current frequency adjustment in ppb (parts per billion)
    frequency_ppb: f64,
    /// Mean path delay in nanoseconds
    mean_path_delay_ns: i64,
    /// Number of samples processed
    sample_count: u64,
    /// Locked state (stable tracking)
    locked: bool,
    /// Lock threshold in nanoseconds (unused but kept for API compatibility)
    lock_threshold_ns: i64,
    /// Consecutive samples within threshold for lock
    samples_in_lock: u32,
    /// Required consecutive samples for lock
    lock_count_threshold: u32,
    /// Consecutive samples outside threshold (for unlock hysteresis)
    samples_out_of_lock: u32,
    /// Required consecutive bad samples for unlock
    unlock_count_threshold: u32,
    /// Start time for elapsed calculation
    start_time: std::time::Instant,
    /// Ring buffer of recent (time, offset) samples for windowed regression
    history: [(i64, i64); 32],
    /// Current position in ring buffer
    history_pos: usize,
    /// Number of valid samples in history
    history_count: usize,
    /// Filtered drift rate (low-pass filtered)
    filtered_drift_ppb: f64,
}

impl PtpServo {
    /// Create a new servo for software PTP frequency estimation
    ///
    /// Uses a sliding window of recent samples to calculate drift rate via
    /// linear regression. This gives a stable estimate that responds to
    /// changes in clock drift over time.
    pub fn new() -> Self {
        Self {
            offset_ns: 0,
            frequency_ppb: 0.0,
            mean_path_delay_ns: 0,
            sample_count: 0,
            locked: false,
            lock_threshold_ns: 10_000_000,
            samples_in_lock: 0,
            lock_count_threshold: 3,
            samples_out_of_lock: 0,
            unlock_count_threshold: 5,
            start_time: std::time::Instant::now(),
            history: [(0, 0); 32],
            history_pos: 0,
            history_count: 0,
            filtered_drift_ppb: 0.0,
        }
    }

    /// Process a new offset measurement
    ///
    /// Measures drift rate using linear regression over recent samples.
    /// The drift rate (ns/s = ppb) tells us how much to adjust packet timing.
    ///
    /// # Arguments
    /// * `offset_ns` - Measured clock offset in nanoseconds (relative to baseline)
    /// * `path_delay_ns` - Measured path delay in nanoseconds
    pub fn update(&mut self, offset_ns: i64, path_delay_ns: i64) {
        self.sample_count += 1;
        self.offset_ns = offset_ns;
        self.mean_path_delay_ns = path_delay_ns;

        // Get elapsed time in nanoseconds
        let now_ns = self.start_time.elapsed().as_nanos() as i64;

        // Store in history for rate calculation
        self.history[self.history_pos] = (now_ns, offset_ns);
        self.history_pos = (self.history_pos + 1) % self.history.len();
        if self.history_count < self.history.len() {
            self.history_count += 1;
        }

        // Need at least 8 samples for stable regression
        if self.history_count >= 8 {
            // Calculate drift rate using linear regression
            // drift_rate = d(offset)/dt in ns/s = ppb
            let drift_ppb = self.calculate_drift_rate_regression();

            // Low-pass filter the drift rate (alpha = 0.1 for smooth response)
            const ALPHA: f64 = 0.1;
            self.filtered_drift_ppb = ALPHA * drift_ppb + (1.0 - ALPHA) * self.filtered_drift_ppb;

            // Output is the filtered drift rate
            // This is what we need to compensate: if drift is +20ppb, we need -20ppb adjustment
            self.frequency_ppb = -self.filtered_drift_ppb;
        }

        // Clamp to reasonable range (+/- 500 ppm)
        self.frequency_ppb = self.frequency_ppb.clamp(-500_000.0, 500_000.0);

        // Lock state machine - based on drift rate being small and stable
        let drift_threshold = 50_000.0; // 50 ppm threshold for lock
        if self.filtered_drift_ppb.abs() < drift_threshold {
            self.samples_in_lock += 1;
            self.samples_out_of_lock = 0;

            if self.samples_in_lock >= self.lock_count_threshold {
                self.locked = true;
            }
        } else {
            self.samples_in_lock = 0;

            if self.locked {
                self.samples_out_of_lock += 1;
                if self.samples_out_of_lock >= self.unlock_count_threshold {
                    self.locked = false;
                    self.samples_out_of_lock = 0;
                }
            }
        }
    }

    /// Calculate drift rate using linear regression over history window
    /// Returns drift rate in ppb (ns per second)
    fn calculate_drift_rate_regression(&self) -> f64 {
        if self.history_count < 2 {
            return 0.0;
        }

        // Collect valid samples from ring buffer
        let mut sum_t = 0.0f64;
        let mut sum_y = 0.0f64;
        let mut sum_tt = 0.0f64;
        let mut sum_ty = 0.0f64;
        let n = self.history_count as f64;

        for i in 0..self.history_count {
            let idx = (self.history_pos + self.history.len() - self.history_count + i) % self.history.len();
            let (t_ns, y_ns) = self.history[idx];
            let t = t_ns as f64 / 1_000_000_000.0; // Convert to seconds
            let y = y_ns as f64; // Keep in nanoseconds

            sum_t += t;
            sum_y += y;
            sum_tt += t * t;
            sum_ty += t * y;
        }

        // Linear regression: slope = (n*sum_ty - sum_t*sum_y) / (n*sum_tt - sum_t*sum_t)
        let denominator = n * sum_tt - sum_t * sum_t;
        if denominator.abs() < 1e-10 {
            return 0.0;
        }

        let slope = (n * sum_ty - sum_t * sum_y) / denominator;

        // slope is in ns/s = ppb
        slope
    }

    /// Get current offset in nanoseconds
    pub fn offset_ns(&self) -> i64 {
        self.offset_ns
    }

    /// Get current offset in microseconds
    pub fn offset_us(&self) -> f64 {
        self.offset_ns as f64 / 1_000.0
    }

    /// Get current frequency adjustment in ppm (parts per million)
    pub fn frequency_ppm(&self) -> f64 {
        self.frequency_ppb / 1_000.0
    }

    /// Get current frequency adjustment in ppb (parts per billion)
    pub fn frequency_ppb(&self) -> f64 {
        self.frequency_ppb
    }

    /// Get mean path delay in nanoseconds
    pub fn mean_path_delay_ns(&self) -> i64 {
        self.mean_path_delay_ns
    }

    /// Get mean path delay in microseconds
    pub fn mean_path_delay_us(&self) -> f64 {
        self.mean_path_delay_ns as f64 / 1_000.0
    }

    /// Check if servo is locked (tracking stably)
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Get number of samples processed
    pub fn sample_count(&self) -> u64 {
        self.sample_count
    }

    /// Reset the servo state
    pub fn reset(&mut self) {
        self.offset_ns = 0;
        self.frequency_ppb = 0.0;
        self.mean_path_delay_ns = 0;
        self.sample_count = 0;
        self.locked = false;
        self.samples_in_lock = 0;
        self.samples_out_of_lock = 0;
        self.start_time = std::time::Instant::now();
        self.history = [(0, 0); 32];
        self.history_pos = 0;
        self.history_count = 0;
        self.filtered_drift_ppb = 0.0;
    }

    /// Get the filtered offset rate in ppb (ns/s)
    pub fn offset_rate_ppb(&self) -> f64 {
        self.frequency_ppb
    }
}

impl Default for PtpServo {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_servo_basic() {
        let mut servo = PtpServo::new();

        // Simulate drifting offset (local clock slow by ~10ppm)
        // At 10ppm slow, offset grows by 10ns per ms
        for i in 0..100 {
            let offset = i * 1000; // Growing by 1000ns per iteration
            servo.update(offset, 500);
            thread::sleep(Duration::from_millis(10));
        }

        // Should detect the drift and have frequency adjustment
        assert!(servo.frequency_ppm().abs() > 0.0);
    }

    #[test]
    fn test_servo_lock_on_stable_rate() {
        let mut servo = PtpServo::new();

        // Constant offset (not drifting) = stable rate = should lock
        for _ in 0..20 {
            servo.update(1000, 500); // Same offset each time
            thread::sleep(Duration::from_millis(10));
        }

        // Offset rate is ~0, should lock
        assert!(servo.is_locked());
    }

    #[test]
    fn test_servo_detects_drift() {
        let mut servo = PtpServo::new();

        // Simulate 50ppm drift (50,000 ns per second)
        // At 10ms intervals: 500ns per measurement
        for i in 0..50 {
            let offset = i * 500; // 500ns per 10ms = 50,000 ns/s = 50ppm
            servo.update(offset, 500);
            thread::sleep(Duration::from_millis(10));
        }

        // Should detect ~50ppm drift and output similar frequency adjustment
        // Allow some margin for filter settling
        let freq = servo.frequency_ppm();
        assert!(freq > 10.0, "Expected positive frequency for slow local clock, got {}", freq);
    }
}
