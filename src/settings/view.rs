//! Fills in the Settings layout and turns what is pressed on it into
//! requests.
//!
//! This is everything about the window that needs no Windows: what it
//! shows of the collector's state, the forms and their verdicts
//! (``form.rs``), the log (``log.rs``), and what each button asks for.
//! The asking itself is left to whoever holds the view (``host.rs``, or a
//! test): a ``Request`` goes out through the function given to ``new``,
//! and how a ``Job`` went comes back through ``finished``.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use super::form;
use super::log::Log;
use crate::config::{MAX_PROBES, Settings};
use crate::log::Line;
use crate::native::ui::{Check, Form, LogRow, Note, ServerVerdict, SettingsWindow, Verdict};
use crate::server::control::{Changed, OpenTarget, ServerAction, StatusBody};

/// How long "Saved." and the like stay beside a button.
const NOTE_FOR: Duration = Duration::from_secs(4);

const NOTE_OK: i32 = 1;
const NOTE_ERROR: i32 = 2;

/// The parts of Settings that each do one thing at a time and say how it
/// went: the page's sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Server = 0,
    Connections,
    Logs,
    Checks,
    About,
}

/// Something for the collector to do.
#[derive(Debug, PartialEq)]
pub enum Work {
    /// Change these settings (a JSON object of the keys to change).
    Change(Value),
    Server(ServerAction),
    Open(OpenTarget),
    Quit,
}

/// One press of a button that has the collector do something. The
/// section is busy until ``View::finished`` is told how it went.
#[derive(Debug, PartialEq)]
pub struct Job {
    pub section: Section,
    /// What to say beside the button if it works, if anything.
    pub done: &'static str,
    pub work: Work,
}

/// What the layout asks of whoever holds it.
#[derive(Debug, PartialEq)]
pub enum Request {
    Job(Job),
    /// Have the compact window follow the readings live, or stop.
    Live(bool),
    /// Open a web address in the browser.
    Link(String),
    Minimize,
    Close,
}

/// How a job went, when it went well.
#[derive(Debug)]
pub enum Outcome {
    /// Settings were changed: what is saved now, and where things stand.
    Changed(Box<Changed>),
    /// The server was started or stopped: where things stand.
    Status(Box<StatusBody>),
    Done,
}

struct State {
    /// Where things stood when last asked.
    status: StatusBody,
    /// The settings as they were last saved.
    saved: Settings,
    /// When each section's note is to be taken away, if it is.
    notes_until: [Option<Instant>; 5],
}

/// What the layout's callbacks share.
struct Inner {
    state: RefCell<State>,
    log: RefCell<Log>,
    /// The rows of the service checks form, as typed.
    checks: Rc<VecModel<Check>>,
    send: Box<dyn Fn(Request)>,
}

/// The Settings layout, filled in and listening.
pub struct View {
    ui: SettingsWindow,
    inner: Rc<Inner>,
}

impl View {
    /// ``status`` and ``saved`` are where things stand and what is in the
    /// settings file as the window opens. ``send`` is called for
    /// everything the layout asks for.
    pub fn new(
        ui: SettingsWindow,
        status: StatusBody,
        saved: Settings,
        send: impl Fn(Request) + 'static,
    ) -> View {
        let inner = Rc::new(Inner {
            state: RefCell::new(State {
                status,
                saved,
                notes_until: [None; 5],
            }),
            log: RefCell::new(Log::default()),
            checks: Rc::new(VecModel::default()),
            send: Box::new(send),
        });
        ui.global::<Form>()
            .set_checks(ModelRc::from(inner.checks.clone()));
        inner.show_status(&ui);
        inner.show_saved(&ui);
        inner.fill_server_form(&ui);
        inner.fill_connections_form(&ui);
        inner.fill_checks_form(&ui);
        listen(&ui, &inner);
        View { ui, inner }
    }

    pub fn ui(&self) -> &SettingsWindow {
        &self.ui
    }

