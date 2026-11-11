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

/// How many ports ``bind`` tries when it may move to the next free one.
pub const PORT_SEARCH: u16 = 100;

/// Opens the listening socket on ``addr``. With ``search`` (the Windows
/// tray, double-clicked, with no ``--port``), a port that's taken moves it
/// on to the next one, up to ``PORT_SEARCH`` ports, so a second copy or
/// another program on the default port doesn't stop it starting.
pub async fn bind(addr: SocketAddr, search: bool) -> anyhow::Result<tokio::net::TcpListener> {
    let tries = if search { PORT_SEARCH } else { 1 };
    let mut last_error = None;
    for offset in 0..tries {
        let Some(port) = addr.port().checked_add(offset) else {
            break;
        };
        let candidate = SocketAddr::new(addr.ip(), port);
        match tokio::net::TcpListener::bind(candidate).await {
            Ok(listener) => {
                if offset > 0 {
                    tracing::warn!("Port {} is in use, so using {} instead", addr.port(), port);
                }
                return Ok(listener);
            }
            // Windows reports ports it reserves (Hyper-V, WSL) as access
            // denied rather than in use; both mean "try the next one".
            Err(e)
                if search
                    && matches!(
                        e.kind(),
                        std::io::ErrorKind::AddrInUse | std::io::ErrorKind::PermissionDenied
                    ) =>
            {
                last_error = Some(e);
            }
            Err(e) => return Err(anyhow::anyhow!("could not listen on {candidate}: {e}")),
        }
    }
    let last = addr.port().saturating_add(tries - 1);
    Err(anyhow::anyhow!(
        "could not find a free port from {} to {last} on {}: {}",
        addr.port(),
        addr.ip(),
        last_error.map_or_else(|| "none left".to_string(), |e| e.to_string())
    ))
}

/// Serves on ``listener`` (from ``bind``) until ``shutdown`` resolves.
/// ``active_ws`` counts the open WebSocket sessions (dashboards watching
/// live). The caller owns it so the Windows tray menu can show it.
pub async fn serve<F>(
    listener: tokio::net::TcpListener,
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
    let addr = listener.local_addr()?;
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

#[cfg(test)]
mod bind_tests {
    use super::bind;

    #[tokio::test]
    async fn moves_past_a_taken_port_only_when_searching() {
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = taken.local_addr().unwrap();
        assert!(bind(addr, false).await.is_err());
        let moved = bind(addr, true).await.unwrap();
        assert!(moved.local_addr().unwrap().port() > addr.port());
    }
}
