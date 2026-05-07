mod cli;
mod config;
mod hub;
mod log;
mod metrics;
mod sampler;
mod server;
mod snapshot;

use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;

use clap::Parser;
use tokio::sync::Notify;

use crate::cli::Cli;
use crate::sampler::Sampler;

fn resolve_addr(host: &str, port: u16) -> anyhow::Result<SocketAddr> {
    if host == "localhost" {
        return Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port));
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, port));
    }
    let mut iter = (host, port).to_socket_addrs()?;
    iter.next()
        .ok_or_else(|| anyhow::anyhow!("could not resolve {host}"))
}

// Single-threaded runtime: the workload is one 1 Hz sampler + a low double-
// digit number of WebSocket clients on cooperative tasks. A multi-threaded
// scheduler would allocate per-core worker threads (each with a 2 MB stack),
// which dominates idle RSS without buying any throughput we actually need.
// Heavy refresh work still happens off-runtime via ``spawn_blocking``.
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    let settings = config::read_settings();

    log::init(&settings.log_level, settings.logging_enabled)?;

    let address = if args.address.is_empty() {
        settings.address.clone()
    } else {
        args.address.clone()
    };

    let port = if args.port == 0 {
        settings.port_number
    } else {
        args.port
    };

    let addr = resolve_addr(&address, port)?;

    let mut sampler = Sampler::new(settings.ws_push_interval);
    sampler.start();

    let mut hub_client = match (args.hub.clone(), args.hub_key.clone()) {
        (Some(url), Some(key)) if !url.is_empty() && !key.is_empty() => {
            let mut hc = hub::HubClient::new(
                url.clone(),
                key,
                args.hub_name.clone(),
                args.parse_tags(),
                sampler.snapshot_handle_arc(),
                sampler.interval(),
            );
            hc.start();
            tracing::info!("Pushing to Vigil Pro hub at {}", url);
            Some(hc)
        }
        (Some(_), None) | (None, Some(_)) => {
            tracing::warn!(
                "Hub config incomplete — both --hub and --hub-key (or env vars) are required. Hub push disabled."
            );
            None
        }
        _ => None,
    };

    // Single shutdown notify shared between signal watcher and server.
    let shutdown = Arc::new(Notify::new());
    let shutdown_for_signal = shutdown.clone();
    tokio::spawn(async move {
        wait_for_signal().await;
        tracing::info!("Shutting down gracefully...");
        shutdown_for_signal.notify_waiters();
    });

    let server_shutdown = shutdown.clone();
    let serve_result = server::serve(addr, &sampler, &settings, async move {
        server_shutdown.notified().await;
    })
    .await;

    if let Some(hc) = hub_client.as_mut() {
        hc.stop();
    }
    sampler.stop();

    serve_result
}

async fn wait_for_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        } else {
            std::future::pending::<()>().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
