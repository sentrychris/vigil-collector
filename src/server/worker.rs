use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use parking_lot::Mutex;
use rand::RngCore;

const TOKEN_BYTES: usize = 32;
const EXPIRY: Duration = Duration::from_secs(5);

/// Tracks pending worker IDs created via `POST /worker`. Each ID is single-
/// use and expires after 5 seconds if no WebSocket claims it. Mirrors the
/// Python ``Worker`` + ``recycle`` pair, minus the WebSocket back-reference
/// (the ws layer owns its own connection lifetime, so the registry only
/// needs to know about pending IDs).
pub struct WorkerRegistry {
    inner: Mutex<HashMap<String, ()>>,
}

impl WorkerRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn issue(self: &Arc<Self>) -> String {
        let id = generate_id();
        self.inner.lock().insert(id.clone(), ());
        let this = Arc::clone(self);
        let expire_id = id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(EXPIRY).await;
            // Best-effort cleanup. If the WS handler already claimed the
            // token, this is a no-op. If it hasn't, the token is now
            // dead and any future ``/connect?id=...`` rejects.
            this.inner.lock().remove(&expire_id);
        });
        id
    }

    /// Claim and remove the token. Returns true if the token was valid and
    /// is now consumed; false if it was missing or already expired.
    pub fn claim(&self, id: &str) -> bool {
        self.inner.lock().remove(id).is_some()
    }
}

fn generate_id() -> String {
    let mut buf = [0u8; TOKEN_BYTES];
    rand::thread_rng().fill_bytes(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}
