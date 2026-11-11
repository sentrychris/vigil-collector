pub mod http;
pub mod probes;
pub mod worker;
pub mod ws;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::routing::{get, post};
use tokio::sync::watch;
use tower_http::cors::{Any, CorsLayer};

use crate::sampler::Sampler;

use self::worker::WorkerRegistry;

#[derive(Clone)]
pub struct AppState {
    pub sampler_json: Arc<crate::sampler::CachedJson>,
    pub workers: Arc<WorkerRegistry>,
    pub max_ws_connections: usize,
    pub push_interval: std::time::Duration,
    pub probes_client: reqwest::Client,
    pub active_ws: Arc<AtomicUsize>,
    /// Flips to ``true`` when the server starts shutting down, so open
    /// WebSocket sessions can send a Close frame instead of being dropped.
    pub closing: watch::Receiver<bool>,
}

/// How long shutdown waits for WebSocket sessions to send their Close
/// frames. Windows gives a console process ~5 s after a close/logoff/
/// shutdown event before killing it, so stay well inside that.
const WS_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

pub fn router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route(
            "/worker",
            post(http::create_worker).options(http::preflight),
        )
        .route("/system", get(http::system).options(http::preflight))
        .route("/network", get(http::network).options(http::preflight))
        .route("/probes", get(http::probes).options(http::preflight))
        .route("/connect", get(ws::connect))
        .fallback(get(http::dashboard))
        .layer(cors)
        .with_state(state)
}

/// ``active_ws`` counts the open WebSocket sessions (dashboards watching
/// live). The caller owns it so the Windows tray menu can show it.
pub async fn serve<F>(
    addr: SocketAddr,
    sampler: &Sampler,
    settings: &crate::config::Settings,
    active_ws: Arc<AtomicUsize>,
    shutdown: F,
) -> anyhow::Result<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let workers = Arc::new(WorkerRegistry::new());
    // One pooled client for the life of the server. The timeout is set per
    // request by ``probes::run`` (re-read from settings each call), which
    // overrides any client-level timeout in reqwest.
    let probes_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()?;

    let (closing_tx, closing) = watch::channel(false);
    let state = AppState {
        sampler_json: sampler.json(),
        workers,
        max_ws_connections: settings.max_ws_connections,
        push_interval: sampler.interval(),
        probes_client,
        active_ws: active_ws.clone(),
        closing,
    };

    let app = router(state);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| anyhow::anyhow!("could not listen on {addr}: {e}"))?;
    tracing::info!("Listening on http://{}", addr);
    #[cfg(windows)]
    crate::tray::notify_listening();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown.await;
            let _ = closing_tx.send(true);
        })
        .await?;

    // Upgraded WebSocket connections aren't tracked by axum's graceful
    // shutdown. Give them a moment to send Close (1001 Going Away) so
    // dashboards see a clean disconnect rather than an abnormal one.
    let drained = tokio::time::timeout(WS_DRAIN_TIMEOUT, async {
        while active_ws.load(Ordering::Acquire) > 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    if drained.is_err() {
        tracing::debug!("shutdown: WebSocket sessions did not close in time");
    }
    Ok(())
}
