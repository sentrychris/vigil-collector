#[cfg(target_os = "linux")]
use std::{fs, path::Path};

use sysinfo::System;

use crate::snapshot::Cpu;

#[cfg(unix)]
use super::round1;
use super::round2;

/// Temperature sensors. sysinfo's ``Components`` only exists when its
/// ``component`` feature is on, which Cargo.toml enables for Unix targets
/// only; on Windows this is an empty stand-in so the sampler can hold one
/// unconditionally.
#[derive(Default)]
pub struct Sensors {
    #[cfg(unix)]
    components: Option<sysinfo::Components>,
}

impl Sensors {
    pub fn new() -> Self {
        Self {
            #[cfg(unix)]
            components: sysinfo::Components::new_with_refreshed_list().ok(),
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

/// Windows reports ``0``, like psutil. The only sensor Windows exposes
/// without a kernel driver is WMI's ``MSAcpi_ThermalZoneTemperature``,
/// which needs admin rights and on most boards returns a fixed ACPI zone
/// value rather than the CPU package temperature.
#[cfg(not(unix))]
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

pub fn collect(sys: &System, sensors: &Sensors, cores: u32) -> Cpu {
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
        temp: read_cpu_temp(sensors),
        freq,
        cores,
        load,
    }
}
