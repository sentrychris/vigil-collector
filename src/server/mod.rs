pub mod control;
pub mod http;
pub mod probes;
pub mod worker;
pub mod ws;

use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::routing::{any, get, post};
use tokio::sync::watch;
use tower_http::cors::{Any, CorsLayer};

use crate::sampler::CachedJson;

use self::worker::WorkerRegistry;

#[derive(Clone)]
pub struct AppState {
    pub sampler_json: Arc<CachedJson>,
    pub workers: Arc<WorkerRegistry>,
    /// The cap on open WebSocket sessions. Read at each connection, so the
    /// Settings page can change it while the server runs.
    pub max_ws: Arc<AtomicUsize>,
    /// Milliseconds between WebSocket frames (the sampler's tick). Read
    /// before each frame for the same reason.
    pub push_interval_ms: Arc<AtomicU64>,
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

/// The routes every listener serves: the API and, for anything else, the
/// dashboard.
fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/worker",
            post(http::create_worker).options(http::preflight),
        )
        .route("/system", get(http::system).options(http::preflight))
        .route("/network", get(http::network).options(http::preflight))
        .route("/probes", get(http::probes).options(http::preflight))
        .route("/connect", get(ws::connect))
}

/// The server other machines and browsers reach. It never has the control
/// API: ``/control`` answers 404 here rather than falling through to the
/// dashboard, so a page can tell at once that it has no Settings.
pub fn router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    routes()
        .route("/control", any(http::no_control))
        .route("/control/{*rest}", any(http::no_control))
        .fallback(get(http::dashboard))
        .layer(cors)
        .with_state(state)
}

/// The desktop window's own server: the same dashboard and API plus the
/// control API, all behind ``control::guard``. No CORS headers, so only
/// pages it served itself can read its answers.
pub fn private_router(state: AppState, control: control::Control) -> Router {
    routes()
        .fallback(get(http::dashboard))
        .with_state(state)
        .merge(control::routes(control.clone()))
        .layer(axum::middleware::from_fn_with_state(
            control,
            control::guard,
        ))
}

/// Turns a listen address from the settings file or the command line
/// into a socket address.
pub fn resolve_addr(host: &str, port: u16) -> anyhow::Result<SocketAddr> {
    if host.eq_ignore_ascii_case("localhost") {
        return Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port));
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, port));
    }
    let mut iter = (host, port)
        .to_socket_addrs()
        .map_err(|e| anyhow::anyhow!("could not resolve {host}: {e}"))?;
    iter.next()
        .ok_or_else(|| anyhow::anyhow!("could not resolve {host}"))
}

/// The address to point a browser at: a wildcard listen address (all
/// interfaces) becomes loopback, which always reaches it.
pub fn browsable(addr: SocketAddr) -> SocketAddr {
    match addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), addr.port())
        }
        IpAddr::V6(ip) if ip.is_unspecified() => {
            SocketAddr::new(IpAddr::V6(std::net::Ipv6Addr::LOCALHOST), addr.port())
        }
        _ => addr,
    }
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

/// What a listener serves with. The public server and the desktop
/// window's private one share the sampler's output and tick rate, and
/// each has its own count of, and cap on, WebSocket sessions.
pub struct Serve {
    pub sampler_json: Arc<CachedJson>,
    pub push_interval_ms: Arc<AtomicU64>,
    pub max_ws: Arc<AtomicUsize>,
    /// Open WebSocket sessions (dashboards watching live). The caller owns
    /// it so the tray menu and the Settings page can show it.
    pub active_ws: Arc<AtomicUsize>,
    /// Set for the desktop window's private listener, which also serves
    /// the control API.
    pub control: Option<control::Control>,
}

/// Serves on ``listener`` (from ``bind``) until ``shutdown`` resolves.
pub async fn serve<F>(
    listener: tokio::net::TcpListener,
    serve: Serve,
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
    let active_ws = serve.active_ws.clone();
    let state = AppState {
        sampler_json: serve.sampler_json,
        workers,
        max_ws: serve.max_ws,
        push_interval_ms: serve.push_interval_ms,
        probes_client,
        active_ws: serve.active_ws,
        closing,
    };

    let app = match serve.control {
        Some(control) => private_router(state, control),
        None => router(state),
    };
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
    use super::{bind, browsable, resolve_addr};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn resolves_localhost_and_literals() {
        assert_eq!(
            resolve_addr("localhost", 1).unwrap().ip(),
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        );
        assert_eq!(
            resolve_addr("LOCALHOST", 1).unwrap().ip(),
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        );
        assert_eq!(resolve_addr("0.0.0.0", 4500).unwrap().port(), 4500);
        assert_eq!(
            resolve_addr("::1", 1).unwrap().ip(),
            IpAddr::V6(Ipv6Addr::LOCALHOST)
        );
    }

    #[test]
    fn browsable_turns_wildcards_into_loopback() {
        let url = |addr: &str| format!("http://{}", browsable(addr.parse().unwrap()));
        assert_eq!(url("0.0.0.0:4500"), "http://127.0.0.1:4500");
        assert_eq!(url("[::]:4500"), "http://[::1]:4500");
        assert_eq!(url("192.168.1.5:4500"), "http://192.168.1.5:4500");
    }

    #[tokio::test]
    async fn moves_past_a_taken_port_only_when_searching() {
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = taken.local_addr().unwrap();
        assert!(bind(addr, false).await.is_err());
        let moved = bind(addr, true).await.unwrap();
        assert!(moved.local_addr().unwrap().port() > addr.port());
    }
}
