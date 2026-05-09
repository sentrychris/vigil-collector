# Runtime semantics

How concurrency, ownership, and lifecycle work in the Collector. If you come
to Rust from Python, TypeScript, or C#, this is where the model differs most.

For the structural map, see [ARCHITECTURE.md](./ARCHITECTURE.md). For the
trace-style lifecycle, see [how-it-flows.md](./how-it-flows.md). For sampler
internals, see [the-sampler.md](./the-sampler.md).

## Tokio runtime: `current_thread` flavor

```rust
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> { ... }
```

[main.rs:36](../src/main.rs#L36).

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
sampler ([sampler.rs:237-242](../src/sampler.rs#L237-L242)) so heavy
`/proc` reads and `statvfs` calls don't stall the runtime thread.

### Implications for new code

- Anything CPU-bound or syscall-heavy must go through `spawn_blocking`. A
  multi-millisecond stall on the runtime thread is a multi-millisecond stall
  for **every** WS push and HTTP response in flight.
- A panic in any spawned task propagates as a `JoinError` on `await`, not a
  process crash. The sampler explicitly handles this — see
  [sampler.rs:248-256](../src/sampler.rs#L248-L256).
- `Send` matters: anything captured by a task running outside the spawning
  task's frame (which includes `spawn_blocking` closures) needs `Send`. See
  [why state is boxed and shuttled](#why-state-is-boxed-and-shuttled) below.

## Concurrency primitives in use

### `Arc<T>` and `Arc<ArcSwap<T>>`

`Arc<T>` is reference-counted shared ownership — analogous to a Python
object reference, or a TS object reference, or a C# class reference. Cloning
an `Arc` bumps a counter; the inner value drops when the last `Arc` does.

`arc_swap::ArcSwap<T>` wraps an `Arc<T>` in an atomic pointer. Readers do
`.load_full()` to get an `Arc<T>` snapshot of the current value. Writers do
`.store(Arc::new(new_value))` to atomically replace it. No locks.

The sampler's [`CachedJson`](../src/sampler.rs#L147-L175) is the canonical
use: one writer, many readers, snapshots are large-ish (`Vec<u8>`) so we
don't want to copy them under a lock. After `store`, in-flight readers keep
their old `Arc<Vec<u8>>` until they drop it; the buffer is freed when the
last reference goes away.

In TS terms: think of `ArcSwap<T>` as a `BehaviorSubject<T>` where
subscribers receive a frozen reference to the current value. In Python:
roughly an atomic reference swap, with the GC equivalent (Rust's drop)
freeing old values once all observers stop holding them.

### `tokio::sync::Notify`

[`Notify`](https://docs.rs/tokio/latest/tokio/sync/struct.Notify.html) is a
fan-out one-shot-ish synchronization primitive.
`shutdown.notify_waiters()` wakes every task currently `await`ing
`shutdown.notified()`. Used in two places:

- [Shutdown propagation](../src/main.rs#L84-L96) — the signal task notifies;
  the sampler's tick loop and the server's graceful-shutdown future both
  await it.
- [Sampler stop](../src/sampler.rs#L181) — same pattern, scoped to the
  sampler.

In TS: think `EventEmitter` but every listener fires exactly once per
`notify_waiters` call. In Python: closer to `asyncio.Event` (with `set()`
broadcasting), except `Notify` re-arms automatically.

### `parking_lot::Mutex` (worker registry)

[`WorkerRegistry`](../src/server/worker.rs#L17-L19) wraps a
`HashMap<String, ()>` in a `parking_lot::Mutex`. We chose `parking_lot` over
`std::sync::Mutex` for the lighter-weight, non-poisoning lock semantics —
the critical sections (insert, remove) are O(1) hash operations and we
never hold the lock across an `.await`.

Holding a sync mutex across an `.await` would be a bug on the
`current_thread` runtime: every other task is also on this thread, so a
held lock could deadlock the very task that would release it. Static
analysis (clippy's `await_holding_lock`) flags it; the codebase respects
this rule.

### `std::sync::atomic::AtomicUsize` (active WS counter)

[`active_ws`](../src/server/mod.rs#L24) is incremented on WS session start
and decremented on end. `Ordering::SeqCst` is used uniformly — the counter
isn't on a hot path and SeqCst is the easiest to reason about.

## Why state is boxed and shuttled

Inside the sampler tick loop ([sampler.rs:236-247](../src/sampler.rs#L236-L247)):

```rust
let owned = state.take().expect("sampler state always restored");
let join = tokio::task::spawn_blocking(move || {
    let mut s = owned;
    let snap = s.refresh();
    (snap, s)
}).await;
match join {
    Ok((snapshot, returned)) => { state = Some(returned); ... }
    Err(e)                   => { state = Some(Box::new(SamplerState::new())); ... }
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

If a tick panics, `await` returns `Err(JoinError)`, our `Box` was eaten by
the failed task, and we rebuild fresh state. Readers continue seeing the
last-good snapshot until the next tick succeeds.

This pattern is idiomatic in Tokio for "single-threaded mutable state that
occasionally needs the blocking pool". A C# analogy: an `async` method that
hands a mutable struct to `Task.Run`, then awaits and reassigns it on
return.

## Lifetime patterns

### Cheaply-cloneable handle types

Several types in the codebase are designed to be cloned per-request or
per-task:

- [`AppState`](../src/server/mod.rs#L17-L25) — wraps a few `Arc`s and an
  `AtomicUsize`. axum clones it for each handler invocation.
- [`Arc<CachedJson>`](../src/sampler.rs#L205-L207) — handed to the server
  on construction and then read-only forever.
- [`Arc<WorkerRegistry>`](../src/server/mod.rs#L20) — handed to handlers
  via `AppState`.
- [`reqwest::Client`](../src/server/mod.rs#L23) — internally `Arc`'d, so
  cloning shares the connection pool.

If you add a new shared resource, follow the same pattern: hold an `Arc`
inside `AppState`, never put plain owned values there.

### Borrowed views over `Snapshot`

[`HttpSystemView<'a>`](../src/snapshot.rs#L114-L126),
[`WsSystemView<'a>`](../src/snapshot.rs#L130-L140), and
[`NetworkView<'a>`](../src/snapshot.rs#L143-L147) all borrow from a
`Snapshot`. They're constructed via the inherent methods on `Snapshot` and
serialized immediately. The `'a` lifetime means they cannot outlive the
snapshot they borrow from — which is fine because we only build them inside
`CachedJson::publish` for a single call to `serde_json::to_vec`.

This avoids cloning the underlying vectors (`disks`, `processes`, etc.) just
to feed them to serde. The total saving is ~50 KB/tick on a busy host with
many disks and processes.

## Shutdown propagation

Three independent tasks need to stop in order:

```
signal task ─ notify_waiters() ─┐
                                ├── shared Notify
sampler tick loop ──────────────┤
                                │
server graceful_shutdown ───────┘
```

Sequence:

1. The OS delivers SIGINT or SIGTERM to the process.
2. The signal task ([main.rs:106-129](../src/main.rs#L106-L129)) returns
   from its `select!` and calls `notify_waiters()`.
3. axum's graceful-shutdown future resolves; axum stops the TCP accept loop
   and waits for in-flight requests to finish.
4. The main task `await`s `serve(...)` returning — typically within
   milliseconds because requests are bounded by the cached-bytes path.
5. `main` calls `sampler.stop()`. That call notifies the sampler's *own*
   `Notify` (a different one — the sampler doesn't share the global
   shutdown handle) and aborts the join handle.
6. `main` returns; the runtime drops; OS threads in the blocking pool are
   joined; the process exits.

WebSocket sessions are not explicitly stopped from outside. They detect
shutdown indirectly: axum's graceful-shutdown drops the WS handler future,
which drops the socket, which causes any client `recv()` to return `None`
and the push loop to exit. If a session is mid-`send` when the runtime is
torn down, the send errors and the loop returns. This is fine because the
sampler keeps publishing until `sampler.stop()` is called *after*
`axum::serve` returns.

## When to break the rules

- **Holding a lock across `.await`**: never. Use `tokio::sync::Mutex` if you
  truly need async-aware mutual exclusion (we currently don't).
- **`spawn_blocking` for trivial work**: don't. The dispatch overhead is
  ~10 µs; only worth it for work that's at least an order of magnitude
  longer.
- **Adding new tasks**: think about shutdown. Either subscribe to the
  shared `Notify` or own a registry that the main task can drain.
