//! Runs the public server and lets it be stopped, started, restarted and
//! moved to another address or port without ending the process.
//!
//! ``main`` used to be a straight line: bind, serve until told to stop,
//! exit (and, for a restart, start a new copy of the program). The desktop
//! window lives in this process, so a restart can no longer be a new one.
//! The supervisor owns everything that can change while Vigil runs:
//!
//! - the public listener, which is closed and opened again by the tray
//!   menu and the Settings page;
//! - the sampler, which keeps ticking throughout (the window still shows
//!   live data while the public server is stopped);
//! - the settings that apply live: the connection cap, the tick rate and
//!   the log level.
//!
//! It takes ``Command``s from the tray menu and the control API, and
//! publishes a ``Status`` both of them show.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::{Notify, oneshot, watch};
use tokio::task::JoinHandle;

use crate::config::{self, Settings};
use crate::sampler::{CachedJson, Sampler};
use crate::server::{self, Serve};

/// How a command's sender hears whether it worked. The error is a
/// sentence fit to show on the Settings page.
pub type Reply = oneshot::Sender<Result<(), String>>;

pub enum Command {
    /// Open the public listener, if it is closed.
    Start(Option<Reply>),
    /// Close the public listener. Connected dashboards are told the
    /// server is going away.
    Stop(Option<Reply>),
    /// Stop, read the settings file again, and start.
    Restart(Option<Reply>),
    /// Use these settings from now on. ``address_set`` and ``port_set``
    /// say that the sender chose that value on purpose, which ends a
    /// ``--address`` or ``--port`` override for this run.
    Apply {
        settings: Box<Settings>,
        address_set: bool,
        port_set: bool,
        reply: Reply,
    },
    /// Stop everything and exit.
    Quit,
}

/// What the tray menu and the Settings page show about the public server.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Status {
    pub serving: bool,
    /// The address being listened on, such as ``0.0.0.0:4500``.
    pub listen: Option<String>,
    /// Where a browser on this computer reaches it.
    pub url: Option<String>,
    /// The address and port a start would use.
    pub address: String,
    pub port: u16,
    /// That value came from ``--address`` / ``--port`` rather than the
    /// settings file.
    pub address_from_cli: bool,
    pub port_from_cli: bool,
    /// Why the server isn't running, when that wasn't asked for.
    pub error: Option<String>,
}

/// How long a stop waits for requests in flight before closing them.
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

struct Running {
    addr: SocketAddr,
    shutdown: Arc<Notify>,
    task: JoinHandle<anyhow::Result<()>>,
}

pub struct Supervisor {
    settings: Settings,
    cli_address: Option<String>,
    cli_port: Option<u16>,
    /// A taken port may be swapped for the next free one (Windows,
    /// double-clicked, until a port is chosen on the Settings page).
    search: bool,
    /// Whether the process outlives the server. With a tray icon it does:
    /// the menu can start it again. Without one, a server that stops by
    /// itself ends the process so whatever started it can notice.
    stays_up: bool,
    sampler: Sampler,
    push_interval_ms: Arc<AtomicU64>,
    max_ws: Arc<AtomicUsize>,
    active_ws: Arc<AtomicUsize>,
    running: Option<Running>,
    last_error: Option<String>,
    status: watch::Sender<Status>,
}

impl Supervisor {
    /// ``cli_address`` and ``cli_port`` are the command line's overrides
    /// of the settings file. The sampler is created here and started by
    /// ``start_sampler``.
    pub fn new(
        settings: Settings,
        cli_address: Option<String>,
        cli_port: Option<u16>,
        search: bool,
        active_ws: Arc<AtomicUsize>,
    ) -> Self {
        let sampler = Sampler::new(settings.ws_push_interval);
        let push_interval_ms = Arc::new(AtomicU64::new(sampler.interval().as_millis() as u64));
        let max_ws = Arc::new(AtomicUsize::new(settings.max_ws_connections));
        let (status, _) = watch::channel(Status::default());
        let supervisor = Self {
            settings,
            cli_address,
            cli_port,
            search,
            stays_up: false,
            sampler,
            push_interval_ms,
            max_ws,
            active_ws,
            running: None,
            last_error: None,
            status,
        };
        supervisor.publish();
        supervisor
    }

    /// Keep running when the server stops by itself (see ``stays_up``).
    pub fn stay_up(&mut self) {
        self.stays_up = true;
    }

    pub fn status(&self) -> watch::Receiver<Status> {
        self.status.subscribe()
    }

    pub fn sampler_json(&self) -> Arc<CachedJson> {
        self.sampler.json()
    }

    pub fn push_interval_ms(&self) -> Arc<AtomicU64> {
        self.push_interval_ms.clone()
    }

