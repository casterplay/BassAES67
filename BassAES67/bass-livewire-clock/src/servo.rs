//! PI controller servo for Livewire clock synchronization.
//!
//! Implements the same algorithm as the reference Axia implementation:
//! - Collects samples and finds minimum offset (rejects jitter)
//! - Uses PI controller for stable frequency output
//! - Works with 8-bit truncated frame values

/// Number of samples to collect before processing
const SAMPLE_SET_SIZE: usize = 28;

/// Microticks per frame (lock range)
const MICROTICKS_PER_FRAME: i32 = 3072;

/// Nanoseconds per microtick (~81.38ns)
const NS_PER_MICROTICK: f64 = 81.380e-9;

/// PI controller proportional gain
const KP: f64 = 0.15;

/// PI controller integral gain
const KI: f64 = 0.01;

/// Maximum frequency adjustment (+/- 100 ppm)
const U_MAX: f64 = 100e-6;

/// PI controller servo for clock synchronization.
/// Uses minimum-seeking filter and PI control as per Axia reference.
pub struct ClockServo {
    /// Current offset in microticks (for display)
    offset_microticks: i32,
    /// Current frequency adjustment in ppb
    frequency_ppb: f64,
    /// Integral accumulator for PI controller (in microticks)
    integral_sum: i64,
    /// Number of samples processed total
    sample_count: u64,
    /// Locked state (stable tracking)
    locked: bool,
    /// Consecutive samples within threshold for lock
    samples_in_lock: u32,
    /// Consecutive samples outside threshold for unlock
    samples_out_of_lock: u32,
    /// Sample buffer for current batch
    samples: [i32; SAMPLE_SET_SIZE],
    /// Current sample index in batch
    current_sample: usize,
    /// Local frame offset adjustment (for large offsets)
    lf_offset: u8,
    /// Count for frame offset adjustment
    lf_offset_count: u32,
}

impl ClockServo {
    /// Create a new PI controller servo.
    pub fn new() -> Self {
        Self {
            offset_microticks: 0,
            frequency_ppb: 0.0,
            integral_sum: 0,
            sample_count: 0,
            locked: false,
            samples_in_lock: 0,
            samples_out_of_lock: 0,
            samples: [0; SAMPLE_SET_SIZE],
            current_sample: 0,
            lf_offset: 0,
            lf_offset_count: 0,
        }
    }

    /// Process a new clock packet with remote and local frame/tick values.
    /// Returns true if a batch was completed and frequency was updated.
    ///
    /// # Arguments
    /// * `remote_frame` - Remote frame counter (full 32-bit, will be truncated to 8-bit)
    /// * `remote_ticks` - Remote microticks (0-3071)
    /// * `local_frame` - Local frame counter (full 32-bit, will be truncated to 8-bit)
    /// * `local_ticks` - Local microticks (0-3071)
    pub fn update(&mut self, remote_frame: u32, remote_ticks: u16, local_frame: u32, local_ticks: u16) -> bool {
        self.sample_count += 1;

        // Truncate to 8-bit frame values (as per reference)
        let rf = (remote_frame & 0xFF) as i32;
        let rt = remote_ticks as i32;
        let mut lf = (local_frame & 0xFF) as i32;
        let lt = local_ticks as i32;

        // Apply local frame offset adjustment
        lf = (lf - self.lf_offset as i32) & 0xFF;

        // Convert to combined microtick values
        let rfval = rf * MICROTICKS_PER_FRAME + rt;
        let lfval = lf * MICROTICKS_PER_FRAME + lt;

        // Calculate delta with wraparound handling (reference algorithm)
        let delta = self.calculate_delta(rfval, lfval);

        // Store sample
        self.samples[self.current_sample] = delta;
        self.current_sample += 1;

        // Check if batch is complete
        if self.current_sample >= SAMPLE_SET_SIZE {
            self.process_batch(rf, lf);
            self.current_sample = 0;
            return true;
        }

        false
    }

