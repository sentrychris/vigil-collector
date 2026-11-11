use std::time::{Duration, Instant};

use futures_util::future::join_all;
use reqwest::{Client, Method};
use serde::Serialize;

use crate::config;

#[derive(Serialize)]
pub struct ProbeResult {
    pub name: String,
    pub url: String,
    pub ok: bool,
    pub status_code: Option<u16>,
    pub latency_ms: u64,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct ProbesResponse {
    pub probes: Vec<ProbeResult>,
}

/// Probe all configured targets concurrently. Settings (probe list +
/// timeout) are re-read on every call so operators can edit
/// ``~/.vigil-collector/settings.json`` without restarting. The timeout is
/// applied per request, which takes precedence over the shared client's.
pub async fn run(client: &Client) -> ProbesResponse {
    let settings = config::read_settings();
    let timeout = Duration::from_secs_f64(settings.probe_timeout.max(0.1));

    let tasks = settings
        .probes
        .into_iter()
        .map(|p| probe_one(client, p.name, p.url, timeout));
    ProbesResponse {
        probes: join_all(tasks).await,
    }
}

async fn probe_one(client: &Client, name: String, url: String, timeout: Duration) -> ProbeResult {
    let started = Instant::now();
    let response = client
        .request(Method::HEAD, &url)
        .timeout(timeout)
        .send()
        .await;

    let elapsed_ms = || started.elapsed().as_millis() as u64;

    let response = match response {
        Ok(r) if r.status().as_u16() == 405 => {
            // HEAD denied — fall back to GET. We don't read the body;
            // status + latency are all we care about.
            client
                .request(Method::GET, &url)
                .timeout(timeout)
                .send()
                .await
        }
        other => other,
    };

    match response {
        Ok(r) => {
            let code = r.status().as_u16();
            ProbeResult {
                name,
                url,
                ok: (200..400).contains(&code),
                status_code: Some(code),
                latency_ms: elapsed_ms(),
                error: None,
            }
        }
        Err(e) => ProbeResult {
            name,
            url,
            ok: false,
            status_code: e.status().map(|s| s.as_u16()),
            latency_ms: elapsed_ms(),
            error: Some(e.to_string()),
        },
    }
}