    /// The public server's connection cap, as it stands now.
    pub fn max_ws(&self) -> Arc<AtomicUsize> {
        self.max_ws.clone()
    }

    pub fn start_sampler(&mut self) -> anyhow::Result<()> {
        self.sampler.start()
    }

    /// The address and port a start would use: the command line's, where
    /// given, else the settings file's.
    fn endpoint(&self) -> (String, u16) {
        (
            self.cli_address
                .clone()
                .unwrap_or_else(|| self.settings.address.clone()),
            self.cli_port.unwrap_or(self.settings.port_number),
        )
    }

    fn publish(&self) {
        let (address, port) = self.endpoint();
        let addr = self.running.as_ref().map(|r| r.addr);
        self.status.send_replace(Status {
            serving: addr.is_some(),
            listen: addr.map(|a| a.to_string()),
            url: addr.map(|a| format!("http://{}", server::browsable(a))),
            address,
            port,
            address_from_cli: self.cli_address.is_some(),
            port_from_cli: self.cli_port.is_some(),
            error: self.last_error.clone(),
        });
    }

    /// Opens the listening socket for the current address and port.
    pub async fn bind(&self) -> anyhow::Result<tokio::net::TcpListener> {
        let (address, port) = self.endpoint();
        let addr = server::resolve_addr(&address, port)?;
        server::bind(addr, self.search).await
    }

    /// Starts serving on a socket from ``bind``.
    pub fn serve_on(&mut self, listener: tokio::net::TcpListener) -> anyhow::Result<()> {
        let addr = listener.local_addr()?;
        let shutdown = Arc::new(Notify::new());
        let stop = shutdown.clone();
        let serve = Serve {
            sampler_json: self.sampler.json(),
            push_interval_ms: self.push_interval_ms.clone(),
            max_ws: self.max_ws.clone(),
            active_ws: self.active_ws.clone(),
            control: None,
        };
        // ``notify_one`` stores a permit, so a stop that lands before the
        // task first runs is not lost.
        let task = tokio::spawn(server::serve(listener, serve, async move {
            stop.notified().await;
        }));
        tracing::info!("Listening on http://{addr}");
        #[cfg(windows)]
        crate::tray::notify_listening();
        self.running = Some(Running {
            addr,
            shutdown,
            task,
        });
        self.last_error = None;
        self.publish();
        Ok(())
    }

    async fn start(&mut self) -> Result<(), String> {
        if self.running.is_some() {
            return Ok(());
        }
        let started = match self.bind().await {
            Ok(listener) => self.serve_on(listener),
            Err(e) => Err(e),
        };
        started.map_err(|e| {
            let message = sentence(&e.to_string());
            tracing::error!("{message}");
            self.last_error = Some(message.clone());
            self.publish();
            message
        })
    }

