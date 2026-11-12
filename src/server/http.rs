use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use rust_embed::Embed;
use serde::Serialize;

use super::AppState;
use super::probes;

/// The Vue dashboard (``web/``), built by build.rs into ``web/dist``.
#[derive(Embed)]
#[folder = "web/dist/"]
struct Dashboard;

/// Serve a file from the dashboard build. Vite fingerprints everything
/// under ``assets/`` so it can be cached for good; the rest (index.html,
/// favicon, images) is revalidated so a new collector version shows up on
/// the next load.
fn embedded(path: &str) -> Option<Response> {
    let file = Dashboard::get(path)?;
    let mime = HeaderValue::from_str(file.metadata.mimetype())
        .unwrap_or(HeaderValue::from_static("application/octet-stream"));
    let cache = if path.starts_with("assets/") {
        HeaderValue::from_static("public, max-age=31536000, immutable")
    } else {
        HeaderValue::from_static("no-cache")
    };
    let mut res = Response::new(Body::from(file.data));
    res.headers_mut().insert(header::CONTENT_TYPE, mime);
    res.headers_mut().insert(header::CACHE_CONTROL, cache);
    Some(res)
}

/// Any GET the API doesn't handle: a dashboard file if one matches, else
/// the dashboard itself so client-side routes survive a reload. Missing
/// fingerprinted assets stay a real 404 rather than a page of HTML.
pub async fn dashboard(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if let Some(res) = embedded(path) {
        return res;
    }
    if path.starts_with("assets/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    embedded("index.html")
        .unwrap_or_else(|| (StatusCode::NOT_FOUND, "dashboard missing").into_response())
}

/// ``/control`` on the public server: there is no control API here.
pub async fn no_control() -> Response {
    (StatusCode::NOT_FOUND, "not found").into_response()
}

pub async fn preflight() -> StatusCode {
    StatusCode::NO_CONTENT
}

#[derive(Serialize)]
struct WorkerResponse {
    id: Option<String>,
    url: Option<String>,
    message: String,
}

pub async fn create_worker(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let id = state.workers.issue();
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let url = if host.is_empty() {
        None
    } else {
        Some(format!("ws://{host}/connect?id={id}"))
    };
    let body = WorkerResponse {
        id: Some(id),
        url,
        message: "Websocket connection ready (Worker expires in 5 seconds if unclaimed).".into(),
    };
    json_response(&body)
}

pub async fn system(State(state): State<AppState>) -> Response {
    raw_json(Bytes::clone(&state.sampler_json.http_system.load()))
}

pub async fn network(State(state): State<AppState>) -> Response {
    raw_json(Bytes::clone(&state.sampler_json.network.load()))
}

pub async fn probes(State(state): State<AppState>) -> Response {
    let result = probes::run(&state.probes_client).await;
    json_response(&result)
}

pub fn json_response<T: Serialize>(value: &T) -> Response {
    match serde_json::to_vec(value) {
        Ok(bytes) => raw_json(Bytes::from(bytes)),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

fn raw_json(bytes: Bytes) -> Response {
    let mut res = Response::new(Body::from(bytes));
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    res
}
