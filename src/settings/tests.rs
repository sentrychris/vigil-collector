//! Draws the Settings window into a picture, with no screen, presses and
//! types on it, and checks what it shows and asks for. Set
//! ``VIGIL_PREVIEW_DIR`` to a folder and the pictures are also saved
//! there as PNGs, which is how to see a change to ``ui/settings.slint``
//! without a Windows machine.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde_json::json;
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, SharedString};

use super::view::{Job, Outcome, Request, Section, View, Work};
use crate::config::{Probe, Settings};
use crate::log::Line;
use crate::native::surface::{self, Frame, Surface};
use crate::native::ui::{Form, SettingsWindow, Theme};
use crate::native::{self, save_preview};
use crate::server::control::{Changed, Connections, OpenTarget, Paths, ServerAction, StatusBody};
use crate::supervisor::Status;

/// The tabs, by the numbers ``ui/settings.slint`` knows them by.
const SERVER: i32 = 0;
const CONNECTIONS: i32 = 1;
const LOGS: i32 = 2;
const CHECKS: i32 = 3;
const ABOUT: i32 = 4;

pub(super) fn status(address: &str, port: u16) -> StatusBody {
    StatusBody {
        version: "0.2.0",
        server: Status {
            serving: true,
            listen: Some(format!("{address}:{port}")),
            url: Some(format!("http://{address}:{port}")),
            address: address.into(),
            port,
            address_from_cli: false,
            port_from_cli: false,
            error: None,
        },
        connections: Connections { active: 3, max: 20 },
        uptime_secs: 2 * 86_400 + 4 * 3600 + 12 * 60,
        paths: Paths {
            settings: r"C:\Users\chris\AppData\Roaming\Vigil\settings.json".into(),
            log: r"C:\Users\chris\AppData\Roaming\Vigil\logs\vigil.log".into(),
        },
        log_level_locked: false,
    }
}

pub(super) fn settings() -> Settings {
    Settings {
        probes: vec![
            Probe {
                name: "versyx.digital".into(),
                url: "https://versyx.digital".into(),
            },
            Probe {
                name: "status.edcs.app".into(),
                url: "https://status.edcs.app/health".into(),
            },
            Probe {
                name: "Router".into(),
                url: "http://192.168.1.1/".into(),
            },
        ],
        ..Settings::default()
    }
}

/// A made-up stretch of the log, numbered from ``from``.
pub(super) fn log_lines(from: u64, count: u64) -> Vec<Line> {
    let texts = [
        " INFO vigil_collector: Vigil 0.2.0 started",
        " INFO vigil_collector::supervisor: Listening on localhost:4500",
        "DEBUG vigil_collector::server::ws: A dashboard connected from 127.0.0.1:53122 (1 of 20)",
        " WARN vigil_collector::server::probes: status.edcs.app took 1840 ms to answer",
        " INFO vigil_collector::server::control: Settings saved",
        "ERROR vigil_collector::supervisor: Couldn't listen on 0.0.0.0:80: access denied (os error 10013)",
        " INFO vigil_collector::supervisor: Listening on localhost:4500",
    ];
    (from..from + count)
        .map(|seq| Line {
            seq,
            text: format!(
                "2026-11-14T09:{:02}:{:02}Z {}",
                (seq / 60) % 60,
                seq % 60,
                texts[seq as usize % texts.len()]
            ),
        })
        .collect()
}

/// The window as the tests hold it: the layout filled in, where it
/// draws, the picture so far, and everything it has asked for.
struct Shown {
    view: View,
    surface: Rc<Surface>,
    frame: Frame,
    asked: Rc<RefCell<Vec<Request>>>,
}

impl Shown {
    /// The window at ``width`` by ``height`` layout pixels and ``scale``
    /// real pixels to each, on the tab numbered ``tab``.
    fn new(width: u32, height: u32, scale: f32, dark: bool, tab: i32) -> Shown {
        Shown::with(width, height, scale, dark, tab, status("localhost", 4500))
    }

