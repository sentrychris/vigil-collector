# Architecture

This document is the canonical map of the Vigil Collector codebase. It
complements the user-facing summary in [README.md](../README.md#architecture)
with the contributor-level detail an agent or new maintainer needs to make
non-trivial changes safely.

For deeper dives, see:

- [how-it-flows.md](./how-it-flows.md) — end-to-end trace from process start
  to bytes-on-the-wire.
- [runtime-semantics.md](./runtime-semantics.md) — Tokio runtime model,
  concurrency primitives, ownership patterns, shutdown propagation.
- [the-sampler.md](./the-sampler.md) — sampler internals plus per-metric
  collector behavior.

## One-line description

A single-process Tokio service that ticks `sysinfo` once per second, builds a
`Snapshot`, and atomically publishes three pre-serialized JSON buffers that
HTTP and WebSocket handlers read without further work.

## Component map

```
src/
├── main.rs              entry point; wires runtime, signals, sampler, server
├── cli.rs               clap parser for command-line flags (--address, --port, --foreground)
├── config.rs            settings.json load + first-run defaults + migration
├── log.rs               tracing setup (stdout + file, classic timestamp, Windows ANSI)
├── tray.rs              Windows: launcher hand-off and the notification-area icon + menu
├── ico.rs               picks the right-sized image out of the .ico for the tray
├── snapshot.rs          shared data model + per-endpoint projections
├── sampler.rs           1 Hz tick loop, refresh state, ArcSwap JSON cache
├── metrics/             per-domain collectors (stateless over sysinfo)
│   ├── cpu.rs           usage, freq, package temp, load average (Unix)
│   ├── temp_windows.rs  Windows CPU temp reader thread (shared memory, WMI, PDH)
│   ├── temp_parse.rs    parsers for those sources (tested on every platform)
│   ├── mem.rs           total/used/free/percent (psutil-compatible)
│   ├── disk.rs          root + all mounts (statvfs / GetDiskFreeSpaceExW)
│   │                    + I/O deltas (/proc/diskstats / IOCTL_DISK_PERFORMANCE)
│   ├── net.rs           throughput delta + per-iface stats + /proc/net/dev
│   ├── platform.rs      distro, kernel, uptime string, current user
│   └── process.rs       top-N by memory and by CPU, PSS-or-RSS detection
├── server/              axum HTTP + WS layer
│   ├── mod.rs           router, AppState, serve()
│   ├── http.rs          GET /system, /network, /probes; embedded dashboard fallback
│   ├── ws.rs            WebSocket upgrade and per-client push loop
│   ├── worker.rs        single-use 5-second token registry for WS handshake
│   └── probes.rs        external HTTP health checks (parallel reqwest)
build.rs                 runs `npm ci` / `vite build` in web/ so the binary can embed web/dist
                         (and, for Windows targets, gives the .exe the favicon as its icon)
web/                     the Vue dashboard (Vite + TypeScript), served at / from the binary
```

## Platform support

Linux, Windows and macOS build from the same tree. Platform-specific reads
are behind `#[cfg]` in `metrics/` and `log.rs`, each with a fallback so
every field is always present in the payload:

| Metric | Linux | Windows | macOS |
|---|---|---|---|
| Disk usage | `statvfs` (psutil/df arithmetic) | `GetDiskFreeSpaceExW` | `statvfs` |
| Disk I/O + IOPS | `/proc/diskstats`, whole disks | `IOCTL_DISK_PERFORMANCE` on `\\.\PhysicalDriveN` | zeros |
| CPU temperature | sysinfo components, then `/sys/class/thermal` | HWiNFO / Afterburner shared memory, LibreHardwareMonitor WMI, live ACPI thermal zone, else `0` | sysinfo components |
| Load average | `getloadavg(3)` | `null` | `getloadavg(3)` |
| Process memory | PSS when readable, else RSS | working set (`"rss"`) | RSS |
| Net drop counters | `/proc/net/dev` | `0` | `0` |
| Shutdown signals | Ctrl-C, SIGTERM | Tray menu (Quit/Restart), Ctrl-C, Ctrl-Break, console close, logoff, shutdown | Ctrl-C, SIGTERM |

## Data flow

The producer/consumer split is the single most important shape of this
service. **One producer, many consumers, zero per-request work.**

```
                            sysinfo / /proc
                                  |
                                  v   spawn_blocking
                       +----------+-----------+
            tick 1Hz ->|     SamplerState     |
                       | (System, Disks, ...) |
                       +----------+-----------+
                                  | build
                                  v
                       +----------+-----------+
                       |       Snapshot       |
                       +----------+-----------+
                                  | project + serde_json (x3)
              +-------------------+-------------------+
              v                   v                   v
       http_system           ws_system            network
       ArcSwap<Bytes>      ArcSwap<Bytes>      ArcSwap<Bytes>
              |                   |                   |
       GET /system          WS /connect          GET /network
              v                   v                   v
          load ->             load ->             load ->
        send bytes          send frame          send bytes
```

Every read path — HTTP handler or WebSocket push — is one atomic pointer load
and one refcount bump on the shared `Bytes` buffer. No serialization, no
copies, no syscalls, no locks.

The three buffers exist because the wire formats differ:

- `http_system` — full payload for `GET /system` ([HttpSystemView](../src/snapshot.rs)).
- `ws_system` — slimmer WebSocket frame; `uptime` is top-level, no `disks` /
  `user` / `platform` ([WsSystemView](../src/snapshot.rs)).
- `network` — interface list + per-iface stats for `GET /network`
  ([NetworkView](../src/snapshot.rs)).

`/probes` is the only handler that does work per-request: it issues live HTTP
HEAD/GET checks against the configured probe URLs.

## Process model

```
            Tokio current-thread runtime
            |
            +-- main task ------------ awaits server, then stops sampler
            |
            +-- signal task ---------- ctrl_c / SIGTERM (Unix) or console
            |                          control events (Windows); fires the
            |                          shutdown Notify
            |
            +-- sampler task --------- 1 Hz; each tick spawn_blocking(refresh)
            |
            +-- server task (axum) --- accepts; per-conn task on upgrade
            |       |
            |       +-- WS push_loop -- per-client; load + send + recv
            |
            +-- worker expiry tasks -- one per token, sleeps 5 s, drops entry
            |
            +-- blocking pool -------- sampler refresh, statvfs, /proc reads,
                                       Win32 disk/process queries

```

On Windows, unless `--foreground` is given (or there's no desktop, or
output is redirected), the process that was started is only a launcher:
it starts a console-less copy with `VIGIL_TRAY_CHILD=1`, relays its
startup log until the copy prints a private "listening" line, and exits.
The copy runs the model above plus one OS thread, `tray`, which owns a
hidden window, the notification-area icon and the Win32 message loop.
Menu choices reach the signal task over an unbounded channel; Restart
sets a flag that makes `main` start a fresh copy after the server and
sampler have stopped and the icon is removed. The menu's connection count
is the server's `active_ws` counter, which `main` creates and shares with
both.

Single runtime thread for everything async. Heavy syscalls go to the blocking
pool so the runtime thread stays unblocked.

## Critical invariants

These are the load-bearing properties of the system. Breaking any of them is
a regression even if tests still pass.


1. **The sampler must never die.** Loss of the tick task means stale buffers
   forever and no log signal. Errors during refresh rebuild state from
   scratch and continue ([sampler.rs](../src/sampler.rs)).

2. **Read paths never call sysinfo.** All per-metric work happens inside the
   sampler's `spawn_blocking` closure. HTTP/WS handlers only do `load`
   on `ArcSwap`. If you find yourself adding a metric read in a handler,
   stop and add it to the sampler instead.

3. **Per-tick allocation is bounded.** Three JSON buffer allocations per
   tick regardless of client count. Old buffers free as readers drop their
   `Bytes` clones. Steady-state heap does not scale with concurrent clients.

4. **CLI flags override settings on startup, not at runtime.** `address` and
   `port` come from CLI if provided, else from `~/.vigil-collector/settings.json`
   ([main.rs](../src/main.rs)). The probe list is the
   exception — re-read on every `/probes` request so operators can edit
   without restart ([probes.rs](../src/server/probes.rs)).

## Configuration

Settings live in `~/.vigil-collector/settings.json`. All keys are optional and
parse failures fall back to defaults rather than refusing to start
([load_settings](../src/config.rs)). Messages about the settings file are
returned as notes and logged once the tracing subscriber is up, since
settings are read before logging can be configured. A UTF-8 byte-order mark
(Notepad, PowerShell 5) is tolerated.

Logs go to `~/.vigil-collector-logs/app.log` (truncated on startup) plus
stdout. `~` is the user's home folder on every platform
(`C:\Users\<you>` on Windows). `RUST_LOG` overrides the configured level if set
([log.rs](../src/log.rs)).

## How to extend

**Adding a metric field:**

1. Add the field to the relevant struct in [snapshot.rs](../src/snapshot.rs)
   and to the appropriate `*View` projection.
2. Compute it inside the matching `metrics/*.rs` collector.
3. Wire the call into [SamplerState::refresh](../src/sampler.rs).
   Decide whether it belongs in every-tick work or one of the slow tiers.
4. If it requires per-tick previous state (e.g., a counter delta), add a
   field to `SamplerState`.

**Adding an HTTP endpoint:**

1. Define the projection in [snapshot.rs](../src/snapshot.rs) if the response
   is a slice of the `Snapshot`.
2. If you want the cache-then-serve pattern, add an `ArcSwap<Bytes>` slot
   to [CachedJson](../src/sampler.rs) and publish it in
   [CachedJson::publish](../src/sampler.rs).
3. Add a handler in [server/http.rs](../src/server/http.rs) that does
   `load()` and returns `raw_json(...)`.
4. Register the route in [server::router](../src/server/mod.rs)
   alongside an `OPTIONS` preflight.

**Changing the tick rate:**

`ws_push_interval` in `settings.json`. The sampler clamps to a 100 ms floor
([sampler.rs](../src/sampler.rs)). The same value drives the WS
push cadence ([AppState::push_interval](../src/server/mod.rs)).
