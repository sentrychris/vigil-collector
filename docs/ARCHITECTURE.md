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
├── native/              what Vigil's own windows are drawn with (Slint, no browser)
│   ├── mod.rs           includes the layouts Slint makes of ui/vigil.slint
│   ├── surface.rs       where Slint paints: a block of pixels in memory
│   ├── backdrop.rs      the page behind a window's cards, painted once per size
│   ├── bar.rs           which part of a window's top drags it or resizes it
│   ├── keys.rs          Windows' key presses as Slint's
│   ├── clipboard.rs     what Copy and Paste in a text field use
│   └── shell.rs         Windows: the Win32 window those pixels are shown in
├── compact/             the Vigil window: a compact dashboard
│   ├── view.rs          words the readings; keeps the sparklines' minute of history
│   ├── spark.rs         the sparklines' curves and the tint under them
│   ├── host.rs          Windows: the window, fed from the sampler
│   └── tests.rs         draws the window into a picture on every platform
├── settings/            the Settings window
│   ├── form.rs          what each form comes to: what changed, what is wrong
│   ├── log.rs           the log folded to the width of its box
│   ├── view.rs          fills the layout in; turns its buttons into requests
│   ├── host.rs          Windows: the window; does what is asked through Panel
│   └── tests.rs         draws, presses and types on it on every platform
├── frame.rs             Windows: what all the windows share (no title bar, remembered place)
├── window.rs            Windows: the full dashboard's window, a WebView2 control
├── window_none.rs       its stand-in where it isn't built (MinGW without the feature)
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
│   ├── control.rs       Panel (what Settings changes) and the /control API over it
│   ├── http.rs          GET /system, /network, /probes; embedded dashboard fallback
│   ├── ws.rs            WebSocket upgrade and per-client push loop
│   ├── worker.rs        single-use 5-second token registry for WS handshake
│   └── probes.rs        external HTTP health checks (parallel reqwest)
build.rs                 runs `npm ci` / `vite build` in web/ so the binary can embed web/dist,
                         compiles ui/vigil.slint (the compact window and Settings)
                         (and, for Windows targets, gives the .exe its icon and version details)
ui/                      the compact window's layout (Slint), its icons, logo and fonts
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
hidden window, the notification-area icon, the Vigil window and the full
dashboard's window when they are open, and the Win32 message loop. Menu choices reach the supervisor as
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
   an address or port in Settings is the one thing that drops
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