    /// Shows whether the compact window follows the readings live.
    pub fn show_live(&self, live: bool) {
        self.ui.global::<Form>().set_live(live);
    }

    /// The number of the newest log line shown, to ask for those after.
    pub fn newest_log(&self) -> u64 {
        self.inner.log.borrow().newest()
    }

    /// Where things stand now. The tray menu can stop, start or move the
    /// server too, and the server form follows it unless it has an
    /// unsaved edit in it.
    pub fn show(&self, status: StatusBody) {
        let form = self.ui.global::<Form>();
        let before = {
            let mut state = self.inner.state.borrow_mut();
            std::mem::replace(&mut state.status, status).server
        };
        let now = self.inner.state.borrow().status.server.clone();
        self.inner.show_status(&self.ui);
        if now == before {
            return;
        }
        if now.address != before.address || now.port != before.port {
            let untouched = form::server(
                form.get_scope(),
                &form.get_custom(),
                &form.get_port(),
                &before,
            );
            if !untouched.dirty {
                self.inner.fill_server_form(&self.ui);
            }
        }
        revise(&form);
    }

    /// Takes log lines newer than those shown; ``newest`` is the number
    /// of the newest there is (see ``log::lines_after``).
    pub fn take_log(&self, lines: Vec<Line>, newest: u64) {
        let (changed, any) = {
            let mut log = self.inner.log.borrow_mut();
            (log.take(lines, newest), !log.is_empty())
        };
        let form = self.ui.global::<Form>();
        form.set_log_any(any);
        if changed {
            form.set_log_revision(form.get_log_revision().wrapping_add(1));
        }
    }

    /// A job came back: shows how it went beside its button, and what it
    /// changed.
    pub fn finished(
        &self,
        section: Section,
        done: &str,
        result: Result<Outcome, String>,
        now: Instant,
    ) {
        let form = self.ui.global::<Form>();
        set_busy(&form, section, false);
        match result {
            Ok(outcome) => {
                match outcome {
                    Outcome::Changed(changed) => {
                        {
                            let mut state = self.inner.state.borrow_mut();
                            state.saved = changed.settings;
                            state.status = changed.status;
                        }
                        self.inner.show_status(&self.ui);
                        self.inner.show_saved(&self.ui);
                        match section {
                            Section::Server => self.inner.fill_server_form(&self.ui),
                            Section::Connections => self.inner.fill_connections_form(&self.ui),
                            Section::Checks => self.inner.fill_checks_form(&self.ui),
                            Section::Logs | Section::About => {}
                        }
                        revise(&form);
                    }
                    Outcome::Status(status) => self.show(*status),
                    Outcome::Done => {}
                }
                if done.is_empty() {
                    self.inner.say(&form, section, 0, "", now);
                } else {
                    self.inner.say(&form, section, NOTE_OK, done, now);
                }
            }
            Err(why) => self.inner.say(&form, section, NOTE_ERROR, &why, now),
        }
    }

    /// Takes away the notes that have been up long enough.
    pub fn tidy(&self, now: Instant) {
        let form = self.ui.global::<Form>();
        let due: Vec<Section> = {
            let mut state = self.inner.state.borrow_mut();
            SECTIONS
                .into_iter()
                .filter(|section| {
                    let until = &mut state.notes_until[*section as usize];
                    let due = until.is_some_and(|until| now >= until);
                    if due {
                        *until = None;
                    }
                    due
                })
                .collect()
        };
        for section in due {
            set_note(&form, section, Note::default());
        }
    }
}

const SECTIONS: [Section; 5] = [
    Section::Server,
    Section::Connections,
    Section::Logs,
    Section::Checks,
    Section::About,
];

/// Has the forms' verdicts worked out again: what they are compared with
/// has changed.
fn revise(form: &Form) {
    form.set_revision(form.get_revision().wrapping_add(1));
}

