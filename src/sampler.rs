use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use bytes::Bytes;
use sysinfo::{
    CpuRefreshKind, DiskRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System, Users,
};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::metrics;
use crate::metrics::cpu::Sensors;
use crate::metrics::disk::DiskIoPrev;
use crate::metrics::net::NetIoPrev;
use crate::snapshot::{Process, ProcessCpu, Snapshot};

// Slow-tick cadences for the heavy refresh phases. Profiling on a typical
// host shows ``processes_refresh`` is ~63% of a tick (~7 ms walking
// /proc/*/{stat,status,statm}) and ``disks.refresh()`` is ~13% (per-mount
// statvfs we then redo ourselves for psutil parity). Top-N-by-memory
// rankings are stable second-to-second, and mounts change rarely, so
// rate-limiting these is essentially free of UX impact while cutting
// idle CPU by ~70%.
//
// PROCESS_SCAN_EVERY = 5  → top-N refreshed every 5 sampler ticks.
// DISK_LIST_REFRESH_EVERY = 60 → re-scan mount table once a minute.
const PROCESS_SCAN_EVERY: u64 = 5;
const DISK_LIST_REFRESH_EVERY: u64 = 60;

struct SamplerState {
    sys: System,
    disks: Disks,
    networks: Networks,
    sensors: Sensors,
    users: Users,
    cores: u32,
    disk_io_prev: Option<DiskIoPrev>,
    net_io_prev: Option<NetIoPrev>,
    tick: u64,
    cached_processes: Vec<Process>,
    cached_processes_metric: String,
    cached_processes_by_cpu: Vec<ProcessCpu>,
}

impl SamplerState {
    fn new() -> anyhow::Result<Self> {
        let sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything())
                .with_processes(metrics::process::refresh_kind()),
        )?;
        // Disks and networks are listed (not refreshed) here; the first
        // ``refresh`` below does the real read.
        let disks = Disks::new()?;
        let networks = Networks::new()?;
        let users = Users::new()?;
        let sensors = Sensors::new();
        let cores = sys.cpus().len() as u32;
        Ok(Self {
            sys,
            disks,
            networks,
            sensors,
            users,
            cores,
            disk_io_prev: None,
            net_io_prev: None,
            tick: 0,
            cached_processes: Vec::new(),
            cached_processes_metric: String::from("rss"),
            cached_processes_by_cpu: Vec::new(),
        })
    }

    fn refresh(&mut self) -> Snapshot {
        // Cheap, every-tick: CPU usage and memory are cumulative-counter
        // deltas — they need a 1 s sampling window to read meaningfully.
        // Networks the same (rx/tx bytes/sec). disk_io is also a delta.
        //
        // ``refresh_cpu_usage`` reads only ``/proc/stat`` (~50 µs); the
        // full ``refresh_cpu_all`` would also touch per-CPU frequency
        // files for every core (~2 ms on a 16-core box). Frequency is a
        // slow-moving display value, so we refresh it on the same slow
        // tick as processes below.
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        // ``true`` drops interfaces that went away; new ones (VPN up, USB
        // NIC plugged in) are picked up by the same call.
        self.networks.refresh(true);
        self.sensors.refresh();

        // Slow, every-Nth-tick: process scan dominates the per-tick CPU
        // budget. Top-N by memory is stable across short windows, so
        // running this every PROCESS_SCAN_EVERY ticks (default 5 s) gives
        // a ~5x reduction in average tick cost without visible UX impact.
        // CPU frequency is rolled into the same cadence — it's a display
        // value that doesn't need 1 Hz freshness and reading it
        // per-core-per-tick was ~2 ms by itself.
        if self.tick.is_multiple_of(PROCESS_SCAN_EVERY) {
            self.sys.refresh_cpu_frequency();
            self.users.refresh();
            self.sys.refresh_processes_specifics(
                sysinfo::ProcessesToUpdate::All,
                true,
                metrics::process::refresh_kind(),
            );
            self.cached_processes = metrics::process::collect_top(&self.sys, &self.users);
            self.cached_processes_metric =
                metrics::process::process_metric_label(&self.sys).to_string();
            self.cached_processes_by_cpu =
                metrics::process::collect_top_cpu(&self.sys, &self.users);
        }

        // Slow, every-Mth-tick: re-scan the mount table to catch new
        // mounts/unmounts. Per-disk usage is read per-tick by
        // ``metrics::disk`` itself (statvfs / GetDiskFreeSpaceExW), so
        // sysinfo's stored usage is only a fallback and its storage read
        // rides along with the re-scan. Kind (SSD/HDD) and per-volume I/O
        // aren't used and are skipped.
        if self.tick.is_multiple_of(DISK_LIST_REFRESH_EVERY) {
            self.disks
                .refresh_specifics(true, DiskRefreshKind::nothing().with_storage());
        }

        self.tick = self.tick.wrapping_add(1);

        let cpu = metrics::cpu::collect(&self.sys, &self.sensors, self.cores);
        let mem = metrics::mem::collect(&self.sys);
        let disk_root = metrics::disk::collect_root(&self.disks);
        let all_disks = metrics::disk::collect_all(&self.disks);
        let disk_io = metrics::disk::collect_io(&mut self.disk_io_prev);
        let hidden = metrics::net::hidden_interfaces();
        let net = metrics::net::collect_throughput(&self.networks, &hidden, &mut self.net_io_prev);
        let plat = metrics::platform::collect();
        let user = metrics::platform::user();
        let interfaces = metrics::net::interface_names(&self.networks, &hidden);
        let network_stats = metrics::net::statistics(&self.networks, &hidden);

        Snapshot {
            cpu,
            mem,
            disk: disk_root,
            disks: all_disks,
            disk_io,
            network: net,
            user,
            uptime: plat.uptime.clone(),
            platform: plat,
            processes: self.cached_processes.clone(),
            processes_metric: self.cached_processes_metric.clone(),
            processes_by_cpu: self.cached_processes_by_cpu.clone(),
            interfaces,
            network_stats,
        }
    }
}

