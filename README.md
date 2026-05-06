# Vigil Collector (Rust)

A lightweight system and network monitoring server. Stream live host metrics to
any client over HTTP or WebSocket. Wire-compatible with the Python
[vigil-collector](https://github.com/sentrychris/vigil-collector) — same routes,
same JSON shapes, same Vigil Pro hub protocol.

This is a Rust rewrite of the original Python implementation. The HTTP/WS API
and the Vigil Pro hub protocol are preserved byte-for-byte; existing clients
([Vigil UI](https://github.com/sentrychris/vigil), Vigil Pro hub) work without
modification.

## Quick start

```sh
git clone https://github.com/sentrychris/vigil-collector-rs && cd vigil-collector-rs
cargo build --release
./target/release/vigil-collector
```

Defaults to `localhost:4500`. Hit `http://localhost:4500/system` to confirm.

```sh
# custom address / port
./target/release/vigil-collector --address 0.0.0.0 --port 4500
```

A settings file is created on first run at `~/.vigil-collector/settings.json`.
The keys and defaults are unchanged from the Python version — see
[Configuration](#configuration).

## What you get

- Live snapshot of CPU, memory, disk, disk I/O, network throughput, and top
  processes — one sample per second by default.
- Multi-disk reporting for every mounted partition, with reserved-block
  accounting that matches `df` exactly.
- External health checks via a configurable allowlist; `/probes` returns
  reachability and latency for each entry.
- PSS-aware process accounting on Linux (when readable), RSS otherwise — the
  payload tells the UI which.
- One sampler shared by N clients; each WebSocket frame is the cached JSON
  bytes from the last tick, so connecting more dashboards adds zero sampling
  cost.

## HTTP endpoints

| Route | Method | Description |
|---|---|---|
| `/` | GET | Built-in dashboard (same `web.html` as the Python build) |
| `/worker` | POST | Issue a single-use, 5s-expiry WebSocket worker token |
| `/system` | GET | Latest sampler snapshot — full payload |
| `/network` | GET | Per-interface counters since boot |
| `/probes` | GET | Run all configured probes concurrently and report status |
| `/connect?id=<token>` | GET (Upgrade) | WebSocket frames at `ws_push_interval` |

The JSON shape of every response is identical to the Python collector — see the
upstream README for the field-by-field reference.

## Configuration

`~/.vigil-collector/settings.json` is created with sensible defaults on first
run; legacy `~/.psmonitor/` is migrated in place.

| Key | Default | Description |
|---|---|---|
| `address` | `"localhost"` | HTTP listen address |
| `port_number` | `4500` | HTTP listen port |
| `max_ws_connections` | `20` | Cap on concurrent WS clients |
| `ws_push_interval` | `1.0` | Seconds between WS frames (and sampler tick) |
| `probes` | `[ ... ]` | List of `{name, url}` entries probed by `/probes` |
| `probe_timeout` | `5.0` | Per-probe HTTP timeout, seconds |
| `logging_enabled` | `true` | Toggle log output |
| `log_level` | `"INFO"` | `DEBUG` / `INFO` / `WARNING` / `ERROR` |

CLI flags (`--address`, `--port`, `--hub`, `--hub-key`, `--hub-name`,
`--hub-tags`) and the matching `VIGIL_COLLECTOR_*` environment variables work
the same as the Python build.

## Vigil Pro hub push (optional)

```sh
vigil-collector \
  --hub wss://hub.example.com/ingest \
  --hub-key <api_key> \
  --hub-name web-01 \
  --hub-tags prod,web
```

The collector opens an outbound WebSocket to the hub, sends a `hello` frame,
and pushes `samples`/`processes` frames at the hub-suggested cadence.
Disconnects are absorbed by exponential backoff (1s → 30s, ×1.7); the local
HTTP/WS server is unaffected.

The hub wire format is the same v1 protocol the Python collector uses —
`name|dim`-flattened metrics map, `disk.used_bytes|/var` etc., 25-row top-N
process frame with PSS/RSS label, snap-loop and Docker mounts skipped.

## Architecture

A single tokio task ticks every `ws_push_interval` seconds, runs the metric
collection on the blocking pool (so sysinfo's `/proc` reads don't tie up a
runtime worker), and publishes the snapshot via `arc_swap::ArcSwap`. HTTP and
WebSocket handlers project from that snapshot — they never touch sysinfo
themselves.

Three serialized JSON buffers are cached per tick (HTTP `/system`, WS frame,
`/network`). At `max_ws_connections = 20`, the WS push path is a single atomic
load + `socket.send()` per client per tick — no per-client serialization.

## Performance vs. the Python collector

Quick measurement on this machine (16-core x86_64, Linux 6.6 WSL2):

| | Rust | Python | Δ |
|---|---|---|---|
| Idle CPU (sampler running, no clients) | **0.23%** | 0.87% | ~3.8× |
| `ab -n 5000 -c 50 -k /system` throughput | **66,948 req/s** | 2,875 req/s | ~23× |
| Mean latency at concurrency 50 | **0.7 ms** | 7.0 ms | ~10× |
| Resident memory (warm, sustained) | **8 MB** | 34 MB | ~4× |
| Virtual memory | **76 MB** | 270 MB | ~3× |
| OS threads | **2** | 12+ | — |
| Stripped release binary | **5.5 MB** | (PyInstaller bundle ~30 MB) | ~5× |

The throughput edge comes from caching pre-serialized JSON bytes for each
endpoint at the sampler tick — every HTTP/WS read is a single atomic load
plus a `Bytes::send()`, with zero serialization on the hot path.

The memory edge comes from three deliberate choices in the runtime layout:

- **`#[tokio::main(flavor = "current_thread")]`** — the workload is one 1 Hz
  sampler plus a low double-digit number of cooperative tasks. Default
  multi-threaded tokio would spawn one worker thread per CPU core (each with
  a 2 MB virtual stack), which on a 16-core box accounts for ~30 MB of idle
  virtual memory we'd never touch.
- **`sysinfo` with `default-features = false`** (drops the `multithread`
  feature) — sysinfo's default build pulls in rayon and parallelizes process
  scanning across a global thread pool sized to `num_cpus`. For a 1 Hz tick
  scanning a few hundred processes that's wasted: rayon's idle pool was the
  source of ~16 background threads in early builds.
- **Cached `Arc<Vec<u8>>` per endpoint** — one allocation per sampler tick
  for each of `/system`, the WS frame, and `/network`. Old buffers are freed
  as readers release their `Arc` clones, so steady-state heap is bounded by
  one tick's worth of buffers regardless of concurrent client count.

We tried `mimalloc` as the global allocator: it raised throughput slightly
(+5%) but increased steady-state RSS by ~1 MB and reserved a 1 GB virtual
region. Default ptmalloc fragments less for our small working set, so we
stayed on it.

### Idle CPU usage

For a monitoring agent the agent itself shouldn't be a top consumer in its
own output, so the per-tick budget got profiled and trimmed.

Profiling a baseline tick (sampler running, no clients) showed:

| Step | Time | % of tick |
|---|---|---|
| `processes_refresh` (sysinfo walks `/proc/*/{stat,status,statm}`) | ~7,000 µs | 63% |
| `cpu_all` (per-core frequency reads) | ~2,000 µs | 18% |
| `disks_refresh` (per-mount `statvfs` we then redo ourselves) | ~1,500 µs | 13% |
| Everything else (mem, network, components, json, ...) | ~500 µs | 5% |
| **Total** | **~11,000 µs** | |

Three rate-limit decisions cut that to ~3,000 µs/tick on average:

- **Top-N processes scanned every 5 sampler ticks**, not every tick. Memory
  rankings are stable second-to-second; the previous top-N is reused on the
  in-between ticks (see `PROCESS_SCAN_EVERY` in `sampler.rs`). This step
  alone was 63% of a tick.
- **CPU frequency rolls into the same slow cadence.** `refresh_cpu_usage()`
  reads only `/proc/stat` (cheap) and runs every tick; the per-core
  frequency files live behind `refresh_cpu_frequency()` and are read on the
  5-tick cycle. Frequency is a display value with no delta semantics, so
  freshness is irrelevant here.
- **Mount table re-scanned once a minute** (`DISK_LIST_REFRESH_EVERY = 60`)
  instead of every tick. We never read sysinfo's stored disk usage anyway —
  `usage_for_mount` calls `statvfs` itself for the byte-compatible
  `(f_blocks - f_bfree)` accounting — so sysinfo's per-tick `Disks::refresh`
  is purely waste and gets skipped. `Disks::refresh_list` catches
  mount/unmount events.

What stays at every-tick cadence (because it's a counter delta and a 1 s
window is the contract):

- CPU usage % (from `/proc/stat` global counters)
- Memory used/free
- Disk I/O bytes/sec and IOPS (from `/proc/diskstats` deltas)
- Network rx/tx bytes/sec (from sysinfo's per-iface counter deltas)

The per-disk usage walk via `statvfs` runs every tick too, but it's only
~30 µs total — cheap enough that the dashboard always sees fresh values.

End result: idle CPU usage drops from being on par with the Python collector
(~0.87%) to about a quarter of it (0.23%) — and stays there regardless of
how many clients connect, because the per-tick work doesn't scale with
client count.

## Single-binary build

```sh
cargo build --release
```

`target/release/vigil-collector` is a self-contained executable — the dashboard
HTML, favicon, and every runtime dependency are baked in. No `_MEI*` extraction
on startup (unlike PyInstaller).

For a fully static binary on Linux:

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

## Connecting from your own app

```js
const worker = await fetch("http://localhost:4500/worker", { method: "POST" })
  .then((r) => r.json());

const ws = new WebSocket(`ws://localhost:4500/connect?id=${worker.id}`);
ws.onmessage = (event) => {
  const snapshot = JSON.parse(event.data);
  // { cpu, mem, disk, disk_io, network, uptime, processes, processes_metric }
};
```

Each worker is single-use and expires in 5 seconds if unclaimed.

## License

MIT.
