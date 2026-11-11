//! Parsers for the Windows CPU temperature sources.
//!
//! Windows has no driver-free API for the CPU's own temperature sensor, so
//! the collector reads it from monitoring apps that ship a driver (HWiNFO,
//! MSI Afterburner, LibreHardwareMonitor) when one is running, and falls
//! back to the ACPI thermal zone when that zone reports live values.
//!
//! Everything here is a pure function over a byte snapshot or a list of
//! readings, so it is unit-tested on every platform. The Windows glue that
//! maps the shared memory and queries PDH and WMI is in ``temp_windows.rs``.

use std::collections::HashMap;

/// Accepted CPU temperature range in °C. Anything outside it is an unset
/// slot (Afterburner uses ``FLT_MAX``), a placeholder or a unit mix-up.
const PLAUSIBLE_C: std::ops::RangeInclusive<f64> = 5.0..=125.0;

fn plausible(c: f64) -> Option<f64> {
    (c.is_finite() && PLAUSIBLE_C.contains(&c)).then_some(c)
}

fn bytes<const N: usize>(buf: &[u8], off: usize) -> Option<[u8; N]> {
    buf.get(off..off.checked_add(N)?)?.try_into().ok()
}

fn u32_at(buf: &[u8], off: usize) -> Option<u32> {
    bytes(buf, off).map(u32::from_le_bytes)
}

fn f32_at(buf: &[u8], off: usize) -> Option<f32> {
    bytes(buf, off).map(f32::from_le_bytes)
}

fn f64_at(buf: &[u8], off: usize) -> Option<f64> {
    bytes(buf, off).map(f64::from_le_bytes)
}

/// A NUL-terminated ANSI string in a fixed-size field.
fn cstr_at(buf: &[u8], off: usize, len: usize) -> Option<&[u8]> {
    let field = buf.get(off..off.checked_add(len)?)?;
    let end = field.iter().position(|&b| b == 0).unwrap_or(len);
    Some(&field[..end])
}

/// Both apps can be set to show Fahrenheit, and the unit string (``°C`` or
/// ``°F`` in the ANSI code page) says which.
fn to_celsius(value: f64, unit: &[u8]) -> f64 {
    if unit.iter().any(|b| b.eq_ignore_ascii_case(&b'f')) {
        (value - 32.0) * 5.0 / 9.0
    } else {
        value
    }
}

/// Pick the first reading whose label matches the earliest entry in
/// ``priority`` (case-insensitive, exact).
fn by_priority<'a, T>(
    items: &'a [T],
    priority: &[&str],
    label: impl Fn(&T) -> &[u8],
) -> Option<&'a T> {
    priority.iter().find_map(|want| {
        items
            .iter()
            .find(|item| label(item).eq_ignore_ascii_case(want.as_bytes()))
    })
}

/// HWiNFO's sensor shared memory (``HWiNFO_SENSORS_SHARED_MEM2``, packed).
/// It has to be switched on in HWiNFO's settings ("Shared Memory Support").
pub mod hwinfo {
    use super::*;

    #[cfg(windows)]
    pub const NAME: &str = "Global\\HWiNFO_SENS_SM2";
    /// ``'HWiS'`` as an MSVC multi-character constant.
    const SIGNATURE: u32 = 0x4857_6953;

    // Header offsets.
    const OFF_READING_SECTION: usize = 32;
    const OFF_READING_SIZE: usize = 36;
    const OFF_READING_COUNT: usize = 40;

    // Reading element offsets.
    const R_TYPE: usize = 0;
    const R_LABEL_ORIG: usize = 12;
    const R_UNIT: usize = 268;
    const R_VALUE: usize = 284;
    const R_MIN_SIZE: usize = R_VALUE + 8;
    const SENSOR_TEMP: u32 = 1;

    /// HWiNFO's own labels for the CPU package (Intel), Tctl/Tdie (AMD),
    /// then broader fallbacks.
    const PRIORITY: &[&str] = &[
        "CPU Package",
        "CPU (Tctl/Tdie)",
        "CPU Die (average)",
        "CPU (Tdie)",
        "CPU (Tctl)",
        "Core Max",
        "CPU",
    ];

