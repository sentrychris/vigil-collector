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
├── cli.rs               clap parser for command-line flags
├── config.rs            settings.json load + first-run defaults + migration
├── log.rs               tracing setup (stdout + file, classic timestamp)
├── snapshot.rs          shared data model + per-endpoint projections
├── sampler.rs           1 Hz tick loop, refresh state, ArcSwap JSON cache
├── metrics/             per-domain collectors (stateless over sysinfo)
│   ├── cpu.rs           usage, freq, package temp, load average
│   ├── mem.rs           total/used/free/percent (psutil-compatible)
│   ├── disk.rs          root + all mounts + /proc/diskstats deltas
│   ├── net.rs           throughput delta + per-iface stats + /proc/net/dev
│   ├── platform.rs      distro, kernel, uptime string, current user
│   └── process.rs       top-N by memory, PSS-or-RSS detection
├── server/              axum HTTP + WS layer
│   ├── mod.rs           router, AppState, serve()
│   ├── http.rs          GET /system, /network, /probes, embedded /, /favicon
│   ├── ws.rs            WebSocket upgrade and per-client push loop
│   ├── worker.rs        single-use 5-second token registry for WS handshake
│   └── probes.rs        external HTTP health checks (parallel reqwest)
└── hub/                 outbound push to a Vigil Pro hub (optional)
    ├── mod.rs
    ├── client.rs
    ├── flatten.rs
    └── protocol.rs
```

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
       ArcSwap<Vec<u8>>   ArcSwap<Vec<u8>>   ArcSwap<Vec<u8>>
              |                   |                   |
       GET /system          WS /connect          GET /network
              v                   v                   v
        load_full ->         load_full ->        load_full ->
        send bytes          send frame          send bytes
```

Every read path — HTTP handler or WebSocket push — is one atomic pointer load
and one byte-buffer copy. No serialization, no syscalls, no locks.

The three buffers exist because the wire formats differ:

- `http_system` — full payload for `GET /system` ([HttpSystemView](../src/snapshot.rs#L114-L126)).
- `ws_system` — slimmer WebSocket frame; `uptime` is top-level, no `disks` /
  `user` / `platform` ([WsSystemView](../src/snapshot.rs#L130-L140)).
- `network` — interface list + per-iface stats for `GET /network`
  ([NetworkView](../src/snapshot.rs#L143-L147)).

`/probes` is the only handler that does work per-request: it issues live HTTP
HEAD/GET checks against the configured probe URLs.

## Process model

```
            Tokio current-thread runtime
            |
            +-- main task ------------ awaits server, then stops sampler/hub
            |
            +-- signal task ---------- ctrl_c / SIGTERM, fires shutdown Notify
            |
            +-- sampler task --------- 1 Hz; each tick spawn_blocking(refresh)
            |
            +-- hub task (optional) -- outbound WS reconnect loop
            |
            +-- server task (axum) --- accepts; per-conn task on upgrade
            |       |
            |       +-- WS push_loop -- per-client; load_full + send + recv
            |
            +-- worker expiry tasks -- one per token, sleeps 5 s, drops entry
            |
            +-- blocking pool -------- sampler refresh, statvfs, /proc reads

```

Single runtime thread for everything async. Heavy syscalls go to the blocking
pool so the runtime thread stays unblocked.

## Critical invariants

These are the load-bearing properties of the system. Breaking any of them is
a regression even if tests still pass.


1. **The sampler must never die.** Loss of the tick task means stale buffers
   forever and no log signal. Errors during refresh rebuild state from
   scratch and continue ([sampler.rs:248-256](../src/sampler.rs#L248-L256)).

2. **Read paths never call sysinfo.** All per-metric work happens inside the
   sampler's `spawn_blocking` closure. HTTP/WS handlers only do `load_full`
   on `ArcSwap`. If you find yourself adding a metric read in a handler,
   stop and add it to the sampler instead.

3. **Per-tick allocation is bounded.** Three `Vec<u8>` allocations per tick
   regardless of client count. Old buffers free as readers drop their `Arc`
   clones. Steady-state heap does not scale with concurrent clients.

4. **CLI flags override settings on startup, not at runtime.** `address` and
   `port` come from CLI if provided, else from `~/.vigil-collector/settings.json`
   ([main.rs:43-55](../src/main.rs#L43-L55)). The probe list is the
   exception — re-read on every `/probes` request so operators can edit
   without restart ([probes.rs:27-31](../src/server/probes.rs#L27-L31)).

## Configuration

Settings live in `~/.vigil-collector/settings.json`. All keys are optional and
parse failures fall back to defaults rather than refusing to start
([read_settings](../src/config.rs#L130-L166)).

Logs go to `~/.vigil-collector-logs/app.log` (truncated on startup) plus
stdout. `RUST_LOG` overrides the configured level if set
([log.rs:62](../src/log.rs#L62)).

## How to extend

**Adding a metric field:**

1. Add the field to the relevant struct in [snapshot.rs](../src/snapshot.rs)
   and to the appropriate `*View` projection.
2. Compute it inside the matching `metrics/*.rs` collector.
3. Wire the call into [SamplerState::refresh](../src/sampler.rs#L71-L143).
   Decide whether it belongs in every-tick work or one of the slow tiers.
4. If it requires per-tick previous state (e.g., a counter delta), add a
   field to `SamplerState`.

**Adding an HTTP endpoint:**

1. Define the projection in [snapshot.rs](../src/snapshot.rs) if the response
   is a slice of the `Snapshot`.
2. If you want the cache-then-serve pattern, add an `ArcSwap<Vec<u8>>` slot
   to [CachedJson](../src/sampler.rs#L147-L175) and publish it in
   [CachedJson::publish](../src/sampler.rs#L164-L174).
3. Add a handler in [server/http.rs](../src/server/http.rs) that does
   `load_full()` and returns `raw_json(...)`.
4. Register the route in [server::router](../src/server/mod.rs#L27-L43)
   alongside an `OPTIONS` preflight.

**Changing the tick rate:**

`ws_push_interval` in `settings.json`. The sampler clamps to a 100 ms floor
([sampler.rs:187](../src/sampler.rs#L187)). The same value drives the WS
push cadence ([AppState::push_interval](../src/server/mod.rs#L22)).

## Hub mode (skipped here)

The collector can optionally push samples to a Vigil Pro hub via an outbound
WebSocket — see [hub/](../src/hub/) and the README's
[Vigil Pro hub push](../README.md#vigil-pro-hub-push-optional) section. This
is independent of the local server: the hub task and the local server task
share only the sampler's `Arc<ArcSwap<Snapshot>>` handle. Disabling hub mode
(no `--hub` flag) drops the entire push branch with zero overhead. Detailed
documentation lives outside this set of docs for now.
