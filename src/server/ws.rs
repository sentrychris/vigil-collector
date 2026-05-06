use std::sync::atomic::Ordering;
use std::time::Instant;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use serde::Deserialize;

use super::AppState;

#[derive(Deserialize)]
pub struct ConnectQuery {
    id: Option<String>,
}

pub async fn connect(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(q): Query<ConnectQuery>,
) -> axum::response::Response {
    let id = q.id.unwrap_or_default();
    if id.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "missing worker id").into_response();
    }
    if !state.workers.claim(&id) {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "Invalid worker id",
        )
            .into_response();
    }
    if state.active_ws.load(Ordering::SeqCst) >= state.max_ws_connections {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "Server is at full capacity. Please try again later.",
        )
            .into_response();
    }
    ws.on_upgrade(move |socket| run_session(socket, state))
}

async fn run_session(mut socket: WebSocket, state: AppState) {
    state.active_ws.fetch_add(1, Ordering::SeqCst);
    let result = push_loop(&mut socket, &state).await;
    state.active_ws.fetch_sub(1, Ordering::SeqCst);
    if let Err(e) = result {
        tracing::debug!("ws session ended: {}", e);
    }
    let _ = socket.close().await;
}

async fn push_loop(socket: &mut WebSocket, state: &AppState) -> anyhow::Result<()> {
    let interval = state.push_interval;
    loop {
        let started = Instant::now();
        let bytes = state.sampler_json.ws_system.load_full();
        // Convert Arc<Vec<u8>> -> Bytes without copying when possible.
        // Axum's Message::Text wants a String/Utf8Bytes. The cached blob
        // is valid UTF-8 (it's serde_json output), so we hand it over
        // without re-validating.
        let s = String::from_utf8((*bytes).clone())?;
        if let Err(e) = socket.send(Message::Text(s.into())).await {
            return Err(e.into());
        }

        // Drain any pending incoming frames (the Python handler echoes
        // them back, but the JS client doesn't actually send anything
        // worth handling — keep the read side awake to detect close).
        // We do this opportunistically without blocking the next push.
        let elapsed = started.elapsed();
        let wait = interval.saturating_sub(elapsed);
        let mut sleep = std::pin::pin!(tokio::time::sleep(wait));
        loop {
            tokio::select! {
                biased;
                _ = sleep.as_mut() => break,
                msg = socket.recv() => match msg {
                    Some(Ok(Message::Close(_))) | None => return Ok(()),
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => return Err(e.into()),
                }
            }
        }
    }
}
