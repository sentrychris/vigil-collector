mod cli;
mod config;
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
    if host.eq_ignore_ascii_case("localhost") {
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

    // Settings are read before logging exists, so anything worth reporting
    // about the settings file comes back as notes and is logged right after
    // the subscriber is installed.
    let (settings, notes) = config::load_settings();
    log::init(&settings.log_level, settings.logging_enabled);
    for note in notes {
        note.emit();
    }

    // reqwest is built without a bundled crypto provider (see Cargo.toml);
    // install ring as the process-wide default before any client exists.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let address = args
        .address
        .clone()
        .unwrap_or_else(|| settings.address.clone());
    let port = args.port.unwrap_or(settings.port_number);
    let addr = resolve_addr(&address, port)?;

    let mut sampler = Sampler::new(settings.ws_push_interval);
    sampler.start()?;

    // Single shutdown notify shared between signal watcher and server.
    // ``notify_one`` stores a permit, so a signal that lands before the
    // server starts awaiting is not lost.
    let shutdown = Arc::new(Notify::new());
    let shutdown_for_signal = shutdown.clone();
    tokio::spawn(async move {
        wait_for_signal().await;
        tracing::info!("Shutting down gracefully...");
        shutdown_for_signal.notify_one();
    });

    let server_shutdown = shutdown.clone();
    let serve_result = server::serve(addr, &sampler, &settings, async move {
        server_shutdown.notified().await;
    })
    .await;

    sampler.stop();

    serve_result
}

#[cfg(unix)]
async fn wait_for_signal() {
    use tokio::signal::unix::{SignalKind, signal};

    let terminate = async {
        match signal(SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate => {}
    }
}

/// Windows has no SIGTERM. Ctrl-C and Ctrl-Break come from the console;
/// closing the console window, logging off and shutting down arrive as
/// their own control events and give the process a few seconds to exit,
/// so listen for all of them.
#[cfg(windows)]
async fn wait_for_signal() {
    use tokio::signal::windows;

    macro_rules! on {
        ($listener:expr) => {
            async {
                match $listener {
                    Ok(mut s) => {
                        s.recv().await;
                    }
                    Err(_) => std::future::pending::<()>().await,
                }
            }
        };
    }

    tokio::select! {
        _ = on!(windows::ctrl_c()) => {}
        _ = on!(windows::ctrl_break()) => {}
        _ = on!(windows::ctrl_close()) => {}
        _ = on!(windows::ctrl_logoff()) => {}
        _ = on!(windows::ctrl_shutdown()) => {}
    }
}

#[cfg(not(any(unix, windows)))]
async fn wait_for_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::resolve_addr;
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
}
