//! What Settings changes, and the control API that lets a page do it.
//!
//! ``Panel`` is Settings' own way in: what Vigil is doing, and the means
//! to change it. The Settings window (``settings/``) calls it directly.
//! The Settings page of the web dashboard, shown in the full dashboard's
//! window, reaches the same ``Panel`` through the API below, so the two
//! check and save everything alike.
//!
//! The API is served only by the window's private listener (``127.0.0.1``, a
//! port of its own), never by the public server, so nothing on the
//! network can reach it whatever address Vigil listens on. On top of that
//! every request to the private listener, the dashboard's files included,
//! passes ``guard``:
//!
//! - the ``Host`` header must name this listener, which stops a web page
//!   on another site from reaching it through a DNS name it controls;
//! - the caller must hold the token made for this run. The window gets it
//!   in its first address (``/control/session?token=...``), which swaps it
//!   for a cookie the page's scripts can't read and other sites' requests
//!   don't carry (``HttpOnly``, ``SameSite=Strict``);
//! - a request that changes something and arrives with the cookie must
//!   also come from a page this listener served.
//!
//! | | |
//! |---|---|
//! | ``GET /control/status`` | version, whether the server is up and where, connections, file locations |
//! | ``GET /control/settings`` | the settings file |
//! | ``PUT /control/settings`` | change some settings (a JSON object of the keys to change) and apply them |
//! | ``POST /control/server/{start,stop,restart}`` | the public server |
//! | ``GET /control/logs?after=N`` | log lines newer than line ``N`` |
//! | ``POST /control/open/{logs,settings,browser}`` | show the log folder, the settings file, or the dashboard in the browser |
//! | ``POST /control/quit`` | exit Vigil |

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{oneshot, watch};

use super::http::json_response;
use crate::config;
use crate::supervisor::{Command, Reply, Status};

const COOKIE: &str = "vigil_session";

/// What Settings shows and changes, however it is drawn: the supervisor
/// to ask, and where to read what it is doing.
#[derive(Clone)]
pub struct Panel {
    pub commands: UnboundedSender<Command>,
    pub status: watch::Receiver<Status>,
    /// The public server's open WebSocket sessions and their cap.
    pub active_ws: Arc<AtomicUsize>,
    pub max_ws: Arc<AtomicUsize>,
    pub started: Instant,
}

/// Why something asked of a ``Panel`` wasn't done: a sentence fit to
/// show, and the status the API answers with.
#[derive(Debug)]
pub struct Refusal {
    pub code: StatusCode,
    pub message: String,
}

impl Refusal {
    fn new(code: StatusCode, message: impl Into<String>) -> Refusal {
        Refusal {
            code,
            message: message.into(),
        }
    }
}

/// What the public server can be asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerAction {
    Start,
    Stop,
    Restart,
}

/// What can be shown outside Vigil's windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenTarget {
    /// The folder the log file is in.
    Logs,
    /// The settings file, in its usual editor.
    Settings,
    /// The dashboard, in the default browser.
    Browser,
}

impl Panel {
    /// Where things stand: the server, its connections, how long Vigil
    /// has run and where its files are.
    pub fn status(&self) -> StatusBody {
        StatusBody {
            version: env!("CARGO_PKG_VERSION"),
            server: self.status.borrow().clone(),
            connections: Connections {
                active: self.active_ws.load(Ordering::Acquire),
                max: self.max_ws.load(Ordering::Acquire),
            },
            uptime_secs: self.started.elapsed().as_secs(),
            paths: Paths {
                settings: config::settings_file().display().to_string(),
                log: crate::log::log_file_path().display().to_string(),
            },
            log_level_locked: crate::log::env_override(),
        }
    }

    /// Sends ``command`` to the supervisor and waits for its answer.
    async fn ask(&self, command: impl FnOnce(Reply) -> Command) -> Result<(), String> {
        let (reply, answer) = oneshot::channel();
        self.commands
            .send(command(reply))
            .map_err(|_| "Vigil is shutting down.".to_string())?;
        answer
            .await
            .unwrap_or_else(|_| Err("Vigil is shutting down.".into()))
    }

