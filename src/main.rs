mod cli;
#[cfg(any(windows, test))]
mod compact;
mod config;
#[cfg(windows)]
mod frame;
#[cfg(any(windows, test))]
mod ico;
mod log;
mod metrics;
#[cfg(any(windows, test))]
mod native;
mod sampler;
mod server;
#[cfg(any(windows, test))]
mod settings;
mod snapshot;
mod supervisor;
#[cfg(windows)]
mod tray;
#[cfg(all(windows, any(target_env = "msvc", feature = "window-on-mingw")))]
mod window;
#[cfg(all(windows, not(any(target_env = "msvc", feature = "window-on-mingw"))))]
#[path = "window_none.rs"]
mod window;

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::{Duration, Instant};

use clap::Parser;
use tokio::sync::{Notify, mpsc};

use crate::cli::Cli;
use crate::server::control::{Control, Panel};
use crate::supervisor::{Command, Supervisor};

/// The most live dashboards the desktop window's own listener serves.
/// The window uses one; the rest allow for reloads that overlap.
const WINDOW_MAX_WS: usize = 8;
/// Whether this build has the Vigil window (see ``window.rs`` and the
/// ``window-on-mingw`` feature in Cargo.toml).
const HAS_WINDOW: bool = cfg!(all(
    windows,
    any(target_env = "msvc", feature = "window-on-mingw")
));
/// With both set, any platform serves the control API on
/// ``127.0.0.1:<port>`` with that token, for working on the Settings page
/// (see "Development" in the README). Normally only the Windows tray
/// process serves it, on a port and with a token of its own choosing.
const CONTROL_PORT_ENV: &str = "VIGIL_CONTROL_PORT";
const CONTROL_TOKEN_ENV: &str = "VIGIL_CONTROL_TOKEN";

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
    #[cfg(not(windows))]
    let tray_mode = false;
    // One copy per computer. Checked before the log file is opened, which
    // would otherwise clear the running copy's log.
    #[cfg(windows)]
    let _instance = match tray::Instance::claim() {
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

    // Double-clicked on Windows with no --port: if the port is taken, use
    // the next free one rather than fail. A port given on the command line
    // is always used as-is.
    #[cfg(windows)]
    let search = args.port.is_none() && tray::find_free_port();
    #[cfg(not(windows))]
    let search = false;

    // Open WebSocket sessions on the public server, shared with the tray
    // menu and Settings.
    let active_ws = Arc::new(AtomicUsize::new(0));
    let mut supervisor = Supervisor::new(
        settings,
        args.address.clone(),
        args.port,
        search,
        active_ws.clone(),
    );

    // The first start is the one that may fail the whole program: a port
    // that can't be opened is reported (in the terminal, or a message box
    // when double-clicked) and Vigil exits, as it always has.
    let listener = supervisor.bind().await?;
    supervisor.start_sampler()?;

    // Requests from the tray menu and Settings.
    let (commands, mut requests) = mpsc::unbounded_channel::<Command>();
    // What Settings shows and changes, whether it is Vigil's own window
    // or the page in the full dashboard's.
    let panel = Panel {
        commands: commands.clone(),
        status: supervisor.status(),
        active_ws: active_ws.clone(),
        max_ws: supervisor.max_ws(),
        started: Instant::now(),
    };

    // The desktop window's private listener, with the control API.
    let window = private_listener(tray_mode && HAS_WINDOW).await;
    let window_stop = Arc::new(Notify::new());
    let mut window_endpoint = None;
    let window_task = match window {
        Some((private, token)) => {
            let port = private.local_addr()?.port();
            window_endpoint = Some((format!("http://127.0.0.1:{port}"), token.clone()));
            let serve = server::Serve {
                sampler_json: supervisor.sampler_json(),
                push_interval_ms: supervisor.push_interval_ms(),
                max_ws: Arc::new(AtomicUsize::new(WINDOW_MAX_WS)),
                active_ws: Arc::new(AtomicUsize::new(0)),
                control: Some(Control {
                    token: token.into(),
                    port,
                    panel: panel.clone(),
                }),
            };
            let stop = window_stop.clone();
            tracing::debug!("The window's own listener is on 127.0.0.1:{port}");
            Some(tokio::spawn(server::serve(private, serve, async move {
                stop.notified().await;
            })))
        }
        None => None,
    };

    // The tray menu and Settings can start the server again, so with
    // either of them the process stays when the server stops.
    if tray_mode || window_task.is_some() {
        supervisor.stay_up();
    }

    #[cfg(windows)]
    let tray = if tray_mode {
        let url = format!("http://{}", server::browsable(listener.local_addr()?));
        let window = window_endpoint
            .take()
            .map(|(origin, token)| tray::WindowEndpoint { origin, token });
        // What the compact window shows: the readings as they are taken.
        let feed = match compact::Feed::new(supervisor.readings()) {
            Ok(feed) => Some(feed),
            Err(e) => {
                tracing::warn!("No compact window: {e}");
                None
            }
        };
        let own = tray::Windows {
            feed,
            panel: panel.clone(),
            full: window,
        };
        match tray::Tray::start(url, commands.clone(), active_ws.clone(), own) {
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
    let _ = (&window_endpoint, tray_mode);
    // From here only the tray and the control API hold a sender, so with
    // neither of them the supervisor waits on signals alone.
    drop(panel);
    drop(commands);

    supervisor.serve_on(listener)?;

    // Keep the tray's tooltip, menu and window title on the address in
    // use, which Settings can change.
    #[cfg(windows)]
    if let Some(tray) = &tray {
        let handle = tray.handle();
        let mut status = supervisor.status();
        tokio::spawn(async move {
            while status.changed().await.is_ok() {
                let url = status.borrow_and_update().url.clone();
                handle.set_url(url);
            }
        });
    }

    let result = supervisor.run(&mut requests, wait_for_signal()).await;

    // Removes the icon and closes the window first, so quitting looks
    // immediate while connections are closed properly behind it.
    #[cfg(windows)]
    drop(tray);
    supervisor.shutdown().await;
    if let Some(mut task) = window_task {
        window_stop.notify_one();
        if tokio::time::timeout(Duration::from_secs(3), &mut task)
            .await
            .is_err()
        {
            task.abort();
        }
    }

    result
}

/// Opens the desktop window's private listener: loopback only, on the
/// port it used last time if that is still free (see
/// ``config::window_port``), with a token made for this run. ``None``
/// when this run has no window, or the listener couldn't be opened (the
/// tray then opens the browser instead).
async fn private_listener(window: bool) -> Option<(tokio::net::TcpListener, String)> {
    use std::net::{Ipv4Addr, SocketAddr};
    let local = |port| SocketAddr::from((Ipv4Addr::LOCALHOST, port));

    let dev_token = std::env::var(CONTROL_TOKEN_ENV)
        .ok()
        .filter(|t| !t.is_empty());
    let dev_port = std::env::var(CONTROL_PORT_ENV)
        .ok()
        .and_then(|p| p.parse::<u16>().ok());
    if let (Some(token), Some(port)) = (dev_token, dev_port) {
        return match tokio::net::TcpListener::bind(local(port)).await {
            Ok(listener) => {
                tracing::info!("Control API for development on http://127.0.0.1:{port}");
                Some((listener, token))
            }
            Err(e) => {
                tracing::warn!("No control API: could not listen on 127.0.0.1:{port}: {e}");
                None
            }
        };
    }
    if !window {
        return None;
    }

    let remembered = match config::window_port() {
        Some(port) => tokio::net::TcpListener::bind(local(port)).await.ok(),
        None => None,
    };
    let listener = match remembered {
        Some(listener) => listener,
        None => match tokio::net::TcpListener::bind(local(0)).await {
            Ok(listener) => listener,
            Err(e) => {
                tracing::warn!("No Vigil window: could not open its local listener: {e}");
                return None;
            }
        },
    };
    if let Ok(addr) = listener.local_addr() {
        config::remember_window_port(addr.port());
    }
    Some((listener, server::worker::generate_id()))
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
