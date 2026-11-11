mod cli;
mod config;
#[cfg(any(windows, test))]
mod ico;
mod log;
mod metrics;
mod sampler;
mod server;
mod snapshot;
#[cfg(windows)]
mod tray;

use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use clap::Parser;
use tokio::sync::{Notify, mpsc};

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

    // On Windows the collector normally lives in the system tray. The
    // process started from a terminal (or by a double-click) hands over to
    // a background copy here, before it reads settings or opens the log.
    #[cfg(windows)]
    let tray_mode = match tray::mode(args.foreground) {
        tray::Mode::Launcher => tray::launch(log::console_supports_color()),
        tray::Mode::Tray => true,
        tray::Mode::Foreground => false,
    };
    // One copy per computer. Checked before the log file is opened, which
    // would otherwise clear the running copy's log.
    #[cfg(windows)]
    let instance = match tray::Instance::claim() {
        Ok(instance) => instance,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };

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

    // Double-clicked on Windows with no --port: if the port is taken, use
    // the next free one rather than fail. A port given on the command line
    // is always used as-is.
    #[cfg(windows)]
    let search = args.port.is_none() && tray::find_free_port();
    #[cfg(not(windows))]
    let search = false;
    let listener = server::bind(addr, search).await?;
    // The tray shows the port actually in use.
    #[cfg(windows)]
    let addr = listener.local_addr()?;

    let mut sampler = Sampler::new(settings.ws_push_interval);
    sampler.start()?;

    // Open WebSocket sessions, shared with the tray menu.
    let active_ws = Arc::new(AtomicUsize::new(0));

    // Menu choices from the Windows tray icon. Elsewhere (and without a
    // tray) the sender is dropped and nothing ever arrives.
    #[allow(unused_variables, unused_mut)]
    let (tray_tx, mut tray_rx) = mpsc::unbounded_channel::<TrayEvent>();
    #[cfg(windows)]
    let tray = if tray_mode {
        match tray::Tray::start(
            format!("http://{}", browsable(addr)),
            tray_tx,
            active_ws.clone(),
        ) {
            Ok(tray) => Some(tray),
            Err(e) => {
                tracing::warn!("No tray icon: {e}");
                None
            }
        }
    } else {
        None
    };
    #[cfg(not(windows))]
    drop(tray_tx);

    // Single shutdown notify shared between signal watcher and server.
    // ``notify_one`` stores a permit, so a signal that lands before the
    // server starts awaiting is not lost.
    let shutdown = Arc::new(Notify::new());
    let shutdown_for_signal = shutdown.clone();
    let restart = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let restart_requested = restart.clone();
    tokio::spawn(async move {
        let tray_event = async {
            match tray_rx.recv().await {
                Some(event) => event,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            _ = wait_for_signal() => {}
            event = tray_event => {
                if event == TrayEvent::Restart {
                    restart_requested.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }
        }
        tracing::info!("Shutting down gracefully...");
        shutdown_for_signal.notify_one();
    });

    let server_shutdown = shutdown.clone();
    let serve_result = server::serve(listener, &sampler, &settings, active_ws, async move {
        server_shutdown.notified().await;
    })
    .await;

    sampler.stop();

    #[cfg(windows)]
    {
        // Removes the icon before a restarted copy adds its own.
        drop(tray);
        if serve_result.is_ok() && restart.load(std::sync::atomic::Ordering::SeqCst) {
            tracing::info!("Restarting");
            // Let the new copy claim the instance.
            drop(instance);
            tray::respawn()?;
        }
    }
    #[cfg(not(windows))]
    let _ = restart;

    serve_result
}

/// The address to point a browser at: a wildcard listen address (all
/// interfaces) becomes loopback, which always reaches it.
#[cfg_attr(not(windows), allow(dead_code))]
fn browsable(addr: SocketAddr) -> SocketAddr {
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

/// What the Windows tray menu asks for. Defined for every platform so the
/// shutdown code reads the same everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayEvent {
    #[cfg_attr(not(windows), allow(dead_code))]
    Restart,
    #[cfg_attr(not(windows), allow(dead_code))]
    Quit,
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
    use super::{browsable, resolve_addr};
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
}
