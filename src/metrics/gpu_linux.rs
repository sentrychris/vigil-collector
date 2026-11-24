//! The graphics card on Linux, from what the kernel's graphics drivers
//! publish about each card under ``/sys/class/drm/card*/device``:
//!
//! - ``gpu_busy_percent``: how busy it is
//! - ``mem_info_vram_total`` and ``mem_info_vram_used``: its memory, bytes
//! - ``hwmon/hwmon*/temp1_input``: its temperature, thousandths of a °C
//!
//! Today that is the ``amdgpu`` driver. Intel's drivers publish no figure
//! for how busy the card is, and NVIDIA's own driver publishes nothing
//! here at all (it has a library of its own instead), so machines with
//! only those have no reading. Plain files, read each tick; no root.

use std::fs;
use std::path::{Path, PathBuf};

use super::gpu::memory;
use crate::snapshot::Gpu;

const CARDS: &str = "/sys/class/drm";
/// Where distributions keep the list of what each PCI device is called.
const PCI_IDS: [&str; 2] = ["/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids"];

pub struct Reader {
    cards: PathBuf,
    /// The card last read and what it is called, so the name is looked
    /// up once.
    named: Option<(PathBuf, String)>,
}

impl Reader {
    pub fn new() -> Self {
        Self::over(Path::new(CARDS))
    }

    fn over(cards: &Path) -> Self {
        Self {
            cards: cards.to_path_buf(),
            named: None,
        }
    }

    /// The card with the most memory, of those that say how busy they
    /// are.
    pub fn read(&mut self) -> Option<Gpu> {
        let (device, total) = fs::read_dir(&self.cards)
            .ok()?
            .flatten()
            .filter(|entry| is_card(&entry.file_name().to_string_lossy()))
            .map(|entry| entry.path().join("device"))
            .filter(|device| device.join("gpu_busy_percent").exists())
            .map(|device| {
                let total = number(&device.join("mem_info_vram_total")).unwrap_or(0);
                (device, total)
            })
            // The lowest-numbered card wins a tie, whatever order the
            // directory lists them in.
            .min_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)))?;

        let name = match &self.named {
            Some((named, name)) if *named == device => name.clone(),
            _ => {
                let name = name_of(&device);
                self.named = Some((device.clone(), name.clone()));
                name
            }
        };

        // A card the kernel has powered down (a laptop's second one) is
        // left asleep: reading its figures would wake it.
        let asleep = fs::read_to_string(device.join("power/runtime_status"))
            .is_ok_and(|status| status.trim() == "suspended");
        if asleep {
            return Some(Gpu {
                name,
                usage: 0.0,
                temp: 0.0,
                mem: memory(total, 0),
            });
        }

        Some(Gpu {
            name,
            usage: number(&device.join("gpu_busy_percent"))?.min(100) as f64,
            temp: temperature(&device).unwrap_or(0.0),
            mem: memory(
                total,
                number(&device.join("mem_info_vram_used")).unwrap_or(0),
            ),
        })
    }
}