    /// Changes the settings named in ``changes`` (a JSON object of the
    /// keys to change). They are applied first, so a port that can't be
    /// used is reported and nothing is saved; the file is written once
    /// the running server has taken them.
    pub async fn change(&self, changes: &serde_json::Value) -> Result<Changed, Refusal> {
        let new = config::read_settings()
            .merged(changes)
            .map_err(|e| Refusal::new(StatusCode::BAD_REQUEST, e))?;
        new.validate()
            .map_err(|e| Refusal::new(StatusCode::BAD_REQUEST, e))?;
        self.ask(|reply| Command::Apply {
            settings: Box::new(new.clone()),
            address_set: changes.get("address").is_some(),
            port_set: changes.get("port_number").is_some(),
            reply,
        })
        .await
        .map_err(|e| Refusal::new(StatusCode::CONFLICT, e))?;
        if let Err(e) = config::save_settings(&new) {
            let message = format!(
                "The change is in use, but the settings file couldn't be written, \
                 so it won't last past the next start: {e}."
            );
            tracing::error!("{message}");
            return Err(Refusal::new(StatusCode::INTERNAL_SERVER_ERROR, message));
        }
        tracing::info!("Settings saved");
        Ok(Changed {
            settings: new,
            status: self.status(),
        })
    }

    /// Starts, stops or restarts the public server, and says where things
    /// stand afterwards.
    pub async fn server(&self, action: ServerAction) -> Result<StatusBody, Refusal> {
        let result = match action {
            ServerAction::Start => self.ask(|reply| Command::Start(Some(reply))).await,
            ServerAction::Stop => self.ask(|reply| Command::Stop(Some(reply))).await,
            ServerAction::Restart => self.ask(|reply| Command::Restart(Some(reply))).await,
        };
        match result {
            Ok(()) => Ok(self.status()),
            Err(e) => Err(Refusal::new(StatusCode::CONFLICT, e)),
        }
    }

    /// Shows something outside the window: a folder, a file, or the
    /// dashboard in the default browser.
    pub fn open(&self, target: OpenTarget) -> Result<(), Refusal> {
        let what = match target {
            OpenTarget::Logs => crate::log::log_dir().display().to_string(),
            OpenTarget::Settings => config::settings_file().display().to_string(),
            OpenTarget::Browser => self.status.borrow().url.clone().ok_or_else(|| {
                Refusal::new(
                    StatusCode::CONFLICT,
                    "The server is stopped, so there's nothing for a browser to open.",
                )
            })?,
        };
        open_with_system(&what).map_err(|e| {
            Refusal::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Couldn't open {what}: {e}."),
            )
        })
    }

    /// Stops everything and exits.
    pub fn quit(&self) {
        let _ = self.commands.send(Command::Quit);
    }
}

#[derive(Clone)]
pub struct Control {
    /// The secret for this run. Only the desktop window is given it.
    pub token: Arc<str>,
    /// The private listener's port, for checking ``Host`` and ``Origin``.
    pub port: u16,
    pub panel: Panel,
}

impl Control {
    /// The names this listener answers to, as a ``Host`` header has them.
    fn hosts(&self) -> [String; 3] {
        [
            format!("127.0.0.1:{}", self.port),
            format!("localhost:{}", self.port),
            format!("[::1]:{}", self.port),
        ]
    }

    fn host_allowed(&self, headers: &HeaderMap) -> bool {
        header_str(headers, header::HOST)
            .is_some_and(|host| self.hosts().iter().any(|h| h.eq_ignore_ascii_case(host)))
    }

    fn origin_allowed(&self, origin: &str) -> bool {
        origin
            .strip_prefix("http://")
            .is_some_and(|host| self.hosts().iter().any(|h| h.eq_ignore_ascii_case(host)))
    }

    /// Compares in constant time, so how long a wrong guess takes says
    /// nothing about how much of it was right.
    fn token_matches(&self, candidate: &str) -> bool {
        let (a, b) = (self.token.as_bytes(), candidate.as_bytes());
        a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
    }
}

fn header_str(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    header_str(headers, header::AUTHORIZATION)?.strip_prefix("Bearer ")
}

fn session_cookie(headers: &HeaderMap) -> Option<&str> {
    header_str(headers, header::COOKIE)?
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find_map(|(name, value)| (name == COOKIE).then_some(value))
}

