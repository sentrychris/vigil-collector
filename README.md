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
| Single `/system` request | **0.7 ms** | 1.3 ms | ~2× |
| `ab -n 3000 -c 20 /system` throughput | **23,277 req/s** | 2,875 req/s | ~8× |
| Mean latency at concurrency 20 | **0.9 ms** | 7.0 ms | ~8× |
| Failed requests at conc 20 | **0** | 2,613 | — |
| Resident memory after warmup | **9 MB** | 34 MB | ~4× |
| Stripped release binary | **5.7 MB** | (PyInstaller bundle ~30 MB) | ~5× |

These come from the cached-JSON design (no per-request serialization on the hot
path) plus tokio's scheduler not being the GIL.

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