/// ``card0``, but not ``card0-DP-1`` (one of its outputs) or
/// ``renderD128``.
fn is_card(name: &str) -> bool {
    name.strip_prefix("card")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

fn number(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn temperature(device: &Path) -> Option<f64> {
    fs::read_dir(device.join("hwmon"))
        .ok()?
        .flatten()
        .find_map(|sensor| number(&sensor.path().join("temp1_input")))
        .map(|milli| super::round1(milli as f64 / 1000.0))
        .filter(|celsius| (5.0..=125.0).contains(celsius))
}

/// What the card is called: its maker, and its model where the system
/// has the list that says.
fn name_of(device: &Path) -> String {
    let id = |file: &str| {
        let text = fs::read_to_string(device.join(file)).ok()?;
        u16::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok()
    };
    let (Some(vendor), Some(model)) = (id("vendor"), id("device")) else {
        return "Graphics card".to_string();
    };
    let listed = PCI_IDS
        .iter()
        .find_map(|path| fs::read_to_string(path).ok())
        .and_then(|ids| model_in(&ids, vendor, model));
    name_from(vendor, listed.as_deref())
}

fn name_from(vendor: u16, model: Option<&str>) -> String {
    let maker = match vendor {
        0x1002 => "AMD",
        0x10de => "NVIDIA",
        0x8086 => "Intel",
        _ => "",
    };
    match (maker, model) {
        ("", None) => "Graphics card".to_string(),
        ("", Some(model)) => model.to_string(),
        (maker, None) => format!("{maker} graphics"),
        (maker, Some(model)) => format!("{maker} {model}"),
    }
}

/// Looks a device up in ``pci.ids``: makers start a line with their id,
/// and their devices follow, each on a line indented by one tab. The
/// marketing name is the part in square brackets, where there is one:
/// "Navi 21 [Radeon RX 6800/6800 XT / 6900 XT]".
fn model_in(ids: &str, vendor: u16, model: u16) -> Option<String> {
    let vendor = format!("{vendor:04x}  ");
    let model = format!("\t{model:04x}  ");
    let line = ids
        .lines()
        .skip_while(|line| !line.starts_with(&vendor))
        .skip(1)
        .take_while(|line| line.starts_with('\t') || line.starts_with('#'))
        .find(|line| line.starts_with(&model))?;
    let described = line[model.len()..].trim();
    let name = match (described.find('['), described.rfind(']')) {
        (Some(open), Some(close)) if open < close => &described[open + 1..close],
        _ => described,
    };
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{Reader, is_card, model_in, name_from};

    /// A made-up ``/sys/class/drm`` in the temporary folder, removed
    /// again when dropped.
    struct Cards(PathBuf);

    impl Cards {
        fn new(test: &str) -> Cards {
            let root =
                std::env::temp_dir().join(format!("vigil-gpu-{test}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            Cards(root)
        }

        fn write(&self, file: &str, text: &str) {
            let path = self.0.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }

        /// A card that reports the way ``amdgpu`` does.
        fn card(&self, name: &str, busy: u32, total_gib: u64, used_gib: u64) {
            let gib = 1024 * 1024 * 1024;
            self.write(
                &format!("{name}/device/gpu_busy_percent"),
                &format!("{busy}\n"),
            );
            self.write(
                &format!("{name}/device/mem_info_vram_total"),
                &format!("{}\n", total_gib * gib),
            );
            self.write(
                &format!("{name}/device/mem_info_vram_used"),
                &format!("{}\n", used_gib * gib),
            );
            // An id no list of devices has, so the name doesn't depend
            // on what this machine has installed.
            self.write(&format!("{name}/device/vendor"), "0x1002\n");
            self.write(&format!("{name}/device/device"), "0xffff\n");
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Cards {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn only_whole_cards_are_cards() {
        assert!(is_card("card0"));
        assert!(is_card("card12"));
        assert!(!is_card("card0-DP-1"));
        assert!(!is_card("renderD128"));
        assert!(!is_card("card"));
        assert!(!is_card("version"));
    }

    #[test]
    fn reads_a_card_the_way_amdgpu_reports_it() {
        let cards = Cards::new("reads");
        cards.card("card0", 37, 16, 4);
        cards.write("card0/device/hwmon/hwmon3/temp1_input", "61500\n");
        // One of its outputs, which is not a card.
        cards.write("card0-DP-1/status", "connected\n");
        let gpu = Reader::over(cards.path()).read().expect("a card");
        assert_eq!(gpu.name, "AMD graphics");
        assert_eq!(gpu.usage, 37.0);
        assert_eq!(gpu.temp, 61.5);
        assert_eq!(
            (gpu.mem.total, gpu.mem.used, gpu.mem.percent),
            (16.0, 4.0, 25.0)
        );
    }

    #[test]
    fn the_card_with_the_most_memory_is_the_one_shown() {
        let cards = Cards::new("most");
        cards.card("card0", 5, 2, 1);
        cards.card("card1", 80, 12, 6);
        let gpu = Reader::over(cards.path()).read().expect("a card");
        assert_eq!((gpu.usage, gpu.mem.total), (80.0, 12.0));
    }

    #[test]
    fn a_card_that_does_not_say_how_busy_it_is_is_no_reading() {
        let cards = Cards::new("silent");
        // What Intel's driver leaves: a card, and nothing to read on it.
        cards.write("card0/device/vendor", "0x8086\n");
        assert_eq!(Reader::over(cards.path()).read(), None);
        // And a machine with no cards at all, or no such folder.
        assert_eq!(Reader::over(&cards.path().join("none")).read(), None);
    }

    #[test]
    fn a_sleeping_card_is_not_woken() {
        let cards = Cards::new("asleep");
        cards.card("card1", 99, 8, 3);
        cards.write("card1/device/power/runtime_status", "suspended\n");
        let gpu = Reader::over(cards.path()).read().expect("a card");
        assert_eq!((gpu.usage, gpu.mem.used, gpu.mem.total), (0.0, 0.0, 8.0));
    }

    #[test]
    fn a_missing_temperature_is_none() {
        let cards = Cards::new("cold");
        cards.card("card0", 1, 8, 1);
        assert_eq!(Reader::over(cards.path()).read().unwrap().temp, 0.0);
    }

    const IDS: &str = "\
# a comment
1002  Advanced Micro Devices, Inc. [AMD/ATI]
\t73bf  Navi 21 [Radeon RX 6800/6800 XT / 6900 XT]
\t\t1002 0e3a  Radeon RX 6900 XT
\t7480  Navi 33
10de  NVIDIA Corporation
\t73bf  Not the one
";

    #[test]
    fn models_are_looked_up_under_their_maker() {
        assert_eq!(
            model_in(IDS, 0x1002, 0x73bf).as_deref(),
            Some("Radeon RX 6800/6800 XT / 6900 XT")
        );
        assert_eq!(model_in(IDS, 0x1002, 0x7480).as_deref(), Some("Navi 33"));
        assert_eq!(
            model_in(IDS, 0x10de, 0x73bf).as_deref(),
            Some("Not the one")
        );
        assert_eq!(model_in(IDS, 0x1002, 0x0001), None);
        assert_eq!(model_in(IDS, 0x8086, 0x73bf), None);
    }

    #[test]
    fn names_fall_back_to_the_maker() {
        assert_eq!(
            name_from(0x1002, Some("Radeon RX 6800")),
            "AMD Radeon RX 6800"
        );
        assert_eq!(name_from(0x1002, None), "AMD graphics");
        assert_eq!(name_from(0x1234, None), "Graphics card");
    }
}