    struct Reading<'a> {
        label: &'a [u8],
        celsius: f64,
    }

    pub fn cpu_temp(buf: &[u8]) -> Option<f64> {
        if u32_at(buf, 0)? != SIGNATURE {
            return None;
        }
        let start = u32_at(buf, OFF_READING_SECTION)? as usize;
        let size = u32_at(buf, OFF_READING_SIZE)? as usize;
        let count = u32_at(buf, OFF_READING_COUNT)? as usize;
        if size < R_MIN_SIZE {
            return None;
        }

        let readings: Vec<Reading> = (0..count)
            .map_while(|i| buf.get(start + i * size..start + (i + 1) * size))
            .filter(|r| u32_at(r, R_TYPE) == Some(SENSOR_TEMP))
            .filter_map(|r| {
                let unit = cstr_at(r, R_UNIT, 16)?;
                Some(Reading {
                    label: cstr_at(r, R_LABEL_ORIG, 128)?,
                    celsius: plausible(to_celsius(f64_at(r, R_VALUE)?, unit))?,
                })
            })
            .collect();

        by_priority(&readings, PRIORITY, |r| r.label).map(|r| r.celsius)
    }

    /// Build a snapshot for tests: ``(type, label, unit, value)`` readings.
    #[cfg(test)]
    pub fn fixture(readings: &[(u32, &str, &str, f64)]) -> Vec<u8> {
        const HEADER: usize = 48;
        const SIZE: usize = 460;
        let mut buf = vec![0u8; HEADER + readings.len() * SIZE];
        buf[0..4].copy_from_slice(&SIGNATURE.to_le_bytes());
        buf[OFF_READING_SECTION..][..4].copy_from_slice(&(HEADER as u32).to_le_bytes());
        buf[OFF_READING_SIZE..][..4].copy_from_slice(&(SIZE as u32).to_le_bytes());
        buf[OFF_READING_COUNT..][..4].copy_from_slice(&(readings.len() as u32).to_le_bytes());
        for (i, (kind, label, unit, value)) in readings.iter().enumerate() {
            let r = &mut buf[HEADER + i * SIZE..][..SIZE];
            r[R_TYPE..][..4].copy_from_slice(&kind.to_le_bytes());
            r[R_LABEL_ORIG..][..label.len()].copy_from_slice(label.as_bytes());
            r[R_UNIT..][..unit.len()].copy_from_slice(unit.as_bytes());
            r[R_VALUE..][..8].copy_from_slice(&value.to_le_bytes());
        }
        buf
    }
}

/// MSI Afterburner's hardware monitoring shared memory
/// (``MAHM_SHARED_MEMORY_HEADER`` / ``_ENTRY``, v2 layout). Afterburner
/// creates it in the signed-in user's session, so a collector running as
/// a service in session 0 can't see it.
pub mod afterburner {
    use super::*;

    #[cfg(windows)]
    pub const NAME: &str = "MAHMSharedMemory";
    /// ``'MAHM'`` as an MSVC multi-character constant.
    const SIGNATURE: u32 = 0x4D41_484D;
    const VERSION_2: u32 = 0x0002_0000;

    const OFF_VERSION: usize = 4;
    const OFF_HEADER_SIZE: usize = 8;
    const OFF_ENTRY_COUNT: usize = 12;
    const OFF_ENTRY_SIZE: usize = 16;

    const E_NAME: usize = 0;
    const E_UNITS: usize = 260;
    const E_DATA: usize = 1300;
    const E_SRC_ID: usize = 1320;
    /// ``MONITORING_SOURCE_ID_CPU_TEMPERATURE``; the package entry and the
    /// per-core ``CPUn temperature`` entries all carry it.
    const SRC_CPU_TEMPERATURE: u32 = 0x80;
    const PACKAGE_NAME: &str = "CPU temperature";

    pub fn cpu_temp(buf: &[u8]) -> Option<f64> {
        if u32_at(buf, 0)? != SIGNATURE {
            return None;
        }
        let has_src_id = u32_at(buf, OFF_VERSION)? >= VERSION_2;
        let start = u32_at(buf, OFF_HEADER_SIZE)? as usize;
        let count = u32_at(buf, OFF_ENTRY_COUNT)? as usize;
        let size = u32_at(buf, OFF_ENTRY_SIZE)? as usize;
        if size < E_DATA + 4 {
            return None;
        }

        let mut package = None;
        let mut hottest_core: Option<f64> = None;
        for e in (0..count).map_while(|i| buf.get(start + i * size..start + (i + 1) * size)) {
            let Some(name) = cstr_at(e, E_NAME, 260) else {
                continue;
            };
            let is_package = name.eq_ignore_ascii_case(PACKAGE_NAME.as_bytes());
            let is_cpu_temp =
                is_package || (has_src_id && u32_at(e, E_SRC_ID) == Some(SRC_CPU_TEMPERATURE));
            if !is_cpu_temp {
                continue;
            }
            let unit = cstr_at(e, E_UNITS, 260).unwrap_or_default();
            let Some(c) = f32_at(e, E_DATA).and_then(|v| plausible(to_celsius(v as f64, unit)))
            else {
                continue;
            };
            if is_package {
                package = Some(c);
                break;
            }
            hottest_core = Some(hottest_core.map_or(c, |h| h.max(c)));
        }
        package.or(hottest_core)
    }