    async fn stop(&mut self) {
        let Some(mut running) = self.running.take() else {
            return;
        };
        running.shutdown.notify_one();
        match tokio::time::timeout(STOP_TIMEOUT, &mut running.task).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(e))) => tracing::warn!("The server stopped with an error: {e}"),
            Ok(Err(e)) => tracing::warn!("The server task failed: {e}"),
            Err(_) => {
                // Requests still open after the wait are closed, which
                // also frees the port for the next start.
                running.task.abort();
                let _ = running.task.await;
                tracing::debug!("stop: closed connections that were still open");
            }
        }
        tracing::info!("Stopped listening on http://{}", running.addr);
        self.publish();
    }

    /// Stop, read the settings file again, start: what the tray's Restart
    /// has always done, without a new process.
    async fn restart(&mut self) -> Result<(), String> {
        tracing::info!("Restarting");
        self.stop().await;
        self.adopt(config::read_settings());
        self.start().await
    }

    /// Takes on the settings that apply without touching the listener.
    fn adopt(&mut self, settings: Settings) {
        self.max_ws
            .store(settings.max_ws_connections, Ordering::Release);
        if let Err(e) = self.sampler.set_interval(settings.ws_push_interval) {
            tracing::error!("Couldn't start sampling again at the new interval: {e}");
        }
        self.push_interval_ms.store(
            self.sampler.interval().as_millis() as u64,
            Ordering::Release,
        );
        crate::log::apply(&settings.log_level, settings.logging_enabled);
        self.settings = settings;
    }

    /// Uses ``new`` from now on. If the address or port changes while
    /// serving, the listener moves: a new port is opened before the old
    /// one is closed, so a port that can't be used leaves the server as
    /// it was and the error comes back to the sender.
    async fn apply(
        &mut self,
        new: Settings,
        address_set: bool,
        port_set: bool,
    ) -> Result<(), String> {
        let before = self.endpoint();
        let cli = (self.cli_address.clone(), self.cli_port);
        if address_set {
            self.cli_address = None;
        }
        if port_set {
            self.cli_port = None;
        }
        let target = (
            self.cli_address
                .clone()
                .unwrap_or_else(|| new.address.clone()),
            self.cli_port.unwrap_or(new.port_number),
        );

        if target != before {
            if let Err(e) = self.move_to(&target.0, target.1).await {
                (self.cli_address, self.cli_port) = cli;
                self.publish();
                return Err(sentence(&e.to_string()));
            }
            if port_set {
                self.search = false;
            }
        }
        self.adopt(new);
        self.publish();
        Ok(())
    }

    /// Moves a running server to ``address:port``. When not serving it
    /// only checks that the address means something.
    async fn move_to(&mut self, address: &str, port: u16) -> anyhow::Result<()> {
        let addr = server::resolve_addr(address, port)?;
        let Some(old) = self.running.as_ref().map(|r| r.addr) else {
            return Ok(());
        };
        if addr.port() != old.port() {
            let listener = server::bind(addr, false).await?;
            self.stop().await;
            return self.serve_on(listener);
        }
        // Same port on another address: the old socket has to go first.
        // If the new one can't be opened, go back to the old.
        self.stop().await;
        match server::bind(addr, false).await {
            Ok(listener) => self.serve_on(listener),
            Err(e) => {
                match server::bind(old, false).await {
                    Ok(listener) => self.serve_on(listener)?,
                    Err(back) => {
                        self.last_error = Some(sentence(&back.to_string()));
                        tracing::error!("Couldn't go back to {old}: {back}");
                    }
                }
                Err(e)
            }
        }
    }

    /// Handles commands until ``Quit`` or ``signal`` (Ctrl-C, SIGTERM,
    /// sign-out). An error means the server stopped by itself and nothing
    /// is here to start it again.
    pub async fn run<S>(
        &mut self,
        commands: &mut UnboundedReceiver<Command>,
        signal: S,
    ) -> anyhow::Result<()>
    where
        S: Future<Output = ()>,
    {
        enum Event {
            Signal,
            Command(Option<Command>),
            Ended(Result<anyhow::Result<()>, tokio::task::JoinError>),
        }

        let mut signal = std::pin::pin!(signal);
        loop {
            let event = {
                let ended = async {
                    match self.running.as_mut() {
                        Some(running) => (&mut running.task).await,
                        None => std::future::pending().await,
                    }
                };
                tokio::select! {
                    _ = &mut signal => Event::Signal,
                    command = commands.recv() => Event::Command(command),
                    result = ended => Event::Ended(result),
                }
            };
            match event {
                Event::Signal | Event::Command(Some(Command::Quit)) => return Ok(()),
                // Nothing can send commands (no tray, no window): only a
                // signal ends the run.
                Event::Command(None) => {
                    signal.await;
                    return Ok(());
                }
                Event::Command(Some(Command::Start(reply))) => {
                    answer(reply, self.start().await);
                }
                Event::Command(Some(Command::Stop(reply))) => {
                    self.stop().await;
                    answer(reply, Ok(()));
                }
                Event::Command(Some(Command::Restart(reply))) => {
                    answer(reply, self.restart().await);
                }
                Event::Command(Some(Command::Apply {
                    settings,
                    address_set,
                    port_set,
                    reply,
                })) => {
                    let result = self.apply(*settings, address_set, port_set).await;
                    answer(Some(reply), result);
                }
                Event::Ended(result) => {
                    self.running = None;
                    let error = match result {
                        Ok(Ok(())) => anyhow::anyhow!("the server stopped by itself"),
                        Ok(Err(e)) => e,
                        Err(e) => anyhow::anyhow!("the server task failed: {e}"),
                    };
                    if !self.stays_up {
                        return Err(error);
                    }
                    let message = sentence(&error.to_string());
                    tracing::error!("{message}");
                    self.last_error = Some(message);
                    self.publish();
                }
            }
        }
    }

    /// Stops the server and the sampler, before the process exits.
    pub async fn shutdown(&mut self) {
        tracing::info!("Shutting down gracefully...");
        self.stop().await;
        self.sampler.stop();
    }
}

fn answer(reply: Option<Reply>, result: Result<(), String>) {
    if let Some(reply) = reply {
        let _ = reply.send(result);
    }
}

