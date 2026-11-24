//! Draws the compact window into a picture, with no screen, and checks
//! what comes out. Set ``VIGIL_PREVIEW_DIR`` to a folder and the pictures
//! are also saved there as PNGs, which is how to see a change to
//! ``ui/compact.slint`` without a Windows machine.

use std::cell::RefCell;
use std::rc::Rc;

use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition};

use super::view::{self, History};
use crate::native::surface::{self, Frame, Surface};
use crate::native::ui::{Charts, CompactWindow, Theme};
use crate::native::{self, save_preview};
use crate::server::probes::ProbeResult;
use crate::snapshot::{DiskInfo, DiskIo, Gpu, GpuMemory, Process, ProcessCpu, Snapshot};

const PAGE_DARK: [u8; 3] = [0x0a, 0x0e, 0x17];

/// A made-up Windows machine, a little busy.
pub(super) fn snapshot(second: u32) -> Snapshot {
    let mut s = without_a_graphics_card(second);
    let wave = f64::from(second % 12) / 12.0;
    let used = 2.6 + wave / 4.0;
    s.gpu = Some(Gpu {
        name: "NVIDIA GeForce RTX 3070".into(),
        usage: 14.0 + 38.0 * (wave * std::f64::consts::PI).sin(),
        temp: 54.0,
        mem: GpuMemory {
            total: 8.0,
            used,
            free: 8.0 - used,
            percent: used / 8.0 * 100.0,
        },
    });
    // Who is using it, which the same machine can tell.
    let share = |name: &str| match name {
        "chrome" => 21.4,
        "Discord" => 3.2,
        "explorer" => 0.4,
        _ => 0.0,
    };
    for p in &mut s.processes {
        p.gpu = Some(share(&p.name));
    }
    for p in &mut s.processes_by_cpu {
        p.gpu = Some(share(&p.name));
    }
    // And the list by graphics card: those using it first, then the
    // busiest on the processor.
    let mut by_gpu = s.processes.clone();
    by_gpu.sort_by(|a, b| {
        b.gpu
            .partial_cmp(&a.gpu)
            .unwrap()
            .then(b.cpu.total_cmp(&a.cpu))
    });
    s.processes_by_gpu = by_gpu;
    s
}

/// The same machine with no card Vigil can read, which is also how one
/// looks anywhere but Windows as far as its processes go.
pub(super) fn without_a_graphics_card(second: u32) -> Snapshot {
    let mut s = Snapshot::default();
    let wave = f64::from(second % 20) / 20.0;
    s.platform.distro = "Windows 11 Pro".into();
    s.platform.kernel = "10.0.26200".into();
    s.uptime = "2 days, 4 hrs, 12 mins, 9 secs".into();
    s.cpu.usage = 6.0 + 30.0 * (wave * std::f64::consts::TAU).sin().max(0.0);
    s.cpu.freq = 3600.0;
    s.cpu.cores = 16;
    s.cpu.temp = 47.0;
    s.cpu.name = "Intel Core i7-11700".into();
    s.mem.kind = "DDR4".into();
    s.mem.manufacturer = "Corsair".into();
    s.mem.total = 31.9;
    s.mem.used = 14.2 + wave;
    s.mem.free = s.mem.total - s.mem.used;
    s.mem.percent = s.mem.used / s.mem.total * 100.0;
    s.disk.total = 931.5;
    s.disk.used = 512.3;
    s.disk.free = 419.2;
    s.disk.percent = 55.0;
    s.disk_io.read_bytes_per_sec = if second.is_multiple_of(9) {
        9_400_000
    } else {
        120_000
    };
    s.disk_io.write_bytes_per_sec = 310_000;
    // Its one disk, as the collector lists it.
    s.disks = vec![DiskInfo {
        device: "C:\\".into(),
        mountpoint: "C:\\".into(),
        label: "Windows".into(),
        fstype: "NTFS".into(),
        total: s.disk.total,
        used: s.disk.used,
        free: s.disk.free,
        percent: s.disk.percent,
        io: Some(s.disk_io.clone()),
    }];
    s.network.rx_bytes_per_sec = 180_000 + u64::from(second % 7) * 260_000;
    s.network.tx_bytes_per_sec = 42_000 + u64::from(second % 5) * 90_000;
    let names = [
        ("chrome", 310.0, 2890.0),
        ("Code", 122.0, 1420.0),
        ("vigil-collector", 0.6, 9.0),
        ("explorer", 14.0, 212.0),
        ("Discord", 9.0, 498.0),
        ("steam", 6.5, 356.0),
        ("WindowsTerminal", 4.0, 143.0),
    ];
    s.processes_by_cpu = names
        .iter()
        .enumerate()
        .map(|(i, (name, cpu, mem))| ProcessCpu {
            pid: i as u32,
            name: (*name).into(),
            username: "chris".into(),
            cpu: *cpu,
            mem: *mem,
            gpu: None,
        })
        .collect();
    let mut by_memory: Vec<_> = names.to_vec();
    by_memory.sort_by(|a, b| b.2.total_cmp(&a.2));
    s.processes = by_memory
        .iter()
        .enumerate()
        .map(|(i, (name, cpu, mem))| Process {
            pid: i as u32,
            name: (*name).into(),
            username: "chris".into(),
            mem: *mem,
            cpu: *cpu,
            gpu: None,
        })
        .collect();
    s
}

