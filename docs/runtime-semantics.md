# Runtime semantics

How concurrency, ownership, and lifecycle work in Vigil. If you come
to Rust from Python, TypeScript, or C#, this is where the model differs most.

For the structural map, see [ARCHITECTURE.md](./ARCHITECTURE.md). For the
trace-style lifecycle, see [how-it-flows.md](./how-it-flows.md). For sampler
internals, see [the-sampler.md](./the-sampler.md).

## Tokio runtime: `current_thread` flavor

```rust
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> { ... }
```

[main.rs](../src/main.rs).

The runtime has **one** OS thread. All `tokio::spawn`'d tasks (the signal
watcher, the sampler tick loop, every WebSocket connection, every worker
expiry task, axum's accept loop) cooperatively share that single thread.

Why not the default multi-threaded runtime? The workload is one 1 Hz sampler
plus a low double-digit number of WS connections. A multi-thread scheduler
would allocate one worker thread per CPU core (each with a 2 MB virtual
stack), buying us nothing because there's almost nothing to parallelize. The
README quantifies the saving — see
[Runtime layout](../README.md#runtime-layout).

**The blocking pool is separate.** `tokio::task::spawn_blocking` runs the
provided closure on a pool of OS threads dedicated to blocking work,
returning a `JoinHandle<T>` that the async side awaits. We use this in the
sampler ([sampler.rs](../src/sampler.rs)) so heavy
`/proc` reads, `statvfs` calls and their Win32 equivalents don't stall the
runtime thread.

### Implications for new code

- Anything CPU-bound or syscall-heavy must go through `spawn_blocking`. A
  multi-millisecond stall on the runtime thread is a multi-millisecond stall
  for **every** WS push and HTTP response in flight.
- A panic in any spawned task propagates as a `JoinError` on `await`, not a
  process crash, in debug builds. The sampler explicitly handles this — see
  [sampler.rs](../src/sampler.rs). Release builds set `panic = "abort"`, so
  there a panic ends the process.
- `Send` matters: anything captured by a task running outside the spawning
  task's frame (which includes `spawn_blocking` closures) needs `Send`. See
  [why state is boxed and shuttled](#why-state-is-boxed-and-shuttled) below.

## Concurrency primitives in use

### `Arc<T>` and `Arc<ArcSwap<T>>`

`Arc<T>` is reference-counted shared ownership — analogous to a Python
object reference, or a TS object reference, or a C# class reference. Cloning
an `Arc` bumps a counter; the inner value drops when the last `Arc` does.

`arc_swap::ArcSwap<T>` wraps an `Arc<T>` in an atomic pointer. Readers do
`.load()` (a cheap guard) or `.load_full()` (an owned `Arc<T>`) to get a
snapshot of the current value. Writers do
`.store(Arc::new(new_value))` to atomically replace it. No locks.

The sampler's [`CachedJson`](../src/sampler.rs) is the canonical
use: one writer, many readers, snapshots are large-ish JSON buffers so we
don't want to copy them under a lock. They're stored as `bytes::Bytes`,
itself a refcounted buffer, so a reader clones the `Bytes` out of the guard
and hands it straight to the HTTP body or WebSocket frame without copying.
After `store`, in-flight readers keep their old `Bytes` until they drop it;
the buffer is freed when the last reference goes away.

In TS terms: think of `ArcSwap<T>` as a `BehaviorSubject<T>` where
subscribers receive a frozen reference to the current value. In Python:
roughly an atomic reference swap, with the GC equivalent (Rust's drop)
freeing old values once all observers stop holding them.

### `tokio::sync::Notify`

[`Notify`](https://docs.rs/tokio/latest/tokio/sync/struct.Notify.html) is a
lightweight synchronization primitive. `notify_waiters()` wakes every task
currently `await`ing `notified()`; `notify_one()` wakes one waiter, or
stores a permit so the *next* `notified()` returns immediately. Used in two
places:

- [Shutdown propagation](../src/main.rs) — the signal task calls
  `notify_one()`; the server's graceful-shutdown future is the single
  waiter. The stored permit means a signal that arrives before the server
  starts awaiting isn't lost.
- [Sampler stop](../src/sampler.rs) — `notify_waiters()`, scoped to the
  sampler; the join handle is aborted right after, so a missed wake-up
  doesn't matter.

In TS: think `EventEmitter`. In Python: closer to `asyncio.Event`, except
`Notify` re-arms automatically.

### `tokio::sync::watch` (WebSocket close-on-shutdown)

[`AppState::closing`](../src/server/mod.rs) is a `watch::Receiver<bool>`
that flips to `true` when graceful shutdown starts. Unlike `Notify`, a
`watch` value is sticky: a session that checks it between sends still sees
the change. Each push loop selects on `changed()` and sends Close `1001
Going Away`.

### `std::sync::Mutex` (worker registry)

[`WorkerRegistry`](../src/server/worker.rs) wraps a `HashSet<String>` in a
`std::sync::Mutex`. The critical sections (insert, remove) are O(1) hash
operations and we never hold the lock across an `.await`. A poisoned lock
is recovered (`into_inner`) because an insert or remove can't leave the set
half-updated.

Holding a sync mutex across an `.await` would be a bug on the
`current_thread` runtime: every other task is also on this thread, so a
held lock could deadlock the very task that would release it. Static
analysis (clippy's `await_holding_lock`) flags it; the codebase respects
this rule.

### `std::sync::atomic::AtomicUsize` (active WS counter)

[`active_ws`](../src/server/mod.rs) counts open WS sessions. A slot is
reserved in `ws::connect` with `fetch_update` (compare-and-swap, so
concurrent connects can't overshoot `max_ws_connections`) and released by
the `Slot` guard's `Drop`, so every exit path gives it back.

## Why state is boxed and shuttled

Inside the sampler tick loop ([sampler.rs](../src/sampler.rs)):

```rust
let owned = match state.take() {
    Some(s) => s,
    None => Box::new(SamplerState::new()?),   // rebuild after a failed tick
};
let join = tokio::task::spawn_blocking(move || {
    let mut s = owned;
    let snap = s.refresh();
    (snap, s)
}).await;
match join {
    Ok((snapshot, returned)) => { state = Some(returned); ... }
    Err(e)                   => { /* state stays None; rebuilt next tick */ }
}
```

What's going on:

- `SamplerState` contains sysinfo handles that are not designed to be shared
  across threads simultaneously. We want exactly one owner that mutates it,
  no clones, no locks.
- `spawn_blocking` requires its closure to be `'static + Send`, so we have
  to **move** state into it. After the closure runs, the state needs to
  return to the loop variable for the next tick.
- The pattern: `Option<Box<SamplerState>>` lives in the loop. Each tick
  takes the value out (`Option::take`), moves it into the closure, and the
  closure returns it as part of the result tuple. We then put it back.
- `Box` keeps the move cheap (just a pointer move regardless of struct
  size).

If a tick panics (debug builds), `await` returns `Err(JoinError)`, our `Box`
was eaten by the failed task, and we rebuild fresh state on the next tick
(retrying each tick if sysinfo itself can't initialise). Readers continue seeing the
last-good snapshot until the next tick succeeds.

This pattern is idiomatic in Tokio for "single-threaded mutable state that
occasionally needs the blocking pool". A C# analogy: an `async` method that
hands a mutable struct to `Task.Run`, then awaits and reassigns it on
return.

## Lifetime patterns

### Cheaply-cloneable handle types

Several types in the codebase are designed to be cloned per-request or
per-task:

- [`AppState`](../src/server/mod.rs) — wraps a few `Arc`s and an
  `AtomicUsize`. axum clones it for each handler invocation.
- [`Arc<CachedJson>`](../src/sampler.rs) — handed to the server
  on construction and then read-only forever.
- [`Arc<WorkerRegistry>`](../src/server/mod.rs) — handed to handlers
  via `AppState`.
- [`reqwest::Client`](../src/server/mod.rs) — internally `Arc`'d, so
  cloning shares the connection pool.

If you add a new shared resource, follow the same pattern: hold an `Arc`
inside `AppState`, never put plain owned values there.

### Borrowed views over `Snapshot`

[`HttpSystemView<'a>`](../src/snapshot.rs),
[`WsSystemView<'a>`](../src/snapshot.rs), and
[`NetworkView<'a>`](../src/snapshot.rs) all borrow from a
`Snapshot`. They're constructed via the inherent methods on `Snapshot` and
serialized immediately. The `'a` lifetime means they cannot outlive the
snapshot they borrow from — which is fine because we only build them inside
`CachedJson::publish` for a single call to `serde_json::to_vec`.

This avoids cloning the underlying vectors (`disks`, `processes`, etc.) just
to feed them to serde. The total saving is ~50 KB/tick on a busy host with
many disks and processes.

## Shutdown propagation

Independent tasks need to stop in order:

```
signal task ─ notify_one() ─── shared Notify ─── server graceful_shutdown
                                                        │
                                                 closing watch = true
                                                        │
                                              WS push loops send Close 1001
```

Sequence:

1. The OS delivers SIGINT or SIGTERM (Unix), or a console control event
   (Ctrl-C, Ctrl-Break, close, logoff, shutdown) on Windows.
2. The signal task ([main.rs](../src/main.rs)) returns
   from its `select!` and calls `notify_one()`.
3. axum's graceful-shutdown future resolves and sets `closing`; axum stops
   the TCP accept loop and waits for in-flight requests to finish.
4. `server::serve` waits up to 2 s for WS sessions to finish their close
   handshake (`active_ws` reaching 0), then returns.
5. `main` calls `sampler.stop()`. That call notifies the sampler's *own*
   `Notify` (a different one — the sampler doesn't share the global
   shutdown handle) and aborts the join handle.
6. `main` returns; the runtime drops; OS threads in the blocking pool are
   joined; the process exits.

Upgraded WebSocket connections aren't tracked by axum's graceful shutdown,
which is why they get their own `closing` signal and drain wait. Without
it the runtime would drop them mid-stream and clients would see an
abnormal closure (1006). The sampler keeps publishing until
`sampler.stop()` is called *after* `serve` returns.

## When to break the rules

- **Holding a lock across `.await`**: never. Use `tokio::sync::Mutex` if you
  truly need async-aware mutual exclusion (we currently don't).
- **`spawn_blocking` for trivial work**: don't. The dispatch overhead is
  ~10 µs; only worth it for work that's at least an order of magnitude
  longer.
- **Adding new tasks**: think about shutdown. Either watch `closing` or own
  a registry that the main task can drain.
