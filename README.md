# Vigil Collector

A lightweight system and network monitoring server. Stream live host metrics to
any client over HTTP or WebSocket.

See it powering a real dashboard [here](https://status.edcs.app), paired with
[Vigil UI](https://github.com/sentrychris/vigil), the comprehensive frontend.

## Quick start

```sh
git clone https://github.com/sentrychris/vigil-collector && cd vigil-collector
cargo build --release
./target/release/vigil-collector
```

Defaults to `localhost:4500`. Hit `http://localhost:4500/system` to confirm
it's running.

```sh
# custom address / port
./target/release/vigil-collector --address 0.0.0.0 --port 4500
```

`Ctrl-C` (or `SIGTERM`) shuts everything down cleanly. A settings file is
created on first run at `~/.vigil-collector/settings.json` - see
[Configuration](#configuration).

## What you get

- **Live stream** of CPU, memory, disk, disk I/O, network throughput, and top
  processes - one snapshot per second by default.
- **Multi-disk** for every mounted partition, with reserved-block accounting
  that matches `df` exactly.
- **External health checks** using a configurable list of URLs. Hits `/probes`
  to get reachability + latency.
- **Process accounting** in PSS when readable, RSS otherwise (the payload
  tells you which).
- **One sampler shared by N clients** so extra dashboards don't multiply the
  work - every HTTP/WS read is an atomic load of pre-serialized JSON.

## HTTP endpoints

### `GET /`

Built-in dashboard at `:4500`. The bundle (HTML, favicon) is baked into the
binary - no extraction on startup, no static-files directory to manage.

### `POST /worker`

Creates a worker for a WebSocket session. Workers expire after 5 seconds if
unclaimed.

| Field | Type | Description |
|---|---|---|
| `id` | string | Worker ID - pass to `/connect?id=...` |
| `url` | string | Pre-built `ws://…/connect?id=…` URL |
| `message` | string | Status text |

### `GET /system`

Current snapshot of the host. Same payload that the WebSocket pushes.

| Field | Type | Description |
|---|---|---|
| `cpu.usage` | number | CPU utilization, % |
| `cpu.temp` | number | Package temperature, °C (`0` if no sensor) |
| `cpu.freq` | number | Current frequency, MHz |
| `mem.total` / `used` / `free` | number | RAM, GiB. `used + free == total` |
| `mem.percent` | number | RAM utilization, % |
| `disk.total` / `used` / `free` | number | Primary partition (root or system drive), GiB |
| `disk.percent` | number | Primary partition utilization, % |
| `disks[]` | array | Every mounted partition: `device`, `mountpoint`, `fstype`, plus the same usage fields |
| `disk_io.read_bytes_per_sec` / `write_bytes_per_sec` | number | Aggregate disk throughput, bytes/sec |
| `disk_io.read_iops` / `write_iops` | number | Aggregate disk operations/sec |
| `network.rx_bytes_per_sec` | number | Receive throughput (loopback excluded) |
| `network.tx_bytes_per_sec` | number | Transmit throughput |
| `processes[]` | array | Top 10 by memory, aggregated by process name |
| `processes_metric` | string | `"pss"` or `"rss"` - tells the UI which accounting was used |
| `platform.distro` / `kernel` / `uptime` | string | OS info + human-readable uptime |
| `user` | string | Logged-in user |

### `GET /network`

| Field | Type | Description |
|---|---|---|
| `interfaces[]` | array | Interface names (`eth0`, `wlan0`, …) |
| `statistics.<iface>` | object | Per-interface counters (see below) |

Per-interface counters under `statistics.<iface>`:

| Field | Type | Description |
|---|---|---|
| `mb_sent` / `mb_received` | number | Bytes since boot, divided by 1024² |
| `pk_sent` / `pk_received` | number | Packet counts |
| `error_in` / `error_out` | number | Error counts |
| `dropin` / `dropout` | number | Dropped packet counts |

### `GET /probes`

Probes every URL in the configured allowlist concurrently. Returns
`{ "probes": [<entry>, ...] }` where each entry is:

| Field | Type | Description |
|---|---|---|
| `name` | string | Probe label |
| `url` | string | Probed URL |
| `ok` | boolean | `true` if the response was 2xx or 3xx within the timeout |
| `status_code` | number / null | HTTP status (null on transport failure) |
| `latency_ms` | number | Round-trip in milliseconds |
| `error` | string / null | Error message on failure |

The allowlist is re-read from settings on every request - edit
`~/.vigil-collector/settings.json` and the next call picks it up, no restart
needed.

## WebSocket

### `GET /connect?id=<worker_id>`

Requires a valid worker ID from `POST /worker`. Once connected, the server
pushes the `/system` snapshot at the configured interval (1 s by default)
until either side closes.

## Configuration

`~/.vigil-collector/settings.json` is created with sensible defaults on first
run; all keys are optional.

| Key | Default | Description |
|---|---|---|
| `address` | `"localhost"` | HTTP listen address |
| `port_number` | `4500` | HTTP listen port |
| `max_ws_connections` | `20` | Cap on concurrent WebSocket clients |
| `ws_push_interval` | `1.0` | Seconds between WS frames (also the sampler tick rate) |
| `probes` | `[ ... ]` | List of `{name, url}` entries probed by `/probes` |
| `probe_timeout` | `5.0` | Per-probe HTTP timeout, seconds |
| `logging_enabled` | `true` | Toggle file logging |
| `log_level` | `"INFO"` | `DEBUG` / `INFO` / `WARNING` / `ERROR` |

CLI flags (`--address`, `--port`, `--hub`, `--hub-key`, `--hub-name`,
`--hub-tags`) and the matching `VIGIL_COLLECTOR_*` environment variables
override the file at startup.

## Connecting from your own app

```js
// 1. Get a worker id from the HTTP endpoint
const worker = await fetch("http://localhost:4500/worker", { method: "POST" })
  .then((r) => r.json());

// 2. Open the websocket using that id
const ws = new WebSocket(`ws://localhost:4500/connect?id=${worker.id}`);
ws.onmessage = (event) => {
  const snapshot = JSON.parse(event.data);
  // { cpu, mem, disk, disk_io, network, uptime, processes, processes_metric }
};
```

Each worker is single-use and expires in 5 seconds if unclaimed. Closing the
WebSocket frees it.

## Vigil Pro hub push (optional)

```sh
vigil-collector \
  --hub wss://hub.example.com/ingest \
  --hub-key <api_key> \
  --hub-name web-01 \
  --hub-tags prod,web
```

The collector opens an outbound WebSocket to the hub, sends a `hello` frame,
and pushes `samples` / `processes` frames at the hub-suggested cadence.
Disconnects are absorbed by exponential backoff (1 s → 30 s, ×1.7); the local
HTTP/WS server is unaffected. The wire format is the v1 protocol - flat
`metric|dim` keys (`disk.used_bytes|/var`, `net.rx_bytes_per_s|eth0`),
25-row top-N process frame with PSS/RSS label, snap-loop and Docker mounts
skipped.

## Architecture

A single tokio task ticks every `ws_push_interval` seconds, runs the metric
collection on the blocking pool (so sysinfo's `/proc` reads don't tie up a
runtime worker), and publishes the snapshot via `arc_swap::ArcSwap`. HTTP and
WebSocket handlers project from that snapshot - they never touch sysinfo
themselves.

Three serialized JSON buffers are cached per tick (HTTP `/system`, WS frame,
`/network`). At `max_ws_connections = 20`, the WS push path is a single
atomic load + `socket.send()` per client per tick, no per-client
serialization.

### Runtime layout

The memory and CPU footprints come from three deliberate choices:

- **`#[tokio::main(flavor = "current_thread")]`** - the workload is one 1 Hz
  sampler plus a low double-digit number of cooperative tasks. Default
  multi-threaded tokio would spawn one worker thread per CPU core (each with
  a 2 MB virtual stack), which on a 16-core box accounts for ~30 MB of idle
  virtual memory we'd never touch.
- **`sysinfo` with `default-features = false`** (drops the `multithread`
  feature) - sysinfo's default build pulls in rayon and parallelizes process
  scanning across a global thread pool sized to `num_cpus`. For a 1 Hz tick
  scanning a few hundred processes that's wasted: rayon's idle pool was the
  source of ~16 background threads in early builds.
- **Cached `Arc<Vec<u8>>` per endpoint** - one allocation per sampler tick
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
  instead of every tick. We never read sysinfo's stored disk usage anyway -
  `usage_for_mount` calls `statvfs` itself for the byte-compatible
  `(f_blocks - f_bfree)` accounting - so sysinfo's per-tick `Disks::refresh`
  is purely waste and gets skipped. `Disks::refresh_list` catches
  mount/unmount events.

What stays at every-tick cadence (because it's a counter delta and a 1 s
window is the contract):

- CPU usage % (from `/proc/stat` global counters)
- Memory used/free
- Disk I/O bytes/sec and IOPS (from `/proc/diskstats` deltas)
- Network rx/tx bytes/sec (from sysinfo's per-iface counter deltas)

The per-disk usage walk via `statvfs` runs every tick too, but it's only
~30 µs total - cheap enough that the dashboard always sees fresh values.

End result: idle CPU usage settles around ~0.23% of a single core, and stays
there regardless of how many clients connect, because the per-tick work
doesn't scale with client count.

## Single-binary build

```sh
cargo build --release
```

`target/release/vigil-collector` is a self-contained executable - the
dashboard HTML, favicon, and every runtime dependency are baked in. Stripped
binary is ~5.5 MB.

For a fully static binary on Linux:

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

## License

MIT.
