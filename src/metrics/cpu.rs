#[cfg(target_os = "linux")]
use std::{fs, path::Path};

use sysinfo::System;

use crate::snapshot::Cpu;

#[cfg(any(unix, windows))]
use super::round1;
use super::{round2, without_marks};

/// Temperature sensors. On Unix this is sysinfo's ``Components`` (its
/// ``component`` feature is enabled for Unix targets only). On Windows it
/// is a background reader over monitoring apps and the ACPI thermal zone
/// (see ``temp_windows.rs``).
pub struct Sensors {
    #[cfg(unix)]
    components: Option<sysinfo::Components>,
    #[cfg(windows)]
    windows: super::temp_windows::CpuTemp,
}

impl Sensors {
    pub fn new() -> Self {
        Self {
            #[cfg(unix)]
            components: sysinfo::Components::new_with_refreshed_list().ok(),
            #[cfg(windows)]
            windows: super::temp_windows::CpuTemp::start(),
        }
    }

    pub fn refresh(&mut self) {
        #[cfg(unix)]
        if let Some(c) = self.components.as_mut() {
            c.refresh(false);
        }
    }
}

#[cfg(unix)]
const CPU_TEMP_LABEL_PRIORITY: &[&str] = &["package", "tctl", "tdie", "cpu"];
#[cfg(target_os = "linux")]
const CPU_TEMP_THERMAL_TYPES: &[&str] =
    &["x86_pkg_temp", "cpu-thermal", "cpu_thermal", "soc_thermal"];

#[cfg(target_os = "linux")]
fn read_cpu_temp(components: &Sensors) -> f64 {
    if let Some(t) = read_via_components(components) {
        return t;
    }
    read_via_thermal_zones().unwrap_or(0.0)
}

#[cfg(all(unix, not(target_os = "linux")))]
fn read_cpu_temp(components: &Sensors) -> f64 {
    read_via_components(components).unwrap_or(0.0)
}

/// Windows has no driver-free API for the CPU's own sensor; the reader
/// takes it from HWiNFO, MSI Afterburner or LibreHardwareMonitor when one
/// is running, else from a live ACPI thermal zone, else reports ``0``.
#[cfg(windows)]
fn read_cpu_temp(sensors: &Sensors) -> f64 {
    sensors.windows.read().map_or(0.0, round1)
}

#[cfg(not(any(unix, windows)))]
fn read_cpu_temp(_sensors: &Sensors) -> f64 {
    0.0
}

#[cfg(unix)]
fn read_via_components(components: &Sensors) -> Option<f64> {
    // Prefer a labelled CPU package sensor; fall back to the first numeric
    // temperature anything in the list exposes. Mirrors the Python sensor
    // priority list — the matching is case-insensitive and substring-based.
    let mut labelled: Option<f64> = None;
    let mut any_numeric: Option<f64> = None;

    let list = components.components.as_ref()?;
    for c in list.iter() {
        let Some(temp) = c.temperature().filter(|t| t.is_finite()) else {
            continue;
        };
        let label = c.label().to_ascii_lowercase();
        if any_numeric.is_none() {
            any_numeric = Some(temp as f64);
        }
        if CPU_TEMP_LABEL_PRIORITY
            .iter()
            .any(|needle| label.contains(needle))
        {
            labelled = Some(temp as f64);
            break;
        }
    }

    let chosen = labelled.or(any_numeric)?;
    Some(round1(chosen))
}

#[cfg(target_os = "linux")]
fn read_via_thermal_zones() -> Option<f64> {
    let base = Path::new("/sys/class/thermal");
    let entries = fs::read_dir(base).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("thermal_zone") {
            continue;
        }
        let dir = entry.path();
        let Ok(zone_type) = fs::read_to_string(dir.join("type")) else {
            continue;
        };
        let zone_type = zone_type.trim();
        if !CPU_TEMP_THERMAL_TYPES.contains(&zone_type) {
            continue;
        }
        let Ok(raw) = fs::read_to_string(dir.join("temp")) else {
            continue;
        };
        let Ok(milli) = raw.trim().parse::<i64>() else {
            continue;
        };
        return Some(round1(milli as f64 / 1000.0));
    }
    None
}

