pub mod cpu;
pub mod disk;
pub mod mem;
pub mod net;
pub mod platform;
pub mod process;

/// Convert bytes to GiB rounded to 2 decimal places. Mirrors the Python
/// ``convert_bytes`` helper so the wire format stays identical.
pub fn bytes_to_gib(x: u64) -> f64 {
    round2(x as f64 / (1024.0_f64.powi(3)))
}

#[inline]
pub fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

#[inline]
pub fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}