/// The first machine with two more disks: a big one for games, nearly
/// full and busy now and then, and a backup disk that mostly sleeps.
pub(super) fn with_more_disks(second: u32) -> Snapshot {
    let mut s = snapshot(second);
    let disk = |mountpoint: &str, label: &str, total: f64, percent: f64, read: u64, write: u64| {
        let used = total * percent / 100.0;
        DiskInfo {
            device: mountpoint.into(),
            mountpoint: mountpoint.into(),
            label: label.into(),
            fstype: "NTFS".into(),
            total,
            used,
            free: total - used,
            percent,
            io: Some(DiskIo {
                read_bytes_per_sec: read,
                write_bytes_per_sec: write,
                ..DiskIo::default()
            }),
        }
    };
    let loading = if (second % 30) < 8 { 46_000_000 } else { 0 };
    s.disks.push(disk(
        "D:\\",
        "Games",
        1863.0,
        84.2,
        loading + u64::from(second % 4) * 900_000,
        24_000,
    ));
    s.disks.push(disk(
        "E:\\",
        "Backup",
        3726.0,
        12.0,
        0,
        if second.is_multiple_of(17) {
            2_100_000
        } else {
            0
        },
    ));
    s
}

fn probes(all_well: bool) -> Vec<ProbeResult> {
    let probe = |name: &str, ok: bool, latency_ms: u64, error: Option<&str>| ProbeResult {
        name: name.into(),
        url: format!("https://{name}/"),
        ok,
        status_code: ok.then_some(200),
        latency_ms,
        error: error.map(str::to_string),
    };
    vec![
        probe("versyx.digital", true, 48, None),
        probe("status.edcs.app", true, 112, None),
        probe(
            "api.example.com",
            all_well,
            if all_well { 95 } else { 3000 },
            (!all_well).then_some("operation timed out"),
        ),
    ]
}

/// The window as the tests hold it: the layout, where it draws, and the
/// picture so far.
struct Shown {
    ui: CompactWindow,
    surface: Rc<Surface>,
    history: Rc<RefCell<History>>,
    readings: fn(u32) -> Snapshot,
    frame: Frame,
}

impl Shown {
    /// The window after a minute of readings, at ``width`` by ``height``
    /// layout pixels and ``scale`` real pixels to each.
    fn new(width: u32, height: u32, scale: f32, dark: bool) -> Shown {
        Shown::of(snapshot, width, height, scale, dark)
    }

