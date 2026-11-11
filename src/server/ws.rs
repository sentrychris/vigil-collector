use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use axum::extract::ws::{CloseFrame, Message, Utf8Bytes, WebSocket, WebSocketUpgrade, close_code};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use bytes::Bytes;
use futures_util::SinkExt;
use serde::Deserialize;

use super::AppState;

#[derive(Deserialize)]
pub struct ConnectQuery {
    id: Option<String>,
}

/// One reserved WebSocket slot. Reserved before the upgrade and released on
/// drop, so the cap holds even when many clients connect at once and a
/// slot is returned whether the session ends normally, errors, or the
/// upgrade never completes.
struct Slot(Arc<AtomicUsize>);

impl Slot {
    fn try_reserve(active: &Arc<AtomicUsize>, max: usize) -> Option<Self> {
        active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < max).then_some(n + 1)
            })
            .ok()
            .map(|_| Slot(active.clone()))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub async fn connect(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(q): Query<ConnectQuery>,
) -> axum::response::Response {
    let id = q.id.unwrap_or_default();
    if id.is_empty() {
        return (StatusCode::BAD_REQUEST, "missing worker id").into_response();
    }
    // Check capacity before claiming, so a client turned away for capacity
    // keeps its worker id and can retry within the expiry window.
    let Some(slot) = Slot::try_reserve(&state.active_ws, state.max_ws_connections) else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Server is at full capacity. Please try again later.",
        )
            .into_response();
    };
    if !state.workers.claim(&id) {
        return (StatusCode::FORBIDDEN, "Invalid worker id").into_response();
    }
    ws.on_upgrade(move |socket| run_session(socket, state, slot))
}

async fn run_session(mut socket: WebSocket, state: AppState, _slot: Slot) {
    if let Err(e) = push_loop(&mut socket, &state).await {
        tracing::debug!("ws session ended: {}", e);
    }
    // ``Sink::close`` flushes the close-frame reply tungstenite queued when
    // the peer's Close arrived (or sends our own Close if we're the side
    // ending the session). Without it the client never sees the handshake
    // complete and reports an abnormal closure (1006).
    let _ = socket.close().await;
}

async fn push_loop(socket: &mut WebSocket, state: &AppState) -> anyhow::Result<()> {
    let interval = state.push_interval;
    let mut closing = state.closing.clone();
    loop {
        if *closing.borrow_and_update() {
            return going_away(socket).await;
        }
        let started = Instant::now();
        // The cached blob is serde_json output, so it is valid UTF-8; the
        // conversion validates without copying.
        let bytes = Bytes::clone(&state.sampler_json.ws_system.load());
        let text = Utf8Bytes::try_from(bytes)?;
        socket.send(Message::Text(text)).await?;

        // Drain any incoming frames while waiting for the next push. The
        // clients don't send anything meaningful, but reading keeps the
        // close handshake and ping/pong flowing and notices a peer that
        // went away.
        let wait = interval.saturating_sub(started.elapsed());
        let mut sleep = std::pin::pin!(tokio::time::sleep(wait));
        loop {
            tokio::select! {
                biased;
                _ = sleep.as_mut() => break,
                _ = closing.changed() => return going_away(socket).await,
                msg = socket.recv() => match msg {
                    Some(Ok(Message::Close(_))) | None => return Ok(()),
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => return Err(e.into()),
                }
            }
        }
    }
}

/// Tell the client the server is shutting down. ``run_session`` then
/// completes the close handshake.
async fn going_away(socket: &mut WebSocket) -> anyhow::Result<()> {
    socket
        .send(Message::Close(Some(CloseFrame {
            code: close_code::AWAY,
            reason: Utf8Bytes::from_static("server shutting down"),
        })))
        .await?;
    Ok(())
}
