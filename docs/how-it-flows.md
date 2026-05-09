# How it flows

A concrete trace from `cargo run` to bytes arriving on a WebSocket client.

- For the structural map, see [ARCHITECTURE.md](./ARCHITECTURE.md).
- For the runtime model, see [runtime-semantics.md](./runtime-semantics.md).
- For the sampler internals, see [the-sampler.md](./the-sampler.md).

## Boot

```
$ vigil-collector --port 4500
```

1. `#[tokio::main(flavor = "current_thread")]` builds a single-threaded
   runtime and enters
   [`async fn main`](../src/main.rs#L37).
2. [`Cli::parse()`](../src/cli.rs#L9) reads CLI flags. Hub flags also read
   from `VIGIL_COLLECTOR_*` env vars via clap's `env` feature.
3. [`config::read_settings()`](../src/config.rs#L130-L166) loads
   `~/.vigil-collector/settings.json`. On first run it creates the file with
   defaults; on parse failure it logs and falls back to defaults so the
   server still starts. A legacy `~/.psmonitor/` directory is renamed
   forward.
4. [`log::init`](../src/log.rs#L40) creates `~/.vigil-collector-logs/`,
   truncates `app.log`, and registers stdout + file `tracing` layers. The
   level comes from `settings.log_level` unless `RUST_LOG` is set.
5. CLI args override settings: empty `--address` falls back to settings;
   `--port 0` falls back to settings ([main.rs:43-55](../src/main.rs#L43-L55)).
6. [`resolve_addr`](../src/main.rs#L19-L29) handles `localhost`, raw IPs,
   and hostname lookup uniformly.
7. [`Sampler::new`](../src/sampler.rs#L186-L195) builds the sampler.
   [`sampler.start()`](../src/sampler.rs#L209-L261) seeds an initial
   snapshot **synchronously** before spawning the tick task — this is what
   prevents the first connecting client from seeing zero values.
8. The hub branch ([main.rs:60-81](../src/main.rs#L60-L81)) constructs a
   `HubClient` if both `--hub` and `--hub-key` are present, warns and
   continues if exactly one is present, otherwise stays silent. Skipped in
   this doc.
9. A shared [`Notify`](../src/main.rs#L84) is created. One spawned task
   waits on `ctrl_c` or SIGTERM ([wait_for_signal](../src/main.rs#L106-L129))
   and calls `notify_waiters()` on it. The server gets the same handle and
   passes a future that awaits it to `axum::serve(...).with_graceful_shutdown(...)`.
10. [`server::serve`](../src/server/mod.rs#L45-L76) binds the TCP listener
    and runs until shutdown.

After `serve` returns, hub and sampler are stopped explicitly so their
tasks drop cleanly before process exit.

## Each sampler tick

```
loop {
    select { stop.notified() => break, sleep(interval) => () }
    state = spawn_blocking(|| state.refresh()).await
    json.publish(&snap)
    snap_slot.store(Arc::new(snap))
}
```

Walked in detail:

1. [`tokio::select!`](../src/sampler.rs#L229-L232) races the shutdown
   `Notify` against the interval sleep. Either fires; on shutdown we break.
2. The `SamplerState` is moved into [`spawn_blocking`](../src/sampler.rs#L237-L242).
   This is a `tokio::task::JoinHandle` returning the new `Snapshot` plus the
   state, which we move back into the loop variable. Boxing the state and
   shuttling it through the join keeps it `Send` while preserving
   single-owner semantics — see
   [runtime-semantics.md](./runtime-semantics.md#why-state-is-boxed-and-shuttled).
3. [`refresh()`](../src/sampler.rs#L71-L143) does the tiered sysinfo work
   (full per-metric breakdown in [the-sampler.md](./the-sampler.md)) and
   returns a `Snapshot`.
4. [`CachedJson::publish`](../src/sampler.rs#L164-L174) projects the
   snapshot into `HttpSystemView` / `WsSystemView` / `NetworkView` (zero-copy
   borrowed views — see [snapshot.rs:113-184](../src/snapshot.rs#L113-L184)),
   serializes each to `Vec<u8>`, and stores it in the matching `ArcSwap`.
5. The full `Snapshot` is also stored in `snap_slot` for hub mode.

If a tick errors (only path: `spawn_blocking` join failure, e.g. panic), the
sampler logs and **rebuilds state from scratch**
([sampler.rs:253-255](../src/sampler.rs#L253-L255)). The previous snapshot
stays visible to readers until the next successful tick. This is deliberate:
the sampler must never go silent.

## A `GET /system` request

```
TCP accept -> axum router -> http::system handler -> response
```

1. axum picks the route from the [router](../src/server/mod.rs#L27-L43) and
   extracts `State<AppState>`. The state is cheap to clone (a few `Arc`s and
   an atomic counter).
2. [`http::system`](../src/server/http.rs#L66-L69) calls
   `state.sampler_json.http_system.load_full()`. This is an atomic acquire
   load returning `Arc<Vec<u8>>`.
3. We clone the bytes out of the `Arc` (a `memcpy` of the JSON buffer; the
   `Arc` itself goes back to the cache), wrap in a `Body`, set the
   `Content-Type` header, and return.

`/network` is identical against the `network` slot. Both `/system` and
`/network` have an `OPTIONS` preflight returning `204` for CORS.

## A `GET /probes` request

This is the only handler that does work per-request.

1. [`probes::run`](../src/server/probes.rs#L27-L65) re-reads
   `settings.json` so the operator can edit the probe list without
   restarting.
2. A fresh `reqwest::Client` is built with the current timeout.
3. For each probe, [`probe_one`](../src/server/probes.rs#L67-L107) runs in
   parallel via `futures_util::future::join_all`:
   - Try `HEAD url`.
   - If status `405`, retry with `GET`.
   - Return `{ name, url, ok, status_code, latency_ms, error }`.

`ok` is `200..400` — 3xx counts as healthy because most probe targets
redirect HTTP to HTTPS.

## A WebSocket session

```
client                              server
  |                                   |
  |-- POST /worker ------------------>|  http::create_worker
  |<- {id, url} ----------------------|  workers.issue() + 5s expiry task
  |                                   |
  |-- GET /connect?id=... ----------->|  ws::connect
  |                                   |    workers.claim(id) -> bool
  |                                   |    active_ws < max ?
  |<= 101 Switching Protocols ========|    ws.on_upgrade()
  |                                   |
  |<- text frame (ws_system json) ----|  push_loop iter 1
  |<- text frame (ws_system json) ----|  push_loop iter 2
  |              ...                  |
  |-- close --------------------------|
  |                                   |    socket.recv() returns None
  |                                   |    active_ws--, socket.close()
```

### `POST /worker` ([http.rs:47-64](../src/server/http.rs#L47-L64))

[`WorkerRegistry::issue`](../src/server/worker.rs#L28-L41) generates 32 bytes
from `rand::thread_rng()`, base64url-encodes them, inserts into a
`Mutex<HashMap>`, and spawns a 5-second expiry task that removes the entry.
The handler also inspects the `Host` header to pre-build the `ws://...` URL
so the client doesn't have to.

### `GET /connect` ([ws.rs:16-40](../src/server/ws.rs#L16-L40))

Three rejection paths before upgrade:

- Empty `id` query param → `400 BAD_REQUEST`.
- `workers.claim(&id)` returns false (token unknown or expired) → `403 FORBIDDEN`.
- `active_ws >= max_ws_connections` → `503 SERVICE_UNAVAILABLE`.

`claim` removes the entry atomically — single-use semantics fall out of
that. Otherwise the connection is upgraded and `run_session` is spawned by
axum's WS layer.

### Push loop ([ws.rs:52-85](../src/server/ws.rs#L52-L85))

```rust
loop {
    let started = Instant::now();
    let bytes = state.sampler_json.ws_system.load_full();
    socket.send(Message::Text(String::from_utf8(bytes.clone())?)).await?;

    let wait = interval.saturating_sub(started.elapsed());
    let mut sleep = pin!(tokio::time::sleep(wait));
    loop {
        select! {
            biased;
            _ = sleep.as_mut() => break,
            msg = socket.recv() => match msg {
                Some(Ok(Message::Close(_))) | None => return Ok(()),
                Some(Ok(_))   => continue,
                Some(Err(e))  => return Err(e.into()),
            }
        }
    }
}
```

Two subtle bits:

- `biased;` — without it, `select!` picks a branch at random when both are
  ready. We always want the sleep to win when due, otherwise a chatty client
  could starve the push cadence.
- `started.elapsed()` is subtracted from the interval before sleeping, so a
  slow `send` doesn't cause cumulative drift in cadence.

Connection counting is symmetric: increment on entering `run_session`,
decrement on exit ([ws.rs:42-50](../src/server/ws.rs#L42-L50)).

## Shutdown

1. User sends Ctrl-C or `SIGTERM`.
2. [`wait_for_signal`](../src/main.rs#L106-L129) returns and the signal task
   logs `Shutting down gracefully...` and calls
   `shutdown.notify_waiters()`.
3. axum's `with_graceful_shutdown` future resolves; axum stops accepting,
   drains in-flight requests, and lets the WS push loops finish their
   current iteration. The push loops detect close on the next `recv` or on
   the connection drop.
4. `axum::serve(...)` returns; back in `main`,
   [`hub_client.stop()`](../src/main.rs#L98-L100) and
   [`sampler.stop()`](../src/sampler.rs#L263-L268) are called. Stopping the
   sampler notifies its loop and aborts the join handle.
5. `main` returns; the runtime shuts down; the process exits.

If the user sends a second signal during graceful shutdown, the kernel kills
the process — no special handling needed because step 3 doesn't block
indefinitely under normal conditions.
