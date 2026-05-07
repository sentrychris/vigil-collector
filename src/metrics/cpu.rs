#[cfg(target_os = "linux")]
use std::{fs, path::Path};

use sysinfo::{Components, System};

use crate::snapshot::Cpu;

use super::{round1, round2};

const CPU_TEMP_LABEL_PRIORITY: &[&str] = &["package", "tctl", "tdie", "cpu"];
const CPU_TEMP_THERMAL_TYPES: &[&str] =
    &["x86_pkg_temp", "cpu-thermal", "cpu_thermal", "soc_thermal"];

#[cfg(target_os = "linux")]
fn read_cpu_temp(components: &Components) -> f64 {
    if let Some(t) = read_via_components(components) {
        return t;
    }
    read_via_thermal_zones().unwrap_or(0.0)
}

#[cfg(target_os = "macos")]
fn read_cpu_temp(components: &Components) -> f64 {
    read_via_components(components).unwrap_or(0.0)
}

#[cfg(target_os = "windows")]
fn read_cpu_temp(_components: &Components) -> f64 {
    // Per project decision: Windows reports 0 instead of shelling out to a
    // bundled libwincputemp.exe. Re-introducing it later means matching
    // the Python behavior of returning the trimmed stdout.
    0.0
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn read_cpu_temp(_components: &Components) -> f64 {
    0.0
}

fn read_via_components(components: &Components) -> Option<f64> {
    // Prefer a labelled CPU package sensor; fall back to the first numeric
    // temperature anything in the list exposes. Mirrors the Python sensor
    // priority list — the matching is case-insensitive and substring-based.
    let mut labelled: Option<f64> = None;
    let mut any_numeric: Option<f64> = None;

    for c in components.iter() {
        let temp = c.temperature();
        if !temp.is_finite() {
            continue;
        }
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

#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
fn read_via_thermal_zones() -> Option<f64> {
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
    let n = unsafe {
        libc::getloadavg(buf.as_mut_ptr(), want)
    };

    if n == want {
        Some(buf)
    } else {
        None
    }
}

#[cfg(not(unix))]
fn read_load_avg() -> Option<[f64; 3]> {
    None
}

pub fn collect(sys: &System, components: &Components, cores: u32) -> Cpu {
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
        usage,
        temp: read_cpu_temp(components),
        freq,
        cores,
        load,
    }
}
