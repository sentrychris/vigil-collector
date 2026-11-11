# Vigil

A system monitoring utility with remote connection capabilities. Vigil
watches a machine's processor, memory, disks, network and processes, and
serves them to its built-in live dashboard, or to any client over HTTP or
WebSocket, on the same machine or across your network.

<img width="1920" height="1016" alt="image" src="https://github.com/user-attachments/assets/4634aa3a-21ee-4629-a76e-feb669e48e38" />

See it running [here](https://status.edcs.app). The dashboard is the Vue app
in [web/](web/), built into the binary, so there's nothing else to deploy.

## Contents

- [Quick start](#quick-start)
- [Building](#building) - [Linux](#linux) · [Windows](#windows) · [macOS](#macos) · [Static and cross-compiled builds](#static-and-cross-compiled-builds)
- [Running](#running)
- [Running in the background](#running-in-the-background)
- [Configuration](#configuration)
- [HTTP endpoints](#http-endpoints) · [WebSocket](#websocket) · [Connecting from your own app](#connecting-from-your-own-app)
- [Platform notes](#platform-notes)
- [Development](#development)
- [Architecture](#architecture)

## Quick start

```sh
git clone https://github.com/sentrychris/vigil-collector
cd vigil-collector
cargo run --release
```

Then open <http://localhost:4500> for the dashboard, or
<http://localhost:4500/system> for the raw JSON. Building needs Rust and
Node.js; platform-specific setup is under [Building](#building).

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

## Building

Vigil builds from the same source on Linux, Windows and macOS.
Every platform needs:

- **Rust 1.95 or newer.** Install it with [rustup](https://rustup.rs); if you
  already have it, run `rustup update`. Check with `rustc --version`.
- **A C compiler.** The TLS library (`ring`, used for `/probes`) compiles a
  small amount of C and assembly. There's no OpenSSL, CMake or NASM
  dependency.
- **Node.js 18 or newer, with npm.** `cargo build` builds the dashboard in
  [web/](web/) through [build.rs](build.rs): `npm ci` the first time, then
  `vite build` whenever a file in `web/` changes. Check with `node --version`.

The release binary is self-contained: the dashboard is embedded, and
nothing else needs to ship alongside it. It's about 6 MB stripped. Node is
only needed to build it, not to run it.

### Linux

Install a C toolchain and Rust:

```sh
# Debian / Ubuntu
sudo apt install build-essential curl git
# Fedora / RHEL
sudo dnf install gcc make curl git
# Arch
sudo pacman -S base-devel curl git

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Install Node.js 18 or newer. Distro packages are often older, so check
`node --version`; [nodejs.org](https://nodejs.org/en/download) lists
up-to-date packages and installers (or use `nvm`).

Build and run:

```sh
git clone https://github.com/sentrychris/vigil-collector
cd vigil-collector
cargo build --release
./target/release/vigil-collector
```

The binary is `target/release/vigil-collector`. To install it system-wide:

```sh
sudo install -m 755 target/release/vigil-collector /usr/local/bin/
```

### Windows

1. Install the **Visual Studio C++ Build Tools**, which Rust's default
   Windows toolchain uses as its linker and C compiler. Either install
   [Build Tools for Visual Studio](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
   with the **Desktop development with C++** workload, or let `rustup-init`
   offer to install it for you.
2. Install Rust with
   [rustup-init.exe](https://win.rustup.rs/x86_64) (or
   `winget install Rustlang.Rustup`), accepting the default
   `x86_64-pc-windows-msvc` toolchain.
3. Install Node.js LTS from [nodejs.org](https://nodejs.org) (or
   `winget install OpenJS.NodeJS.LTS`). Open a new terminal afterwards so
   `npm` is on `PATH`.
4. Build and run from PowerShell:

```powershell
git clone https://github.com/sentrychris/vigil-collector
cd vigil-collector
cargo build --release
.\target\release\vigil-collector.exe
```

The binary is `target\release\vigil-collector.exe`. Copy it anywhere, for
example `C:\Program Files\Vigil\` (writing there needs an elevated
PowerShell). It needs no installer, and the C runtime
is linked in statically (see [.cargo/config.toml](.cargo/config.toml)), so
it runs on machines without the Visual C++ Redistributable.

Tested on Windows 11; Windows 10 uses the same APIs. No administrator
rights are needed to run it.

### macOS

Install the Xcode command line tools, Rust and Node.js, then build as on
Linux:

```sh
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
brew install node                  # or the installer from nodejs.org

git clone https://github.com/sentrychris/vigil-collector
cd vigil-collector
cargo build --release
./target/release/vigil-collector
```

macOS is built and smoke-tested in CI but hasn't been tried on a real Mac
yet. It reports less than Linux and Windows (no disk I/O; see
[Platform notes](#platform-notes)).

### Static and cross-compiled builds

**Fully static Linux binary** (runs on any x86-64 distro, including Alpine
and minimal containers):

```sh
sudo apt install musl-tools        # provides musl-gcc for the C parts
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
# -> target/x86_64-unknown-linux-musl/release/vigil-collector
```

**Windows binary from Linux**, using the MinGW toolchain:

```sh
sudo apt install gcc-mingw-w64-x86-64
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
# -> target/x86_64-pc-windows-gnu/release/vigil-collector.exe
```

The dashboard is plain HTML/JS, so the Linux machine's Node builds it for
any target. The `x86_64-pc-windows-msvc` target can't be cross-compiled from Linux this
way (ring needs `clang-cl` for it). Build MSVC binaries on Windows, or use
the GNU target above; both behave the same at runtime.

## Running

```sh
vigil-collector                                  # localhost:4500
vigil-collector --address 0.0.0.0 --port 4500    # listen on every interface
vigil-collector --help
```

By default Vigil listens on `localhost:4500`, so only the same
machine can reach it. Use `--address 0.0.0.0` (or set `address` in the
[settings file](#configuration)) to serve other machines on your network.
On Windows, the first time it listens on a non-local address, Windows
Defender Firewall asks whether to allow it. Allow it on private networks.

On first run it logs where the settings file was created, then the
address it's listening on:

```text
[2026-11-11 14:12:47]  INFO Created settings file at /home/you/.vigil-collector/settings.json
[2026-11-11 14:12:47]  INFO Listening on http://127.0.0.1:4500
```

**Stopping:** `Ctrl-C`, or `SIGTERM` on Linux and macOS. On Windows,
use the tray menu (below), or with `--foreground`, `Ctrl-C`,
`Ctrl-Break` or closing the console window. Signing out and shutting
down stop it cleanly too. Connected WebSocket clients get a
`1001 Going Away` close frame, so dashboards can tell a restart from a
crash.

### The system tray on Windows

On Windows, Vigil runs in the background with an icon in the
notification area. Double-click the icon to open the dashboard in your
browser. Right-click it for a menu with **Open dashboard**, how many
dashboards are connected and the address, **Restart** and **Quit**, and
the version. Restart starts it again with the same options and re-reads
the settings file. The executable has the same icon, and its Properties >
Details tab shows the version.

- **Double-clicked**, it goes straight to the tray, with no console
  window. If its port (4500 unless the settings file says otherwise) is
  already in use, it takes the next free one, up to 99 ports further on;
  hover over the icon to see which. If it can't start at all, a message
  box says why.
- **From a terminal**, it prints its startup log, including the
  `Listening on ...` line, then gives you the prompt back while it keeps
  running in the tray. Closing the terminal doesn't stop it. Here, and
  whenever `--port` is given, the port is used exactly as set: if it's
  taken, Vigil stops with an error.
- **`--foreground`** keeps it in the console instead, with no tray icon,
  as on Linux and macOS. It also stays in the console on its own when its
  output is redirected to a file or pipe, and when it runs as a service
  or a `SYSTEM` task, which have no desktop to show an icon on.

Only one copy runs per computer. Started again while it's running, it
opens the running copy's dashboard instead (double-clicked), or prints
where it's running and exits (from a terminal, or with `--foreground`).

In the tray, the log is written only to the log file (see
[Configuration](#configuration)).

## Running in the background

### Linux (systemd)

A unit file is included at
[systemd/vigil-collector.service](systemd/vigil-collector.service).

```sh
sudo install -m 755 target/release/vigil-collector /usr/local/bin/
sudo cp systemd/vigil-collector.service /etc/systemd/system/
sudo nano /etc/systemd/system/vigil-collector.service   # set User= and Group=
sudo systemctl daemon-reload
sudo systemctl enable --now vigil-collector
journalctl -u vigil-collector -f                        # follow the logs
```

The unit listens on `0.0.0.0:4700`; change `ExecStart` to suit. The
settings file lives in the home folder of the `User=` you chose.

### Windows (Task Scheduler)

To have it running whenever you're signed in, with its tray icon, put a
shortcut to the exe in your Startup folder (`Win+R`, `shell:startup`).

To start it at boot, before anyone signs in, register a scheduled task
that runs it as `SYSTEM`. It isn't a Windows service, and a `SYSTEM` task
has no tray icon, so `--foreground` keeps it in its own (invisible)
console. From an **elevated** PowerShell:

```powershell
$exe = "C:\Program Files\Vigil\vigil-collector.exe"   # wherever you put it
$action = New-ScheduledTaskAction -Execute $exe -Argument "--foreground --address 0.0.0.0 --port 4500"
$trigger = New-ScheduledTaskTrigger -AtStartup
$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) `
    -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)
Register-ScheduledTask -TaskName "Vigil" -Action $action -Trigger $trigger `
    -Settings $settings -User "SYSTEM" -RunLevel Highest
Start-ScheduledTask -TaskName "Vigil"
```

Running as `SYSTEM`, its home folder is
`C:\Windows\System32\config\systemprofile`, so its settings file and logs
are under that folder rather than yours.

A task running as `SYSTEM` has no desktop, so Windows Defender Firewall
can't ask whether to allow it, and other machines are blocked. If you
listen on `0.0.0.0`, add an inbound rule for the exe (same elevated
PowerShell):

```powershell
New-NetFirewallRule -DisplayName "Vigil" -Direction Inbound `
    -Program $exe -Action Allow -Profile Private
```

To remove the task and the rule:

```powershell
Unregister-ScheduledTask -TaskName "Vigil" -Confirm:$false
Remove-NetFirewallRule -DisplayName "Vigil"
```

### macOS (launchd)

Save this as `~/Library/LaunchAgents/app.vigil.collector.plist`, adjusting
the binary path, then run
`launchctl load ~/Library/LaunchAgents/app.vigil.collector.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>app.vigil.collector</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/local/bin/vigil-collector</string>
    <string>--address</string><string>0.0.0.0</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
</dict>
</plist>
```

## Configuration

A settings file is created with defaults on first run. All keys are
optional; a missing key takes its default, and a file that doesn't parse is
reported in the log and ignored, so a typo never stops Vigil starting.

| | Linux / macOS | Windows |
|---|---|---|
| Settings | `~/.vigil-collector/settings.json` | `%USERPROFILE%\.vigil-collector\settings.json` |
| Log file | `~/.vigil-collector-logs/app.log` | `%USERPROFILE%\.vigil-collector-logs\app.log` |

| Key | Default | Description |
|---|---|---|
| `address` | `"localhost"` | HTTP listen address |
| `port_number` | `4500` | HTTP listen port |
| `max_ws_connections` | `20` | Cap on concurrent WebSocket clients |
| `ws_push_interval` | `1.0` | Seconds between WS frames (also the sampler tick rate; minimum `0.1`) |
| `probes` | `[ ... ]` | List of `{name, url}` entries probed by `/probes` |
| `probe_timeout` | `5.0` | Per-probe HTTP timeout, seconds |
| `logging_enabled` | `true` | Toggle logging (console and file) |
| `log_level` | `"INFO"` | `DEBUG` / `INFO` / `WARNING` / `ERROR` |

Example:

```json
{
  "address": "0.0.0.0",
  "port_number": 4500,
  "ws_push_interval": 1.0,
  "probes": [
    { "name": "my site", "url": "https://example.com" }
  ]
}
```

- `--address` and `--port` override the file at startup.
- `RUST_LOG` (e.g. `RUST_LOG=vigil_collector=debug`) overrides `log_level`.
- `NO_COLOR=1` turns off coloured console output. Colour is also off
  automatically when output isn't a terminal (a file, journald, a pipe).
- The log file is cleared at each startup.
- On Windows, editing the file in Notepad is fine: the UTF-8 byte-order mark
  it adds is accepted.
- The probe list is re-read on every `/probes` request, so it can be
  changed without a restart. Other settings need a restart.

## HTTP endpoints

### `GET /`

The dashboard. Its files are baked into the binary, so there's no
extraction on startup and no static-files directory to manage. Any GET
path the API doesn't use serves the dashboard too, so its client-side
routes survive a reload. The dashboard connects back to whichever
Vigil instance served it.

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
| `cpu.temp` | number | Package temperature, °C (`0` if no sensor; see [Platform notes](#platform-notes) for Windows) |
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

## Platform notes

| Metric | Linux | Windows | macOS |
|---|---|---|---|
| Disk usage | `statvfs`, `df`-exact | `GetDiskFreeSpaceExW` | `statvfs` |
| Disk I/O + IOPS | `/proc/diskstats` | `IOCTL_DISK_PERFORMANCE` per physical drive | not reported (`0`) |
| CPU temperature | hwmon / thermal zones | from a monitoring app or a live ACPI thermal zone, else `0` (see below) | SMC sensors |
| Load average | yes | `null` | yes |
| Process memory | PSS when readable, else RSS | working set | RSS |
| Dropped packets | yes | `0` | `0` |

Nothing needs admin/root on any platform. Running as root on Linux lets
Vigil read PSS for every process instead of only your own.

On Windows, network throughput leaves out NDIS filter interfaces (such as
"WiFi-VirtualBox NDIS Light-Weight Filter-0000"), which mirror the adapter
underneath them and would otherwise count its traffic twice. Hyper-V / WSL
virtual switches are kept, like `docker0` on Linux.

### CPU temperature on Windows

Windows has no built-in way for a program to read the CPU's own
temperature sensor without a kernel driver, which is why Task Manager
doesn't show it. Vigil doesn't ship a driver. Instead it checks
these once a second and uses the first that has a reading:

1. **HWiNFO**, with *Shared Memory Support* turned on in its settings. The
   free version switches this off again after 12 hours.
2. **MSI Afterburner**. It has to run in the same Windows session as
   Vigil, so this doesn't work when Vigil runs as a `SYSTEM` task.
3. **LibreHardwareMonitor** (or OpenHardwareMonitor), through its WMI
   sensors.
4. **The ACPI thermal zone** Windows exposes itself. Many desktops report a
   fixed placeholder there (often 27.85 °C), so a zone is only used once
   its value has changed since Vigil started. On laptops it is
   often real, but it may be a board or chassis sensor rather than the CPU
   package.

If none of these is available, `cpu.temp` is `0`, as before. The log says
which source is in use, and when the source changes.

## Development

```sh
cargo fmt                                   # format
cargo clippy --all-targets -- -D warnings   # lint (warnings are errors in CI)
cargo test                                  # unit tests
RUST_LOG=vigil_collector=debug cargo run    # run with debug logging
```

The dashboard has its own tools in [web/](web/):

```sh
cd web
npm run dev          # dashboard on :8888 with hot reload
npm run type-check   # vue-tsc
npx vitest run --environment jsdom --root src/   # unit tests
```

`npm run dev` forwards `/system`, `/worker`, `/connect` and the other API
paths to Vigil on `127.0.0.1:4500`, so run it alongside
(`cargo run`), or point it elsewhere with `VIGIL_COLLECTOR_URL` in
`web/.env` (see [web/.env.example](web/.env.example)). Set
`VIGIL_SKIP_WEB_BUILD=1` to make `cargo build` reuse an existing
`web/dist` instead of rebuilding it.

CI ([.github/workflows/ci.yml](.github/workflows/ci.yml)) type-checks and
unit-tests the dashboard, runs the Rust checks on Linux, Windows and macOS,
then starts the release binary and checks that `/system`, `/network`,
`/worker` and the dashboard (page and assets) respond. Platform-specific code lives behind `#[cfg(...)]` in
`src/metrics/` and `src/log.rs`; if you change it, cross-check the other
targets:

```sh
cargo clippy --all-targets --target x86_64-pc-windows-gnu -- -D warnings
```

Contributor docs, including the full data flow and runtime model, are in
[docs/](docs/ARCHITECTURE.md).

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

Profiling a baseline tick on Linux (sampler running, no clients) showed:

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
  instead of every tick. Per-disk usage is read directly every tick
  (`statvfs` on Unix for the `df`-compatible `(f_blocks - f_bfree)`
  accounting, `GetDiskFreeSpaceExW` on Windows), so sysinfo only needs to
  re-list the disks to catch mount/unmount events.

What stays at every-tick cadence (because it's a counter delta and a 1 s
window is the contract):

- CPU usage % (from `/proc/stat` global counters; PDH counters on Windows)
- Memory used/free
- Disk I/O bytes/sec and IOPS (from `/proc/diskstats` deltas;
  `IOCTL_DISK_PERFORMANCE` on Windows)
- Network rx/tx bytes/sec (from sysinfo's per-iface counter deltas)

The per-disk usage walk runs every tick too, but it's only
~30 µs total - cheap enough that the dashboard always sees fresh values.

End result: idle CPU usage settles around ~0.23% of a single core, and stays
there regardless of how many clients connect, because the per-tick work
doesn't scale with client count.
