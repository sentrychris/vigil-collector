//! Reading Windows' own graphics counters, the ones behind Task Manager's
//! GPU figures.
//!
//! Windows keeps them per engine (3D, video decode, compute…) of each
//! card, per process, under names like
//! ``pid_4321_luid_0x00000000_0x0000D3A5_phys_0_eng_0_engtype_3D``, and
//! a card's memory under ``luid_0x00000000_0x0000D3A5_phys_0``. This
//! picks the card to show and adds the counters up into its reading.
//!
//! Everything here is a pure function over what was sampled, so it is
//! unit-tested on every platform. The Windows glue that samples the
//! counters and asks the cards about themselves is in ``gpu_windows.rs``.

use std::collections::HashMap;

use super::gpu::memory;
use crate::snapshot::Gpu;

/// A card with this much video memory of its own is read by it. One with
/// less (integrated graphics, which have a token amount) is read by the
/// system memory it shares instead, which is where its work really goes.
const OWN_MEMORY_FROM: u64 = 512 * 1024 * 1024;

/// Accepted temperature range in °C. Outside it the driver isn't
/// reporting one (most integrated graphics say 0).
const PLAUSIBLE_C: std::ops::RangeInclusive<f64> = 5.0..=125.0;

/// The id Windows gives a card until the next restart.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Luid {
    pub high: u32,
    pub low: u32,
}

/// A graphics card, as it describes itself.
#[derive(Clone, PartialEq, Debug)]
pub struct Adapter {
    pub luid: Luid,
    pub name: String,
    /// Video memory of its own, in bytes.
    pub dedicated: u64,
    /// System memory it may borrow, in bytes.
    pub shared: u64,
    /// The discrete card of a laptop that also has integrated graphics:
    /// Windows powers it down while nothing uses it.
    pub sleeps: bool,
}

impl Adapter {
    fn has_own_memory(&self) -> bool {
        self.dedicated >= OWN_MEMORY_FROM
    }
}

/// The card to show: the one with the most memory of its own, which on a
/// machine with integrated graphics and a card is the card. The first
/// listed wins a tie.
pub fn primary(adapters: &[Adapter]) -> Option<&Adapter> {
    adapters.iter().reduce(|best, next| {
        if next.dedicated > best.dedicated {
            next
        } else {
            best
        }
    })
}

/// What a counter's instance name says it counts.
#[derive(Default, PartialEq, Debug)]
struct Instance {
    pid: Option<u32>,
    luid: Option<Luid>,
    phys: u32,
    engine: Option<u32>,
}

impl Instance {
    fn parse(name: &str) -> Instance {
        let mut instance = Instance::default();
        let mut words = name.split('_');
        while let Some(word) = words.next() {
            match word {
                "pid" => instance.pid = words.next().and_then(|w| w.parse().ok()),
                "luid" => {
                    let high = words.next().and_then(hex);
                    let low = words.next().and_then(hex);
                    if let (Some(high), Some(low)) = (high, low) {
                        instance.luid = Some(Luid { high, low });
                    }
                }
                "phys" => {
                    instance.phys = words.next().and_then(|w| w.parse().ok()).unwrap_or(0);
                }
                "eng" => instance.engine = words.next().and_then(|w| w.parse().ok()),
                _ => {}
            }
        }
        instance
    }
}

fn hex(word: &str) -> Option<u32> {
    let digits = word
        .strip_prefix("0x")
        .or_else(|| word.strip_prefix("0X"))?;
    u32::from_str_radix(digits, 16).ok()
}

/// One sample of a counter: a value for each of its instances.
pub type Sample = [(String, f64)];

/// How busy ``adapter`` is, in percent: each engine's use added up over
/// the processes on it, and the busiest engine's total. A card decoding
/// video flat out is busy even while its 3D engine idles, and this is the
/// figure Task Manager shows.
pub fn usage(engines: &Sample, adapter: Luid) -> f64 {
    let mut totals: HashMap<(u32, u32), f64> = HashMap::new();
    for (name, value) in engines {
        let instance = Instance::parse(name);
        if instance.luid != Some(adapter) || !value.is_finite() {
            continue;
        }
        let Some(engine) = instance.engine else {
            continue;
        };
        *totals.entry((instance.phys, engine)).or_default() += value.max(0.0);
    }
    totals
        .values()
        .fold(0.0_f64, |most, v| most.max(*v))
        .min(100.0)
}

