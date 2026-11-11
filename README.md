# Vigil Collector

A lightweight system and network monitoring server. Stream live host metrics to
any client over HTTP or WebSocket.

<img width="1920" height="1016" alt="image" src="https://github.com/user-attachments/assets/4634aa3a-21ee-4629-a76e-feb669e48e38" />

See it powering a real dashboard [here](https://status.edcs.app), paired with
[Vigil UI](https://github.com/sentrychris/vigil), the comprehensive frontend.

## Quick start

Runs on Linux, Windows and macOS. Requires Rust 1.95 or newer
(`rustup update`).

```sh
git clone https://github.com/sentrychris/vigil-collector && cd vigil-collector
cargo build --release
./target/release/vigil-collector
```

On Windows (PowerShell), with the MSVC toolchain that `rustup` installs by
default:

```powershell
git clone https://github.com/sentrychris/vigil-collector; cd vigil-collector
cargo build --release
.\target\release\vigil-collector.exe
```

Defaults to `localhost:4500`. Hit `http://localhost:4500/system` to confirm
it's running.

```sh
# custom address / port
./target/release/vigil-collector --address 0.0.0.0 --port 4500
```

`Ctrl-C` (or `SIGTERM`; on Windows also Ctrl-Break, closing the console,
logoff and shutdown) shuts everything down cleanly, sending connected
WebSocket clients a `1001 Going Away` close. A settings file is created on
first run at `~/.vigil-collector/settings.json`
(`%USERPROFILE%\.vigil-collector\settings.json` on Windows) - see
[Configuration](#configuration).

Listening on `0.0.0.0` on Windows triggers a Windows Defender Firewall
prompt the first time; allow it on private networks for other machines to
reach the collector.

## What you get

- **Live stream** of CPU, memory, disk, disk I/O, network throughput, and top
  processes - one snapshot per second by default.
- **Multi-disk** for every mounted partition (every fixed/removable drive on
  Windows), with reserved-block accounting that matches `df` exactly.
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
| `cpu.temp` | number | Package temperature, °C (`0` if no sensor; always `0` on Windows) |
| `cpu.freq` | number | Current frequency, MHz |
| `cpu.cores` | number | Logical core count |
| `cpu.load` | array / null | 1/5/15-minute load average (`null` on Windows) |
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
| `processes_metric` | string | `"pss"` or `"rss"` - tells the UI which accounting was used (`"rss"` = working set on Windows) |
| `processes_by_cpu[]` | array | Top 10 by CPU %, aggregated by process name |
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
| `dropin` / `dropout` | number | Dropped packet counts (Linux only; `0` elsewhere) |

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

CLI flags (`--address`, `--port`) override the file at startup. `RUST_LOG`
overrides `log_level`, and `NO_COLOR` turns off coloured console output.
The file may be saved with a UTF-8 byte-order mark (as Notepad does).

## Platform notes

| Metric | Linux | Windows | macOS |
|---|---|---|---|
| Disk usage | `statvfs`, `df`-exact | `GetDiskFreeSpaceExW` | `statvfs` |
| Disk I/O + IOPS | `/proc/diskstats` | `IOCTL_DISK_PERFORMANCE` per physical drive | not reported (`0`) |
| CPU temperature | hwmon / thermal zones | not reported (`0`) | SMC sensors |
| Load average | yes | `null` | yes |
| Process memory | PSS when readable, else RSS | working set | RSS |
| Dropped packets | yes | `0` | `0` |

Nothing needs admin/root on any platform. Running as root on Linux lets the
collector read PSS for every process instead of only your own.

## Running in the background

**Linux (systemd):** see [systemd/vigil-collector.service](systemd/vigil-collector.service).
Copy the binary to `/usr/local/bin`, fill in `User`/`Group`, then
`sudo systemctl enable --now vigil-collector`.

**Windows:** the collector is a console program, not a Windows service.
To start it automatically in the background, register a scheduled task
from an elevated PowerShell. Running it as `SYSTEM` starts it at boot with
no console window:

```powershell
$exe = "C:\Program Files\Vigil\vigil-collector.exe"   # wherever you put it
$action = New-ScheduledTaskAction -Execute $exe -Argument "--address 0.0.0.0 --port 4500"
$trigger = New-ScheduledTaskTrigger -AtStartup
$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) `
    -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)
Register-ScheduledTask -TaskName "Vigil Collector" -Action $action -Trigger $trigger `
    -Settings $settings -User "SYSTEM" -RunLevel Highest
```

As `SYSTEM`, its home folder is `C:\Windows\System32\config\systemprofile`,
so that's where `.vigil-collector\settings.json` and the logs go.


## Connecting from your own app

```js
// 1. Get a worker id from the HTTP endpoint
const worker = await fetch("http://localhost:4500/worker", { method: "POST" })
  .then((r) => r.json());

// 2. Open the websocket using that id
const ws = new WebSocket(`ws://localhost:4500/connect?id=${worker.id}`);
ws.onmessage = (event) => {
  const snapshot = JSON.parse(event.data);
  // { cpu, mem, disk, disk_io, network, uptime, processes, processes_metric, processes_by_cpu }
};
```

Each worker is single-use and expires in 5 seconds if unclaimed. Closing the
WebSocket frees it. A connect refused with `503` (server at
`max_ws_connections`) doesn't use up the worker id.

## Architecture

A single tokio task ticks every `ws_push_interval` seconds, runs the metric
collection on the blocking pool (so sysinfo's `/proc` reads, or Win32 calls
on Windows, don't tie up a runtime worker), and publishes the snapshot via
`arc_swap::ArcSwap`. HTTP and
WebSocket handlers project from that snapshot - they never touch sysinfo
themselves.

Three serialized JSON buffers are cached per tick (HTTP `/system`, WS frame,
`/network`). At `max_ws_connections = 20`, the WS push path is a single
atomic load + `socket.send()` per client per tick, no per-client
serialization or copying.

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
- **Cached `Bytes` per endpoint** - one allocation per sampler tick for each
  of `/system`, the WS frame, and `/network`, shared by refcount with every
  reader. Old buffers are freed as readers release their clones, so
  steady-state heap is bounded by one tick's worth of buffers regardless of
  concurrent client count.

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

`target/release/vigil-collector` (`vigil-collector.exe` on Windows) is a
self-contained executable - the dashboard HTML, favicon, and every runtime
dependency are baked in. TLS for `/probes` uses rustls with the ring
provider and the OS certificate store, so there is no OpenSSL dependency
and no CMake/NASM needed to build on Windows. Stripped binary is ~5 MB.

For a fully static binary on Linux:

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```
