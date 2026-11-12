use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use base64::Engine;

const TOKEN_BYTES: usize = 32;
const EXPIRY: Duration = Duration::from_secs(5);

/// Tracks pending worker IDs created via `POST /worker`. Each ID is single-
/// use and expires after 5 seconds if no WebSocket claims it. Mirrors the
/// Python ``Worker`` + ``recycle`` pair, minus the WebSocket back-reference
/// (the ws layer owns its own connection lifetime, so the registry only
/// needs to know about pending IDs).
pub struct WorkerRegistry {
    inner: Mutex<HashSet<String>>,
}

impl WorkerRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashSet::new()),
        }
    }

    /// The set is only ever inserted into / removed from, so a panic while
    /// holding the lock can't leave it inconsistent: recover from poison.
    fn ids(&self) -> MutexGuard<'_, HashSet<String>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn issue(self: &Arc<Self>) -> String {
        let id = generate_id();
        self.ids().insert(id.clone());
        let this = Arc::clone(self);
        let expire_id = id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(EXPIRY).await;
            // Best-effort cleanup. If the WS handler already claimed the
            // token, this is a no-op. If it hasn't, the token is now
            // dead and any future ``/connect?id=...`` rejects.
            this.ids().remove(&expire_id);
        });
        id
    }

    /// Claim and remove the token. Returns true if the token was valid and
    /// is now consumed; false if it was missing or already expired.
    pub fn claim(&self, id: &str) -> bool {
        self.ids().remove(id)
    }
}

/// A random URL-safe token: 256 bits from the OS, as text.
pub fn generate_id() -> String {
    let mut buf = [0u8; TOKEN_BYTES];
    // The OS CSPRNG (getrandom(2) / BCryptGenRandom). It only fails if the
    // OS has no entropy source at all, which no supported platform lacks.
    getrandom::fill(&mut buf).expect("OS random number generator unavailable");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}
