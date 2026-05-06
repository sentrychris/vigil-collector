pub mod http;
pub mod probes;
pub mod worker;
pub mod ws;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
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
    pub active_ws: Arc<std::sync::atomic::AtomicUsize>,
}

pub fn router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/", get(http::serve_index))
        .route("/favicon.ico", get(http::serve_favicon))
        .route("/worker", post(http::create_worker).options(http::preflight))
        .route("/system", get(http::system).options(http::preflight))
        .route("/network", get(http::network).options(http::preflight))
        .route("/probes", get(http::probes).options(http::preflight))
        .route("/connect", get(ws::connect))
        .layer(cors)
        .with_state(state)
}

pub async fn serve<F>(
    addr: SocketAddr,
    sampler: &Sampler,
    settings: &crate::config::Settings,
    shutdown: F,
) -> anyhow::Result<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let workers = Arc::new(WorkerRegistry::new());
    let probes_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs_f64(settings.probe_timeout))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()?;

    let state = AppState {
        sampler_json: sampler.json(),
        workers,
        max_ws_connections: settings.max_ws_connections,
        push_interval: sampler.interval(),
        probes_client,
        active_ws: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };

    let app = router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("Listening on http://{}", addr);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}
