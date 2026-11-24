//! Turns the collector's readings into what the compact window shows: the
//! wording of every figure, the rows of its two lists, and the minute of
//! history its sparklines draw.
//!
//! The wording follows web/src/components/compact/ line for line, so the
//! native window and the web dashboard say the same thing in the same
//! way.

use std::cell::RefCell;
use std::rc::Rc;

use slint::{
    Color, ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString,
    VecModel,
};

use super::spark::{self, HISTORY, Scale};
use crate::native::ui::{Charts, CompactWindow, DiskRow, ProcessRow, ServiceRow};
use crate::server::probes::ProbeResult;
use crate::snapshot::{DiskInfo, Snapshot};

/// A response slower than this, in milliseconds, counts as slow (the web
/// dashboard's ``config.services.thresholds.slow``).
const SLOW_MS: u64 = 800;

/// The sparklines. ``ui/compact.slint`` knows each by a number (see
/// ``Series::index``).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Series {
    Cpu,
    Memory,
    /// Reads and writes of every disk together.
    DiskIo,
    Sent,
    Received,
    Gpu,
    /// Reads and writes of one disk: the one on the Disk card this many
    /// down from the first.
    Disk(usize),
}

/// The number of the first disk's own series; the next disk's is one
/// more, and so on.
const FIRST_DISK: i32 = 100;

impl Series {
    fn index(self) -> i32 {
        match self {
            Series::Cpu => 0,
            Series::Memory => 1,
            Series::DiskIo => 2,
            Series::Sent => 3,
            Series::Received => 4,
            Series::Gpu => 5,
            Series::Disk(row) => FIRST_DISK + row as i32,
        }
    }

    fn from_index(index: i32) -> Option<Series> {
        if index >= FIRST_DISK {
            return Some(Series::Disk((index - FIRST_DISK) as usize));
        }
        [
            Series::Cpu,
            Series::Memory,
            Series::DiskIo,
            Series::Sent,
            Series::Received,
            Series::Gpu,
        ]
        .into_iter()
        .find(|series| series.index() == index)
    }
}

/// The last minute of readings, oldest first.
#[derive(Default)]
pub struct History {
    cpu: Vec<f64>,
    memory: Vec<f64>,
    disk_io: Vec<f64>,
    sent: Vec<f64>,
    received: Vec<f64>,
    gpu: Vec<f64>,
    /// Each shown disk's own reads and writes, by its mount point, in
    /// the order of the Disk cards.
    disks: Vec<(String, Vec<f64>)>,
}

impl History {
    pub fn remember(&mut self, snapshot: &Snapshot) {
        let keep = |series: &mut Vec<f64>, value: f64| {
            series.push(if value.is_finite() { value } else { 0.0 });
            if series.len() > HISTORY {
                series.drain(..series.len() - HISTORY);
            }
        };
        keep(&mut self.cpu, snapshot.cpu.usage);
        keep(&mut self.memory, snapshot.mem.percent);
        keep(
            &mut self.disk_io,
            (snapshot.disk_io.read_bytes_per_sec + snapshot.disk_io.write_bytes_per_sec) as f64,
        );
        keep(&mut self.sent, snapshot.network.tx_bytes_per_sec as f64);
        keep(&mut self.received, snapshot.network.rx_bytes_per_sec as f64);
        // Nothing while there is no card, so one that turns up later
        // starts its line from there.
        keep(
            &mut self.gpu,
            snapshot.gpu.as_ref().map_or(0.0, |gpu| gpu.usage),
        );
        // A disk keeps its line wherever its card moves to; one that is
        // taken away loses it, and starts again if it comes back.
        let shown = shown_disks(snapshot);
        let mut disks = Vec::with_capacity(shown.len());
        for disk in &shown {
            let mut series = self
                .disks
                .iter_mut()
                .find(|(mountpoint, _)| *mountpoint == disk.mountpoint)
                .map(|(_, series)| std::mem::take(series))
                .unwrap_or_default();
            keep(
                &mut series,
                disk_throughput(disk, &shown, snapshot).unwrap_or(0.0),
            );
            disks.push((disk.mountpoint.clone(), series));
        }
        self.disks = disks;
    }

    fn of(&self, series: Series) -> &[f64] {
        match series {
            Series::Cpu => &self.cpu,
            Series::Memory => &self.memory,
            Series::DiskIo => &self.disk_io,
            Series::Sent => &self.sent,
            Series::Received => &self.received,
            Series::Gpu => &self.gpu,
            Series::Disk(row) => self.disks.get(row).map_or(&[], |(_, series)| series),
        }
    }

    /// The top of the scale ``series`` is drawn against. Sent and
    /// received share a box, so they share a scale.
    fn top(&self, series: Series) -> f64 {
        match series {
            Series::Cpu => Scale::AtLeast(25.0).top(&[&self.cpu]),
            Series::Gpu => Scale::AtLeast(25.0).top(&[&self.gpu]),
            Series::Memory => Scale::Top(100.0).top(&[]),
            Series::DiskIo | Series::Disk(_) => {
                Scale::AtLeast(1024.0 * 1024.0).top(&[self.of(series)])
            }
            Series::Sent | Series::Received => {
                Scale::AtLeast(64.0 * 1024.0).top(&[&self.sent, &self.received])
            }
        }
    }
}