    /// Calculate delta between remote and local values with wraparound handling.
    /// Finds the smallest magnitude difference considering 8-bit frame wraparound.
    fn calculate_delta(&self, rfval: i32, lfval: i32) -> i32 {
        let bsize = 256 * MICROTICKS_PER_FRAME;
        let mut smallest = bsize * 2;
        let mut delta = 0i32;

        // Test all four possible interpretations for wraparound
        let test1 = lfval - rfval;
        if test1 >= 0 && test1 < smallest {
            delta = test1;
            smallest = test1;
        }

        let test2 = lfval + bsize - rfval;
        if test2 >= 0 && test2 < smallest {
            delta = test2;
            smallest = test2;
        }

        let test3 = rfval - lfval;
        if test3 >= 0 && test3 < smallest {
            smallest = test3;
            delta = -test3;
        }

        let test4 = rfval + bsize - lfval;
        if test4 >= 0 && test4 < smallest {
            delta = -test4;
        }

        delta
    }

    /// Process a complete batch of samples using minimum filter and PI controller.
    fn process_batch(&mut self, rf_last: i32, lf_last: i32) {
        // Find minimum delta in batch (statistical filter to reject jitter)
        let delta_min = *self.samples[..SAMPLE_SET_SIZE]
            .iter()
            .min()
            .unwrap_or(&0);

        let lock_range = MICROTICKS_PER_FRAME;

        // Check for large offset - may need frame offset adjustment
        if delta_min.abs() > 64 * lock_range {
            self.lf_offset_count += 1;
            if self.lf_offset_count > 3 {
                self.lf_offset_count = 0;
                if self.lf_offset != 0 {
                    self.lf_offset = 0;
                    self.lf_offset_count = 2; // Speed up next decision
                } else {
                    self.lf_offset = ((lf_last - rf_last) & 0xFF) as u8;
                }
            }
        } else {
            self.lf_offset_count = 0;
        }

        // Convert negative to positive for modulo operation
        let mut delta = delta_min;
        if delta < 0 {
            delta += 256 * MICROTICKS_PER_FRAME;
        }

        // Find position within lock range (one frame)
        let mut dframe = delta % lock_range;
        if dframe > lock_range / 2 {
            dframe -= lock_range;
        }

        // Store for display
        self.offset_microticks = dframe;

        // Accumulate integral
        self.integral_sum += dframe as i64;

        // PI controller
        let ep = dframe as f64 * NS_PER_MICROTICK; // Proportional error in seconds
        let ei = self.integral_sum as f64 * NS_PER_MICROTICK; // Integral error in seconds
        let mut u = KP * ep + KI * ei;

        // Clamp to maximum adjustment
        if u > U_MAX {
            u = U_MAX;
        } else if u < -U_MAX {
            u = -U_MAX;
        }

        // Convert to ppb (negative because we're correcting)
        self.frequency_ppb = -(u * 1e9);

        // Lock state machine
        let lock_threshold = 50_000.0; // 50 ppm
        if self.frequency_ppb.abs() < lock_threshold {
            self.samples_in_lock += 1;
            self.samples_out_of_lock = 0;
            if self.samples_in_lock >= 3 {
                self.locked = true;
            }
        } else {
            self.samples_in_lock = 0;
            if self.locked {
                self.samples_out_of_lock += 1;
                if self.samples_out_of_lock >= 5 {
                    self.locked = false;
                    self.samples_out_of_lock = 0;
                }
            }
        }
    }

    /// Get current offset in nanoseconds (for display/stats).
    pub fn offset_ns(&self) -> i64 {
        // NS_PER_MICROTICK is ~81.38 nanoseconds per microtick
        (self.offset_microticks as f64 * NS_PER_MICROTICK) as i64
    }

    /// Get current frequency adjustment in ppm.
    pub fn frequency_ppm(&self) -> f64 {
        self.frequency_ppb / 1_000.0
    }

    /// Get current frequency adjustment in ppb.
    pub fn frequency_ppb(&self) -> f64 {
        self.frequency_ppb
    }

    /// Check if servo is locked (tracking stably).
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Get number of samples processed.
    pub fn sample_count(&self) -> u64 {
        self.sample_count
    }

    /// Reset the servo state.
    pub fn reset(&mut self) {
        self.offset_microticks = 0;
        self.frequency_ppb = 0.0;
        self.integral_sum = 0;
        self.sample_count = 0;
        self.locked = false;
        self.samples_in_lock = 0;
        self.samples_out_of_lock = 0;
        self.samples = [0; SAMPLE_SET_SIZE];
        self.current_sample = 0;
        self.lf_offset = 0;
        self.lf_offset_count = 0;
    }
}

impl Default for ClockServo {
    fn default() -> Self {
        Self::new()
    }
}
