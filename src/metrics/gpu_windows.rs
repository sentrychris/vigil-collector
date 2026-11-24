//! The graphics card on Windows.
//!
//! A background thread reads, once a second:
//!
//! - how busy each engine of each card is, per process, and how much of
//!   each card's memory is in use, from the "GPU Engine" and "GPU Adapter
//!   Memory" performance counters (Windows 10 1709 and later). These are
//!   kept by Windows' own graphics scheduler, so they cover every make of
//!   card, and they are what Task Manager shows.
//! - the card's temperature, which its driver reports to Windows (WDDM
//!   2.4 and later; integrated graphics mostly don't).
//!
//! What each card is called and how much memory it has is asked of it
//! now and then (``kmt``). The thread keeps a slow counter read from
//! stalling the sampler, which only ever loads the newest reading.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use super::gpu_counters::{self, Adapter};
use super::pdh::Pdh;
use crate::snapshot::Gpu;

const POLL_EVERY: Duration = Duration::from_secs(1);
/// How many polls pass between looks at which cards there are, and, on a
/// PC without the counters, between tries at opening them.
const LOOK_EVERY: u64 = 30;

const ENGINES: &str = "\\GPU Engine(*)\\Utilization Percentage";
const DEDICATED: &str = "\\GPU Adapter Memory(*)\\Dedicated Usage";
const SHARED: &str = "\\GPU Adapter Memory(*)\\Shared Usage";

/// What the thread last read.
#[derive(Default)]
struct Latest {
    gpu: Option<Gpu>,
    /// Percent of a card by pid; ``None`` until the counters have been
    /// read, and on a PC that hasn't got them.
    by_process: Option<HashMap<u32, f64>>,
}

/// The newest reading, kept fresh by a background thread that stops once
/// this is dropped.
pub struct Reader {
    latest: Arc<Mutex<Latest>>,
}

impl Reader {
    pub fn start() -> Self {
        let latest = Arc::new(Mutex::new(Latest::default()));
        let weak = Arc::downgrade(&latest);
        if let Err(e) = std::thread::Builder::new()
            .name("gpu".into())
            .spawn(move || poll(weak))
        {
            tracing::warn!("GPU readings disabled: could not start their thread: {}", e);
        }
        Self { latest }
    }

    pub fn read(&self) -> Option<Gpu> {
        self.latest.lock().ok()?.gpu.clone()
    }

    pub fn by_process(&self) -> Option<HashMap<u32, f64>> {
        self.latest.lock().ok()?.by_process.clone()
    }
}

/// The counters, opened together: without the first there is nothing to
/// show, and the other two are the memory, either of which may be missing.
struct Counters {
    engines: Pdh,
    dedicated: Option<Pdh>,
    shared: Option<Pdh>,
}

impl Counters {
    fn open() -> Option<Counters> {
        Some(Counters {
            engines: Pdh::open(ENGINES)?,
            dedicated: Pdh::open(DEDICATED),
            shared: Pdh::open(SHARED),
        })
    }
}

fn poll(latest: Weak<Mutex<Latest>>) {
    let mut counters: Option<Counters> = None;
    let mut adapters: Vec<Adapter> = Vec::new();
    // What the log last said was being read, so it says so once.
    let mut said: Option<Option<String>> = None;
    let mut tick: u64 = 0;

    loop {
        let Some(out) = latest.upgrade() else {
            return;
        };

        if tick.is_multiple_of(LOOK_EVERY) {
            adapters = kmt::adapters();
            if counters.is_none() {
                counters = Counters::open();
            }
        }
        tick = tick.wrapping_add(1);

        let read = read(counters.as_mut(), &adapters);
        // The first sample of a rate has no value: keep what is shown
        // rather than flash an empty card.
        if let Read::Fresh(gpu, by_process) = read {
            let name = gpu.as_ref().map(|g| g.name.clone());
            if said.as_ref() != Some(&name) {
                match &name {
                    Some(name) => tracing::info!("GPU: reading {}", name),
                    None => tracing::info!("GPU: no graphics card to read"),
                }
                said = Some(name);
            }
            if let Ok(mut latest) = out.lock() {
                *latest = Latest { gpu, by_process };
            }
        }
        drop(out);

        std::thread::sleep(POLL_EVERY);
    }
}