/// An error message as a sentence for the Settings page: capitalised,
/// with a full stop.
fn sentence(message: &str) -> String {
    let mut chars = message.trim().chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out: String = first.to_uppercase().chain(chars).collect();
    if !out.ends_with(['.', '!', '?']) {
        out.push('.');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    /// A port nothing is listening on. Another program could take it
    /// between this call and its use; the tests accept that small chance.
    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn settings(port: u16) -> Settings {
        // The server's probe client needs a TLS provider, as in ``main``.
        let _ = rustls::crypto::ring::default_provider().install_default();
        Settings {
            address: "127.0.0.1".into(),
            port_number: port,
            logging_enabled: false,
            ..Settings::default()
        }
    }

    fn supervisor(port: u16) -> Supervisor {
        let mut s = Supervisor::new(
            settings(port),
            None,
            None,
            false,
            Arc::new(AtomicUsize::new(0)),
        );
        s.stay_up();
        s
    }

    async fn reachable(port: u16) -> bool {
        tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
    }

    #[test]
    fn errors_read_as_sentences() {
        assert_eq!(sentence("could not listen"), "Could not listen.");
        assert_eq!(sentence("Already fine."), "Already fine.");
        assert_eq!(sentence("  "), "");
    }

    #[tokio::test]
    async fn stops_and_starts_on_the_same_port() {
        let port = free_port();
        let mut s = supervisor(port);
        s.start().await.unwrap();
        assert!(reachable(port).await);
        let status = s.status().borrow().clone();
        assert!(status.serving);
        assert_eq!(
            status.url.as_deref(),
            Some(&*format!("http://127.0.0.1:{port}"))
        );

        s.stop().await;
        assert!(!reachable(port).await);
        assert!(!s.status().borrow().serving);

        s.start().await.unwrap();
        assert!(reachable(port).await);
        s.stop().await;
    }

    #[tokio::test]
    async fn apply_moves_to_a_new_port() {
        let (old, new) = (free_port(), free_port());
        let mut s = supervisor(old);
        s.start().await.unwrap();
        s.apply(settings(new), false, true).await.unwrap();
        assert!(reachable(new).await);
        assert!(!reachable(old).await);
        assert_eq!(s.status().borrow().port, new);
        s.stop().await;
    }

    #[tokio::test]
    async fn a_taken_port_leaves_the_server_where_it_was() {
        let old = free_port();
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let taken_port = taken.local_addr().unwrap().port();
        let mut s = supervisor(old);
        s.start().await.unwrap();

        let error = s
            .apply(settings(taken_port), false, true)
            .await
            .unwrap_err();
        assert!(error.contains(&taken_port.to_string()), "{error}");
        assert!(reachable(old).await);
        let status = s.status().borrow().clone();
        assert!(status.serving);
        assert_eq!(status.port, old);
        s.stop().await;
    }

    #[tokio::test]
    async fn apply_changes_the_cap_without_moving() {
        let port = free_port();
        let mut s = supervisor(port);
        s.start().await.unwrap();
        let mut new = settings(port);
        new.max_ws_connections = 3;
        s.apply(new, false, false).await.unwrap();
        assert_eq!(s.max_ws().load(Ordering::Acquire), 3);
        assert!(reachable(port).await);
        s.stop().await;
    }

    #[tokio::test]
    async fn a_command_line_port_holds_until_one_is_chosen() {
        let (file, cli, chosen) = (free_port(), free_port(), free_port());
        let mut s = Supervisor::new(
            settings(file),
            None,
            Some(cli),
            false,
            Arc::new(AtomicUsize::new(0)),
        );
        s.start().await.unwrap();
        assert!(reachable(cli).await);
        assert!(s.status().borrow().port_from_cli);

        // Saving something else keeps the command line's port.
        let mut other = settings(file);
        other.max_ws_connections = 5;
        s.apply(other, false, false).await.unwrap();
        assert!(reachable(cli).await);

        // Choosing a port ends the override.
        s.apply(settings(chosen), false, true).await.unwrap();
        assert!(reachable(chosen).await);
        assert!(!reachable(cli).await);
        assert!(!s.status().borrow().port_from_cli);
        s.stop().await;
    }

    #[tokio::test]
    async fn run_answers_commands_until_quit() {
        let port = free_port();
        let mut s = supervisor(port);
        s.start().await.unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();

        let (reply, stopped) = oneshot::channel();
        tx.send(Command::Stop(Some(reply))).unwrap();
        let (reply, started) = oneshot::channel();
        tx.send(Command::Start(Some(reply))).unwrap();
        tx.send(Command::Quit).unwrap();

        s.run(&mut rx, std::future::pending()).await.unwrap();
        assert_eq!(stopped.await.unwrap(), Ok(()));
        assert_eq!(started.await.unwrap(), Ok(()));
        assert!(reachable(port).await);
        s.shutdown().await;
        assert!(!reachable(port).await);
    }
}