/// Has the layout ask ``history`` for its sparklines: the line as an
/// outline, and the tint under it as a small picture.
pub fn draw_from(ui: &CompactWindow, history: &Rc<RefCell<History>>) {
    let charts = ui.global::<Charts>();
    charts.on_line({
        let history = history.clone();
        move |series, _revision, width, height| {
            let Some(series) = Series::from_index(series) else {
                return SharedString::new();
            };
            let history = history.borrow();
            spark::line(history.of(series), history.top(series), width, height).into()
        }
    });
    charts.on_tint({
        let history = history.clone();
        move |series, _revision, width, height, scale, color| {
            // Painted at the display's real pixels, so it is as sharp as
            // the line over it.
            let (width, height) = (
                (width * scale).round() as u32,
                (height * scale).round() as u32,
            );
            let Some(series) = Series::from_index(series) else {
                return Image::default();
            };
            let history = history.borrow();
            let picture = spark::tint(
                history.of(series),
                history.top(series),
                width,
                height,
                [color.red(), color.green(), color.blue()],
            );
            if picture.is_empty() {
                return Image::default();
            }
            Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
                &picture, width, height,
            ))
        }
    });
}

/// Shows ``snapshot``'s readings.
pub fn show(ui: &CompactWindow, snapshot: &Snapshot) {
    let distro = snapshot.platform.distro.trim();
    ui.set_platform_is_windows(distro.to_lowercase().contains("windows"));
    ui.set_platform_name(
        if distro.is_empty() {
            "PLATFORM".into()
        } else {
            distro.to_uppercase()
        }
        .into(),
    );
    ui.set_kernel(or_dash(&snapshot.platform.kernel).into());
    ui.set_uptime(or_dash(&short_uptime(&snapshot.uptime)).into());

    ui.set_cpu_value(percent(snapshot.cpu.usage).into());
    ui.set_cpu_name(snapshot.cpu.name.as_str().into());
    ui.set_cpu_detail(cpu_detail(snapshot.cpu.freq, snapshot.cpu.cores).into());
    if snapshot.cpu.temp > 0.0 {
        ui.set_cpu_temperature(format!("{:.0} °C", snapshot.cpu.temp).into());
        ui.set_cpu_temperature_color(temperature_color(snapshot.cpu.temp));
    } else {
        ui.set_cpu_temperature(SharedString::new());
    }

    ui.set_gpu_shown(snapshot.gpu.is_some());
    if let Some(gpu) = &snapshot.gpu {
        let memory = Usage::of(gpu.mem.used, gpu.mem.total, gpu.mem.free);
        ui.set_gpu_value(percent(gpu.usage).into());
        ui.set_gpu_name(gpu_model(&gpu.name).into());
        ui.set_gpu_detail(format!("{} / {} {}", memory.used, memory.total, memory.unit).into());
        if gpu.temp > 0.0 {
            ui.set_gpu_temperature(format!("{:.0} °C", gpu.temp).into());
            ui.set_gpu_temperature_color(temperature_color(gpu.temp));
        } else {
            ui.set_gpu_temperature(SharedString::new());
        }
    }

    let memory = Usage::of(snapshot.mem.used, snapshot.mem.total, snapshot.mem.free);
    ui.set_memory_value(percent(snapshot.mem.percent).into());
    ui.set_memory_name(memory_name(&snapshot.mem.kind, &snapshot.mem.manufacturer).into());
    ui.set_memory_share(snapshot.mem.percent as f32);
    ui.set_memory_detail(
        format!(
            "{} / {} {} · {} free",
            memory.used, memory.total, memory.unit, memory.free
        )
        .into(),
    );

    show_rows(&ui.get_disks(), disk_rows(snapshot), |rows| {
        ui.set_disks(rows)
    });

    ui.set_network_down(format!("↓ {}", rate(snapshot.network.rx_bytes_per_sec as f64)).into());
    ui.set_network_up(format!("↑ {}", rate(snapshot.network.tx_bytes_per_sec as f64)).into());

    ui.set_processes_gpu(
        snapshot.processes.iter().any(|p| p.gpu.is_some())
            || snapshot.processes_by_cpu.iter().any(|p| p.gpu.is_some()),
    );
    let (by_cpu, by_gpu, by_memory) = process_rows(snapshot);
    if let Some(rows) = changed(&ui.get_processes_by_cpu(), by_cpu) {
        ui.set_processes_by_cpu(rows);
    }
    if let Some(rows) = changed(&ui.get_processes_by_gpu(), by_gpu) {
        ui.set_processes_by_gpu(rows);
    }
    if let Some(rows) = changed(&ui.get_processes_by_memory(), by_memory) {
        ui.set_processes_by_memory(rows);
    }
}