/// Read the system load average with full kernel precision. sysinfo's
/// ``System::load_average`` parses ``/proc/loadavg`` which truncates to
/// two decimals; ``getloadavg(3)`` consults the same fixed-point counters
/// that ``psutil.getloadavg`` does, so we match its byte representation.
#[cfg(unix)]
fn read_load_avg() -> Option<[f64; 3]> {
    let mut buf = [0.0_f64; 3];
    let want = buf.len() as libc::c_int;

    // SAFETY: buf is a valid mutable pointer with 3 f64 slots.
    // libc::getloadavg writes at most 3 f64s.
    let n = unsafe { libc::getloadavg(buf.as_mut_ptr(), want) };

    if n == want { Some(buf) } else { None }
}

/// Windows has no load average. sysinfo emulates one from a PDH
/// processor-queue counter on a background thread; it isn't comparable
/// to the Unix value, so ``load`` stays ``null`` like psutil's.
#[cfg(not(unix))]
fn read_load_avg() -> Option<[f64; 3]> {
    None
}

/// The processor's model as people say it, from the brand string it
/// reports: without the trademark signs, the generation, the word
/// "processor", the core count, the graphics it comes with and the
/// clock speed (which ``freq`` has, as it is now).
/// "11th Gen Intel(R) Core(TM) i7-11700 @ 2.50GHz" is "Intel Core
/// i7-11700", and "AMD Ryzen 7 5800X 8-Core Processor" is "AMD Ryzen 7
/// 5800X".
pub fn model(sys: &System) -> String {
    sys.cpus()
        .first()
        .map_or_else(String::new, |c| tidy(c.brand()))
}

fn tidy(brand: &str) -> String {
    let brand = without_marks(brand);
    let brand = brand.split(" @ ").next().unwrap_or_default();
    let brand = brand.split(" with ").next().unwrap_or_default();
    let words: Vec<&str> = brand.split_whitespace().collect();
    let ordinal = |word: &str| {
        ["st", "nd", "rd", "th"].iter().any(|end| {
            word.strip_suffix(end)
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
    };
    let mut kept: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        if ordinal(word) && words.get(i + 1) == Some(&"Gen") {
            i += 2;
            continue;
        }
        let dropped = ["cpu", "processor", "apu"].contains(&word.to_lowercase().as_str())
            || word.to_lowercase().ends_with("-core");
        if !dropped {
            kept.push(word);
        }
        i += 1;
    }
    kept.join(" ")
}

pub fn collect(sys: &System, sensors: &Sensors, cores: u32, name: &str) -> Cpu {
    let usage = if sys.cpus().is_empty() {
        0.0
    } else {
        round2(sys.global_cpu_usage() as f64)
    };

    let freqs: Vec<f64> = sys
        .cpus()
        .iter()
        .map(|c| c.frequency() as f64)
        .filter(|f| *f > 0.0)
        .collect();
    let freq = if freqs.is_empty() {
        0.0
    } else {
        round2(freqs.iter().sum::<f64>() / freqs.len() as f64)
    };

    let load = read_load_avg();

    Cpu {
        name: name.to_string(),
        usage,
        temp: read_cpu_temp(sensors),
        freq,
        cores,
        load,
    }
}

#[cfg(test)]
mod tests {
    use super::tidy;

    #[test]
    fn brand_strings_are_tidied_into_models() {
        for (brand, model) in [
            (
                "11th Gen Intel(R) Core(TM) i7-11700 @ 2.50GHz",
                "Intel Core i7-11700",
            ),
            (
                "Intel(R) Core(TM) i5-8250U CPU @ 1.60GHz",
                "Intel Core i5-8250U",
            ),
            ("Intel(R) Core(TM) Ultra 7 155H", "Intel Core Ultra 7 155H"),
            (
                "Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz",
                "Intel Xeon Platinum 8370C",
            ),
            ("AMD Ryzen 7 5800X 8-Core Processor", "AMD Ryzen 7 5800X"),
            (
                "AMD Ryzen 5 5600G with Radeon Graphics",
                "AMD Ryzen 5 5600G",
            ),
            ("AMD FX(tm)-8350 Eight-Core Processor", "AMD FX-8350"),
            ("AMD EPYC 7763 64-Core Processor", "AMD EPYC 7763"),
            ("Apple M2 Pro", "Apple M2 Pro"),
            ("", ""),
        ] {
            assert_eq!(tidy(brand), model, "{brand}");
        }
    }
}