enum Read {
    /// A reading, or that there is no card to read.
    Fresh(Option<Gpu>, Option<HashMap<u32, f64>>),
    /// Nothing new this time.
    NotYet,
}

fn read(counters: Option<&mut Counters>, adapters: &[Adapter]) -> Read {
    let Some(counters) = counters else {
        return Read::Fresh(None, None);
    };
    let Some(engines) = counters.engines.sample() else {
        return Read::NotYet;
    };
    let by_process = Some(gpu_counters::usage_by_process(&engines));
    let Some(adapter) = gpu_counters::primary(adapters) else {
        return Read::Fresh(None, by_process);
    };
    let dedicated = counters.dedicated.as_mut().and_then(Pdh::sample);
    let shared = counters.shared.as_mut().and_then(Pdh::sample);
    // A laptop's sleeping card is left asleep: asking its driver for the
    // temperature could wake it, so that waits until something uses it.
    let awake = !adapter.sleeps || gpu_counters::usage(&engines, adapter.luid) > 0.0;
    let celsius = awake.then(|| kmt::temperature(adapter.luid)).flatten();
    Read::Fresh(
        Some(gpu_counters::reading(
            adapter,
            &engines,
            dedicated.as_deref(),
            shared.as_deref(),
            celsius,
        )),
        by_process,
    )
}

/// What the cards say about themselves, asked through the kernel's
/// graphics interface (the ``D3DKMT`` calls in gdi32), which needs no
/// Direct3D device and no admin rights.
mod kmt {
    use std::ffi::c_void;

    use windows_sys::Wdk::Graphics::Direct3D::{
        D3DKMT_ADAPTER_PERFDATA, D3DKMT_ADAPTERINFO, D3DKMT_ADAPTERREGISTRYINFO,
        D3DKMT_ADAPTERTYPE, D3DKMT_CLOSEADAPTER, D3DKMT_ENUMADAPTERS2, D3DKMT_OPENADAPTERFROMLUID,
        D3DKMT_QUERYADAPTERINFO, D3DKMT_SEGMENTSIZEINFO, D3DKMTCloseAdapter, D3DKMTEnumAdapters2,
        D3DKMTOpenAdapterFromLuid, D3DKMTQueryAdapterInfo, KMTQAITYPE_ADAPTERPERFDATA,
        KMTQAITYPE_ADAPTERREGISTRYINFO, KMTQAITYPE_ADAPTERTYPE, KMTQAITYPE_GETSEGMENTSIZE,
        KMTQUERYADAPTERINFOTYPE,
    };
    use windows_sys::Win32::Foundation::LUID;

    use super::super::gpu_counters::{Adapter, Luid};

    /// Bits of ``D3DKMT_ADAPTERTYPE``.
    const RENDERS: u32 = 1 << 0;
    const SOFTWARE: u32 = 1 << 2;
    const HYBRID_DISCRETE: u32 = 1 << 4;
    const INDIRECT_DISPLAY: u32 = 1 << 6;

    /// Asks an open adapter for one fixed-size answer.
    ///
    /// # Safety
    /// ``T`` must be the structure ``kind`` answers with.
    unsafe fn ask<T>(adapter: u32, kind: KMTQUERYADAPTERINFOTYPE, answer: &mut T) -> bool {
        let mut query = D3DKMT_QUERYADAPTERINFO {
            hAdapter: adapter,
            Type: kind,
            pPrivateDriverData: (answer as *mut T).cast::<c_void>(),
            PrivateDriverDataSize: std::mem::size_of::<T>() as u32,
        };
        // SAFETY: the answer is a live, writable ``T`` of the size given.
        unsafe { D3DKMTQueryAdapterInfo(&mut query) == 0 }
    }

    fn close(adapter: u32) {
        let close = D3DKMT_CLOSEADAPTER { hAdapter: adapter };
        // SAFETY: a handle this module opened, closed once.
        unsafe { D3DKMTCloseAdapter(&close) };
    }