/// Triple-cached snapshot bytes — one per wire-format projection.
///
/// Stored as ``Bytes`` so handing a buffer to an HTTP body or a WebSocket
/// frame is a refcount bump rather than a copy of the JSON.
pub struct CachedJson {
    pub http_system: ArcSwap<Bytes>,
    pub ws_system: ArcSwap<Bytes>,
    pub network: ArcSwap<Bytes>,
}

impl CachedJson {
    fn new() -> Self {
        Self {
            http_system: ArcSwap::from_pointee(Bytes::from_static(b"{}")),
            ws_system: ArcSwap::from_pointee(Bytes::from_static(b"{}")),
            network: ArcSwap::from_pointee(Bytes::from_static(
                br#"{"interfaces":[],"statistics":{}}"#,
            )),
        }
    }

    fn publish(&self, snap: &Snapshot) {
        let store = |slot: &ArcSwap<Bytes>, json: serde_json::Result<Vec<u8>>| match json {
            Ok(b) => slot.store(Arc::new(Bytes::from(b))),
            Err(e) => tracing::warn!("failed to serialize snapshot: {e}"),
        };
        store(&self.http_system, serde_json::to_vec(&snap.http_view()));
        store(&self.ws_system, serde_json::to_vec(&snap.ws_view()));
        store(&self.network, serde_json::to_vec(&snap.network_view()));
    }
}

pub struct Sampler {
    interval: Duration,
    json: Arc<CachedJson>,
    stop: Arc<Notify>,
    handle: Option<JoinHandle<()>>,
}

impl Sampler {
    pub fn new(interval_secs: f64) -> Self {
        let interval = Duration::from_secs_f64(interval_secs.max(0.1));
        Self {
            interval,
            json: Arc::new(CachedJson::new()),
            stop: Arc::new(Notify::new()),
            handle: None,
        }
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    pub fn json(&self) -> Arc<CachedJson> {
        self.json.clone()
    }

    /// Start the sampling loop. Fails only if sysinfo can't initialise at
    /// all on this platform, which leaves nothing to serve.
    pub fn start(&mut self) -> anyhow::Result<()> {
        if self.handle.is_some() {
            return Ok(());
        }

        let interval = self.interval;
        let json = self.json.clone();
        let stop = self.stop.clone();

        // Seed once synchronously so the first connecting client doesn't
        // see zeros for up to ``interval`` seconds.
        let mut state = Box::new(SamplerState::new()?);
        json.publish(&state.refresh());

        let handle = tokio::spawn(async move {
            // ``interval`` rather than ``sleep`` keeps a steady cadence:
            // the tick's own refresh time no longer adds to the period.
            // ``Delay`` means a stalled tick (machine suspended, heavy load)
            // resumes the cadence instead of firing a burst to catch up.
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticker.tick().await; // the first tick completes immediately

            let mut state = Some(state);
            loop {
                tokio::select! {
                    _ = stop.notified() => break,
                    _ = ticker.tick() => {}
                }
                let owned = match state.take() {
                    Some(s) => s,
                    // The previous rebuild failed; try again this tick.
                    None => match SamplerState::new() {
                        Ok(s) => Box::new(s),
                        Err(e) => {
                            tracing::warn!("sampler state rebuild failed: {e}");
                            continue;
                        }
                    },
                };
                // Heavy refresh runs on the blocking pool — sysinfo touches
                // /proc (or the Win32 process/perf APIs) and we don't want
                // the runtime thread stalled for 5-10ms each tick.
                let join = tokio::task::spawn_blocking(move || {
                    let mut s = owned;
                    let snap = s.refresh();
                    (snap, s)
                })
                .await;
                match join {
                    Ok((snapshot, returned)) => {
                        state = Some(returned);
                        json.publish(&snapshot);
                    }
                    Err(e) => {
                        // The sampler must never die on a transient error;
                        // the state is rebuilt on the next tick. Previous
                        // snapshot stays in place until then.
                        tracing::warn!("sampler tick failed, rebuilding state: {e}");
                    }
                }
            }
        });

        self.handle = Some(handle);
        Ok(())
    }

    pub fn stop(&mut self) {
        self.stop.notify_waiters();
        if let Some(h) = self.handle.take() {
            h.abort();
        }
    }
}
