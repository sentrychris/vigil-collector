//! CPU temperature on Windows.
//!
//! A background thread polls the sources once a second, in this order,
//! and publishes the first reading it gets:
//!
//! 1. HWiNFO's shared memory (if HWiNFO runs with shared memory enabled)
//! 2. MSI Afterburner's shared memory (if Afterburner runs in this session)
//! 3. LibreHardwareMonitor / OpenHardwareMonitor's WMI namespace
//! 4. The ACPI thermal zone, once its value has been seen to move
//!
//! None of these needs admin rights or a driver of our own. The thread
//! keeps WMI's COM state on one thread, and keeps a slow or hung WMI query
//! from stalling the sampler.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use serde::Deserialize;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Memory::{
    FILE_MAP_READ, MEMORY_BASIC_INFORMATION, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile,
    VirtualQuery,
};

use super::pdh::Pdh;
use super::temp_parse::{ThermalZones, afterburner, hwinfo, lhm};

const POLL_EVERY: Duration = Duration::from_secs(1);
/// How often to look for a monitor app's WMI namespace while none is
/// running. Connecting to a missing namespace costs a COM round trip.
const WMI_RETRY_EVERY: Duration = Duration::from_secs(30);
/// Upper bound on a shared-memory copy, far above either app's real size.
const MAX_SHARED_MEMORY: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Source {
    HwInfo,
    Afterburner,
    HardwareMonitor,
    ThermalZone,
}

impl Source {
    fn describe(self) -> &'static str {
        match self {
            Source::HwInfo => "HWiNFO",
            Source::Afterburner => "MSI Afterburner",
            Source::HardwareMonitor => "LibreHardwareMonitor",
            Source::ThermalZone => "the ACPI thermal zone",
        }
    }
}

/// The latest CPU temperature, kept fresh by a background thread that
/// stops once this is dropped.
pub struct CpuTemp {
    /// °C as ``f64`` bits; NaN when no source has a reading.
    latest: Arc<AtomicU64>,
}

impl CpuTemp {
    pub fn start() -> Self {
        let latest = Arc::new(AtomicU64::new(f64::NAN.to_bits()));
        let weak = Arc::downgrade(&latest);
        if let Err(e) = std::thread::Builder::new()
            .name("cpu-temp".into())
            .spawn(move || poll(weak))
        {
            tracing::warn!(
                "CPU temperature disabled: could not start its thread: {}",
                e
            );
        }
        Self { latest }
    }

    pub fn read(&self) -> Option<f64> {
        let v = f64::from_bits(self.latest.load(Ordering::Relaxed));
        (!v.is_nan()).then_some(v)
    }
}

fn poll(latest: Weak<AtomicU64>) {
    let mut wmi = Wmi::default();
    let mut zones = open_thermal_zones();
    let mut tracker = ThermalZones::default();
    let mut current: Option<Option<Source>> = None;

    loop {
        let Some(out) = latest.upgrade() else {
            return;
        };

        // Sample the zones every tick even while another source wins, so a
        // live zone is already known if that app exits.
        let zone = zones
            .as_mut()
            .and_then(sample_zones)
            .and_then(|samples| tracker.update(&samples));

        let reading = shared_memory(hwinfo::NAME)
            .and_then(|b| hwinfo::cpu_temp(&b))
            .map(|c| (Source::HwInfo, c))
            .or_else(|| {
                shared_memory(afterburner::NAME)
                    .and_then(|b| afterburner::cpu_temp(&b))
                    .map(|c| (Source::Afterburner, c))
            })
            .or_else(|| wmi.cpu_temp().map(|c| (Source::HardwareMonitor, c)))
            .or(zone.map(|c| (Source::ThermalZone, c)));

        let source = reading.map(|(s, _)| s);
        if current != Some(source) {
            match source {
                Some(s) => tracing::info!("CPU temperature: reading from {}", s.describe()),
                None => tracing::info!(
                    "CPU temperature: no sensor available, reporting 0. Windows needs a \
                     monitoring app for this; run HWiNFO (with shared memory enabled), \
                     MSI Afterburner or LibreHardwareMonitor"
                ),
            }
            current = Some(source);
        }
        let value = reading.map_or(f64::NAN, |(_, c)| c);
        out.store(value.to_bits(), Ordering::Relaxed);
        drop(out);

        std::thread::sleep(POLL_EVERY);
    }
}