    /// Build a snapshot for tests: ``(name, units, value, source id)``.
    #[cfg(test)]
    pub fn fixture(entries: &[(&str, &str, f32, u32)]) -> Vec<u8> {
        const HEADER: usize = 32;
        const SIZE: usize = 1324;
        let mut buf = vec![0u8; HEADER + entries.len() * SIZE];
        buf[0..4].copy_from_slice(&SIGNATURE.to_le_bytes());
        buf[OFF_VERSION..][..4].copy_from_slice(&VERSION_2.to_le_bytes());
        buf[OFF_HEADER_SIZE..][..4].copy_from_slice(&(HEADER as u32).to_le_bytes());
        buf[OFF_ENTRY_COUNT..][..4].copy_from_slice(&(entries.len() as u32).to_le_bytes());
        buf[OFF_ENTRY_SIZE..][..4].copy_from_slice(&(SIZE as u32).to_le_bytes());
        for (i, (name, units, value, src)) in entries.iter().enumerate() {
            let e = &mut buf[HEADER + i * SIZE..][..SIZE];
            e[E_NAME..][..name.len()].copy_from_slice(name.as_bytes());
            e[E_UNITS..][..units.len()].copy_from_slice(units.as_bytes());
            e[E_DATA..][..4].copy_from_slice(&value.to_le_bytes());
            e[E_SRC_ID..][..4].copy_from_slice(&src.to_le_bytes());
        }
        buf
    }
}

/// LibreHardwareMonitor / OpenHardwareMonitor, read from the ``Sensor``
/// class each publishes in its own WMI namespace.
pub mod lhm {
    use super::*;

    pub struct Sensor {
        pub identifier: String,
        pub name: String,
        pub value: f64,
    }

    const PRIORITY: &[&str] = &[
        "CPU Package",
        "Core (Tctl/Tdie)",
        "Core (Tdie)",
        "Core (Tctl)",
        "Core Max",
        "Core Average",
    ];

    /// Temperatures are always °C in WMI. Only sensors under a CPU
    /// hardware node (``/intelcpu/0/...``, ``/amdcpu/0/...``) count.
    pub fn cpu_temp(sensors: &[Sensor]) -> Option<f64> {
        let cpu: Vec<&Sensor> = sensors
            .iter()
            .filter(|s| s.identifier.to_ascii_lowercase().contains("cpu/"))
            .filter(|s| plausible(s.value).is_some())
            .collect();
        by_priority(&cpu, PRIORITY, |s| s.name.as_bytes())
            .map(|s| s.value)
            .or_else(|| cpu.iter().map(|s| s.value).reduce(f64::max))
    }
}

/// The ACPI thermal zones Windows exposes through the "Thermal Zone
/// Information" performance counters. On many desktops a zone is a fixed
/// placeholder (27.85 °C is common), so a zone only counts once its value
/// has moved since the collector started.
#[derive(Default)]
pub struct ThermalZones {
    zones: HashMap<String, Zone>,
}

struct Zone {
    first_kelvin: f64,
    live: bool,
}

