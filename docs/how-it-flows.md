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
   [`async fn main`](../src/main.rs).
2. [`Cli::parse()`](../src/cli.rs) reads CLI flags (`--address`,
   `--port`).
3. [`config::load_settings()`](../src/config.rs) loads
   `~/.vigil-collector/settings.json`. On first run it creates the file with
   defaults; on parse failure it falls back to defaults so the server still
   starts. A legacy `~/.psmonitor/` directory is renamed forward. Anything
   worth reporting comes back as a list of notes, because logging isn't set
   up yet.
4. [`log::init`](../src/log.rs) captures the local UTC offset (while the
   process is still single-threaded), creates `~/.vigil-collector-logs/`,
   truncates `app.log`, and registers stdout + file `tracing` layers. The
   level comes from `settings.log_level` unless `RUST_LOG` is set. Colour
   is used only when stdout is a terminal and `NO_COLOR` is unset; on
   Windows it also turns on virtual-terminal processing for the console.
   The settings notes from step 3 are logged now.
5. The ring crypto provider is installed for rustls (reqwest is built
   without one, see Cargo.toml).
6. CLI args override settings: a missing `--address` or `--port` falls back
   to settings ([main.rs](../src/main.rs)).
7. [`resolve_addr`](../src/main.rs) handles `localhost`, raw IPs,
   and hostname lookup uniformly.
8. [`Sampler::new`](../src/sampler.rs) builds the sampler.
   [`sampler.start()`](../src/sampler.rs) seeds an initial
   snapshot **synchronously** before spawning the tick task — this is what
   prevents the first connecting client from seeing zero values. It fails
   only if sysinfo can't initialise on this platform at all.