/// Copy a named shared-memory section, or ``None`` if no app has created
/// it. The section is opened and closed each time so an app that restarts
/// is picked up again.
fn shared_memory(name: &str) -> Option<Vec<u8>> {
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // SAFETY: ``wide`` is NUL-terminated. The view is only read within the
    // region size VirtualQuery reports, then unmapped, and the handle is
    // closed on every path. The writer may update the region while we
    // copy; the parsers validate everything they read.
    unsafe {
        let handle = OpenFileMappingW(FILE_MAP_READ, 0, wide.as_ptr());
        if handle.is_null() {
            return None;
        }
        let view = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0);
        let mut copy = None;
        if !view.Value.is_null() {
            let mut info: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
            let got = VirtualQuery(
                view.Value,
                &mut info,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            );
            if got != 0 {
                let len = info.RegionSize.min(MAX_SHARED_MEMORY);
                let mut buf = vec![0u8; len];
                std::ptr::copy_nonoverlapping(view.Value as *const u8, buf.as_mut_ptr(), len);
                copy = Some(buf);
            }
            UnmapViewOfFile(view);
        }
        CloseHandle(handle);
        copy
    }
}

#[derive(Deserialize)]
#[serde(rename = "Sensor", rename_all = "PascalCase")]
struct WmiSensor {
    identifier: String,
    name: String,
    value: f64,
}

/// A connection to whichever hardware monitor's WMI namespace exists.
#[derive(Default)]
struct Wmi {
    conn: Option<wmi::WMIConnection>,
    last_attempt: Option<Instant>,
}

impl Wmi {
    const NAMESPACES: [&str; 2] = ["ROOT\\LibreHardwareMonitor", "ROOT\\OpenHardwareMonitor"];
    const QUERY: &str =
        "SELECT Identifier, Name, Value FROM Sensor WHERE SensorType = 'Temperature'";

    fn cpu_temp(&mut self) -> Option<f64> {
        if self.conn.is_none() {
            if self
                .last_attempt
                .is_some_and(|t| t.elapsed() < WMI_RETRY_EVERY)
            {
                return None;
            }
            self.last_attempt = Some(Instant::now());
            self.conn = Self::NAMESPACES
                .iter()
                .find_map(|ns| wmi::WMIConnection::with_namespace_path(ns).ok());
        }
        let rows: Vec<WmiSensor> = match self.conn.as_ref()?.raw_query(Self::QUERY) {
            Ok(rows) => rows,
            Err(e) => {
                // The app exited (the namespace goes with it) or WMI hiccuped.
                tracing::debug!("hardware monitor WMI query failed: {}", e);
                self.conn = None;
                return None;
            }
        };
        let sensors: Vec<lhm::Sensor> = rows
            .into_iter()
            .map(|r| lhm::Sensor {
                identifier: r.identifier,
                name: r.name,
                value: r.value,
            })
            .collect();
        lhm::cpu_temp(&sensors)
    }
}

/// The thermal zones' counter and the multiplier from its unit to Kelvin:
/// "High Precision Temperature" (tenths of a Kelvin, Windows 10 1709+)
/// with "Temperature" (whole Kelvin) as the fallback. ``None`` when the
/// PC exposes no thermal zones.
fn open_thermal_zones() -> Option<(Pdh, f64)> {
    [
        (
            "\\Thermal Zone Information(*)\\High Precision Temperature",
            0.1,
        ),
        ("\\Thermal Zone Information(*)\\Temperature", 1.0),
    ]
    .into_iter()
    .find_map(|(path, to_kelvin)| Some((Pdh::open(path)?, to_kelvin)))
}

/// One reading per zone, in Kelvin.
fn sample_zones((pdh, to_kelvin): &mut (Pdh, f64)) -> Option<Vec<(String, f64)>> {
    let mut samples = pdh.sample()?;
    for (_, value) in &mut samples {
        *value *= *to_kelvin;
    }
    Some(samples)
}
