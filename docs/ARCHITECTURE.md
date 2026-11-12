# Architecture

This document is the canonical map of the Vigil codebase (the `vigil-collector` repository). It
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
├── main.rs              entry point; wires runtime, signals, supervisor, listeners, tray
├── supervisor.rs        owns the sampler and the public listener; start/stop/restart/apply
├── cli.rs               clap parser for command-line flags (--address, --port, --foreground)
├── config.rs            settings.json load + first-run defaults + migration
├── log.rs               tracing setup (stdout + file, classic timestamp, Windows ANSI)
├── tray.rs              Windows: launcher hand-off and the notification-area icon + menu
├── window.rs            Windows: the Vigil window, a WebView2 control on the tray thread
├── window_none.rs       its stand-in where the window isn't built (MinGW without the feature)
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
│   ├── mod.rs           public and private routers, AppState, serve()
│   ├── control.rs       /control API behind the Settings page (private listener only)
│   ├── http.rs          GET /system, /network, /probes; embedded dashboard fallback
│   ├── ws.rs            WebSocket upgrade and per-client push loop
│   ├── worker.rs        single-use 5-second token registry for WS handshake
│   └── probes.rs        external HTTP health checks (parallel reqwest)
build.rs                 runs `npm ci` / `vite build` in web/ so the binary can embed web/dist
                         (and, for Windows targets, gives the .exe its icon and version details)
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
            +-- main task ------------ runs the supervisor: handles commands
            |                          (start, stop, restart, apply, quit)
            |                          until a signal or Quit, then stops
            |                          the server and the sampler
            |
            +-- signal task ---------- ctrl_c / SIGTERM (Unix) or console
            |                          control events (Windows); fires the
            |                          shutdown Notify
            |
            +-- sampler task --------- 1 Hz; each tick spawn_blocking(refresh)
            |
            +-- server task (axum) --- accepts; per-conn task on upgrade.
            |       |                  Ended and started again by the
            |       |                  supervisor; absent while stopped
            |       +-- WS push_loop -- per-client; load + send + recv
            |
            +-- private server task -- the window's own listener (Windows
            |                          tray, or VIGIL_CONTROL_PORT); lives
            |                          for the whole run
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
hidden window, the notification-area icon, the Vigil window when it's
open, and the Win32 message loop. Menu choices reach the supervisor as
`supervisor::Command`s over an unbounded channel, the same one the
control API sends on; a task in `main` watches the supervisor's status
and tells the tray when the address changes or the server stops. The
menu's connection count is the server's `active_ws` counter, which `main`
creates and shares with both.
A double-clicked launcher also sets `VIGIL_FIND_FREE_PORT=1`, which lets
`server::bind` move past a taken port (without `--port`) instead of
failing; `main` binds before starting the tray so the tooltip and
double-click URL show the port actually used.
Every Windows copy except the launcher claims the named mutex
`Global\VigilCollector` before opening the log, so only one runs per
computer; the launcher only probes it, and when it's taken asks the
running copy's tray window (found by class, URL in its title) to open the
Vigil window. Restart no longer starts a new process: the supervisor
stops the listener, re-reads the settings file and binds again.

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

4. **CLI flags override the settings file until the user chooses
   otherwise.** `address` and `port` come from CLI if provided, else from
   `~/.vigil-collector/settings.json`, and a Restart keeps them. Saving
   an address or port on the Settings page is the one thing that drops
   the matching flag for the rest of the run
   ([Supervisor::apply](../src/supervisor.rs)). The probe list is re-read
   on every `/probes` request so operators can edit without restart
   ([probes.rs](../src/server/probes.rs)).

5. **The control API is never on the public listener.** `/control` is
   mounted only by `server::private_router`; `server::router` answers 404
   for it explicitly, so a later fallback change can't expose it. The
   private listener binds `127.0.0.1` only and checks Host, the per-run
   token and, for cookie sessions, Origin
   ([control.rs](../src/server/control.rs)).

