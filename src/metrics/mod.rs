pub mod cpu;
pub mod disk;
pub mod gpu;
#[cfg(any(windows, test))]
mod gpu_counters;
#[cfg(target_os = "linux")]
mod gpu_linux;
#[cfg(windows)]
mod gpu_windows;
pub mod mem;
pub mod net;
#[cfg(windows)]
mod pdh;
pub mod platform;
pub mod process;
pub mod smbios;
#[cfg(any(windows, test))]
mod temp_parse;
#[cfg(windows)]
pub mod temp_windows;

/// Convert bytes to GiB rounded to 2 decimal places. Mirrors the Python
/// ``convert_bytes`` helper so the wire format stays identical.
pub fn bytes_to_gib(x: u64) -> f64 {
    round2(x as f64 / (1024.0_f64.powi(3)))
}

#[inline]
pub fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// A product's name without the trademark signs its maker writes into
/// it: "Intel(R) Core(TM) i7" as "Intel Core i7".
pub fn without_marks(name: &str) -> String {
    let mut name = name.to_string();
    for mark in ["(R)", "(r)", "(TM)", "(tm)", "(C)", "®", "™"] {
        name = name.replace(mark, "");
    }
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[inline]
pub fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::without_marks;

    #[test]
    fn trademark_signs_are_dropped_from_names() {
        assert_eq!(
            without_marks("Intel(R) UHD Graphics 750"),
            "Intel UHD Graphics 750"
        );
        assert_eq!(
            without_marks("AMD Radeon(TM)  RX 6800 XT "),
            "AMD Radeon RX 6800 XT"
        );
        assert_eq!(
            without_marks("NVIDIA GeForce RTX 3070"),
            "NVIDIA GeForce RTX 3070"
        );
    }
}