impl ThermalZones {
    /// Feed one sample per zone (instance name, Kelvin) and get the CPU
    /// temperature in °C from the best live zone: one named for the CPU
    /// if there is one, otherwise the hottest.
    pub fn update(&mut self, samples: &[(String, f64)]) -> Option<f64> {
        let mut best: Option<(bool, f64)> = None;
        for (name, kelvin) in samples {
            if !kelvin.is_finite() {
                continue;
            }
            let zone = self.zones.entry(name.clone()).or_insert(Zone {
                first_kelvin: *kelvin,
                live: false,
            });
            zone.live |= (kelvin - zone.first_kelvin).abs() >= 0.05;
            if !zone.live {
                continue;
            }
            let Some(c) = plausible(kelvin - 273.15) else {
                continue;
            };
            let candidate = (name.to_ascii_lowercase().contains("cpu"), c);
            best = match best {
                Some(b) if (b.0, b.1) >= candidate => Some(b),
                _ => Some(candidate),
            };
        }
        best.map(|(_, c)| c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hwinfo_prefers_package_over_cores_and_skips_other_types() {
        let buf = hwinfo::fixture(&[
            (1, "Core #0", "\u{b0}C", 55.0),
            (5, "CPU Package", "W", 48.0), // power, not temperature
            (1, "GPU Temperature", "\u{b0}C", 40.0),
            (1, "CPU Package", "\u{b0}C", 61.5),
        ]);
        assert_eq!(hwinfo::cpu_temp(&buf), Some(61.5));
    }

    #[test]
    fn hwinfo_amd_and_fahrenheit() {
        let buf = hwinfo::fixture(&[(1, "CPU (Tctl/Tdie)", "\u{b0}F", 158.0)]);
        assert_eq!(hwinfo::cpu_temp(&buf), Some(70.0));
    }

    #[test]
    fn hwinfo_rejects_dead_or_truncated_memory() {
        let mut buf = hwinfo::fixture(&[(1, "CPU Package", "\u{b0}C", 61.5)]);
        assert_eq!(hwinfo::cpu_temp(&buf[..100]), None);
        buf[0..4].copy_from_slice(b"DAED");
        assert_eq!(hwinfo::cpu_temp(&buf), None);
        assert_eq!(hwinfo::cpu_temp(&[]), None);
    }

    #[test]
    fn hwinfo_ignores_cpu_less_sensors() {
        let buf = hwinfo::fixture(&[(1, "GPU Temperature", "\u{b0}C", 40.0)]);
        assert_eq!(hwinfo::cpu_temp(&buf), None);
    }

    #[test]
    fn afterburner_package_entry() {
        let buf = afterburner::fixture(&[
            ("GPU temperature", "\u{b0}C", 40.0, 0x01),
            ("CPU1 temperature", "\u{b0}C", 63.0, 0x80),
            ("CPU temperature", "\u{b0}C", 58.0, 0x80),
        ]);
        assert_eq!(afterburner::cpu_temp(&buf), Some(58.0));
    }

    #[test]
    fn afterburner_falls_back_to_hottest_core() {
        let buf = afterburner::fixture(&[
            ("CPU1 temperature", "\u{b0}C", 63.0, 0x80),
            ("CPU2 temperature", "\u{b0}C", 66.0, 0x80),
            ("CPU3 temperature", "\u{b0}C", f32::MAX, 0x80), // no data
        ]);
        assert_eq!(afterburner::cpu_temp(&buf), Some(66.0));
    }

    #[test]
    fn afterburner_without_cpu_entries() {
        let buf = afterburner::fixture(&[("GPU temperature", "\u{b0}C", 40.0, 0x01)]);
        assert_eq!(afterburner::cpu_temp(&buf), None);
        assert_eq!(afterburner::cpu_temp(&buf[..20]), None);
    }

    fn sensor(identifier: &str, name: &str, value: f64) -> lhm::Sensor {
        lhm::Sensor {
            identifier: identifier.into(),
            name: name.into(),
            value,
        }
    }

    #[test]
    fn lhm_picks_cpu_package() {
        let sensors = [
            sensor("/gpu-nvidia/0/temperature/0", "GPU Core", 45.0),
            sensor("/intelcpu/0/temperature/0", "CPU Core #1", 60.0),
            sensor("/intelcpu/0/temperature/8", "CPU Package", 62.0),
        ];
        assert_eq!(lhm::cpu_temp(&sensors), Some(62.0));
    }

    #[test]
    fn lhm_amd_and_fallback_to_hottest() {
        let amd = [
            sensor("/amdcpu/0/temperature/2", "Core (Tctl/Tdie)", 71.0),
            sensor("/amdcpu/0/temperature/3", "CCD1 (Tdie)", 69.0),
        ];
        assert_eq!(lhm::cpu_temp(&amd), Some(71.0));
        let unnamed = [
            sensor("/amdcpu/0/temperature/3", "CCD1 (Tdie)", 69.0),
            sensor("/amdcpu/0/temperature/4", "CCD2 (Tdie)", 72.0),
            sensor("/nvme/0/temperature/0", "Composite", 50.0),
        ];
        assert_eq!(lhm::cpu_temp(&unnamed), Some(72.0));
        assert_eq!(lhm::cpu_temp(&[]), None);
    }

    #[test]
    fn thermal_zone_placeholder_is_never_reported() {
        let mut tz = ThermalZones::default();
        for _ in 0..10 {
            assert_eq!(tz.update(&[("\\_TZ.TZ00".into(), 301.0)]), None);
        }
    }

    #[test]
    fn thermal_zone_reported_once_it_moves() {
        let mut tz = ThermalZones::default();
        assert_eq!(tz.update(&[("\\_TZ.THM0".into(), 318.15)]), None);
        assert_eq!(tz.update(&[("\\_TZ.THM0".into(), 319.15)]), Some(46.0));
        // Stays live when it settles back.
        let back = tz.update(&[("\\_TZ.THM0".into(), 318.15)]).unwrap();
        assert!((back - 45.0).abs() < 1e-9);
    }

    #[test]
    fn thermal_zone_prefers_cpu_named_zone() {
        let mut tz = ThermalZones::default();
        let first = [("\\_TZ.CPUZ".into(), 320.0), ("\\_TZ.TZ01".into(), 330.0)];
        tz.update(&first);
        let next = [("\\_TZ.CPUZ".into(), 321.0), ("\\_TZ.TZ01".into(), 331.0)];
        let c = tz.update(&next).unwrap();
        assert!((c - 47.85).abs() < 1e-9);
    }
}