    /// The same, of the machine ``readings`` makes up.
    fn of(readings: fn(u32) -> Snapshot, width: u32, height: u32, scale: f32, dark: bool) -> Shown {
        let (ui, surface) = surface::open(CompactWindow::new).expect("the layout can be made");
        let history = Rc::new(RefCell::new(History::default()));
        view::draw_from(&ui, &history);
        ui.global::<Theme>().set_dark(dark);
        ui.global::<Charts>().set_scale(scale);
        let (real_width, real_height) = (
            (width as f32 * scale) as u32,
            (height as f32 * scale) as u32,
        );
        surface.resize(real_width, real_height, scale);
        native::paint_page(&ui, real_width, real_height, scale);
        let mut shown = Shown {
            ui,
            surface,
            history,
            readings,
            frame: Frame::default(),
        };
        for second in 0..60 {
            shown.read(second);
        }
        view::show_services(&shown.ui, &probes(true));
        shown.paint();
        shown
    }

    fn read(&mut self, second: u32) {
        let snapshot = (self.readings)(second);
        self.history.borrow_mut().remember(&snapshot);
        view::show(&self.ui, &snapshot);
        view::redraw_sparklines(&self.ui);
    }

    fn paint(&mut self) -> bool {
        surface::tick();
        self.surface.paint(&mut self.frame)
    }

    /// The colour at a place given in layout pixels.
    fn at(&self, x: f32, y: f32) -> [u8; 3] {
        let scale = self.surface.scale();
        let (x, y) = ((x * scale) as u32, (y * scale) as u32);
        self.frame.pixels[(y * self.frame.width + x) as usize].rgb()
    }

    fn click(&mut self, x: f32, y: f32) {
        let position = LogicalPosition::new(x, y);
        let button = PointerEventButton::Left;
        self.surface.send(WindowEvent::PointerMoved { position });
        self.surface
            .send(WindowEvent::PointerPressed { position, button });
        self.surface
            .send(WindowEvent::PointerReleased { position, button });
        self.surface.send(WindowEvent::PointerExited);
        self.paint();
    }

    /// Saves the picture as ``name``.png in ``VIGIL_PREVIEW_DIR``, if set.
    fn save(&self, name: &str) {
        save_preview(&self.frame, name);
    }
}