9. A shared [`Notify`](../src/main.rs) is created. One spawned task
   waits on Ctrl-C or SIGTERM on Unix, or on Ctrl-C, Ctrl-Break, console
   close, logoff and shutdown events on Windows
   ([wait_for_signal](../src/main.rs)), and calls `notify_one()` on it
   (which stores a permit, so an early signal isn't lost). The server gets the same handle and
   passes a future that awaits it to `axum::serve(...).with_graceful_shutdown(...)`.
10. [`server::serve`](../src/server/mod.rs) binds the TCP listener
    and runs until shutdown.

After `serve` returns, the sampler is stopped explicitly so its task drops
cleanly before process exit.

## Each sampler tick

```
let mut ticker = interval(interval);   // MissedTickBehavior::Delay
loop {
    select { stop.notified() => break, ticker.tick() => () }
    state = spawn_blocking(|| state.refresh()).await
    json.publish(&snap)
}
```

Walked in detail:

1. [`tokio::select!`](../src/sampler.rs) races the shutdown
   `Notify` against the interval ticker. Either fires; on shutdown we break.
   A `tokio::time::Interval` (rather than a fresh `sleep` per tick) keeps
   the cadence steady: the refresh time doesn't add to the period.
2. The `SamplerState` is moved into [`spawn_blocking`](../src/sampler.rs).
   This is a `tokio::task::JoinHandle` returning the new `Snapshot` plus the
   state, which we move back into the loop variable. Boxing the state and
   shuttling it through the join keeps it `Send` while preserving
   single-owner semantics — see
   [runtime-semantics.md](./runtime-semantics.md#why-state-is-boxed-and-shuttled).
3. [`refresh()`](../src/sampler.rs) does the tiered sysinfo work
   (full per-metric breakdown in [the-sampler.md](./the-sampler.md)) and
   returns a `Snapshot`.
4. [`CachedJson::publish`](../src/sampler.rs) projects the
   snapshot into `HttpSystemView` / `WsSystemView` / `NetworkView` (zero-copy
   borrowed views — see [snapshot.rs](../src/snapshot.rs)),
   serializes each, and stores it as `Bytes` in the matching `ArcSwap`.

If a tick errors (only path: `spawn_blocking` join failure, e.g. panic in a
debug build; release builds abort on panic), the sampler logs and
**rebuilds state from scratch** on the next tick
([sampler.rs](../src/sampler.rs)). The previous snapshot
stays visible to readers until the next successful tick. This is deliberate:
the sampler must never go silent.

## A `GET /system` request

```
TCP accept -> axum router -> http::system handler -> response
```

1. axum picks the route from the [router](../src/server/mod.rs) and
   extracts `State<AppState>`. The state is cheap to clone (a few `Arc`s and
   an atomic counter).
2. [`http::system`](../src/server/http.rs) calls
   `state.sampler_json.http_system.load()`. This is an atomic acquire
   load returning a guard over `Arc<Bytes>`.
3. We clone the `Bytes` (a refcount bump, no copy of the JSON), wrap it in
   a `Body`, set the `Content-Type` header, and return.

`/network` is identical against the `network` slot. Both `/system` and
`/network` have an `OPTIONS` preflight returning `204` for CORS.

## A `GET /probes` request

This is the only handler that does work per-request.

1. [`probes::run`](../src/server/probes.rs) re-reads
   `settings.json` so the operator can edit the probe list without
   restarting.
2. The shared `reqwest::Client` from `AppState` is reused (connection pool
   included); the current timeout is applied per request.
3. For each probe, [`probe_one`](../src/server/probes.rs) runs in
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
  |                                   |    reserve slot (active_ws < max)
  |                                   |    workers.claim(id) -> bool
  |<= 101 Switching Protocols ========|    ws.on_upgrade()
  |                                   |
  |<- text frame (ws_system json) ----|  push_loop iter 1
  |<- text frame (ws_system json) ----|  push_loop iter 2
  |              ...                  |
  |-- close --------------------------|
  |                                   |    socket.recv() returns Close
  |<- close --------------------------|    socket.close() completes handshake
  |                                   |    slot dropped -> active_ws--
```

### `POST /worker` ([http.rs](../src/server/http.rs))

[`WorkerRegistry::issue`](../src/server/worker.rs) generates 32 bytes
from the OS CSPRNG (`getrandom`), base64url-encodes them, inserts into a
`Mutex<HashSet>`, and spawns a 5-second expiry task that removes the entry.
The handler also inspects the `Host` header to pre-build the `ws://...` URL
so the client doesn't have to.

### `GET /connect` ([ws.rs](../src/server/ws.rs))

Three rejection paths before upgrade:

- Empty `id` query param → `400 BAD_REQUEST`.
- No free slot (`active_ws >= max_ws_connections`) → `503
  SERVICE_UNAVAILABLE`. Checked before the claim, so a client turned away
  for capacity keeps its token and can retry within the 5-second window.
- `workers.claim(&id)` returns false (token unknown or expired) → `403 FORBIDDEN`.

The slot is reserved with a compare-and-swap (`fetch_update`), so
concurrent connects can't overshoot the cap, and it's released by `Drop`
whether the session ends normally, errors, or the upgrade never completes.
`claim` removes the entry atomically — single-use semantics fall out of
that. Otherwise the connection is upgraded and `run_session` is spawned by
axum's WS layer.

### Push loop ([ws.rs](../src/server/ws.rs))

```rust
loop {
    if *closing.borrow_and_update() { return going_away(socket).await; }
    let started = Instant::now();
    let bytes = Bytes::clone(&state.sampler_json.ws_system.load());
    socket.send(Message::Text(Utf8Bytes::try_from(bytes)?)).await?;

    let wait = interval.saturating_sub(started.elapsed());
    let mut sleep = pin!(tokio::time::sleep(wait));
    loop {
        select! {
            biased;
            _ = sleep.as_mut() => break,
            _ = closing.changed() => return going_away(socket).await,
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

Connection counting is symmetric: a `Slot` guard is reserved in `connect`
and released when `run_session` drops it ([ws.rs](../src/server/ws.rs)).

## Shutdown

1. User sends Ctrl-C or `SIGTERM` (Unix), or Ctrl-C / Ctrl-Break / closes
   the console window / logs off / shuts down (Windows).
2. [`wait_for_signal`](../src/main.rs) returns and the signal task
   logs `Shutting down gracefully...` and calls
   `shutdown.notify_one()`.
3. axum's `with_graceful_shutdown` future resolves and flips the `closing`
   watch channel; axum stops accepting and drains in-flight requests. Each
   WS push loop sees `closing` change and sends Close `1001 Going Away`.
4. `server::serve` waits up to 2 s for the WS sessions to finish their
   close handshakes (upgraded connections aren't tracked by axum's graceful
   shutdown; 2 s stays inside the ~5 s Windows allows after a console close
   or shutdown event).
5. Back in `main`, [`sampler.stop()`](../src/sampler.rs) is called. Stopping the
   sampler notifies its loop and aborts the join handle.
6. `main` returns; the runtime shuts down; the process exits.

If the user sends a second signal during graceful shutdown, the kernel kills
the process — no special handling needed because step 3 doesn't block
indefinitely under normal conditions.