fn set_busy(form: &Form, section: Section, busy: bool) {
    match section {
        Section::Server => form.set_server_busy(busy),
        Section::Connections => form.set_connections_busy(busy),
        Section::Logs => form.set_logs_busy(busy),
        Section::Checks => form.set_checks_busy(busy),
        Section::About => form.set_about_busy(busy),
    }
}

fn busy(form: &Form, section: Section) -> bool {
    match section {
        Section::Server => form.get_server_busy(),
        Section::Connections => form.get_connections_busy(),
        Section::Logs => form.get_logs_busy(),
        Section::Checks => form.get_checks_busy(),
        Section::About => form.get_about_busy(),
    }
}

fn set_note(form: &Form, section: Section, note: Note) {
    match section {
        Section::Server => form.set_server_note(note),
        Section::Connections => form.set_connections_note(note),
        Section::Logs => form.set_logs_note(note),
        Section::Checks => form.set_checks_note(note),
        Section::About => form.set_about_note(note),
    }
}

impl Inner {
    /// Shows a note beside a section's buttons, or with no text takes it
    /// away. One that says something worked goes again by itself.
    fn say(&self, form: &Form, section: Section, kind: i32, text: &str, now: Instant) {
        self.state.borrow_mut().notes_until[section as usize] =
            (kind == NOTE_OK).then(|| now + NOTE_FOR);
        set_note(
            form,
            section,
            Note {
                kind,
                text: text.into(),
            },
        );
    }

    /// Starts a job, unless the section is still busy with the last one.
    fn job(&self, ui: &SettingsWindow, section: Section, done: &'static str, work: Work) {
        let form = ui.global::<Form>();
        if busy(&form, section) {
            return;
        }
        set_busy(&form, section, true);
        (self.send)(Request::Job(Job {
            section,
            done,
            work,
        }));
    }

    /// Shows where things stand (``State::status``).
    fn show_status(&self, ui: &SettingsWindow) {
        let state = self.state.borrow();
        let status = &state.status;
        let form = ui.global::<Form>();
        form.set_serving(status.server.serving);
        form.set_listen(status.server.listen.as_deref().unwrap_or_default().into());
        form.set_server_error(status.server.error.as_deref().unwrap_or_default().into());
        form.set_from_command_line(form::from_command_line(&status.server).into());
        form.set_connections_active(status.connections.active as i32);
        form.set_connections_max(status.connections.max as i32);
        form.set_log_level_locked(status.log_level_locked);
        form.set_version(status.version.into());
        form.set_uptime(form::uptime(status.uptime_secs).into());
        form.set_settings_path(status.paths.settings.as_str().into());
        form.set_log_path(status.paths.log.as_str().into());
    }

    /// Shows the settings that are changed with one press and have no
    /// form to fill in.
    fn show_saved(&self, ui: &SettingsWindow) {
        let state = self.state.borrow();
        let form = ui.global::<Form>();
        form.set_logging(state.saved.logging_enabled);
        form.set_log_level(form::level_shown(&state.saved.log_level).into());
    }

    fn fill_server_form(&self, ui: &SettingsWindow) {
        let (address, port) = {
            let state = self.state.borrow();
            (
                state.status.server.address.clone(),
                state.status.server.port,
            )
        };
        let scope = form::scope_of(&address);
        let form = ui.global::<Form>();
        form.set_scope(scope);
        form.set_custom(if scope == form::CUSTOM {
            address.into()
        } else {
            SharedString::default()
        });
        form.set_port(port.to_string().into());
    }

    fn fill_connections_form(&self, ui: &SettingsWindow) {
        let (limit, interval) = {
            let state = self.state.borrow();
            (state.saved.max_ws_connections, state.saved.ws_push_interval)
        };
        let form = ui.global::<Form>();
        form.set_limit(limit.to_string().into());
        form.set_interval(form::shown(interval).into());
    }

