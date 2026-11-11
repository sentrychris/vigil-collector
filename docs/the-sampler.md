# The sampler

The sampler is the single source of metric truth. Every other component is
either feeding the sampler ([config.rs](../src/config.rs), startup) or
consuming what it published ([server/](../src/server/)).

For the structural map, see [ARCHITECTURE.md](./ARCHITECTURE.md). For
end-to-end lifecycle, see [how-it-flows.md](./how-it-flows.md). For runtime
primitives, see [runtime-semantics.md](./runtime-semantics.md).

## Outer shape

```
Sampler                       (start/stop wrapper, lives in main.rs)
├── interval: Duration
├── json: Arc<CachedJson>                (three pre-serialized Bytes)
├── stop: Arc<Notify>
└── handle: Option<JoinHandle<()>>       (the tick task)
```

[sampler.rs](../src/sampler.rs).

`Sampler` itself is small — it owns the spawn handle and the channels for
control and output. The actual sysinfo state lives behind the spawn boundary
in `SamplerState`.

## `SamplerState` — what gets refreshed

```rust
struct SamplerState {
    sys: System,                              // CPU, memory, processes
    disks: Disks,                             // mount table + per-mount info
    networks: Networks,                       // per-iface counters
    sensors: Sensors,                         // temperature (reader thread on Windows)
    users: Users,                             // uid/SID -> name lookup
    cores: u32,
    disk_io_prev: Option<DiskIoPrev>,         // for disk I/O counter deltas
    net_io_prev: Option<NetIoPrev>,           // for sysinfo network deltas
    tick: u64,
    cached_processes: Vec<Process>,           // last top-N by memory (slow tier)
    cached_processes_metric: String,          // "pss" or "rss"
    cached_processes_by_cpu: Vec<ProcessCpu>, // last top-N by CPU (slow tier)
}
```

[sampler.rs](../src/sampler.rs).

