# The sampler

The sampler is the single source of metric truth. Every other component is
either feeding the sampler ([config.rs](../src/config.rs), startup) or
consuming what it published ([server/](../src/server/), [hub/](../src/hub/)).

For the structural map, see [ARCHITECTURE.md](./ARCHITECTURE.md). For
end-to-end lifecycle, see [how-it-flows.md](./how-it-flows.md). For runtime
primitives, see [runtime-semantics.md](./runtime-semantics.md).

## Outer shape

```
Sampler                       (start/stop wrapper, lives in main.rs)
├── interval: Duration
├── snapshot: Arc<ArcSwap<Snapshot>>     (full Snapshot, for hub)
├── json: Arc<CachedJson>                (three pre-serialized Vec<u8>)
├── stop: Arc<Notify>
└── handle: Option<JoinHandle<()>>       (the tick task)
```

[sampler.rs:177-184](../src/sampler.rs#L177-L184).

`Sampler` itself is small — it owns the spawn handle and the channels for
control and output. The actual sysinfo state lives behind the spawn boundary
in `SamplerState`.

## `SamplerState` — what gets refreshed

```rust
struct SamplerState {
    sys: System,                              // CPU, memory, processes
    disks: Disks,                             // mount table + per-mount info
    networks: Networks,                       // per-iface counters
    components: Components,                   // temperature sensors
    users: Users,                             // uid -> name lookup
    cores: u32,
    disk_io_prev: Option<DiskIoPrev>,         // for /proc/diskstats deltas
    net_io_prev: Option<NetIoPrev>,           // for sysinfo network deltas
    tick: u64,
    cached_processes: Vec<Process>,           // last top-N (slow tier)
    cached_processes_metric: String,          // "pss" or "rss"
}
```

[sampler.rs:29-41](../src/sampler.rs#L29-L41).

Construction in
[`SamplerState::new`](../src/sampler.rs#L44-L69) is non-trivial: every
sysinfo handle has to be told *what* to refresh. The defaults pull in too
much (per-process cmdline, every CPU's frequency on every refresh,
per-mount statvfs). The constructor tunes each handle to the minimum we
actually serialize — see
[`metrics::process::refresh_kind`](../src/metrics/process.rs#L220-L227).

## Tiered refresh cadences

Profiling on a typical Linux host showed `processes_refresh` at ~63% of a
tick and `disks.refresh()` at ~13%. These rate limits are the difference
between ~11 ms/tick and ~3 ms/tick at idle:

| Cadence | What runs | Why |
|---|---|---|
| Every tick (1 s) | CPU usage, memory, networks, components, disk I/O delta, root statvfs | Counter deltas need a fixed window; statvfs is ~30 µs |
| Every 5 ticks (`PROCESS_SCAN_EVERY`) | Full process scan, CPU frequency, user list | Top-N rankings stable second-to-second; frequency is a display value |
| Every 60 ticks (`DISK_LIST_REFRESH_EVERY`) | Mount table re-scan | Mounts change rarely; per-mount usage comes from statvfs separately |

[`SamplerState::refresh`](../src/sampler.rs#L71-L143) is where the cadence
decisions live. The cadence constants are at
[sampler.rs:26-27](../src/sampler.rs#L26-L27).

A subtle point: `Disks::refresh()` would do per-mount `statvfs` itself, but
we ignore the resulting numbers and call `statvfs` again in
[`usage_for_mount`](../src/metrics/disk.rs#L61-L84) for `df`-compatible
accounting. So `Disks::refresh()` is pure overhead for us — we use
`Disks::refresh_list()` (mount discovery only) and skip the usage refresh
entirely.

## Tick loop

```rust
let handle = tokio::spawn(async move {
    let mut state = Some(state);
    loop {
        tokio::select! {
            _ = stop.notified() => break,
            _ = tokio::time::sleep(interval) => {}
        }
        let owned = state.take().expect("sampler state always restored");
        let join = tokio::task::spawn_blocking(move || {
            let mut s = owned;
            let snap = s.refresh();
            (snap, s)
        }).await;
        match join {
            Ok((snapshot, returned)) => {
                state = Some(returned);
                json.publish(&snapshot);
                snap_slot.store(Arc::new(snapshot));
            }
            Err(e) => {
                tracing::warn!("sampler tick failed, rebuilding state: {}", e);
                state = Some(Box::new(SamplerState::new()));
            }
        }
    }
});
```

[sampler.rs:226-258](../src/sampler.rs#L226-L258).

Three things make this work:

- **The state shuttle**: see
  [runtime-semantics.md / Why state is boxed and shuttled](./runtime-semantics.md#why-state-is-boxed-and-shuttled).
- **Sleep before work, not after**: the first tick fires at `T+interval`,
  not `T+0`. The synchronous seed call in
  [`Sampler::start`](../src/sampler.rs#L221-L224) is what makes the first
  HTTP/WS read non-empty.
- **Error path rebuilds state**: the sampler must never go silent. Even if
  sysinfo somehow corrupts itself or a tick panics, we recreate state and
  keep ticking. The previous snapshot stays visible to readers via
  `ArcSwap` until the next successful tick.

## Snapshot publishing

[`CachedJson::publish`](../src/sampler.rs#L164-L174) does three things:

```rust
fn publish(&self, snap: &Snapshot) {
    if let Ok(b) = serde_json::to_vec(&snap.http_view()) { self.http_system.store(Arc::new(b)); }
    if let Ok(b) = serde_json::to_vec(&snap.ws_view())   { self.ws_system.store(Arc::new(b)); }
    if let Ok(b) = serde_json::to_vec(&snap.network_view()) { self.network.store(Arc::new(b)); }
}
```

The three views ([snapshot.rs:113-184](../src/snapshot.rs#L113-L184)) are
borrowed projections over the same `Snapshot`, so this serializes the same
underlying data three times into three different shapes without cloning the
inner vectors.

`if let Ok(...)` swallows serde errors silently. In practice serde never
fails on this data (no foreign types, no maps with non-string keys), but
defensive: a serialization error doesn't tear down the loop.

## Per-metric collectors

Each lives in [metrics/](../src/metrics/) and is a stateless function over
the already-refreshed sysinfo handles. Stateless = the function is called
each tick, takes references to the handles plus any per-tick previous state,
and returns a fresh value.

### CPU ([metrics/cpu.rs](../src/metrics/cpu.rs))

- `usage`: rounded `global_cpu_usage()` from sysinfo.
- `freq`: arithmetic mean of per-core frequencies (filtering zeros).
- `cores`: cached at startup.
- `temp`: priority-list lookup over labelled components ("package", "tctl",
  "tdie", "cpu") with a fallback to `/sys/class/thermal/`. Windows always
  reports `0` (no shipped binary helper).
- `load`: `[1m, 5m, 15m]` via `libc::getloadavg(3)` for full kernel
  precision (sysinfo's `/proc/loadavg` parser truncates to 2 decimals).

### Memory ([metrics/mem.rs](../src/metrics/mem.rs))

Uses `sys.available_memory()` as the basis for `free`/`used`, so
`used + free == total`. The raw `used` and `free` fields would exclude
reclaimable cache and break that identity, psutil's behavior is what we
match.

### Disk ([metrics/disk.rs](../src/metrics/disk.rs))

Three independent collectors:

- [`collect_root`](../src/metrics/disk.rs#L86-L104) — primary partition
  (root on Unix, `C:\` on Windows).
- [`collect_all`](../src/metrics/disk.rs#L106-L147) — every non-pseudo
  partition. On Linux we read `/proc/filesystems` and skip filesystems
  marked `nodev` (proc, sysfs, tmpfs, overlay, fuse.snapfuse, ...).
- [`collect_io`](../src/metrics/disk.rs#L248-L275) — bytes/sec and IOPS
  from `/proc/diskstats`. We sum only **whole-disk** rows (filtered via
  `/sys/block`) to avoid double-counting bytes attributed to both a disk
  and its partitions. Sectors are 512-byte regardless of physical sector
  size — see kernel docs.

### Network ([metrics/net.rs](../src/metrics/net.rs))

- [`collect_throughput`](../src/metrics/net.rs#L18-L41) — sum total bytes
  across non-loopback interfaces, take the delta against the last tick's
  values, divide by elapsed seconds.
- [`statistics`](../src/metrics/net.rs#L47-L77) — per-iface stats. sysinfo
  doesn't expose dropped-packet counters, so on Linux we backfill from
  `/proc/net/dev` ([read_proc_net_drops](../src/metrics/net.rs#L80-L101)).
- [`interface_names`](../src/metrics/net.rs#L43-L45) — name list for the
  `/network` payload.

### Process ([metrics/process.rs](../src/metrics/process.rs))

The most subtle collector. Two algorithms depending on PSS availability:

**RSS path** (when `/proc/<pid>/smaps_rollup` is unreadable, which includes
all non-Linux): aggregate by process name using `Process::memory()` (RSS),
sort descending, take top 10.

**PSS path** (when smaps_rollup is readable, typically same-uid or root):
PSS reads are syscalls — hundreds per slow tick on a busy host would be
expensive. Instead we use a **streaming top-N** algorithm
([process.rs:159-183](../src/metrics/process.rs#L159-L183)):

1. Walk RSS-aggregated entries in descending order.
2. For each, compute PSS by summing per-pid `smaps_rollup` reads.
3. Maintain the running top-N by PSS, sorted descending.
4. Stop when the next candidate's RSS no longer exceeds the smallest PSS in
   the running top-N. Since PSS ≤ RSS and entries are RSS-descending, no
   later entry can break in.

This typically reads ~10-20 smaps_rollup files instead of all of them.

PSS detection ([`detect_pss`](../src/metrics/process.rs#L41-L79)) runs once
and is cached in a `OnceLock`. We skip our own PID so we don't trivially
self-detect, and we sort PIDs ascending to match psutil's `/proc` listdir
order — without that, sysinfo's hash-ordered iteration could land on a
same-uid process first and mis-detect support.

Threads are filtered via `proc_.thread_kind().is_some()`
([process.rs:129-131](../src/metrics/process.rs#L129-L131)). Without this,
multi-threaded processes would double-count RSS and the top-N would be
polluted with thread names like `tokio-runtime-w`.

The `processes_metric` string in the snapshot tells the dashboard which
algorithm produced the numbers — important because PSS values are typically
30-50% lower than RSS for shared-library-heavy processes.

### Platform ([metrics/platform.rs](../src/metrics/platform.rs))

- `user`: `whoami::username()`, cached in a `OnceLock`.
- `distro`: `/etc/os-release`'s `PRETTY_NAME` on Linux (falling back to
  `NAME`, then `System::name()`); `System::name()` on macOS/Windows. Cached.
- `kernel`: `System::kernel_version()`. Read fresh each tick (it's a
  pointer dereference; not worth caching).
- `uptime`: human-readable string built by
  [`format_uptime`](../src/metrics/platform.rs#L50-L83). Only non-zero
  components appear, and singulars/plurals are correct.

## Snapshot wire formats

[`Snapshot`](../src/snapshot.rs#L6-L20) is the rich internal model. The
three on-the-wire shapes are *projections* — borrowed views constructed via
inherent methods:

- [`http_view()`](../src/snapshot.rs#L150-L163) → `/system` payload.
- [`ws_view()`](../src/snapshot.rs#L165-L176) → WebSocket frame. Differs
  from the HTTP view: no `disks`, no `user`, no `platform`; `uptime` is
  top-level instead of nested.
- [`network_view()`](../src/snapshot.rs#L178-L183) → `/network` payload.

These projections exist because the original Python collector exposed
different shapes per endpoint and we preserve byte-compatibility. If you
add a new field, decide which views need it and update the matching struct.

`Cpu::temp` has a custom serializer
([snapshot.rs:30-43](../src/snapshot.rs#L30-L43)) that emits integer `0`
when the value is exactly `0.0`, matching psutil's behavior on platforms
without thermal sensors. Mixed JSON types is intentional.

## Performance budget

The README's [Idle CPU usage](../README.md#idle-cpu-usage) table is the
authoritative reference. In one line: **about 0.23% of one core, regardless
of client count**, because the per-tick work is fixed and the per-client
cost is a single atomic load.

If you change the sampler, run the binary alone for a minute and confirm
the figure hasn't drifted. The dominant terms are process scanning (slow
tier) and the per-disk statvfs walk (fast tier); regressions usually come
from making something that should be slow-tier into fast-tier.