/// How far apart two colours are, as the largest difference in any of
/// red, green and blue.
fn apart(a: [u8; 3], b: [u8; 3]) -> u8 {
    (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap_or(0)
}

/// Where the memory card's bar is on a machine with no graphics card:
/// under the bar, the platform card and the processor's.
const MEMORY_BAR_Y: f32 = 33.0 + 8.0 + 46.0 + 6.0 + 46.0 + 6.0 + 45.5;
/// How much further down everything is with the graphics card's card
/// above it.
const GPU_CARD: f32 = 46.0 + 6.0;
/// And with one more disk's card above it; a disk's bar is this far
/// under the memory card's.
const DISK_CARD: f32 = 55.0 + 6.0;
const AMBER: [u8; 3] = [0xfb, 0xbf, 0x24];
const GREEN: [u8; 3] = [0x34, 0xd3, 0x99];

#[test]
fn draws_the_dashboard_at_its_usual_size() {
    let shown = Shown::new(420, 632, 1.0, true);
    shown.save("compact-dark");
    assert_eq!((shown.frame.width, shown.frame.height), (420, 632));
    // The title bar is the dark slate of the web version's, not the page.
    let bar = shown.at(60.0, 16.0);
    assert!(apart(bar, [0x17, 0x20, 0x31]) < 24, "{bar:?}");
    // Between two cards the page shows, tinted by its glows.
    assert!(apart(shown.at(4.0, 300.0), PAGE_DARK) < 60);
    // The memory card's bar is amber where it is filled and not beyond.
    let bar_y = MEMORY_BAR_Y + GPU_CARD;
    assert!(apart(shown.at(40.0, bar_y), AMBER) < 40);
    assert!(apart(shown.at(380.0, bar_y), AMBER) > 100);
}

#[test]
fn the_graphics_card_has_a_card_between_the_processor_and_memory() {
    let shown = Shown::new(420, 632, 1.0, true);
    // Where the memory card's bar would be without it is its own card,
    // which has a pink sparkline and no bar.
    assert!(apart(shown.at(40.0, MEMORY_BAR_Y), AMBER) > 100);
    let pink = (236..332).any(|x| {
        (MEMORY_BAR_Y as u32 - 36..MEMORY_BAR_Y as u32 - 8)
            .any(|y| apart(shown.at(x as f32, y as f32), [0xf4, 0x72, 0xb6]) < 40)
    });
    assert!(pink, "no pink line where the card's sparkline should be");
}

#[test]
fn a_machine_without_a_graphics_card_has_no_card_for_one() {
    let shown = Shown::of(without_a_graphics_card, 420, 580, 1.0, true);
    shown.save("compact-no-gpu");
    // Memory follows the processor directly, as it always did.
    assert!(apart(shown.at(40.0, MEMORY_BAR_Y), AMBER) < 40);
    assert!(apart(shown.at(40.0, MEMORY_BAR_Y + GPU_CARD), AMBER) > 100);
}

#[test]
fn a_graphics_card_that_turns_up_gets_its_card_then() {
    let mut shown = Shown::of(without_a_graphics_card, 420, 632, 1.0, true);
    assert!(apart(shown.at(40.0, MEMORY_BAR_Y), AMBER) < 40);
    shown.readings = snapshot;
    shown.read(61);
    shown.paint();
    assert!(apart(shown.at(40.0, MEMORY_BAR_Y + GPU_CARD), AMBER) < 40);
}

#[test]
fn every_disk_has_a_card_of_its_own() {
    let shown = Shown::of(with_more_disks, 420, 754, 1.0, true);
    shown.save("compact-disks-dark");
    // Three green bars under memory's amber one, each as long as its disk
    // is full: 55%, 84% and 12%.
    let first = MEMORY_BAR_Y + GPU_CARD + DISK_CARD;
    assert!(apart(shown.at(40.0, first - DISK_CARD), AMBER) < 40);
    for (card, full_at, empty_at) in [(0.0, 200.0, 260.0), (1.0, 320.0, 380.0), (2.0, 40.0, 80.0)] {
        let y = first + card * DISK_CARD;
        assert!(apart(shown.at(full_at, y), GREEN) < 40, "card {card}");
        assert!(apart(shown.at(empty_at, y), GREEN) > 100, "card {card}");
    }
    // And no fourth.
    assert!(apart(shown.at(40.0, first + 3.0 * DISK_CARD), GREEN) > 100);

    Shown::of(with_more_disks, 420, 754, 1.0, false).save("compact-disks-light");
}

#[test]
fn a_disk_that_is_plugged_in_gets_its_card_then() {
    let mut shown = Shown::new(420, 754, 1.0, true);
    let second_card = MEMORY_BAR_Y + GPU_CARD + 2.0 * DISK_CARD;
    assert!(apart(shown.at(320.0, second_card), GREEN) > 100);
    shown.readings = with_more_disks;
    shown.read(61);
    shown.paint();
    assert!(apart(shown.at(320.0, second_card), GREEN) < 40);
    // And loses it when it is taken away again.
    shown.readings = snapshot;
    shown.read(62);
    shown.paint();
    assert!(apart(shown.at(320.0, second_card), GREEN) > 100);
}

#[test]
fn draws_at_twice_the_size_on_a_dense_display() {
    let shown = Shown::new(420, 580, 2.0, true);
    shown.save("compact-dark-200");
    assert_eq!((shown.frame.width, shown.frame.height), (840, 1160));
}

#[test]
fn draws_sharper_on_a_scaled_display() {
    let shown = Shown::new(420, 580, 1.5, true);
    shown.save("compact-dark-150");
    assert_eq!((shown.frame.width, shown.frame.height), (630, 870));
    assert!(apart(shown.at(200.0, 16.0), [0x17, 0x20, 0x31]) < 24);
}

#[test]
fn the_light_theme_is_light() {
    let shown = Shown::new(420, 632, 1.0, false);
    shown.save("compact-light");
    let page = shown.at(4.0, 300.0);
    assert!(page.iter().all(|c| *c > 0xc0), "{page:?}");
    // The title bar stays dark in both themes.
    assert!(shown.at(200.0, 16.0).iter().all(|c| *c < 0x60));
}

#[test]
fn nothing_is_painted_until_something_changes() {
    let mut shown = Shown::new(420, 580, 1.0, true);
    assert!(!shown.paint());
    shown.read(61);
    assert!(shown.paint());
    assert!(!shown.paint());
}

#[test]
fn a_failing_service_is_listed_without_being_asked() {
    let mut shown = Shown::new(420, 640, 1.0, true);
    let before = shown.frame.pixels.clone();
    view::show_services(&shown.ui, &probes(false));
    shown.paint();
    shown.save("compact-service-down");
    assert_ne!(before, shown.frame.pixels);
}

#[test]
fn pressing_view_all_lists_every_process() {
    let mut shown = Shown::new(420, 752, 1.0, true);
    // Five rows end where the panel does; the sixth would be under it.
    let sixth_row_y =
        33.0 + 8.0 + 5.0 * 52.0 + 2.0 * 9.0 + 46.0 + 6.0 + 34.0 + 6.0 + 34.0 + 22.0 + 5.5 * 22.0;
    let below = shown.at(100.0, sixth_row_y + 30.0);
    shown.click(380.0, sixth_row_y - 5.5 * 22.0 - 22.0 - 17.0);
    shown.save("compact-all-processes");
    assert_ne!(below, shown.at(100.0, sixth_row_y + 30.0));
}

#[test]
fn pressing_the_gpu_heading_sorts_by_the_graphics_card() {
    let mut shown = Shown::new(420, 752, 1.0, true);
    let rows_y = 33.0 + 8.0 + 5.0 * 52.0 + 2.0 * 9.0 + 46.0 + 6.0 + 34.0 + 6.0 + 34.0 + 22.0;
    // A third of the way along the second row's bar. By processor that
    // row is Code, at two fifths of the busiest; by graphics card it is
    // Discord, at a seventh.
    let bar = (182.0, rows_y + 22.0 + 11.0);
    let green = shown.at(bar.0, bar.1);
    shown.click(305.0, rows_y - 11.0);
    shown.save("compact-processes-by-gpu");
    assert_ne!(green, shown.at(bar.0, bar.1));
    // And back by the processor's heading.
    shown.click(253.0, rows_y - 11.0);
    assert_eq!(green, shown.at(bar.0, bar.1));
}

#[test]
fn the_same_readings_again_paint_nothing() {
    let mut shown = Shown::new(420, 580, 1.0, true);
    // The same readings, shown again without a new point of history, as
    // when nothing on the machine moved: not a pixel needs repainting.
    view::show(&shown.ui, &snapshot(59));
    view::show_services(&shown.ui, &probes(true));
    assert!(!shown.paint());
}

#[test]
fn fits_a_narrow_window() {
    let shown = Shown::new(360, 300, 1.0, true);
    shown.save("compact-smallest");
    assert_eq!((shown.frame.width, shown.frame.height), (360, 300));
}

#[test]
fn stretches_across_a_wide_window() {
    let shown = Shown::new(860, 580, 1.0, true);
    shown.save("compact-wide");
    assert_eq!(shown.frame.width, 860);
}