/// ``rows`` as a list for the layout, unless they are what ``shown``
/// already holds. Handing the layout a new list has it build every row
/// again, and the collector only looks at the processes every few
/// readings, so most of the time there is nothing new to build.
fn changed<T: Clone + PartialEq + 'static>(shown: &ModelRc<T>, rows: Vec<T>) -> Option<ModelRc<T>> {
    if shown.row_count() == rows.len() && shown.iter().eq(rows.iter().cloned()) {
        return None;
    }
    Some(ModelRc::new(VecModel::from(rows)))
}

/// Shows ``rows`` in the list the layout holds as ``shown``. While there
/// are as many rows as before, each one that reads differently is changed
/// where it is, which leaves the layout's cards standing; only a
/// different number of rows has ``replace`` hand it a new list.
fn show_rows<T: Clone + PartialEq + 'static>(
    shown: &ModelRc<T>,
    rows: Vec<T>,
    replace: impl FnOnce(ModelRc<T>),
) {
    if shown.row_count() != rows.len() {
        return replace(ModelRc::new(VecModel::from(rows)));
    }
    for (index, row) in rows.into_iter().enumerate() {
        if shown.row_data(index).as_ref() != Some(&row) {
            shown.set_row_data(index, row);
        }
    }
}

/// How many Disk cards ``snapshot`` fills.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn disk_cards(snapshot: &Snapshot) -> usize {
    shown_disks(snapshot).len().max(1)
}

/// The disks that get a card each, in the order of the cards: the
/// collector's list (the primary disk first) without what isn't a disk
/// to its owner. That is anything of no size (a card reader with no card
/// in it), Linux's loop devices (snap packages), and a device already
/// listed under another mount point.
fn shown_disks(snapshot: &Snapshot) -> Vec<&DiskInfo> {
    let mut shown: Vec<&DiskInfo> = Vec::new();
    for disk in &snapshot.disks {
        let sized = disk.total.is_finite() && disk.total > 0.0;
        let listed = shown.iter().any(|other| other.device == disk.device);
        if sized && !listed && !disk.device.starts_with("/dev/loop") {
            shown.push(disk);
        }
    }
    shown
}

/// What ``disk`` is reading and writing, in bytes a second, where that
/// is known. A machine's only disk does all of the machine's reading
/// and writing, so there the total stands in for a figure of its own.
fn disk_throughput(disk: &DiskInfo, shown: &[&DiskInfo], snapshot: &Snapshot) -> Option<f64> {
    let both =
        |io: &crate::snapshot::DiskIo| (io.read_bytes_per_sec + io.write_bytes_per_sec) as f64;
    match &disk.io {
        Some(io) => Some(both(io)),
        None if shown.len() == 1 => Some(both(&snapshot.disk_io)),
        None => None,
    }
}

/// Which disk a card is for: where it is mounted, and what its owner
/// called it if anything. "C: · Games", or "/home".
fn disk_name(disk: &DiskInfo) -> String {
    let mountpoint = match disk.mountpoint.trim_end_matches('\\') {
        "" => disk.mountpoint.as_str(),
        trimmed => trimmed,
    };
    if disk.label.is_empty() {
        mountpoint.to_string()
    } else {
        format!("{mountpoint} · {}", disk.label)
    }
}

/// The Disk cards: one for each shown disk, or one for the primary disk
/// alone from a collector that lists none.
fn disk_rows(snapshot: &Snapshot) -> Vec<DiskRow> {
    let row = |name: String, used, total, free, share: f64, io: Option<f64>, series: Series| {
        let usage = Usage::of(used, total, free);
        let mut detail = format!("{} / {} {}", usage.used, usage.total, usage.unit);
        if let Some(io) = io {
            detail.push_str(" · ");
            detail.push_str(&rate(io));
        }
        DiskRow {
            name: name.into(),
            value: percent(share).into(),
            detail: detail.into(),
            share: share as f32,
            // Nothing to draw for a disk whose reads and writes aren't
            // known.
            series: if io.is_some() { series.index() } else { -1 },
        }
    };
    let shown = shown_disks(snapshot);
    if shown.is_empty() {
        let disk = &snapshot.disk;
        let io = snapshot.disk_io.read_bytes_per_sec + snapshot.disk_io.write_bytes_per_sec;
        return vec![row(
            String::new(),
            disk.used,
            disk.total,
            disk.free,
            disk.percent,
            Some(io as f64),
            Series::DiskIo,
        )];
    }
    shown
        .iter()
        .enumerate()
        .map(|(index, disk)| {
            row(
                disk_name(disk),
                disk.used,
                disk.total,
                disk.free,
                disk.percent,
                disk_throughput(disk, &shown, snapshot),
                Series::Disk(index),
            )
        })
        .collect()
}

/// Has the sparklines drawn again, after ``History::remember``.
pub fn redraw_sparklines(ui: &CompactWindow) {
    let charts = ui.global::<Charts>();
    charts.set_revision(charts.get_revision().wrapping_add(1));
}

/// Shows what the service checks last found.
pub fn show_services(ui: &CompactWindow, probes: &[ProbeResult]) {
    let rows: Vec<ServiceRow> = probes.iter().map(service_row).collect();
    let (overall, label) = overall(&rows);
    ui.set_services_overall(overall);
    ui.set_services_label(label.into());
    if let Some(rows) = changed(&ui.get_services(), rows) {
        ui.set_services(rows);
    }
}

