//! The deterministic hash the procedurally placed content shares.
//!
//! Track shape, obstacle placement and roadside dressing all draw from this one
//! function: changing it reshapes every level, so it is deliberately dull.

/// 32-bit integer hash (the classic xor-shift-multiply chain).
pub fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// A stable `0..1` value for a seed and salt pair.
pub fn unit(seed: u32, salt: u32) -> f32 {
    (hash(seed ^ salt) % 10_000) as f32 / 10_000.0
}