/// Lets a request through to the private listener only when it comes
/// from the desktop window (see the module docs).
pub async fn guard(State(control): State<Control>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    if !control.host_allowed(headers) {
        return refuse(StatusCode::FORBIDDEN, "This address isn't served here.");
    }
    // The way in: it checks the token itself.
    if request.uri().path() == "/control/session" {
        return next.run(request).await;
    }
    let by_header = bearer(headers).is_some_and(|t| control.token_matches(t));
    let by_cookie = session_cookie(headers).is_some_and(|t| control.token_matches(t));
    if !(by_header || by_cookie) {
        return refuse(
            StatusCode::UNAUTHORIZED,
            "This page belongs to the Vigil window. Open Vigil from its tray icon.",
        );
    }
    // A cookie travels with any request the browser makes, so a request
    // that changes something must also come from one of our own pages. A
    // header can only have been set by a caller that knows the token.
    let changes = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    if changes && !by_header {
        let own = header_str(headers, header::ORIGIN).is_none_or(|o| control.origin_allowed(o));
        if !own {
            return refuse(
                StatusCode::FORBIDDEN,
                "That request came from another site.",
            );
        }
    }
    next.run(request).await
}

pub fn routes(control: Control) -> Router {
    Router::new()
        .route("/control/session", get(session))
        .route("/control/status", get(status))
        .route("/control/settings", get(settings).put(change_settings))
        .route("/control/server/{action}", post(server_action))
        .route("/control/logs", get(logs))
        .route("/control/open/{target}", post(open))
        .route("/control/quit", post(quit))
        .with_state(control)
}

#[derive(Serialize)]
struct Failure<'a> {
    error: &'a str,
}

fn refuse(code: StatusCode, message: &str) -> Response {
    let mut response = json_response(&Failure { error: message });
    *response.status_mut() = code;
    response
}

#[derive(Deserialize)]
struct SessionQuery {
    token: String,
    next: Option<String>,
}

