//! Gain and level utility functions.

/// Convert decibels to linear gain.
#[inline]
pub fn db_to_linear(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

/// Convert linear gain to decibels.
#[inline]
pub fn linear_to_db(linear: f32) -> f32 {
    if linear > 0.0 {
        20.0 * linear.log10()
    } else {
        -100.0 // Silence floor
    }
}

/// Apply gain to a buffer of samples in-place.
#[inline]
pub fn apply_gain(buffer: &mut [f32], gain_linear: f32) {
    for sample in buffer.iter_mut() {
        *sample *= gain_linear;
    }
}

/// Calculate peak level from buffer.
pub fn peak_level(buffer: &[f32]) -> f32 {
    buffer.iter().fold(0.0f32, |max, &s| max.max(s.abs()))
}