/// How a check stands, as ``ui/compact.slint`` numbers it.
mod status {
    pub const ONLINE: i32 = 1;
    pub const SLOW: i32 = 2;
    pub const OFFLINE: i32 = 3;
}

fn service_row(probe: &ProbeResult) -> ServiceRow {
    let status = if !probe.ok {
        status::OFFLINE
    } else if probe.latency_ms >= SLOW_MS {
        status::SLOW
    } else {
        status::ONLINE
    };
    let detail = if status == status::OFFLINE {
        probe
            .error
            .clone()
            .unwrap_or_else(|| "no response".to_string())
    } else {
        probe.latency_ms.to_string()
    };
    ServiceRow {
        name: probe.name.as_str().into(),
        url: probe.url.as_str().into(),
        status,
        code: probe
            .status_code
            .map(|code| code.to_string())
            .unwrap_or_default()
            .into(),
        detail: detail.into(),
    }
}

/// The one-line verdict on all the checks, and how to colour it.
fn overall(rows: &[ServiceRow]) -> (i32, String) {
    if rows.is_empty() {
        return (0, "CHECKING".into());
    }
    let offline = rows.iter().filter(|r| r.status == status::OFFLINE).count();
    let slow = rows.iter().filter(|r| r.status == status::SLOW).count();
    if offline == rows.len() {
        (status::OFFLINE, "ALL DOWN".into())
    } else if offline > 0 {
        (status::OFFLINE, format!("{offline} DOWN"))
    } else if slow > 0 {
        (status::SLOW, "DEGRADED".into())
    } else {
        (status::ONLINE, "ALL OPERATIONAL".into())
    }
}

/// The lists the collector sends, the ten busiest by each measure, as
/// rows: by processor, by graphics card (empty where the collector
/// can't tell that apart by process) and by memory. A process's CPU
/// arrives as a share of one core and is shown as a share of the whole
/// machine, the scale the CPU card uses.
fn process_rows(snapshot: &Snapshot) -> (Vec<ProcessRow>, Vec<ProcessRow>, Vec<ProcessRow>) {
    let cores = f64::from(snapshot.cpu.cores.max(1));
    let row = |name: &str, cpu: f64, gpu: Option<f64>, mem: f64, amount: f64, most: f64| {
        let (mem, mem_unit) = process_memory(mem);
        ProcessRow {
            name: name.to_uppercase().into(),
            share: (amount / most * 100.0).clamp(2.0, 100.0) as f32,
            cpu: format!("{:.1}", cpu / cores).into(),
            gpu: gpu
                .map(|gpu| format!("{gpu:.1}"))
                .unwrap_or_default()
                .into(),
            mem: mem.into(),
            mem_unit: mem_unit.into(),
        }
    };
    let most_cpu = snapshot
        .processes_by_cpu
        .iter()
        .fold(1.0_f64, |most, p| most.max(p.cpu / cores));
    let by_cpu = snapshot
        .processes_by_cpu
        .iter()
        .map(|p| row(&p.name, p.cpu, p.gpu, p.mem, p.cpu / cores, most_cpu))
        .collect();
    // Against a whole percent at least, so a card nobody is using
    // doesn't fill every bar.
    let most_gpu = snapshot
        .processes_by_gpu
        .iter()
        .fold(1.0_f64, |most, p| most.max(p.gpu.unwrap_or(0.0)));
    let by_gpu = snapshot
        .processes_by_gpu
        .iter()
        .map(|p| {
            let amount = p.gpu.unwrap_or(0.0);
            row(&p.name, p.cpu, p.gpu, p.mem, amount, most_gpu)
        })
        .collect();
    let most_memory = snapshot
        .processes
        .iter()
        .fold(0.001_f64, |most, p| most.max(p.mem));
    let by_memory = snapshot
        .processes
        .iter()
        .map(|p| row(&p.name, p.cpu, p.gpu, p.mem, p.mem, most_memory))
        .collect();
    (by_cpu, by_gpu, by_memory)
}

/// A process's memory, which arrives in MiB: whole MiB below a GiB, and
/// two decimals of the larger unit from there.
fn process_memory(mib: f64) -> (String, &'static str) {
    if !mib.is_finite() {
        return ("–".into(), "");
    }
    match size(mib, Unit::MiB) {
        (_, Unit::MiB) => (format!("{mib:.0}"), "MiB"),
        (value, unit) => (format!("{value:.2}"), unit.name()),
    }
}

fn or_dash(text: &str) -> String {
    if text.trim().is_empty() {
        "–".into()
    } else {
        text.to_string()
    }
}

/// A percent to one decimal, or a dash for a reading that isn't one.
fn percent(value: f64) -> String {
    if value.is_finite() {
        format!("{value:.1}")
    } else {
        "–".into()
    }
}