/// Swaps the token in the window's first address for the session cookie
/// and sends the window on to the dashboard or the Settings page.
async fn session(State(control): State<Control>, Query(query): Query<SessionQuery>) -> Response {
    if !control.token_matches(&query.token) {
        return refuse(StatusCode::UNAUTHORIZED, "That link has expired.");
    }
    let location = match query.next.as_deref() {
        Some("settings") => "/settings",
        _ => "/",
    };
    let cookie = format!(
        "{COOKIE}={}; Path=/; HttpOnly; SameSite=Strict",
        control.token
    );
    let mut response = StatusCode::SEE_OTHER.into_response();
    let headers = response.headers_mut();
    headers.insert(header::LOCATION, HeaderValue::from_static(location));
    if let Ok(cookie) = HeaderValue::from_str(&cookie) {
        headers.insert(header::SET_COOKIE, cookie);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[derive(Clone, Debug, Serialize)]
pub struct Connections {
    pub active: usize,
    pub max: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct Paths {
    pub settings: String,
    pub log: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct StatusBody {
    pub version: &'static str,
    #[serde(flatten)]
    pub server: Status,
    pub connections: Connections,
    /// Seconds since Vigil started.
    pub uptime_secs: u64,
    pub paths: Paths,
    /// ``RUST_LOG`` is set, so the log level in the settings has no effect.
    pub log_level_locked: bool,
}

fn refused(refusal: &Refusal) -> Response {
    refuse(refusal.code, &refusal.message)
}

async fn status(State(control): State<Control>) -> Response {
    json_response(&control.panel.status())
}

async fn settings() -> Response {
    json_response(&config::read_settings())
}

/// The settings as they are after a change, and where things stand.
#[derive(Clone, Debug, Serialize)]
pub struct Changed {
    pub settings: config::Settings,
    pub status: StatusBody,
}

async fn change_settings(
    State(control): State<Control>,
    Json(changes): Json<serde_json::Value>,
) -> Response {
    match control.panel.change(&changes).await {
        Ok(changed) => json_response(&changed),
        Err(refusal) => refused(&refusal),
    }
}

async fn server_action(State(control): State<Control>, Path(action): Path<String>) -> Response {
    let action = match action.as_str() {
        "start" => ServerAction::Start,
        "stop" => ServerAction::Stop,
        "restart" => ServerAction::Restart,
        _ => return refuse(StatusCode::NOT_FOUND, "There's no such action."),
    };
    match control.panel.server(action).await {
        Ok(status) => json_response(&status),
        Err(refusal) => refused(&refusal),
    }
}

#[derive(Deserialize)]
struct LogsQuery {
    after: Option<u64>,
}

#[derive(Serialize)]
struct Logs {
    lines: Vec<crate::log::Line>,
    /// The newest line's number: pass it as ``after`` next time.
    newest: u64,
}

async fn logs(Query(query): Query<LogsQuery>) -> Response {
    let (lines, newest) = crate::log::lines_after(query.after.unwrap_or(0));
    json_response(&Logs { lines, newest })
}

async fn open(State(control): State<Control>, Path(target): Path<String>) -> Response {
    let target = match target.as_str() {
        "logs" => OpenTarget::Logs,
        "settings" => OpenTarget::Settings,
        "browser" => OpenTarget::Browser,
        _ => {
            return refuse(
                StatusCode::NOT_FOUND,
                "There's nothing by that name to open.",
            );
        }
    };
    match control.panel.open(target) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(refusal) => refused(&refusal),
    }
}

/// Opens a path or web address with whatever the system uses for it.
fn open_with_system(what: &str) -> std::io::Result<()> {
    // Explorer opens folders, files (in their usual program) and web
    // addresses (in the default browser) alike.
    #[cfg(windows)]
    let program = "explorer.exe";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(any(windows, target_os = "macos")))]
    let program = "xdg-open";
    std::process::Command::new(program)
        .arg(what)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}

async fn quit(State(control): State<Control>) -> Response {
    control.panel.quit();
    StatusCode::ACCEPTED.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    fn control() -> Control {
        let (commands, _) = mpsc::unbounded_channel();
        let (_, status) = watch::channel(Status::default());
        Control {
            token: "secret-token".into(),
            port: 5123,
            panel: Panel {
                commands,
                status,
                active_ws: Arc::new(AtomicUsize::new(0)),
                max_ws: Arc::new(AtomicUsize::new(20)),
                started: Instant::now(),
            },
        }
    }

    fn headers(pairs: &[(header::HeaderName, &str)]) -> HeaderMap {
        pairs
            .iter()
            .map(|(name, value)| (name.clone(), HeaderValue::from_str(value).unwrap()))
            .collect()
    }

    #[test]
    fn only_this_listeners_names_are_hosts() {
        let c = control();
        assert!(c.host_allowed(&headers(&[(header::HOST, "127.0.0.1:5123")])));
        assert!(c.host_allowed(&headers(&[(header::HOST, "LOCALHOST:5123")])));
        assert!(!c.host_allowed(&headers(&[(header::HOST, "127.0.0.1:4500")])));
        assert!(!c.host_allowed(&headers(&[(header::HOST, "evil.example:5123")])));
        assert!(!c.host_allowed(&HeaderMap::new()));
    }

    #[test]
    fn origins_must_be_our_own_pages() {
        let c = control();
        assert!(c.origin_allowed("http://127.0.0.1:5123"));
        assert!(!c.origin_allowed("https://127.0.0.1:5123"));
        assert!(!c.origin_allowed("http://example.com"));
        assert!(!c.origin_allowed("null"));
    }

    #[test]
    fn tokens_match_exactly() {
        let c = control();
        assert!(c.token_matches("secret-token"));
        assert!(!c.token_matches("secret-tokem"));
        assert!(!c.token_matches("secret"));
        assert!(!c.token_matches(""));
    }

    #[test]
    fn finds_the_session_cookie_among_others() {
        let h = headers(&[(header::COOKIE, "theme=dark; vigil_session=abc; x=1")]);
        assert_eq!(session_cookie(&h), Some("abc"));
        let h = headers(&[(header::COOKIE, "theme=dark")]);
        assert_eq!(session_cookie(&h), None);
        let h = headers(&[(header::AUTHORIZATION, "Bearer abc")]);
        assert_eq!(bearer(&h), Some("abc"));
    }
}