    /// The real graphics cards: not Windows' software renderer, and not
    /// the display-only adapters remote desktops and virtual screens add.
    pub fn adapters() -> Vec<Adapter> {
        let mut list = D3DKMT_ENUMADAPTERS2::default();
        // SAFETY: asked with no buffer, it only counts.
        if unsafe { D3DKMTEnumAdapters2(&mut list) } != 0 || list.NumAdapters == 0 {
            return Vec::new();
        }
        let mut found = vec![D3DKMT_ADAPTERINFO::default(); list.NumAdapters as usize];
        list.pAdapters = found.as_mut_ptr();
        // SAFETY: the buffer holds the ``NumAdapters`` entries it says.
        if unsafe { D3DKMTEnumAdapters2(&mut list) } != 0 {
            return Vec::new();
        }
        found.truncate(list.NumAdapters as usize);

        let mut adapters = Vec::new();
        for info in found {
            // Every adapter listed comes open, and is closed again here.
            let adapter = describe(&info);
            close(info.hAdapter);
            adapters.extend(adapter);
        }
        adapters
    }

    fn describe(info: &D3DKMT_ADAPTERINFO) -> Option<Adapter> {
        let handle = info.hAdapter;
        let mut kind = D3DKMT_ADAPTERTYPE::default();
        // SAFETY: each structure is the one its query answers with, and a
        // union of plain integers is read as the integer.
        let kind = unsafe {
            if !ask(handle, KMTQAITYPE_ADAPTERTYPE, &mut kind) {
                return None;
            }
            kind.Anonymous.Value
        };
        if kind & RENDERS == 0 || kind & (SOFTWARE | INDIRECT_DISPLAY) != 0 {
            return None;
        }
        let mut memory = D3DKMT_SEGMENTSIZEINFO::default();
        let mut registry = D3DKMT_ADAPTERREGISTRYINFO::default();
        // SAFETY: as above.
        let named = unsafe {
            if !ask(handle, KMTQAITYPE_GETSEGMENTSIZE, &mut memory) {
                return None;
            }
            ask(handle, KMTQAITYPE_ADAPTERREGISTRYINFO, &mut registry)
        };
        let name = named
            .then(|| {
                let text = &registry.AdapterString;
                let end = text.iter().position(|c| *c == 0).unwrap_or(text.len());
                super::super::without_marks(&String::from_utf16_lossy(&text[..end]))
            })
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Graphics card".to_string());
        Some(Adapter {
            luid: Luid {
                high: info.AdapterLuid.HighPart as u32,
                low: info.AdapterLuid.LowPart,
            },
            name,
            // Integrated graphics count their token memory as one or the
            // other of these.
            dedicated: memory.DedicatedVideoMemorySize + memory.DedicatedSystemMemorySize,
            shared: memory.SharedSystemMemorySize,
            sleeps: kind & HYBRID_DISCRETE != 0,
        })
    }

    /// The card's temperature in °C as its driver gives it, which is 0
    /// from a driver that doesn't.
    pub fn temperature(luid: Luid) -> Option<f64> {
        let mut open = D3DKMT_OPENADAPTERFROMLUID {
            AdapterLuid: LUID {
                LowPart: luid.low,
                HighPart: luid.high as i32,
            },
            hAdapter: 0,
        };
        // SAFETY: a plain call filling in the handle.
        if unsafe { D3DKMTOpenAdapterFromLuid(&mut open) } != 0 {
            return None;
        }
        let mut data = D3DKMT_ADAPTER_PERFDATA::default();
        // SAFETY: the structure this query answers with.
        let answered = unsafe { ask(open.hAdapter, KMTQAITYPE_ADAPTERPERFDATA, &mut data) };
        close(open.hAdapter);
        // Tenths of a degree.
        answered.then(|| f64::from(data.Temperature) / 10.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{Counters, kmt, read};

    /// Whatever this machine has (a build server has no card, and Wine
    /// has no counters), looking must neither fail nor hang.
    #[test]
    fn looking_for_a_card_is_safe_on_any_machine() {
        let adapters = kmt::adapters();
        for adapter in &adapters {
            assert!(!adapter.name.is_empty());
            let _ = kmt::temperature(adapter.luid);
        }
        let mut counters = Counters::open();
        for _ in 0..2 {
            let _ = read(counters.as_mut(), &adapters);
        }
    }
}