    fn with(width: u32, height: u32, scale: f32, dark: bool, tab: i32, at: StatusBody) -> Shown {
        let (ui, surface) = surface::open(SettingsWindow::new).expect("the layout can be made");
        ui.global::<Theme>().set_dark(dark);
        ui.global::<Form>().set_tab(tab);
        let (real_width, real_height) = (
            (width as f32 * scale) as u32,
            (height as f32 * scale) as u32,
        );
        surface.resize(real_width, real_height, scale);
        native::paint_page(&ui, real_width, real_height, scale);
        let asked = Rc::new(RefCell::new(Vec::new()));
        let view = View::new(ui, at, settings(), {
            let asked = asked.clone();
            move |request| asked.borrow_mut().push(request)
        });
        view.take_log(log_lines(1, 40), 40);
        let mut shown = Shown {
            view,
            surface,
            frame: Frame::default(),
            asked,
        };
        shown.paint();
        shown
    }

    fn form(&self) -> Form<'_> {
        self.view.ui().global::<Form>()
    }

    fn paint(&mut self) -> bool {
        surface::tick();
        self.surface.paint(&mut self.frame)
    }

    /// The colour at a place given in layout pixels.
    fn at(&self, x: f32, y: f32) -> [u8; 3] {
        let scale = self.surface.scale();
        let (x, y) = ((x * scale) as u32, (y * scale) as u32);
        self.frame.pixels[(y * self.frame.width + x) as usize].rgb()
    }

    fn point(&mut self, x: f32, y: f32) {
        let position = LogicalPosition::new(x, y);
        self.surface.send(WindowEvent::PointerMoved { position });
        self.paint();
    }

    fn click(&mut self, x: f32, y: f32) {
        let position = LogicalPosition::new(x, y);
        let button = PointerEventButton::Left;
        self.surface.send(WindowEvent::PointerMoved { position });
        self.surface
            .send(WindowEvent::PointerPressed { position, button });
        self.surface
            .send(WindowEvent::PointerReleased { position, button });
        self.paint();
    }

    fn press(&mut self, key: impl Into<SharedString>) {
        let text = key.into();
        self.surface
            .send(WindowEvent::KeyPressed { text: text.clone() });
        self.surface.send(WindowEvent::KeyReleased { text });
        self.paint();
    }

    fn type_in(&mut self, text: &str) {
        for typed in text.chars() {
            self.press(typed);
        }
    }

    /// Everything asked for since this was last called.
    fn asked(&self) -> Vec<Request> {
        std::mem::take(&mut self.asked.borrow_mut())
    }

    /// Saves the picture as ``name``.png in ``VIGIL_PREVIEW_DIR``, if set.
    fn save(&self, name: &str) {
        save_preview(&self.frame, name);
    }
}

