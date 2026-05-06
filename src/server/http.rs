use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;
use serde::Serialize;

use super::probes;
use super::AppState;

#[derive(Embed)]
#[folder = "assets/"]
struct Assets;

fn embedded(path: &str) -> Option<Response> {
    let asset = Assets::get(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut res = Response::new(Body::from(asset.data.into_owned()));
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.as_ref()).unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    Some(res)
}

pub async fn serve_index() -> Response {
    embedded("web.html")
        .unwrap_or_else(|| (StatusCode::NOT_FOUND, "index missing").into_response())
}

pub async fn serve_favicon() -> Response {
    embedded("favicon.ico")
        .unwrap_or_else(|| (StatusCode::NOT_FOUND, "no favicon").into_response())
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
    let bytes = state.sampler_json.http_system.load_full();
    raw_json((*bytes).clone())
}

pub async fn network(State(state): State<AppState>) -> Response {
    let bytes = state.sampler_json.network.load_full();
    raw_json((*bytes).clone())
}

pub async fn probes(State(state): State<AppState>) -> Response {
    let result = probes::run(&state.probes_client).await;
    json_response(&result)
}

fn json_response<T: Serialize>(value: &T) -> Response {
    match serde_json::to_vec(value) {
        Ok(bytes) => raw_json(bytes),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

fn raw_json(bytes: Vec<u8>) -> Response {
    let mut res = Response::new(Body::from(bytes));
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    res
}