Construction in
[`SamplerState::new`](../src/sampler.rs) returns a `Result` (sysinfo
0.40's constructors are fallible) and is non-trivial: every
sysinfo handle has to be told *what* to refresh. The defaults pull in too
much (per-process cmdline, every CPU's frequency on every refresh,
per-mount statvfs). The constructor tunes each handle to the minimum we
actually serialize — see
[`metrics::process::refresh_kind`](../src/metrics/process.rs).

## Tiered refresh cadences

Profiling on a typical Linux host showed `processes_refresh` at ~63% of a
tick and `disks.refresh()` at ~13%. These rate limits are the difference
between ~11 ms/tick and ~3 ms/tick at idle:

| Cadence | What runs | Why |
|---|---|---|
| Every tick (1 s) | CPU usage, memory, networks (incl. new/removed interfaces), temperature sensors, disk I/O delta, per-mount usage | Counter deltas need a fixed window; statvfs / `GetDiskFreeSpaceExW` is ~30 µs |
| Every 5 ticks (`PROCESS_SCAN_EVERY`) | Full process scan, CPU frequency, user list | Top-N rankings stable second-to-second; frequency is a display value |
| Every 60 ticks (`DISK_LIST_REFRESH_EVERY`) | Mount / drive list re-scan | Mounts change rarely; per-mount usage is read separately every tick |

[`SamplerState::refresh`](../src/sampler.rs) is where the cadence
decisions live. The cadence constants are at
[sampler.rs](../src/sampler.rs).

A subtle point: sysinfo's per-disk usage is only refreshed when the list is
re-scanned (once a minute), and on Unix it doesn't do `df`'s
reserved-block accounting. So [`metrics::disk`](../src/metrics/disk.rs)
reads usage itself every tick (`statvfs` on Unix, `GetDiskFreeSpaceExW` on
Windows) and uses sysinfo's stored values only as a fallback. The re-scan
asks sysinfo for storage only (`DiskRefreshKind::nothing().with_storage()`),
skipping SSD/HDD detection and per-volume I/O, which we don't use.

## Tick loop

```rust
let handle = tokio::spawn(async move {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker.tick().await; // first tick is immediate; the seed covered it

    let mut state = Some(state);
    loop {
        tokio::select! {
            _ = stop.notified() => break,
            _ = ticker.tick() => {}
        }
        let owned = match state.take() {
            Some(s) => s,
            None => match SamplerState::new() { Ok(s) => Box::new(s), Err(_) => continue },
        };
        let join = tokio::task::spawn_blocking(move || {
            let mut s = owned;
            let snap = s.refresh();
            (snap, s)
        }).await;
        match join {
            Ok((snapshot, returned)) => {
                state = Some(returned);
                json.publish(&snapshot);
            }
            Err(e) => tracing::warn!("sampler tick failed, rebuilding state: {e}"),
        }
    }
});
```

[sampler.rs](../src/sampler.rs).

Three things make this work:

- **The state shuttle**: see
  [runtime-semantics.md / Why state is boxed and shuttled](./runtime-semantics.md#why-state-is-boxed-and-shuttled).
- **Steady cadence**: an `Interval` measures from tick to tick, so the
  refresh's own duration doesn't stretch the period (the old
  sleep-after-work loop drifted by the refresh time every tick). `Delay`
  means a stalled tick (suspend/resume, heavy load) resumes the cadence
  rather than firing a burst. The first real tick fires at `T+interval`. The synchronous seed call in
  [`Sampler::start`](../src/sampler.rs) is what makes the first
  HTTP/WS read non-empty.
- **Error path rebuilds state**: the sampler must never go silent. Even if
  sysinfo somehow corrupts itself or a tick panics, we recreate state on
  the next tick and keep ticking. The previous snapshot stays visible to readers via
  `ArcSwap` until the next successful tick.

## Snapshot publishing

[`CachedJson::publish`](../src/sampler.rs) does three things:

```rust
fn publish(&self, snap: &Snapshot) {
    let store = |slot: &ArcSwap<Bytes>, json| match json {
        Ok(b) => slot.store(Arc::new(Bytes::from(b))),
        Err(e) => tracing::warn!("failed to serialize snapshot: {e}"),
    };
    store(&self.http_system, serde_json::to_vec(&snap.http_view()));
    store(&self.ws_system, serde_json::to_vec(&snap.ws_view()));
    store(&self.network, serde_json::to_vec(&snap.network_view()));
}
```

The three views ([snapshot.rs](../src/snapshot.rs)) are
borrowed projections over the same `Snapshot`, so this serializes the same
underlying data three times into three different shapes without cloning the
inner vectors.

A serde error is logged and the previous buffer kept. In practice serde
never fails on this data (no foreign types, no maps with non-string keys),
but defensive: a serialization error doesn't tear down the loop.

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
  "tdie", "cpu") with a fallback to `/sys/class/thermal/` on Linux.
  Windows has no driverless API for the CPU sensor, so sysinfo's
  `component` feature is Unix-only and `metrics/temp_windows.rs` runs a
  1 Hz thread instead. It reads HWiNFO's and MSI Afterburner's shared
  memory, LibreHardwareMonitor's WMI namespace, then the "Thermal Zone
  Information" PDH counters, and publishes the first reading through an
  atomic the sampler loads. A thermal zone only counts once its value has
  moved, because many desktops report a fixed placeholder. With no source
  the value is `0`, as on psutil.
- `load`: `[1m, 5m, 15m]` via `libc::getloadavg(3)` for full kernel
  precision (sysinfo's `/proc/loadavg` parser truncates to 2 decimals).
  `null` on Windows, which has no load average.

### Memory ([metrics/mem.rs](../src/metrics/mem.rs))

Uses `sys.available_memory()` as the basis for `free`/`used`, so
`used + free == total`. The raw `used` and `free` fields would exclude
reclaimable cache and break that identity, psutil's behavior is what we
match.

### Disk ([metrics/disk.rs](../src/metrics/disk.rs))

Three independent collectors:

- [`collect_root`](../src/metrics/disk.rs) — primary partition
  (`/` on Unix, `%SystemDrive%` — normally `C:\` — on Windows).
- [`collect_all`](../src/metrics/disk.rs) — every non-pseudo
  partition. On Linux we read `/proc/filesystems` and skip filesystems
  marked `nodev` (proc, sysfs, tmpfs, overlay, fuse.snapfuse, ...). On
  Windows sysinfo already lists only fixed and removable drives; `device`
  is the drive root (`C:\`), as psutil reports it, because sysinfo's
  `name()` there is the volume label.
- Usage per mount: `statvfs` on Unix (`df` arithmetic: reserved blocks
  count as used), `GetDiskFreeSpaceExW` on Windows (`used = total -
  total_free`, `free = available to caller`). `percent` is `used / (used +
  free)` on both.
- [`collect_io`](../src/metrics/disk.rs) — bytes/sec and IOPS. On Linux
  from `/proc/diskstats`, summing only **whole-disk** rows (filtered via
  `/sys/block`) to avoid double-counting bytes attributed to both a disk
  and its partitions; sectors are 512-byte regardless of physical sector
  size — see kernel docs. On Windows from `IOCTL_DISK_PERFORMANCE` on each
  `\\.\PhysicalDriveN` (physical drives, so again no double counting),
  which works without admin. Zeros on macOS.

### Network ([metrics/net.rs](../src/metrics/net.rs))

- [`collect_throughput`](../src/metrics/net.rs) — sum total bytes
  across non-loopback interfaces (`lo`, `lo0`, or every address loopback;
  sysinfo already drops the Windows loopback), take the delta against the
  last tick's values, divide by elapsed seconds.
- [`statistics`](../src/metrics/net.rs) — per-iface stats. sysinfo
  doesn't expose dropped-packet counters, so on Linux we backfill from
  `/proc/net/dev` ([read_proc_net_drops](../src/metrics/net.rs)).
- [`interface_names`](../src/metrics/net.rs) — sorted name list for the
  `/network` payload.

### Process ([metrics/process.rs](../src/metrics/process.rs))

The most subtle collector. Two algorithms depending on PSS availability:

**RSS path** (when `/proc/<pid>/smaps_rollup` is unreadable, which includes
all non-Linux): aggregate by process name using `Process::memory()` (RSS),
sort descending, take top 10.

**PSS path** (when smaps_rollup is readable, typically same-uid or root):
PSS reads are syscalls — hundreds per slow tick on a busy host would be
expensive. Instead we use a **streaming top-N** algorithm
([process.rs](../src/metrics/process.rs)):

1. Walk RSS-aggregated entries in descending order.
2. For each, compute PSS by summing per-pid `smaps_rollup` reads.
3. Maintain the running top-N by PSS, sorted descending.
4. Stop when the next candidate's RSS no longer exceeds the smallest PSS in
   the running top-N. Since PSS ≤ RSS and entries are RSS-descending, no
   later entry can break in.

This typically reads ~10-20 smaps_rollup files instead of all of them.

PSS detection ([`detect_pss`](../src/metrics/process.rs)) runs once
and is cached in a `OnceLock`. We skip our own PID so we don't trivially
self-detect, and we sort PIDs ascending to match psutil's `/proc` listdir
order — without that, sysinfo's hash-ordered iteration could land on a
same-uid process first and mis-detect support.

Threads are filtered via `proc_.thread_kind().is_some()`
([process.rs](../src/metrics/process.rs)). Without this,
multi-threaded processes would double-count RSS and the top-N would be
polluted with thread names like `tokio-runtime-w`. On Windows pid 0
("System Idle Process") is skipped too: its CPU% is every core's idle
time, so it would top `processes_by_cpu` on any machine that isn't busy.

The representative `pid` for a name group is the lowest pid for the memory
list and the highest-CPU member for the CPU list, so both are stable from
scan to scan.

The `processes_metric` string in the snapshot tells the dashboard which
algorithm produced the numbers — important because PSS values are typically
30-50% lower than RSS for shared-library-heavy processes.

### Platform ([metrics/platform.rs](../src/metrics/platform.rs))

- `user`: `whoami::username()`, cached in a `OnceLock`.
- `distro`: `/etc/os-release`'s `PRETTY_NAME` on Linux (falling back to
  `NAME`); `System::long_os_version()` elsewhere ("Windows 11 Pro",
  "macOS 15.1 Sequoia"). Cached.
- `kernel`: `System::kernel_version()` (the build number on Windows).
  Cached: it can't change while the process runs, and reading it means a
  file read on Linux and a registry read on Windows.
- `uptime`: human-readable string built by
  [`format_uptime`](../src/metrics/platform.rs). Only non-zero
  components appear, and singulars/plurals are correct.

## Snapshot wire formats

[`Snapshot`](../src/snapshot.rs) is the rich internal model. The
three on-the-wire shapes are *projections* — borrowed views constructed via
inherent methods:

- [`http_view()`](../src/snapshot.rs) → `/system` payload.
- [`ws_view()`](../src/snapshot.rs) → WebSocket frame. Differs
  from the HTTP view: no `disks`, no `user`, no `platform`; `uptime` is
  top-level instead of nested.
- [`network_view()`](../src/snapshot.rs) → `/network` payload.

These projections exist because the original Python collector exposed
different shapes per endpoint and we preserve byte-compatibility. If you
add a new field, decide which views need it and update the matching struct.

`Cpu::temp` has a custom serializer
([snapshot.rs](../src/snapshot.rs)) that emits integer `0`
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