This is about the window for the full dashboard, which is a web page in
a WebView2 control. (The window the tray icon opens and Settings are
different ones; see [The compact window](#the-compact-window) and
[The Settings window](#the-settings-window).) The dashboard can't tell a
browser from this window by itself; what it can do is ask for
`/control/status`. Only the private listener answers, so only the window
gets a gear. In the window the gear asks the collector for the Settings
window (`settings` over `window.ipc`); the same settings as a page of the
dashboard (`/settings`, lazily loaded) are what the gear leads to where
there is a private listener but no window, which is how they are worked
on (see the README's Development section).

```
 tray thread (Win32)                     tokio runtime
 ┌──────────────────────────┐            ┌─────────────────────────────────┐
 │ icon + menu ─────────────┼─ Command ─▶│ Supervisor ── public listener   │
 │ Full dashboard (WebView2)│            │   ▲   │       address:port      │
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
  destroyed when closed, so an unopened window costs nothing. It has no
  title bar: `WM_NCCALCSIZE` gives that strip to the page, and
  `SiteNavbar.vue` draws the buttons and reports presses on the bar with
  `window.ipc.postMessage` (`drag`, `maximize`, `close`, `resize:top`
  and so on), which the window turns into the same non-client messages a
  real title bar would produce. The side and bottom resize borders, the
  shadow and snapping stay Windows' own. The page learns whether it is
  maximised from a `vigil:window` event. Links to
  other sites go to the default browser. If WebView2 isn't installed the
  tray falls back to the browser. While the window is minimised the
  control is hidden (`show_page`), because WebView2 doesn't otherwise
  tell the page it can't be seen and the charts would go on drawing.
- **The page's own compact layout.** The window opens with room for the
  full dashboard. Made narrower than 900 pixels, the page shows a compact
  dashboard
  ([components/compact](../web/src/components/compact),
  [useCompactLayout.ts](../web/src/composables/useCompactLayout.ts))
  instead of the full one; a browser always gets the full one. This is
  the layout the native compact window (below) was copied from, and what
  the Vigil window was before it. The compact one has no chart loops at
  all: its sparklines are redrawn from `system.history` when a reading
  arrives. Where each window was and how big is kept in `window.json`
  beside the private port (`place` for the compact window, `full` for
  this one), and given back with `SetWindowPlacement`, which also brings
  a window home from a display that has gone.
- **Drawing** ([frame-loop.ts](../web/src/utilities/frame-loop.ts)). A
  monitor shouldn't be the busiest thing it monitors, and a page that
  redraws on every display frame is: it measured about one whole core.
  Everything that moves on a chart draws on one shared clock, thirty
  times a second and all in the same frame, and not at all while the
  page is hidden or the chart is scrolled away. Use `frameLoop` or
  `tween` rather than `requestAnimationFrame` in a chart, keep
  always-running CSS animations to `transform` and `opacity`, and don't
  put `backdrop-filter` on anything a chart sits inside (it is worked
  out again for the whole element on every frame drawn within it).

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

## The compact window

The window the tray icon opens ([src/compact/](../src/compact/mod.rs)) is
not a web page. It is the same picture as the page's compact layout,
laid out by [Slint](https://slint.dev) from
[ui/compact.slint](../ui/compact.slint) and painted by Slint's software
renderer, with no WebView2, no GPU and no windowing library.

```
 sampler tick ── watch::Sender<Arc<Snapshot>> ──┐          (runtime thread)
                                                │ a task posts WM_READINGS
                                                ▼
 tray thread:  host.rs window procedure ── view.rs sets the layout's properties
                    │  WM_PAINT                    │ Slint marks what changed
                    ▼                              ▼
              SetDIBitsToDevice  ◀── surface.rs: SoftwareRenderer paints the
              (one copy to screen)   changed part of a kept BGRX buffer
```

- **No windowing backend.** Slint is built with only `renderer-software`
  (see Cargo.toml), so winit and the GPU renderers are not compiled in.
  `surface.rs` gives Slint a `Platform` with a clock and a
  `WindowAdapter` that is just a size, a scale and a dirty flag. The
  renderer paints into pixels laid out as a Windows 32-bit bitmap, so
  `host.rs` hands the buffer to GDI unconverted.
- **Paints on change.** Slint tracks which properties a frame read. The
  window is invalidated when one of them changes, which while it sits
  there is once per reading; the renderer then repaints only the changed
  regions. A timer runs Slint's clock only while an animation (a switch
  sliding) is in progress. Minimised, readings still go into the
  sparklines' history but nothing is laid out or painted.
- **Readings without JSON.** The sampler keeps its newest `Snapshot` in a
  `watch` channel beside the JSON cache. A task on the runtime thread
  posts the window a message per reading and the window reads the
  snapshot itself. Service checks run on the runtime thread
  (`probes::run`) and come back over a channel the same way.
- **The frame** is shared with the full dashboard's window
  ([frame.rs](../src/frame.rs)): `WM_NCCALCSIZE` removes the title bar,
  and `WM_NCHITTEST` says which parts of the picture stand in for it
  ([bar.rs](../src/native/bar.rs)): the bar drags, its top edge resizes.
- **Shared with Settings.** Everything above that isn't about readings is
  in [src/native/](../src/native/mod.rs): `surface.rs`, and `shell.rs`,
  the Win32 window with its painting, mouse, keyboard and hit testing.
  `host.rs` here keeps a `Shell` and answers only the messages that are
  the compact window's own.
- **What Slint's software renderer doesn't do**, and how the layout gets
  by: no gradients inside paths (the tint under a sparkline is a small
  picture painted in `spark.rs`), no drop shadows and no clipping to
  rounded corners (the glows around the web version's icons are left
  out).
- **The page behind the cards** is a picture too
  ([backdrop.rs](../src/native/backdrop.rs)), painted when the window's
  size or theme changes. As five overlapping round gradients in the
  layout, it was worked out again for every repainted pixel and was
  nearly all of the cost of a reading: about 18 ms at 150% scaling on a
  2.1 GHz cloud processor, against about 2.5 ms with the picture.
- **Fonts and icons** are built in: Montserrat, IBM Plex Mono and Lato
  ([ui/fonts](../ui/fonts/README.md), Open Font License) and outlines
  from Font Awesome Free ([ui/icons.slint](../ui/icons.slint), CC BY 4.0).
- **Theme.** The theme and whether it is live or static (a switch in
  Settings, which tells this window: `compact::set_live`) are kept in
  `window.json`. The full dashboard's page starts in the same theme
  (`start_in_theme` in window.rs) and each tells the other when its
  switch is used (`theme:` over `window.ipc`, a `vigil:theme` event
  back).
- **Tests.** `tests.rs` draws the window headlessly on every platform and
  checks pixels (`VIGIL_PREVIEW_DIR` saves the pictures). On Windows,
  `host.rs` also has a test that opens the real window with made-up
  readings, paints it, sends it the mouse and closes it.
- **Licence.** Slint is used under its royalty-free licence, whose
  condition is the "Made with Slint" badge on the project's public page
  (it is in the README). Vigil's own code stays MIT.

## The Settings window

Settings ([src/settings/](../src/settings/mod.rs),
[ui/settings.slint](../ui/settings.slint)) is the second window drawn
this way: the web Settings page's sections as tabs, at the compact
window's size and in its style. It needs no WebView2 and no private
listener.

```
 tray thread                                         tokio runtime
 ┌────────────────────────────────────────┐          ┌──────────────────────┐
 │ settings.slint ── press ─▶ view.rs     │          │                      │
 │      ▲                      │ Request  │          │                      │
 │      │ properties           ▼          │  spawn   │ Panel::change/server │
 │   view.rs ◀─ WM_FINISHED ─ host.rs ────┼─────────▶│   └─▶ Supervisor     │
 │      ▲                      │          │◀─ result ┤                      │
 │      └── once a second: Panel::status, │ (channel │                      │
 │          log::lines_after              │ + post)  │                      │
 └────────────────────────────────────────┘          └──────────────────────┘
```

- **One way in.** `Panel` ([control.rs](../src/server/control.rs)) is what
  Settings shows and changes: status, `change`, `server`, `open`, `quit`.
  The window calls it directly; the `/control` routes are thin wrappers
  around the same calls. So a change is validated, applied and saved the
  same way whichever asked for it.
- **The rules are the page's.** `form.rs` works out, for each form, what
  differs from what is saved, what is wrong, and the JSON to send, with
  the page's wording. The layout asks for those verdicts through `pure`
  callbacks, so they are worked out again exactly when a field or the
  saved state changes.
- **Nothing blocks the tray thread.** A press becomes a `Request`, which
  the window is posted (a press arrives in the middle of Slint handling
  the mouse). Changes and server actions wait on the supervisor on the
  runtime thread and come back over a channel and a posted message;
  opening a file or folder is done at once.
- **The log** is drawn a row at a time. `log.rs` folds the kept lines to
  the number of characters that fit (the font is fixed-width) and the
  layout asks only for the rows in view, so two thousand lines cost the
  same to draw as twenty. Scrolling up stops it following.
- **Typing.** The fields are Slint's own `TextInput` in a box
  ([settings.slint](../ui/settings.slint)). `shell.rs` passes on
  `WM_KEYDOWN` for the keys that type nothing and for Ctrl with a letter,
  and `WM_CHAR` for everything typed ([keys.rs](../src/native/keys.rs));
  with AltGr held it tells Slint that Ctrl and Alt are up for the length
  of the character, or Slint would take it for a shortcut. Copy and Paste
  reach the Windows clipboard through the `Platform`
  ([clipboard.rs](../src/native/clipboard.rs)). Slint has no way for a
  custom platform to set the pointer, so the layout counts the fields
  under it and the window shows the I-beam on `WM_SETCURSOR`.
- **Tests.** `tests.rs` draws every tab headlessly, types into fields,
  presses buttons and checks what was asked for and what is shown
  afterwards. On Windows, `host.rs` opens the real window with a stand-in
  supervisor, types into it with `WM_CHAR`, and has the server stopped
  from the other thread. Neither touches the settings file.

## Configuration

Settings live in `~/.vigil-collector/settings.json`. All keys are optional and
parse failures fall back to defaults rather than refusing to start
([load_settings](../src/config.rs)). Messages about the settings file are
returned as notes and logged once the tracing subscriber is up, since
settings are read before logging can be configured. A UTF-8 byte-order mark
(Notepad, PowerShell 5) is tolerated.

Logs go to `~/.vigil-collector-logs/app.log` (truncated on startup and
again at 4 MiB), stdout, and an in-memory buffer of the last 2000 lines
for Settings. `~` is the user's home folder on every platform
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