6. **A change that can't be applied changes nothing.** Moving the
   listener opens the new port before closing the old one (or goes back
   to the old address when only the address changed), and the settings
   file is written only after the running server has taken the change.

## The window and the control API

The dashboard can't tell a browser from the Vigil window by itself; what
it can do is ask for `/control/status`. Only the private listener answers,
so only the window gets a Settings link and the lazily loaded Settings
page.

```
 tray thread (Win32)                     tokio runtime
 ┌──────────────────────────┐            ┌─────────────────────────────────┐
 │ icon + menu ─────────────┼─ Command ─▶│ Supervisor ── public listener   │
 │ Vigil window (WebView2)  │            │   ▲   │       address:port      │
 │   └ loads 127.0.0.1:P ───┼── HTTP ───▶│   │   └─ status (watch) ─▶ tray │
 └──────────────────────────┘            │ private listener 127.0.0.1:P    │
                                         │   dashboard + /control (token)  │
                                         └─────────────────────────────────┘
```

- **Supervisor** ([supervisor.rs](../src/supervisor.rs)). One owner for
  the sampler, the settings in use and the public listener. `Start`,
  `Stop`, `Restart`, `Apply` and `Quit` arrive on one channel from the
  tray menu and the control API, so there is no second path that could
  disagree about whether the server is up. The connection cap and push
  interval are atomics that `ws.rs` reads per connection and per frame;
  the log level goes through `tracing_subscriber::reload`.
- **Private listener** ([server::private_router](../src/server/mod.rs)).
  Same routes as the public one plus `/control`, no CORS, its own
  connection counter and cap so the window can't be locked out by the
  public limit and isn't counted in it. It keeps running while the public
  server is stopped, which is what lets "Stop" mean "stop serving others"
  rather than "stop monitoring". Its port is remembered in
  `~/.vigil-collector/window.json` so the WebView2 profile (which is
  keyed by origin) keeps the theme and other saved choices.
- **Session.** The window is navigated to
  `/control/session?token=...`, which sets an `HttpOnly`,
  `SameSite=Strict` cookie and redirects to the dashboard, so the token
  isn't left in the address or the page's scripts. Tools can send
  `Authorization: Bearer` instead.
- **Window** ([window.rs](../src/window.rs)). A plain Win32 window on the
  tray thread hosting a `wry` WebView2 control, created when opened and
  destroyed when closed, so an unopened window costs nothing. Links to
  other sites go to the default browser. If WebView2 isn't installed the
  tray falls back to the browser.

| Route | Does |
|---|---|
| `GET /control/session?token=&next=` | Starts the window's session (303 + cookie) |
| `GET /control/status` | Version, whether and where it's serving, connections, uptime, file paths |
| `GET /control/settings` | The settings file as it is now |
| `PUT /control/settings` | Any subset of keys: validated, applied to the running server, then saved |
| `POST /control/server/{start,stop,restart}` | Same as the tray menu |
| `GET /control/logs?after=N` | Log lines newer than `N` from the in-memory buffer (last 2000) |
| `POST /control/open/{logs,settings,browser}` | Opens the file or the dashboard with the system's default program |
| `POST /control/quit` | Stops Vigil |

Errors are `{"error": "..."}` with 400 (not valid), 409 (couldn't be
applied, for example a port in use) or 500 (applied but not saved).

For development, `VIGIL_CONTROL_PORT` and `VIGIL_CONTROL_TOKEN` start the
private listener on any platform (see the README's Development section).

## Configuration

Settings live in `~/.vigil-collector/settings.json`. All keys are optional and
parse failures fall back to defaults rather than refusing to start
([load_settings](../src/config.rs)). Messages about the settings file are
returned as notes and logged once the tracing subscriber is up, since
settings are read before logging can be configured. A UTF-8 byte-order mark
(Notepad, PowerShell 5) is tolerated.

Logs go to `~/.vigil-collector-logs/app.log` (truncated on startup and
again at 4 MiB), stdout, and an in-memory buffer of the last 2000 lines
for the Settings page. `~` is the user's home folder on every platform
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