    fn fill_checks_form(&self, ui: &SettingsWindow) {
        let (probes, timeout) = {
            let state = self.state.borrow();
            (state.saved.probes.clone(), state.saved.probe_timeout)
        };
        let rows: Vec<Check> = probes
            .iter()
            .map(|probe| Check {
                name: probe.name.as_str().into(),
                url: probe.url.as_str().into(),
            })
            .collect();
        // Left alone when it already says the same, so a field being
        // typed in isn't made again under the cursor.
        if self.checks.iter().ne(rows.iter().cloned()) {
            self.checks.set_vec(rows);
            // The pointer may have been over a field that is now gone.
            ui.invoke_forget_pointer();
        }
        let form = ui.global::<Form>();
        form.set_timeout(form::shown(timeout).into());
        form.set_checks_edits(form.get_checks_edits().wrapping_add(1));
    }

    /// The service checks as typed.
    fn check_rows(&self) -> Vec<(String, String)> {
        self.checks
            .iter()
            .map(|check| (check.name.to_string(), check.url.to_string()))
            .collect()
    }
}

/// Connects the layout's callbacks.
fn listen(ui: &SettingsWindow, inner: &Rc<Inner>) {
    let form = ui.global::<Form>();

    // What the forms come to, worked out again whenever one changes.
    form.on_server_verdict({
        let inner = inner.clone();
        move |scope, custom, port, _revision| {
            let state = inner.state.borrow();
            let verdict = form::server(scope, &custom, &port, &state.status.server);
            ServerVerdict {
                dirty: verdict.dirty,
                savable: verdict.savable,
                port_valid: verdict.port_valid,
            }
        }
    });
    form.on_connections_verdict({
        let inner = inner.clone();
        move |limit, interval, _revision| {
            let state = inner.state.borrow();
            let verdict = form::connections(&limit, &interval, &state.saved);
            Verdict {
                dirty: verdict.dirty,
                problem: verdict.problem.into(),
            }
        }
    });
    form.on_checks_verdict({
        let inner = inner.clone();
        move |timeout, _edits, _revision| {
            let rows = inner.check_rows();
            let state = inner.state.borrow();
            let verdict = form::checks(&rows, &timeout, &state.saved);
            Verdict {
                dirty: verdict.dirty,
                problem: verdict.problem.into(),
            }
        }
    });

    // The log, a row at a time.
    form.on_log_rows({
        let inner = inner.clone();
        move |columns, filter, _revision| {
            inner
                .log
                .borrow_mut()
                .rows(columns.max(1) as usize, &filter) as i32
        }
    });
    form.on_log_row({
        let inner = inner.clone();
        move |row, columns, filter, _revision| {
            let mut log = inner.log.borrow_mut();
            usize::try_from(row)
                .ok()
                .and_then(|row| log.row(row, columns.max(1) as usize, &filter))
                .map(|(text, level)| LogRow {
                    text: text.into(),
                    level,
                })
                .unwrap_or_default()
        }
    });

    // The buttons. Each gets the layout and what the callbacks share.
    let with = |act: fn(&SettingsWindow, &Inner)| {
        let (ui, inner) = (ui.as_weak(), inner.clone());
        move || {
            if let Some(ui) = ui.upgrade() {
                act(&ui, &inner);
            }
        }
    };

    form.on_start(with(|ui, inner| {
        let work = Work::Server(ServerAction::Start);
        inner.job(ui, Section::Server, "Serving again.", work);
    }));
    form.on_stop(with(|ui, inner| {
        let work = Work::Server(ServerAction::Stop);
        inner.job(ui, Section::Server, "Stopped serving.", work);
    }));
    form.on_restart(with(|ui, inner| {
        let work = Work::Server(ServerAction::Restart);
        inner.job(ui, Section::Server, "Restarted.", work);
    }));
    form.on_open_browser(with(|ui, inner| {
        inner.job(ui, Section::Server, "", Work::Open(OpenTarget::Browser));
    }));
    form.on_save_server(with(|ui, inner| {
        let form = ui.global::<Form>();
        let (verdict, serving) = {
            let state = inner.state.borrow();
            let server = &state.status.server;
            (
                form::server(
                    form.get_scope(),
                    &form.get_custom(),
                    &form.get_port(),
                    server,
                ),
                server.serving,
            )
        };
        if verdict.savable {
            let done = if serving {
                "Saved. Vigil is now serving on the new address."
            } else {
                "Saved."
            };
            let work = Work::Change(Value::Object(verdict.changes));
            inner.job(ui, Section::Server, done, work);
        }
    }));
    form.on_reset_server(with(|ui, inner| inner.fill_server_form(ui)));

    form.on_save_connections(with(|ui, inner| {
        let form = ui.global::<Form>();
        let verdict = form::connections(
            &form.get_limit(),
            &form.get_interval(),
            &inner.state.borrow().saved,
        );
        if verdict.dirty && verdict.problem.is_empty() {
            let work = Work::Change(Value::Object(verdict.changes));
            inner.job(ui, Section::Connections, "Saved.", work);
        }
    }));

    // Nothing to wait for or save to the settings file: it is the
    // compact window's own, and takes effect as it is pressed.
    form.on_set_live({
        let (ui, inner) = (ui.as_weak(), inner.clone());
        move |live| {
            if let Some(ui) = ui.upgrade() {
                ui.global::<Form>().set_live(live);
                (inner.send)(Request::Live(live));
            }
        }
    });

    form.on_set_logging({
        let (ui, inner) = (ui.as_weak(), inner.clone());
        move |on| {
            if let Some(ui) = ui.upgrade() {
                let work = Work::Change(json!({ "logging_enabled": on }));
                inner.job(&ui, Section::Logs, "Saved.", work);
            }
        }
    });
    form.on_set_log_level({
        let (ui, inner) = (ui.as_weak(), inner.clone());
        move |level| {
            if let Some(ui) = ui.upgrade() {
                let work = Work::Change(json!({ "log_level": level.as_str() }));
                inner.job(&ui, Section::Logs, "Saved.", work);
            }
        }
    });
    form.on_open_logs(with(|ui, inner| {
        inner.job(ui, Section::Logs, "", Work::Open(OpenTarget::Logs));
    }));

    form.on_add_check(with(|ui, inner| {
        if inner.checks.row_count() < MAX_PROBES {
            inner.checks.push(Check {
                name: SharedString::default(),
                url: "https://".into(),
            });
            let form = ui.global::<Form>();
            form.set_checks_edits(form.get_checks_edits().wrapping_add(1));
        }
    }));
    form.on_remove_check({
        let (ui, inner) = (ui.as_weak(), inner.clone());
        move |index| {
            let Some(ui) = ui.upgrade() else {
                return;
            };
            if let Ok(index) = usize::try_from(index)
                && index < inner.checks.row_count()
            {
                inner.checks.remove(index);
                let form = ui.global::<Form>();
                form.set_checks_edits(form.get_checks_edits().wrapping_add(1));
            }
        }
    });
    form.on_save_checks(with(|ui, inner| {
        let form = ui.global::<Form>();
        let verdict = form::checks(
            &inner.check_rows(),
            &form.get_timeout(),
            &inner.state.borrow().saved,
        );
        if verdict.dirty && verdict.problem.is_empty() {
            let work = Work::Change(Value::Object(verdict.changes));
            inner.job(
                ui,
                Section::Checks,
                "Saved. The dashboard uses the new list from its next check.",
                work,
            );
        }
    }));
    form.on_reset_checks(with(|ui, inner| inner.fill_checks_form(ui)));

    form.on_open_settings_file(with(|ui, inner| {
        inner.job(ui, Section::About, "", Work::Open(OpenTarget::Settings));
    }));
    form.on_open_link({
        let inner = inner.clone();
        move |address| (inner.send)(Request::Link(address.to_string()))
    });
    form.on_quit(with(|ui, inner| {
        inner.job(ui, Section::About, "", Work::Quit);
    }));

    form.on_minimize(with(|_, inner| (inner.send)(Request::Minimize)));
    form.on_close(with(|_, inner| (inner.send)(Request::Close)));
}