/// How busy each process keeps the graphics cards, in percent, by pid:
/// its busiest engine on any card, as Task Manager's GPU column has it.
/// Processes that use none are left out.
pub fn usage_by_process(engines: &Sample) -> HashMap<u32, f64> {
    let mut busiest: HashMap<u32, f64> = HashMap::new();
    for (name, value) in engines {
        let Some(pid) = Instance::parse(name).pid else {
            continue;
        };
        if !value.is_finite() || *value <= 0.0 {
            continue;
        }
        let most = busiest.entry(pid).or_default();
        *most = most.max(value.min(100.0));
    }
    busiest
}

/// The bytes of memory in use on ``adapter``, from a memory counter.
fn in_use(memory: &Sample, adapter: Luid) -> u64 {
    memory
        .iter()
        .filter(|(name, value)| {
            Instance::parse(name).luid == Some(adapter) && value.is_finite() && *value > 0.0
        })
        .map(|(_, value)| *value as u64)
        .sum()
}

/// ``adapter``'s reading. ``dedicated`` and ``shared`` are the two memory
/// counters, either of which may not have been readable; ``celsius`` is
/// what the driver gave as its temperature, if anything.
pub fn reading(
    adapter: &Adapter,
    engines: &Sample,
    dedicated: Option<&Sample>,
    shared: Option<&Sample>,
    celsius: Option<f64>,
) -> Gpu {
    let (total, counter) = if adapter.has_own_memory() {
        (adapter.dedicated, dedicated)
    } else {
        (adapter.shared, shared)
    };
    let used = counter.map_or(0, |sample| in_use(sample, adapter.luid));
    Gpu {
        name: adapter.name.clone(),
        usage: super::round1(usage(engines, adapter.luid)),
        temp: celsius
            .filter(|c| c.is_finite() && PLAUSIBLE_C.contains(c))
            .map_or(0.0, super::round1),
        mem: memory(total, used),
    }
}

#[cfg(test)]
mod tests {
    use super::{Adapter, Instance, Luid, primary, reading, usage, usage_by_process};

    const CARD: Luid = Luid {
        high: 0,
        low: 0xd3a5,
    };
    const INTEGRATED: Luid = Luid {
        high: 0,
        low: 0xc1f0,
    };
    const GIB: u64 = 1024 * 1024 * 1024;

    fn card() -> Adapter {
        Adapter {
            luid: CARD,
            name: "NVIDIA GeForce RTX 3070".into(),
            dedicated: 8 * GIB,
            shared: 16 * GIB,
            sleeps: false,
        }
    }

    fn integrated() -> Adapter {
        Adapter {
            luid: INTEGRATED,
            name: "Intel(R) UHD Graphics 750".into(),
            dedicated: 128 * 1024 * 1024,
            shared: 16 * GIB,
            sleeps: false,
        }
    }

    fn sample(rows: &[(&str, f64)]) -> Vec<(String, f64)> {
        rows.iter().map(|(n, v)| (n.to_string(), *v)).collect()
    }

    #[test]
    fn instance_names_are_taken_apart() {
        assert_eq!(
            Instance::parse("pid_4321_luid_0x00000000_0x0000D3A5_phys_0_eng_3_engtype_VideoDecode"),
            Instance {
                pid: Some(4321),
                luid: Some(CARD),
                phys: 0,
                engine: Some(3),
            }
        );
        assert_eq!(
            Instance::parse("luid_0x00000000_0x0000C1F0_phys_0"),
            Instance {
                pid: None,
                luid: Some(INTEGRATED),
                phys: 0,
                engine: None,
            }
        );
        // Nothing recognisable is nothing, not a panic.
        assert_eq!(Instance::parse("_Total"), Instance::default());
        assert_eq!(Instance::parse("luid_0xZZ_0x1"), Instance::default());
        assert_eq!(Instance::parse(""), Instance::default());
    }

    #[test]
    fn usage_is_the_busiest_engine_over_all_processes() {
        let engines = sample(&[
            // The 3D engine, shared by two programs.
            (
                "pid_100_luid_0x00000000_0x0000D3A5_phys_0_eng_0_engtype_3D",
                30.0,
            ),
            (
                "pid_200_luid_0x00000000_0x0000D3A5_phys_0_eng_0_engtype_3D",
                12.5,
            ),
            // Video decode, less busy.
            (
                "pid_100_luid_0x00000000_0x0000D3A5_phys_0_eng_3_engtype_VideoDecode",
                20.0,
            ),
            // Another card altogether.
            (
                "pid_300_luid_0x00000000_0x0000C1F0_phys_0_eng_0_engtype_3D",
                90.0,
            ),
        ]);
        assert_eq!(usage(&engines, CARD), 42.5);
        assert_eq!(usage(&engines, INTEGRATED), 90.0);
        assert_eq!(usage(&[], CARD), 0.0);
    }