/// "3.60 GHz · 16 cores", without the speed where the processor doesn't
/// report one.
fn cpu_detail(mhz: f64, cores: u32) -> String {
    if mhz <= 0.0 || !mhz.is_finite() {
        format!("{cores} cores")
    } else if mhz >= 1000.0 {
        format!("{:.2} GHz · {cores} cores", mhz / 1000.0)
    } else {
        format!("{mhz:.0} MHz · {cores} cores")
    }
}

/// A graphics card's model without its maker, which the card has no room
/// for: "NVIDIA GeForce RTX 3070" as "GeForce RTX 3070".
fn gpu_model(name: &str) -> &str {
    ["NVIDIA ", "AMD ", "Intel "]
        .iter()
        .find_map(|maker| name.strip_prefix(maker))
        .filter(|model| !model.is_empty())
        .unwrap_or(name)
}

/// What memory is fitted, as much as is known: "DDR4 · Corsair".
fn memory_name(kind: &str, manufacturer: &str) -> String {
    [kind, manufacturer]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Green to 50 °C, orange to 70, red beyond (the web dashboard's
/// ``mapTemperatureColor``).
fn temperature_color(celsius: f64) -> Color {
    if celsius > 70.0 {
        Color::from_rgb_u8(0xff, 0x41, 0x36)
    } else if celsius > 50.0 {
        Color::from_rgb_u8(0xff, 0x90, 0x6b)
    } else {
        Color::from_rgb_u8(0x41, 0xb8, 0x83)
    }
}

/// "1 day, 6 hrs, 6 mins, 4 secs" as "1d 6h 6m 4s".
fn short_uptime(uptime: &str) -> String {
    uptime
        .split(',')
        .filter_map(|part| {
            let mut words = part.split_whitespace();
            let count = words.next()?;
            let unit = words.next()?.chars().next()?;
            Some(format!("{count}{unit}"))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unit {
    MiB,
    GiB,
    TiB,
}

impl Unit {
    const ALL: [Unit; 3] = [Unit::MiB, Unit::GiB, Unit::TiB];

    fn name(self) -> &'static str {
        match self {
            Unit::MiB => "MiB",
            Unit::GiB => "GiB",
            Unit::TiB => "TiB",
        }
    }

    /// How many MiB one of it is.
    fn in_mib(self) -> f64 {
        1024_f64.powi(self as i32)
    }
}

/// ``amount`` of ``from`` in the largest of MiB, GiB and TiB that keeps
/// the number at 1 or more, so a 1863 GiB drive reads as 1.82 TiB.
fn size(amount: f64, from: Unit) -> (f64, Unit) {
    let mib = amount * from.in_mib();
    let unit = Unit::ALL
        .into_iter()
        .rev()
        .find(|unit| mib.abs() >= unit.in_mib())
        .unwrap_or(Unit::MiB);
    (mib / unit.in_mib(), unit)
}

/// Used, total and free in one unit, the total's: "12.4 / 31.8 GiB".
struct Usage {
    used: String,
    total: String,
    free: String,
    unit: &'static str,
}

impl Usage {
    /// All three arrive in GiB.
    fn of(used: f64, total: f64, free: f64) -> Usage {
        let (total, unit) = if total.is_finite() {
            size(total, Unit::GiB)
        } else {
            (f64::NAN, Unit::GiB)
        };
        let in_unit = |gib: f64| {
            if gib.is_finite() {
                format!("{:.1}", gib * Unit::GiB.in_mib() / unit.in_mib())
            } else {
                "–".to_string()
            }
        };
        Usage {
            used: in_unit(used),
            free: in_unit(free),
            total: if total.is_finite() {
                format!("{total:.1}")
            } else {
                "--".into()
            },
            unit: unit.name(),
        }
    }
}

/// A throughput in bytes a second, in the largest unit that keeps the
/// number at 1 or more.
fn rate(bytes_per_second: f64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let n = if bytes_per_second.is_finite() {
        bytes_per_second.max(0.0)
    } else {
        0.0
    };
    if n >= GB {
        format!("{:.2} GB/s", n / GB)
    } else if n >= MB {
        format!("{:.2} MB/s", n / MB)
    } else if n >= KB {
        format!("{:.1} KB/s", n / KB)
    } else {
        format!("{n:.0} B/s")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        History, Series, Unit, Usage, cpu_detail, disk_cards, disk_rows, gpu_model, memory_name,
        overall, percent, process_memory, process_rows, rate, service_row, short_uptime, size,
        status,
    };
    use crate::server::probes::ProbeResult;
    use crate::snapshot::{DiskInfo, DiskIo, Gpu, Process, ProcessCpu, Snapshot};

    #[test]
    fn uptime_is_shortened_to_initials() {
        assert_eq!(short_uptime("1 day, 6 hrs, 6 mins, 4 secs"), "1d 6h 6m 4s");
        assert_eq!(short_uptime("23 mins, 24 secs"), "23m 24s");
        assert_eq!(short_uptime("1 hr"), "1h");
        assert_eq!(short_uptime(""), "");
    }

    #[test]
    fn sizes_use_the_largest_unit_that_fits() {
        assert_eq!(size(15.7, Unit::GiB), (15.7, Unit::GiB));
        assert_eq!(size(0.5, Unit::GiB), (512.0, Unit::MiB));
        let (value, unit) = size(1863.0, Unit::GiB);
        assert_eq!(unit, Unit::TiB);
        assert_eq!(format!("{value:.2}"), "1.82");
        assert_eq!(size(455.0, Unit::MiB), (455.0, Unit::MiB));
    }

    #[test]
    fn usage_is_all_in_the_totals_unit() {
        let usage = Usage::of(0.7, 15.7, 15.0);
        assert_eq!(
            (
                usage.used.as_str(),
                usage.total.as_str(),
                usage.free.as_str(),
                usage.unit
            ),
            ("0.7", "15.7", "15.0", "GiB")
        );
        // A two-terabyte drive: used and free follow it into TiB.
        let usage = Usage::of(931.5, 1863.0, 931.5);
        assert_eq!(
            (usage.used.as_str(), usage.total.as_str(), usage.unit),
            ("0.9", "1.8", "TiB")
        );
    }

    #[test]
    fn rates_step_up_through_the_units() {
        assert_eq!(rate(0.0), "0 B/s");
        assert_eq!(rate(1023.0), "1023 B/s");
        assert_eq!(rate(250_000.0), "244.1 KB/s");
        assert_eq!(rate(5.0 * 1024.0 * 1024.0), "5.00 MB/s");
        assert_eq!(rate(f64::NAN), "0 B/s");
        assert_eq!(rate(-4.0), "0 B/s");
    }

    #[test]
    fn readings_that_are_not_numbers_show_a_dash() {
        assert_eq!(percent(1.84), "1.8");
        assert_eq!(percent(f64::NAN), "–");
    }

    #[test]
    fn cpu_detail_leaves_out_an_unknown_speed() {
        assert_eq!(cpu_detail(2100.0, 4), "2.10 GHz · 4 cores");
        assert_eq!(cpu_detail(800.0, 2), "800 MHz · 2 cores");
        assert_eq!(cpu_detail(0.0, 16), "16 cores");
    }

    #[test]
    fn process_memory_is_whole_mib_below_a_gib() {
        assert_eq!(process_memory(455.4), ("455".into(), "MiB"));
        assert_eq!(process_memory(2048.0), ("2.00".into(), "GiB"));
    }

    fn snapshot() -> Snapshot {
        let mut snapshot = Snapshot::default();
        snapshot.cpu.cores = 4;
        snapshot.processes_by_cpu = vec![
            ProcessCpu {
                pid: 1,
                name: "chrome".into(),
                username: "chris".into(),
                cpu: 200.0,
                mem: 455.0,
                gpu: Some(41.26),
            },
            ProcessCpu {
                pid: 2,
                name: "idle".into(),
                username: "chris".into(),
                cpu: 0.0,
                mem: 7.0,
                gpu: None,
            },
        ];
        snapshot.processes = vec![Process {
            pid: 1,
            name: "chrome".into(),
            username: "chris".into(),
            mem: 455.0,
            cpu: 200.0,
            gpu: Some(41.26),
        }];
        snapshot
    }

    /// The same machine with a list by graphics card too.
    fn snapshot_with_a_list_by_gpu() -> Snapshot {
        let mut snapshot = snapshot();
        let process = |name: &str, cpu: f64, gpu: f64| Process {
            pid: 1,
            name: name.into(),
            username: "chris".into(),
            mem: 455.0,
            cpu,
            gpu: Some(gpu),
        };
        snapshot.processes_by_gpu = vec![
            process("game", 40.0, 82.5),
            process("chrome", 200.0, 41.26),
            process("idle", 0.0, 0.0),
        ];
        snapshot
    }

    #[test]
    fn process_cpu_is_a_share_of_the_whole_machine() {
        let (by_cpu, by_gpu, by_memory) = process_rows(&snapshot());
        // No list by graphics card from a collector that can't tell.
        assert!(by_gpu.is_empty());
        // Two of four cores flat out is half the machine.
        assert_eq!(by_cpu[0].cpu, "50.0");
        assert_eq!(by_cpu[0].name, "CHROME");
        assert_eq!(by_cpu[0].share, 100.0);
        // An idle process still gets a sliver of bar.
        assert_eq!(by_cpu[1].share, 2.0);
        assert_eq!(by_memory[0].mem, "455");
        assert_eq!(by_memory[0].share, 100.0);
        // Its share of the graphics card, and blank where that isn't known.
        assert_eq!(by_cpu[0].gpu, "41.3");
        assert_eq!(by_cpu[1].gpu, "");
        assert_eq!(by_memory[0].gpu, "41.3");
    }

    #[test]
    fn the_list_by_gpu_is_drawn_against_its_busiest() {
        let (_, by_gpu, _) = process_rows(&snapshot_with_a_list_by_gpu());
        let shown: Vec<_> = by_gpu
            .iter()
            .map(|row| (row.name.as_str(), row.gpu.as_str(), row.share))
            .collect();
        assert_eq!(
            shown,
            [
                ("GAME", "82.5", 100.0),
                ("CHROME", "41.3", (41.26_f64 / 82.5 * 100.0) as f32),
                ("IDLE", "0.0", 2.0),
            ]
        );
    }

    #[test]
    fn an_idle_card_fills_no_bars() {
        let mut snapshot = snapshot_with_a_list_by_gpu();
        for process in &mut snapshot.processes_by_gpu {
            process.gpu = Some(0.2);
        }
        let (_, by_gpu, _) = process_rows(&snapshot);
        assert!(by_gpu.iter().all(|row| row.share == 20.0));
    }

    #[test]
    fn part_names_are_cut_to_what_matters() {
        assert_eq!(gpu_model("NVIDIA GeForce RTX 3070"), "GeForce RTX 3070");
        assert_eq!(gpu_model("AMD Radeon RX 6800 XT"), "Radeon RX 6800 XT");
        assert_eq!(gpu_model("Intel UHD Graphics 750"), "UHD Graphics 750");
        assert_eq!(gpu_model("Graphics card"), "Graphics card");
        assert_eq!(memory_name("DDR4", "Corsair"), "DDR4 · Corsair");
        assert_eq!(memory_name("DDR5", ""), "DDR5");
        assert_eq!(memory_name("", ""), "");
    }

    #[test]
    fn the_graphics_card_has_a_line_of_its_own() {
        let mut history = History::default();
        let mut snapshot = Snapshot::default();
        history.remember(&snapshot);
        snapshot.gpu = Some(Gpu {
            usage: 80.0,
            ..Gpu::default()
        });
        history.remember(&snapshot);
        assert_eq!(history.of(Series::Gpu), [0.0, 80.0]);
        assert_eq!(history.top(Series::Gpu), 80.0 * 1.15);
    }

    /// A disk half full, reading and writing ``bytes`` a second between
    /// them, or not saying if ``None``.
    fn disk(mountpoint: &str, label: &str, total: f64, bytes: Option<u64>) -> DiskInfo {
        DiskInfo {
            device: mountpoint.into(),
            mountpoint: mountpoint.into(),
            label: label.into(),
            fstype: "NTFS".into(),
            total,
            used: total / 2.0,
            free: total / 2.0,
            percent: 50.0,
            io: bytes.map(|bytes| DiskIo {
                read_bytes_per_sec: bytes / 4,
                write_bytes_per_sec: bytes - bytes / 4,
                ..DiskIo::default()
            }),
        }
    }

    /// What the cards say: each one's name, detail and series.
    fn said(snapshot: &Snapshot) -> Vec<(String, String, i32)> {
        disk_rows(snapshot)
            .iter()
            .map(|row| (row.name.to_string(), row.detail.to_string(), row.series))
            .collect()
    }

    #[test]
    fn every_disk_has_a_card_with_its_own_reads_and_writes() {
        let mut snapshot = Snapshot::default();
        snapshot.disk_io.read_bytes_per_sec = 9 * 1024 * 1024;
        snapshot.disks = vec![
            disk("C:\\", "", 475.1, Some(2 * 1024 * 1024)),
            disk("D:\\", "Games", 1863.0, Some(0)),
            disk("E:\\", "Backup", 931.5, None),
        ];
        assert_eq!(
            said(&snapshot),
            [
                ("C:".into(), "237.6 / 475.1 GiB · 2.00 MB/s".into(), 100),
                ("D: · Games".into(), "0.9 / 1.8 TiB · 0 B/s".into(), 101),
                // No figure and no sparkline where the disk doesn't say.
                ("E: · Backup".into(), "465.8 / 931.5 GiB".into(), -1),
            ]
        );
        assert_eq!(disk_rows(&snapshot)[1].value, "50.0");
        assert_eq!(disk_rows(&snapshot)[1].share, 50.0);
        assert_eq!(disk_cards(&snapshot), 3);
    }

    #[test]
    fn only_real_disks_get_a_card() {
        let mut snapshot = Snapshot::default();
        let on = |device: &str, mountpoint: &str, total: f64| DiskInfo {
            device: device.into(),
            ..disk(mountpoint, "", total, Some(0))
        };
        snapshot.disks = vec![
            on("/dev/nvme0n1p2", "/", 475.1),
            // The same partition again, a snap package and an empty slot.
            on("/dev/nvme0n1p2", "/var/lib/docker", 475.1),
            on("/dev/loop3", "/snap/core22/1122", 0.07),
            on("/dev/sdb1", "/media/card", 0.0),
            on("/dev/sda1", "/home", 1863.0),
        ];
        let names: Vec<_> = said(&snapshot).into_iter().map(|row| row.0).collect();
        assert_eq!(names, ["/", "/home"]);
    }

    #[test]
    fn a_machine_that_lists_no_disks_shows_its_primary_one() {
        let mut snapshot = Snapshot::default();
        snapshot.disk.total = 475.1;
        snapshot.disk.used = 400.1;
        snapshot.disk.free = 75.0;
        snapshot.disk.percent = 84.2;
        snapshot.disk_io.write_bytes_per_sec = 1_331_692;
        assert_eq!(
            said(&snapshot),
            [(
                String::new(),
                "400.1 / 475.1 GiB · 1.27 MB/s".into(),
                Series::DiskIo.index()
            )]
        );
        assert_eq!(disk_rows(&snapshot)[0].value, "84.2");
        assert_eq!(disk_cards(&snapshot), 1);
    }

    #[test]
    fn an_only_disk_that_does_not_say_takes_the_machines_total() {
        let mut snapshot = Snapshot::default();
        snapshot.disk_io.read_bytes_per_sec = 5 * 1024 * 1024;
        snapshot.disks = vec![disk("/", "", 100.0, None)];
        assert_eq!(
            said(&snapshot),
            [("/".into(), "50.0 / 100.0 GiB · 5.00 MB/s".into(), 100)]
        );
        let mut history = History::default();
        history.remember(&snapshot);
        assert_eq!(history.of(Series::Disk(0)), [5.0 * 1024.0 * 1024.0]);
    }

    #[test]
    fn a_disks_line_follows_the_disk() {
        let mut history = History::default();
        let mut snapshot = Snapshot {
            disks: vec![
                disk("C:\\", "", 475.1, Some(100)),
                disk("D:\\", "", 931.5, Some(200)),
                disk("E:\\", "", 931.5, Some(300)),
            ],
            ..Snapshot::default()
        };
        history.remember(&snapshot);
        history.remember(&snapshot);
        assert_eq!(history.of(Series::Disk(1)), [200.0, 200.0]);
        assert_eq!(history.of(Series::Disk(2)), [300.0, 300.0]);
        // D: is unplugged: E: moves up a card and keeps its line.
        snapshot.disks.remove(1);
        history.remember(&snapshot);
        assert_eq!(history.of(Series::Disk(1)), [300.0, 300.0, 300.0]);
        assert!(history.of(Series::Disk(2)).is_empty());
        // Back again, it starts a new one.
        snapshot.disks.insert(1, disk("D:\\", "", 931.5, Some(200)));
        history.remember(&snapshot);
        assert_eq!(history.of(Series::Disk(1)), [200.0]);
        // Each is drawn against its own busiest second, a megabyte at least.
        assert_eq!(history.top(Series::Disk(1)), 1024.0 * 1024.0);
        assert_eq!(
            Series::from_index(Series::Disk(2).index()),
            Some(Series::Disk(2))
        );
        assert_eq!(Series::from_index(5), Some(Series::Gpu));
        assert_eq!(Series::from_index(6), None);
    }

    fn probe(ok: bool, latency_ms: u64) -> ProbeResult {
        ProbeResult {
            name: "site".into(),
            url: "https://example.com".into(),
            ok,
            status_code: ok.then_some(200),
            latency_ms,
            error: None,
        }
    }

    #[test]
    fn services_are_online_slow_or_offline() {
        assert_eq!(service_row(&probe(true, 12)).status, status::ONLINE);
        assert_eq!(service_row(&probe(true, 12)).detail, "12");
        assert_eq!(service_row(&probe(true, 800)).status, status::SLOW);
        let down = service_row(&probe(false, 3000));
        assert_eq!(down.status, status::OFFLINE);
        assert_eq!(down.detail, "no response");
        assert_eq!(down.code, "");
    }

    #[test]
    fn the_verdict_is_the_worst_of_the_checks() {
        let rows = |probes: &[ProbeResult]| probes.iter().map(service_row).collect::<Vec<_>>();
        assert_eq!(overall(&[]).1, "CHECKING");
        assert_eq!(
            overall(&rows(&[probe(true, 1), probe(true, 2)])).1,
            "ALL OPERATIONAL"
        );
        assert_eq!(
            overall(&rows(&[probe(true, 1), probe(true, 900)])).1,
            "DEGRADED"
        );
        assert_eq!(
            overall(&rows(&[probe(false, 1), probe(true, 900)])).1,
            "1 DOWN"
        );
        assert_eq!(overall(&rows(&[probe(false, 1)])).1, "ALL DOWN");
    }

    #[test]
    fn history_keeps_one_minute() {
        let mut history = History::default();
        let mut snapshot = Snapshot::default();
        for second in 0..75 {
            snapshot.cpu.usage = f64::from(second);
            history.remember(&snapshot);
        }
        assert_eq!(history.of(Series::Cpu).len(), 60);
        assert_eq!(history.of(Series::Cpu)[0], 15.0);
        assert_eq!(history.of(Series::Cpu)[59], 74.0);
        // The CPU's scale follows the tallest reading; memory's is fixed.
        assert_eq!(history.top(Series::Cpu), 74.0 * 1.15);
        assert_eq!(history.top(Series::Memory), 100.0);
    }

    #[test]
    fn sent_and_received_share_a_scale() {
        let mut history = History::default();
        let mut snapshot = Snapshot::default();
        snapshot.network.rx_bytes_per_sec = 1_000_000;
        snapshot.network.tx_bytes_per_sec = 10;
        history.remember(&snapshot);
        assert_eq!(history.top(Series::Sent), history.top(Series::Received));
        assert_eq!(history.top(Series::Sent), 1_000_000.0 * 1.15);
    }
}