/// How far apart two colours are, as the largest difference in any of
/// red, green and blue.
fn apart(a: [u8; 3], b: [u8; 3]) -> u8 {
    (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap_or(0)
}

#[test]
fn draws_every_tab_at_its_usual_size() {
    for (tab, name) in [
        (SERVER, "settings-server"),
        (CONNECTIONS, "settings-connections"),
        (LOGS, "settings-logs"),
        (CHECKS, "settings-checks"),
        (ABOUT, "settings-about"),
    ] {
        let shown = Shown::new(420, 520, 1.0, true, tab);
        shown.save(name);
        assert_eq!((shown.frame.width, shown.frame.height), (420, 520));
        // The title bar is the compact dashboard's dark slate.
        let bar = shown.at(200.0, 16.0);
        assert!(apart(bar, [0x17, 0x20, 0x31]) < 24, "{name}: {bar:?}");
    }
}

#[test]
fn the_light_theme_is_light() {
    for (tab, name) in [
        (SERVER, "settings-server-light"),
        (LOGS, "settings-logs-light"),
        (CHECKS, "settings-checks-light"),
    ] {
        let shown = Shown::new(420, 520, 1.0, false, tab);
        shown.save(name);
        let page = shown.at(3.0, 300.0);
        assert!(page.iter().all(|c| *c > 0xc0), "{name}: {page:?}");
        assert!(shown.at(200.0, 16.0).iter().all(|c| *c < 0x60));
    }
}

#[test]
fn fits_its_smallest_size_and_a_scaled_display() {
    let shown = Shown::new(360, 320, 1.0, true, SERVER);
    shown.save("settings-smallest");
    assert_eq!((shown.frame.width, shown.frame.height), (360, 320));
    let shown = Shown::new(420, 520, 1.5, true, SERVER);
    shown.save("settings-server-150");
    assert_eq!((shown.frame.width, shown.frame.height), (630, 780));
}

#[test]
fn a_server_that_is_not_serving_says_why() {
    let mut at = status("192.168.1.20", 80);
    at.server.serving = false;
    at.server.listen = None;
    at.server.url = None;
    at.server.error = Some("Couldn't listen on 192.168.1.20:80: access denied.".into());
    at.server.port_from_cli = true;
    let shown = Shown::with(420, 520, 1.0, true, SERVER, at);
    shown.save("settings-server-stopped");
    let form = shown.form();
    assert!(!form.get_serving());
    assert_eq!(form.get_scope(), 2);
    assert_eq!(form.get_custom(), "192.168.1.20");
    assert_eq!(form.get_from_command_line(), "--port 80");
}

#[test]
fn pressing_a_tab_shows_it() {
    let mut shown = Shown::new(420, 520, 1.0, true, SERVER);
    let before = shown.frame.pixels.clone();
    shown.click(232.0, 56.0);
    assert_eq!(shown.form().get_tab(), LOGS);
    assert!(before != shown.frame.pixels, "the picture did not change");
}

/// Where the port's field and the Save button under it are on the Server
/// tab at the usual size.
const PORT_FIELD: (f32, f32) = (60.0, 360.0);
const SAVE_SERVER: (f32, f32) = (42.0, 392.0);

#[test]
fn a_typed_port_is_saved_and_the_form_follows_what_was_saved() {
    let mut shown = Shown::new(420, 520, 1.0, true, SERVER);
    // Nothing to save yet: pressing Save asks for nothing.
    shown.click(SAVE_SERVER.0, SAVE_SERVER.1);
    assert_eq!(shown.asked(), []);

    shown.click(PORT_FIELD.0, PORT_FIELD.1);
    shown.press(Key::End);
    for _ in 0..4 {
        shown.press(Key::Backspace);
    }
    assert_eq!(shown.form().get_port(), "");
    shown.type_in("8080");
    assert_eq!(shown.form().get_port(), "8080");
    // Letters don't go into a field for a number.
    shown.type_in("x");
    assert_eq!(shown.form().get_port(), "8080");
    shown.save("settings-server-edited");

    shown.click(SAVE_SERVER.0, SAVE_SERVER.1);
    let done = "Saved. Vigil is now serving on the new address.";
    assert_eq!(
        shown.asked(),
        [Request::Job(Job {
            section: Section::Server,
            done,
            work: Work::Change(json!({"port_number": 8080})),
        })]
    );
    // Until it comes back, a second press asks for nothing more.
    assert!(shown.form().get_server_busy());
    shown.click(SAVE_SERVER.0, SAVE_SERVER.1);
    shown.form().invoke_stop();
    assert_eq!(shown.asked(), []);

    let now = Instant::now();
    let changed = Changed {
        settings: Settings {
            port_number: 8080,
            ..settings()
        },
        status: status("localhost", 8080),
    };
    shown.view.finished(
        Section::Server,
        done,
        Ok(Outcome::Changed(Box::new(changed))),
        now,
    );
    shown.paint();
    shown.save("settings-server-saved");
    let form = shown.form();
    assert!(!form.get_server_busy());
    assert_eq!(form.get_listen(), "localhost:8080");
    assert_eq!(form.get_server_note().text, done);
    assert_eq!(form.get_server_note().kind, 1);
    let verdict =
        form.invoke_server_verdict(form.get_scope(), form.get_custom(), form.get_port(), 0);
    assert!(!verdict.dirty && !verdict.savable);

    // The note goes by itself after a few seconds.
    shown.view.tidy(now + Duration::from_secs(3));
    assert_eq!(shown.form().get_server_note().text, done);
    shown.view.tidy(now + Duration::from_secs(5));
    assert_eq!(shown.form().get_server_note().text, "");
}

#[test]
fn enter_in_a_field_saves_too() {
    let mut shown = Shown::new(420, 520, 1.0, true, SERVER);
    shown.click(PORT_FIELD.0, PORT_FIELD.1);
    shown.press(Key::End);
    shown.press(Key::Backspace);
    shown.type_in("1");
    shown.press(Key::Return);
    assert_eq!(
        shown.asked(),
        [Request::Job(Job {
            section: Section::Server,
            done: "Saved. Vigil is now serving on the new address.",
            work: Work::Change(json!({"port_number": 4501})),
        })]
    );
}

#[test]
fn what_went_wrong_is_said_and_stays() {
    let mut shown = Shown::new(420, 520, 1.0, true, SERVER);
    shown.form().invoke_restart();
    assert_eq!(
        shown.asked(),
        [Request::Job(Job {
            section: Section::Server,
            done: "Restarted.",
            work: Work::Server(ServerAction::Restart),
        })]
    );
    let now = Instant::now();
    // Long enough to need a second line beside the button.
    let why = "Couldn't listen on 192.168.1.20:4500: the requested address \
               is not valid in its context (os error 10049).";
    shown
        .view
        .finished(Section::Server, "Restarted.", Err(why.into()), now);
    shown.paint();
    shown.save("settings-server-refused");
    shown.view.tidy(now + Duration::from_secs(60));
    let note = shown.form().get_server_note();
    assert_eq!((note.text.as_str(), note.kind), (why, 2));
    assert!(!shown.form().get_server_busy());
}

#[test]
fn stopping_the_server_shows_it_stopped() {
    let shown = Shown::new(420, 520, 1.0, true, SERVER);
    shown.form().invoke_stop();
    assert_eq!(
        shown.asked(),
        [Request::Job(Job {
            section: Section::Server,
            done: "Stopped serving.",
            work: Work::Server(ServerAction::Stop),
        })]
    );
    let mut stopped = status("localhost", 4500);
    stopped.server.serving = false;
    stopped.server.listen = None;
    shown.view.finished(
        Section::Server,
        "Stopped serving.",
        Ok(Outcome::Status(Box::new(stopped))),
        Instant::now(),
    );
    let form = shown.form();
    assert!(!form.get_serving() && !form.get_server_busy());
    assert_eq!(form.get_server_note().text, "Stopped serving.");
}

#[test]
fn the_server_form_follows_the_tray_unless_it_is_being_edited() {
    let shown = Shown::new(420, 520, 1.0, true, SERVER);
    // Stopped from the tray menu.
    let mut stopped = status("localhost", 4500);
    stopped.server.serving = false;
    shown.view.show(stopped);
    assert!(!shown.form().get_serving());

    // Moved by something else while the form is as it was: it follows.
    shown.view.show(status("0.0.0.0", 9000));
    assert_eq!(shown.form().get_scope(), 1);
    assert_eq!(shown.form().get_port(), "9000");

    // With an edit in it, the edit stays.
    shown.form().set_port("1234".into());
    shown.view.show(status("localhost", 9100));
    assert_eq!(shown.form().get_scope(), 1);
    assert_eq!(shown.form().get_port(), "1234");
    assert_eq!(shown.form().get_listen(), "localhost:9100");
}

#[test]
fn opening_things_and_quitting_are_asked_for() {
    let shown = Shown::new(420, 520, 1.0, true, ABOUT);
    let form = shown.form();
    form.invoke_open_browser();
    form.invoke_open_logs();
    form.invoke_open_settings_file();
    form.invoke_minimize();
    form.invoke_close();
    let open = |section, target| {
        Request::Job(Job {
            section,
            done: "",
            work: Work::Open(target),
        })
    };
    assert_eq!(
        shown.asked(),
        [
            open(Section::Server, OpenTarget::Browser),
            open(Section::Logs, OpenTarget::Logs),
            open(Section::About, OpenTarget::Settings),
            Request::Minimize,
            Request::Close,
        ]
    );
    // An opening that worked says nothing.
    let now = Instant::now();
    shown
        .view
        .finished(Section::About, "", Ok(Outcome::Done), now);
    assert_eq!(shown.form().get_about_note().text, "");
    assert!(!shown.form().get_about_busy());

    shown.form().invoke_quit();
    assert_eq!(
        shown.asked(),
        [Request::Job(Job {
            section: Section::About,
            done: "",
            work: Work::Quit,
        })]
    );
}

#[test]
fn the_links_on_about_open_in_the_browser() {
    let mut shown = Shown::new(420, 520, 1.0, true, ABOUT);
    // The source, on the line after the version.
    shown.point(200.0, 140.0);
    assert!(shown.view.ui().get_over_link());
    shown.click(200.0, 140.0);
    assert_eq!(
        shown.asked(),
        [Request::Link(
            "https://github.com/sentrychris/vigil-collector".into()
        )]
    );
    shown.point(200.0, 300.0);
    assert!(!shown.view.ui().get_over_link());
    // Who built it, at the right of the last line.
    shown
        .form()
        .invoke_open_link("https://versyx.digital".into());
    assert_eq!(
        shown.asked(),
        [Request::Link("https://versyx.digital".into())]
    );
}

#[test]
fn quitting_asks_first() {
    let mut shown = Shown::new(420, 520, 1.0, true, ABOUT);
    let before = shown.frame.pixels.clone();
    shown.click(60.0, 263.0);
    shown.save("settings-about-quit");
    assert!(before != shown.frame.pixels, "the picture did not change");
    // Nothing has been asked of Vigil yet.
    assert_eq!(shown.asked(), []);
}

#[test]
fn the_connections_form_saves_what_changed() {
    let shown = Shown::new(420, 520, 1.0, true, CONNECTIONS);
    let form = shown.form();
    assert_eq!(form.get_limit(), "20");
    assert_eq!(form.get_interval(), "1");
    assert_eq!(
        (form.get_connections_active(), form.get_connections_max()),
        (3, 20)
    );
    form.invoke_save_connections();
    assert_eq!(shown.asked(), []);

    form.set_interval("0.5".into());
    form.invoke_save_connections();
    assert_eq!(
        shown.asked(),
        [Request::Job(Job {
            section: Section::Connections,
            done: "Saved.",
            work: Work::Change(json!({"ws_push_interval": 0.5})),
        })]
    );
    let changed = Changed {
        settings: Settings {
            ws_push_interval: 0.5,
            ..settings()
        },
        status: status("localhost", 4500),
    };
    shown.view.finished(
        Section::Connections,
        "Saved.",
        Ok(Outcome::Changed(Box::new(changed))),
        Instant::now(),
    );
    let verdict = form.invoke_connections_verdict("50".into(), "0.5".into(), 0);
    assert!(verdict.dirty && verdict.problem.is_empty());
    let verdict = form.invoke_connections_verdict("20".into(), "0.5".into(), 0);
    assert!(!verdict.dirty);

    // A form that is wrong asks for nothing.
    form.set_limit("20000".into());
    form.invoke_save_connections();
    assert_eq!(shown.asked(), []);
}

#[test]
fn the_compact_window_is_made_live_or_static_from_here() {
    let mut shown = Shown::new(420, 520, 1.0, true, CONNECTIONS);
    assert!(shown.form().get_live());
    let before = shown.frame.pixels.clone();
    shown.form().invoke_set_live(false);
    shown.paint();
    shown.save("settings-connections-static");
    // It is shown at once and asked of the compact window, with nothing
    // to wait for.
    assert!(!shown.form().get_live());
    assert!(before != shown.frame.pixels, "the picture did not change");
    assert_eq!(shown.asked(), [Request::Live(false)]);
    // As the window opens, it shows how the compact window was left.
    shown.view.show_live(true);
    assert!(shown.form().get_live());
    assert_eq!(shown.asked(), []);
}

#[test]
fn logging_is_switched_with_one_press() {
    let shown = Shown::new(420, 520, 1.0, true, LOGS);
    let form = shown.form();
    assert!(form.get_logging());
    assert_eq!(form.get_log_level(), "INFO");
    form.invoke_set_log_level("WARNING".into());
    assert_eq!(
        shown.asked(),
        [Request::Job(Job {
            section: Section::Logs,
            done: "Saved.",
            work: Work::Change(json!({"log_level": "WARNING"})),
        })]
    );
    let changed = Changed {
        settings: Settings {
            log_level: "WARNING".into(),
            logging_enabled: false,
            ..settings()
        },
        status: status("localhost", 4500),
    };
    shown.view.finished(
        Section::Logs,
        "Saved.",
        Ok(Outcome::Changed(Box::new(changed))),
        Instant::now(),
    );
    assert_eq!(form.get_log_level(), "WARNING");
    assert!(!form.get_logging());

    form.invoke_set_logging(true);
    assert_eq!(
        shown.asked(),
        [Request::Job(Job {
            section: Section::Logs,
            done: "Saved.",
            work: Work::Change(json!({"logging_enabled": true})),
        })]
    );
}

/// Where the filter's field is on the Logs tab at the usual size, and a
/// place in the log under it.
const FILTER_FIELD: (f32, f32) = (100.0, 152.0);
const IN_THE_LOG: (f32, f32) = (200.0, 350.0);

#[test]
fn the_log_is_filtered_by_what_is_typed() {
    let mut shown = Shown::new(420, 520, 1.0, true, LOGS);
    let form = shown.form();
    assert_eq!(form.invoke_log_rows(400, "".into(), 0), 40);
    let before = shown.frame.pixels.clone();

    shown.click(FILTER_FIELD.0, FILTER_FIELD.1);
    shown.type_in("Error");
    shown.save("settings-logs-filtered");
    let form = shown.form();
    assert_eq!(form.get_log_filter(), "Error");
    assert!(before != shown.frame.pixels, "the picture did not change");
    // Every seventh made-up line is an error.
    assert_eq!(form.invoke_log_rows(400, form.get_log_filter(), 0), 6);
    let row = form.invoke_log_row(0, 400, form.get_log_filter(), 0);
    assert!(row.text.contains("ERROR") && row.level == 1, "{row:?}");
    // Past the end there is nothing.
    assert_eq!(
        form.invoke_log_row(6, 400, form.get_log_filter(), 0).text,
        ""
    );
}

#[test]
fn new_lines_are_shown_as_they_come() {
    let mut shown = Shown::new(420, 520, 1.0, true, LOGS);
    assert_eq!(shown.view.newest_log(), 40);
    assert!(!shown.paint());
    // Nothing new: nothing to paint.
    shown.view.take_log(Vec::new(), 40);
    assert!(!shown.paint());
    shown.view.take_log(log_lines(41, 3), 43);
    assert_eq!(shown.view.newest_log(), 43);
    assert!(shown.paint());
    assert_eq!(shown.form().invoke_log_rows(400, "".into(), 0), 43);
}

#[test]
fn scrolling_up_stops_following_and_the_end_starts_it_again() {
    let mut shown = Shown::new(420, 520, 1.0, true, LOGS);
    assert!(shown.form().get_log_follow());
    let before = shown.frame.pixels.clone();
    let position = LogicalPosition::new(IN_THE_LOG.0, IN_THE_LOG.1);
    let wheel = |shown: &mut Shown, delta_y: f32| {
        shown.surface.send(WindowEvent::PointerScrolled {
            position,
            delta_x: 0.0,
            delta_y,
        });
        shown.paint();
    };
    wheel(&mut shown, 96.0);
    assert!(!shown.form().get_log_follow());
    assert!(before != shown.frame.pixels, "the picture did not change");
    shown.save("settings-logs-scrolled");
    // New lines don't move it while it isn't following.
    let held = shown.frame.pixels.clone();
    shown.view.take_log(log_lines(41, 2), 42);
    shown.paint();
    let rows =
        |shown: &Shown| -> Vec<[u8; 3]> { (190..500).map(|y| shown.at(30.0, y as f32)).collect() };
    let text_before: Vec<[u8; 3]> = (190..500)
        .map(|y| {
            let pixel = held[(y * shown.frame.width + 30) as usize];
            pixel.rgb()
        })
        .collect();
    assert_eq!(rows(&shown), text_before);

    wheel(&mut shown, -2000.0);
    assert!(shown.form().get_log_follow());
}

#[test]
fn the_pointer_is_known_to_be_over_a_field() {
    let mut shown = Shown::new(420, 520, 1.0, true, SERVER);
    assert!(!shown.view.ui().get_over_text());
    shown.point(PORT_FIELD.0, PORT_FIELD.1);
    assert!(shown.view.ui().get_over_text());
    shown.point(300.0, 450.0);
    assert!(!shown.view.ui().get_over_text());
    shown.point(PORT_FIELD.0, PORT_FIELD.1);
    shown.surface.send(WindowEvent::PointerExited);
    shown.paint();
    assert!(!shown.view.ui().get_over_text());
}

#[test]
fn copy_and_paste_work_in_a_field() {
    let mut shown = Shown::new(420, 520, 1.0, true, LOGS);
    shown.click(FILTER_FIELD.0, FILTER_FIELD.1);
    shown.type_in("warn");
    let control = |shown: &mut Shown, down: bool| {
        let text = SharedString::from(Key::Control);
        shown.surface.send(if down {
            WindowEvent::KeyPressed { text }
        } else {
            WindowEvent::KeyReleased { text }
        });
    };
    control(&mut shown, true);
    shown.type_in("ac");
    control(&mut shown, false);
    assert_eq!(native::clipboard::get().as_deref(), Some("warn"));
    // Lets go of the selection, with the cursor at its end. (Not End,
    // which Slint leaves alone where it is built for a Mac.)
    shown.press(Key::RightArrow);
    control(&mut shown, true);
    shown.type_in("v");
    control(&mut shown, false);
    assert_eq!(shown.form().get_log_filter(), "warnwarn");
    // With Ctrl held, a letter is a shortcut and types nothing.
    control(&mut shown, true);
    shown.type_in("x");
    control(&mut shown, false);
    assert_eq!(shown.form().get_log_filter(), "warnwarn");
}

/// Where the fourth check's name and address are typed on the Checks tab
/// at the usual size.
const FOURTH_NAME: (f32, f32) = (70.0, 206.0);
const FOURTH_ADDRESS: (f32, f32) = (250.0, 206.0);

#[test]
fn a_check_is_added_typed_in_and_saved() {
    let mut shown = Shown::new(420, 520, 1.0, true, CHECKS);
    let verdict = |shown: &Shown| {
        let form = shown.form();
        form.invoke_checks_verdict(form.get_timeout(), form.get_checks_edits(), 0)
    };
    assert!(!verdict(&shown).dirty);

    shown.form().invoke_add_check();
    shown.paint();
    assert_eq!(
        (verdict(&shown).dirty, verdict(&shown).problem.as_str()),
        (true, "Every check needs a name.")
    );
    shown.click(FOURTH_NAME.0, FOURTH_NAME.1);
    shown.type_in("NAS");
    assert_eq!(verdict(&shown).problem, "");
    // The address starts as "https://", ready to be finished.
    shown.click(FOURTH_ADDRESS.0, FOURTH_ADDRESS.1);
    shown.press(Key::End);
    shown.press(Key::Backspace);
    assert_eq!(
        verdict(&shown).problem,
        "Every address must start with http:// or https://."
    );
    shown.type_in("/nas.local/");
    assert_eq!(verdict(&shown).problem, "");
    shown.save("settings-checks-edited");

    shown.form().invoke_save_checks();
    let done = "Saved. The dashboard uses the new list from its next check.";
    let mut probes = settings().probes;
    probes.push(Probe {
        name: "NAS".into(),
        url: "https://nas.local/".into(),
    });
    assert_eq!(
        shown.asked(),
        [Request::Job(Job {
            section: Section::Checks,
            done,
            work: Work::Change(json!({ "probes": probes })),
        })]
    );
    let changed = Changed {
        settings: Settings {
            probes,
            ..settings()
        },
        status: status("localhost", 4500),
    };
    shown.view.finished(
        Section::Checks,
        done,
        Ok(Outcome::Changed(Box::new(changed))),
        Instant::now(),
    );
    assert!(!verdict(&shown).dirty);
}

#[test]
fn a_check_is_removed_and_discard_puts_it_back() {
    let mut shown = Shown::new(420, 520, 1.0, true, CHECKS);
    let before = shown.frame.pixels.clone();
    shown.form().invoke_remove_check(0);
    shown.form().set_timeout("2.5".into());
    shown.paint();
    assert!(before != shown.frame.pixels, "the picture did not change");
    shown.form().invoke_save_checks();
    let Some(Request::Job(job)) = shown.asked().pop() else {
        panic!("saving asked for nothing");
    };
    let Work::Change(changes) = job.work else {
        panic!("saving didn't change settings");
    };
    assert_eq!(changes["probe_timeout"], json!(2.5));
    assert_eq!(changes["probes"].as_array().map(Vec::len), Some(2));
    assert_eq!(changes["probes"][0]["name"], json!("status.edcs.app"));

    // It came back refused, and the edit is discarded.
    shown.view.finished(
        Section::Checks,
        job.done,
        Err("Vigil is shutting down.".into()),
        Instant::now(),
    );
    shown.form().invoke_reset_checks();
    shown.paint();
    let form = shown.form();
    assert_eq!(form.get_timeout(), "5");
    let verdict = form.invoke_checks_verdict(form.get_timeout(), form.get_checks_edits(), 0);
    assert!(!verdict.dirty);
    // No more checks than the settings file can hold.
    for _ in 0..60 {
        shown.form().invoke_add_check();
    }
    shown.form().invoke_save_checks();
    assert_eq!(shown.asked(), []);
}

#[test]
fn tab_moves_from_one_field_to_the_next() {
    let mut shown = Shown::new(420, 520, 1.0, true, CONNECTIONS);
    // The limit's field, then on to the interval's.
    shown.click(100.0, 262.0);
    shown.press(Key::Tab);
    shown.type_in("5");
    let form = shown.form();
    assert_eq!(form.get_limit(), "20");
    assert!(form.get_interval().contains('5'), "{}", form.get_interval());
}