    #[test]
    fn usage_never_passes_a_hundred() {
        let engines = sample(&[
            (
                "pid_1_luid_0x00000000_0x0000D3A5_phys_0_eng_0_engtype_3D",
                70.0,
            ),
            (
                "pid_2_luid_0x00000000_0x0000D3A5_phys_0_eng_0_engtype_3D",
                45.0,
            ),
            (
                "pid_3_luid_0x00000000_0x0000D3A5_phys_0_eng_0_engtype_3D",
                f64::NAN,
            ),
            (
                "pid_4_luid_0x00000000_0x0000D3A5_phys_0_eng_0_engtype_3D",
                -3.0,
            ),
        ]);
        assert_eq!(usage(&engines, CARD), 100.0);
    }

    #[test]
    fn a_process_is_as_busy_as_its_busiest_engine() {
        let engines = sample(&[
            (
                "pid_100_luid_0x00000000_0x0000D3A5_phys_0_eng_0_engtype_3D",
                30.0,
            ),
            (
                "pid_100_luid_0x00000000_0x0000D3A5_phys_0_eng_3_engtype_VideoDecode",
                55.0,
            ),
            (
                "pid_100_luid_0x00000000_0x0000C1F0_phys_0_eng_0_engtype_3D",
                4.0,
            ),
            (
                "pid_200_luid_0x00000000_0x0000D3A5_phys_0_eng_0_engtype_3D",
                0.0,
            ),
            ("luid_0x00000000_0x0000D3A5_phys_0", 50.0),
        ]);
        let by_process = usage_by_process(&engines);
        assert_eq!(by_process.get(&100), Some(&55.0));
        // Idle processes are left out rather than listed at nothing.
        assert_eq!(by_process.len(), 1);
    }

    #[test]
    fn the_card_with_the_most_memory_of_its_own_is_shown() {
        assert_eq!(primary(&[integrated(), card()]), Some(&card()));
        assert_eq!(primary(&[card(), integrated()]), Some(&card()));
        assert_eq!(primary(&[integrated()]), Some(&integrated()));
        assert_eq!(primary(&[]), None);
    }

    #[test]
    fn a_card_is_read_by_its_own_memory() {
        let engines = sample(&[(
            "pid_1_luid_0x00000000_0x0000D3A5_phys_0_eng_0_engtype_3D",
            37.46,
        )]);
        let dedicated = sample(&[
            ("luid_0x00000000_0x0000D3A5_phys_0", 2.0 * GIB as f64),
            ("luid_0x00000000_0x0000C1F0_phys_0", 1.0 * GIB as f64),
        ]);
        let shared = sample(&[("luid_0x00000000_0x0000D3A5_phys_0", 0.5 * GIB as f64)]);
        let gpu = reading(
            &card(),
            &engines,
            Some(&dedicated),
            Some(&shared),
            Some(54.0),
        );
        assert_eq!(gpu.name, "NVIDIA GeForce RTX 3070");
        assert_eq!(gpu.usage, 37.5);
        assert_eq!(gpu.temp, 54.0);
        assert_eq!((gpu.mem.total, gpu.mem.used, gpu.mem.free), (8.0, 2.0, 6.0));
        assert_eq!(gpu.mem.percent, 25.0);
    }

    #[test]
    fn integrated_graphics_are_read_by_the_memory_they_share() {
        let dedicated = sample(&[("luid_0x00000000_0x0000C1F0_phys_0", 100_000_000.0)]);
        let shared = sample(&[("luid_0x00000000_0x0000C1F0_phys_0", 4.0 * GIB as f64)]);
        let gpu = reading(
            &integrated(),
            &[],
            Some(&dedicated),
            Some(&shared),
            Some(0.0),
        );
        assert_eq!((gpu.mem.total, gpu.mem.used), (16.0, 4.0));
        // No temperature from the driver is none, as for the processor.
        assert_eq!(gpu.temp, 0.0);
        assert_eq!(gpu.usage, 0.0);
    }

    #[test]
    fn memory_that_cannot_be_read_is_none_in_use() {
        let gpu = reading(&card(), &[], None, None, None);
        assert_eq!(
            (gpu.mem.total, gpu.mem.used, gpu.mem.percent),
            (8.0, 0.0, 0.0)
        );
    }
}
