use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use sysinfo::{
    Components, CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System, Users,
};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::metrics;
use crate::metrics::disk::DiskIoPrev;
use crate::metrics::net::NetIoPrev;
use crate::snapshot::Snapshot;

struct SamplerState {
    sys: System,
    disks: Disks,
    networks: Networks,
    components: Components,
    users: Users,
    cores: u32,
    disk_io_prev: Option<DiskIoPrev>,
    net_io_prev: Option<NetIoPrev>,
}

impl SamplerState {
    fn new() -> Self {
        let sys = System::new_with_specifics(
            RefreshKind::new()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything())
                .with_processes(metrics::process::refresh_kind()),
        );
        let disks = Disks::new_with_refreshed_list();
        let networks = Networks::new_with_refreshed_list();
        let components = Components::new_with_refreshed_list();
        let users = Users::new_with_refreshed_list();
        let cores = sys.cpus().len() as u32;
        Self {
            sys,
            disks,
            networks,
            components,
            users,
            cores,
            disk_io_prev: None,
            net_io_prev: None,
        }
    }

    fn refresh(&mut self) -> Snapshot {
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.sys.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All,
            true,
            metrics::process::refresh_kind(),
        );
        self.disks.refresh();
        self.networks.refresh();
        self.components.refresh();
        self.users.refresh_list();

        let cpu = metrics::cpu::collect(&self.sys, &self.components, self.cores);
        let mem = metrics::mem::collect(&self.sys);
        let disk_root = metrics::disk::collect_root(&self.disks);
        let all_disks = metrics::disk::collect_all(&self.disks);
        let disk_io = metrics::disk::collect_io(&mut self.disk_io_prev);
        let net = metrics::net::collect_throughput(&self.networks, &mut self.net_io_prev);
        let processes = metrics::process::collect_top(&self.sys, &self.users);
        let processes_metric = metrics::process::process_metric_label(&self.sys).to_string();
        let plat = metrics::platform::collect();
        let user = metrics::platform::user();
        let interfaces = metrics::net::interface_names(&self.networks);
        let network_stats = metrics::net::statistics(&self.networks);

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
            processes,
            processes_metric,
            interfaces,
            network_stats,
        }
    }
}

/// Triple-cached snapshot bytes — one per wire-format projection.
pub struct CachedJson {
    pub http_system: Arc<ArcSwap<Vec<u8>>>,
    pub ws_system: Arc<ArcSwap<Vec<u8>>>,
    pub network: Arc<ArcSwap<Vec<u8>>>,
}

impl CachedJson {
    fn new() -> Self {
        Self {
            http_system: Arc::new(ArcSwap::from_pointee(b"{}".to_vec())),
            ws_system: Arc::new(ArcSwap::from_pointee(b"{}".to_vec())),
            network: Arc::new(ArcSwap::from_pointee(
                br#"{"interfaces":[],"statistics":{}}"#.to_vec(),
            )),
        }
    }

    fn publish(&self, snap: &Snapshot) {
        if let Ok(b) = serde_json::to_vec(&snap.http_view()) {
            self.http_system.store(Arc::new(b));
        }
        if let Ok(b) = serde_json::to_vec(&snap.ws_view()) {
            self.ws_system.store(Arc::new(b));
        }
        if let Ok(b) = serde_json::to_vec(&snap.network_view()) {
            self.network.store(Arc::new(b));
        }
    }
}

pub struct Sampler {
    interval: Duration,
    snapshot: Arc<ArcSwap<Snapshot>>,
    json: Arc<CachedJson>,
    stop: Arc<Notify>,
    handle: Option<JoinHandle<()>>,
}

impl Sampler {
    pub fn new(interval_secs: f64) -> Self {
        let interval = Duration::from_secs_f64(interval_secs.max(0.1));
        Self {
            interval,
            snapshot: Arc::new(ArcSwap::from_pointee(Snapshot::default())),
            json: Arc::new(CachedJson::new()),
            stop: Arc::new(Notify::new()),
            handle: None,
        }
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    pub fn snapshot_handle_arc(&self) -> Arc<ArcSwap<Snapshot>> {
        self.snapshot.clone()
    }

    pub fn json(&self) -> Arc<CachedJson> {
        self.json.clone()
    }

    pub fn start(&mut self) {
        if self.handle.is_some() {
            return;
        }
        let interval = self.interval;
        let snap_slot = self.snapshot.clone();
        let json = self.json.clone();
        let stop = self.stop.clone();

        // Seed once synchronously so the first connecting client doesn't
        // see zeros for up to ``interval`` seconds.
        let mut state = Box::new(SamplerState::new());
        let initial = state.refresh();
        json.publish(&initial);
        snap_slot.store(Arc::new(initial));

        let handle = tokio::spawn(async move {
            let mut state = Some(state);
            loop {
                tokio::select! {
                    _ = stop.notified() => break,
                    _ = tokio::time::sleep(interval) => {}
                }
                // Heavy refresh runs on the blocking pool — sysinfo touches
                // /proc and other syscalls and we don't want a runtime
                // worker stalled for 5-10ms each tick.
                let owned = state.take().expect("sampler state always restored");
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
                        snap_slot.store(Arc::new(snapshot));
                    }
                    Err(e) => {
                        // The sampler must never die on a transient error;
                        // rebuild fresh state and keep ticking. Previous
                        // snapshot stays in place until the next tick.
                        tracing::warn!("sampler tick failed, rebuilding state: {}", e);
                        state = Some(Box::new(SamplerState::new()));
                    }
                }
            }
        });

        self.handle = Some(handle);
    }

    pub fn stop(&mut self) {
        self.stop.notify_waiters();
        if let Some(h) = self.handle.take() {
            h.abort();
        }
    }
}
