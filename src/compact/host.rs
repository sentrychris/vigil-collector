//! The compact window on Windows: one of Vigil's own windows
//! (``native/shell.rs``) showing ``ui/compact.slint``.
//!
//! Slint only paints when something on it changed, which while the window
//! just sits there is once per set of readings. Minimised, nothing is
//! laid out or painted at all.
//!
//! It lives on the tray thread, and everything here must be called on
//! that thread. The readings come from the collector's own thread: a
//! small task there posts this window a message for each new set, and the
//! window then reads the newest one. The service checks run on that
//! thread too and come back the same way.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;

use slint::ComponentHandle;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, PostMessageW, SW_MINIMIZE, SetTimer, ShowWindow, WM_APP, WM_CLOSE,
    WM_DPICHANGED, WM_SIZE, WM_TIMER, WS_MAXIMIZEBOX, WS_OVERLAPPEDWINDOW,
};

use super::view::{self, History};
use crate::config::{self, WindowKind, WindowLook, WindowPlace};
use crate::frame;
use crate::native::shell::{self, FIRST_OWN_TIMER, Make, Shell};
use crate::native::ui::{Charts, CompactWindow, Host, Theme};
use crate::native::{self, surface};
use crate::server::probes::{self, ProbeResult};
use crate::snapshot::Snapshot;
use crate::tray::open_external;

const CLASS_NAME: &str = "VigilCompactWindow";
/// The window's first size and its smallest, in pixels at 100% scaling.
/// At the first, everything fits without scrolling.
const DEFAULT_SIZE: (i32, i32) = (420, 580);
const MIN_SIZE: (i32, i32) = (360, 300);

/// Posted by the collector's thread: there are new readings.
const WM_READINGS: u32 = WM_APP + 1;
/// Posted by the collector's thread: the service checks have come back.
const WM_SERVICES: u32 = WM_APP + 2;
/// Posted by the layout's own buttons, with an ``Ask`` in ``wparam``.
/// Posted rather than acted on at once because the press arrives while
/// Slint is in the middle of handling the mouse, which is no time to
/// minimise the window under it or open another.
const WM_ASKED: u32 = WM_APP + 3;
const SERVICES_TIMER: usize = FIRST_OWN_TIMER;
/// Brings the readings up to date now and then while they aren't being
/// followed live.
const PAUSED_TIMER: usize = FIRST_OWN_TIMER + 1;

/// How often the services are checked, and how often paused readings are
/// brought up to date: the web dashboard's own intervals.
const SERVICES_EVERY_MS: u32 = 30_000;
const PAUSED_EVERY_MS: u32 = 30_000;

/// What the layout's buttons can ask of the window.
#[derive(Clone, Copy)]
#[repr(usize)]
enum Ask {
    Minimize = 1,
    /// Open the full dashboard, in its own window.
    Full,
    /// Open Settings, in its own window.
    Settings,
    /// Open the address waiting in ``Open::link`` in the browser.
    Link,
    /// The theme was changed here: paint the page behind the cards in it,
    /// and tell the full dashboard, if it is open.
    Theme,
}

impl Ask {
    fn from_wparam(wparam: WPARAM) -> Option<Ask> {
        [
            Ask::Minimize,
            Ask::Full,
            Ask::Settings,
            Ask::Link,
            Ask::Theme,
        ]
        .into_iter()
        .find(|ask| *ask as usize == wparam)
    }
}

/// What the window is fed from, handed over by ``main``: the collector's
/// readings, and the means to run the service checks on its thread.
#[derive(Clone)]
pub struct Feed {
    readings: watch::Receiver<Arc<Snapshot>>,
    runtime: tokio::runtime::Handle,
    client: reqwest::Client,
}

impl Feed {
    /// Call on the collector's thread, inside its runtime.
    pub fn new(readings: watch::Receiver<Arc<Snapshot>>) -> Result<Feed, String> {
        Ok(Feed {
            readings,
            runtime: tokio::runtime::Handle::current(),
            client: crate::server::probes_client().map_err(|e| e.to_string())?,
        })
    }

    /// The collector's thread, for work that has to wait on it.
    pub fn runtime(&self) -> &tokio::runtime::Handle {
        &self.runtime
    }
}

/// The window while it is open.
struct Open {
    shell: Shell,
    ui: CompactWindow,
    history: Rc<RefCell<History>>,
    feed: Feed,
    /// The task that posts ``WM_READINGS``.
    forward: JoinHandle<()>,
    /// Where finished service checks arrive, and the end handed to each
    /// run of them.
    checked: std_mpsc::Receiver<Vec<ProbeResult>>,
    report: std_mpsc::Sender<Vec<ProbeResult>>,
    /// Whether a run of checks is out, so a slow one isn't doubled up.
    checking: Cell<bool>,
    /// The address a pressed link wants opened (see ``Ask::Link``).
    link: Rc<RefCell<Option<String>>>,
}

thread_local! {
    static OPEN: RefCell<Option<Rc<Open>>> = const { RefCell::new(None) };
}

/// The open window, if there is one. A clone, so nothing stays borrowed
/// while the window is used: much of what is done to a window comes
/// straight back in through its window procedure.
fn open() -> Option<Rc<Open>> {
    OPEN.with_borrow(|open| open.clone())
}

/// Shows the compact window, creating it if it isn't open, and brings it
/// to the front. An error means there is no window to show.
pub fn show(feed: &Feed) -> Result<(), String> {
    if let Some(open) = open() {
        frame::bring_forward(open.shell.hwnd);
        return Ok(());
    }
    let open = raise(
        feed,
        config::window_look(),
        config::window_place(WindowKind::Compact),
    )?;
    open.check_services();
    // SAFETY: a plain call on this thread's own window.
    unsafe { SetTimer(open.shell.hwnd, SERVICES_TIMER, SERVICES_EVERY_MS, None) };
    Ok(())
}

/// Makes the window and puts it on the screen, showing the newest
/// readings: at ``place`` if it has been open before, looking as it was
/// left.
fn raise(feed: &Feed, look: WindowLook, place: Option<WindowPlace>) -> Result<Rc<Open>, String> {
    let (ui, surface) = surface::open(CompactWindow::new)?;
    ui.global::<Theme>().set_dark(look.dark);
    ui.set_live(look.live);
    let history = Rc::new(RefCell::new(History::default()));
    view::draw_from(&ui, &history);

    // SAFETY: plain Win32 calls on this thread.
    let hwnd = unsafe {
        Shell::create(&Make {
            class: CLASS_NAME,
            title: "Vigil",
            // It has no use for being maximised: the full dashboard is
            // what a big window is for, and the bar has a button for it.
            style: WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX,
            procedure: Some(window_proc),
            place,
            size: DEFAULT_SIZE,
        })?
    };
    let link = Rc::new(RefCell::new(None));
    listen(&ui, hwnd, &link);

    let forward = feed.runtime.spawn({
        let mut readings = feed.readings.clone();
        let hwnd = hwnd as usize;
        async move {
            while readings.changed().await.is_ok() {
                // SAFETY: posting to a window from another thread is
                // allowed; once the window has gone it fails, and this
                // task (stopped by ``close`` anyway) ends.
                if unsafe { PostMessageW(hwnd as HWND, WM_READINGS, 0, 0) } == 0 {
                    break;
                }
            }
        }
    });
    let (report, checked) = std_mpsc::channel();
    let open = Rc::new(Open {
        shell: Shell::new(hwnd, surface),
        ui,
        history,
        feed: feed.clone(),
        forward,
        checked,
        report,
        checking: Cell::new(false),
        link,
    });
    OPEN.set(Some(open.clone()));

    open.fit();
    open.take_readings();
    // SAFETY: a plain call on this thread's own window.
    unsafe { SetTimer(hwnd, PAUSED_TIMER, PAUSED_EVERY_MS, None) };
    frame::bring_forward(hwnd);
    Ok(open)
}

/// Connects the layout's buttons and switches to the window.
fn listen(ui: &CompactWindow, hwnd: HWND, link: &Rc<RefCell<Option<String>>>) {
    let ask = move |ask: Ask| {
        // SAFETY: posting to this thread's own window; one that has gone
        // just fails.
        unsafe { PostMessageW(hwnd, WM_ASKED, ask as WPARAM, 0) };
    };
    let host = ui.global::<Host>();
    host.on_minimize(move || ask(Ask::Minimize));
    host.on_close(move || {
        // SAFETY: as in ``ask``.
        unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
    });
    host.on_open_full(move || ask(Ask::Full));
    host.on_open_settings(move || ask(Ask::Settings));
    host.on_open_url({
        let link = link.clone();
        move |url| {
            *link.borrow_mut() = Some(url.to_string());
            ask(Ask::Link);
        }
    });
    host.on_set_dark({
        let ui = ui.as_weak();
        move |dark| {
            if let Some(ui) = ui.upgrade() {
                ui.global::<Theme>().set_dark(dark);
                remember_look(&ui);
                ask(Ask::Theme);
            }
        }
    });
}

fn remember_look(ui: &CompactWindow) {
    config::remember_window_look(WindowLook {
        dark: ui.global::<Theme>().get_dark(),
        live: ui.get_live(),
    });
}

/// The full dashboard changed the theme: follow it, here and next time.
// A MinGW build may have no full dashboard to say so (see window_none.rs).
#[allow(dead_code)]
pub fn theme_changed(dark: bool) {
    crate::settings::theme_changed(dark);
    match open() {
        Some(open) => {
            open.ui.global::<Theme>().set_dark(dark);
            open.fit();
            remember_look(&open.ui);
        }
        None => config::remember_window_look(WindowLook {
            dark,
            ..config::window_look()
        }),
    }
}

/// Settings switched following the readings live on or off: do so, here
/// and next time. Off, the readings are brought up to date now and then
/// (see ``PAUSED_TIMER``).
pub fn set_live(live: bool) {
    match open() {
        Some(open) => {
            open.ui.set_live(live);
            remember_look(&open.ui);
            if live {
                // Catch up at once rather than at the next reading.
                open.take_readings();
            }
        }
        None => config::remember_window_look(WindowLook {
            live,
            ..config::window_look()
        }),
    }
}

/// Closes the window, if it is open, and remembers where it was.
pub fn close() {
    if let Some(place) = shut() {
        config::remember_window_place(WindowKind::Compact, place);
    }
}

/// Takes the window off the screen and apart, and says where it was.
fn shut() -> Option<WindowPlace> {
    let open = OPEN.take()?;
    open.forward.abort();
    open.shell.destroy()
}

/// Has the services checked again now, if the window is open: the list of
/// them was changed in Settings.
pub fn services_changed() {
    if let Some(open) = open() {
        open.check_services();
    }
}

impl Open {
    /// Tells the layout the size and scaling the window has now, and
    /// paints the page behind the cards to fit (and in the theme of the
    /// moment, so this is also what follows a change of theme).
    fn fit(&self) {
        self.ui.global::<Charts>().set_scale(self.shell.scale());
        if let Some((width, height, scale)) = self.shell.fit() {
            native::paint_page(&self.ui, width, height, scale);
        }
    }

    /// Takes the newest readings: into the sparklines' history, and onto
    /// the window unless it is minimised (it catches up when it is put
    /// back).
    fn take_readings(&self) {
        let readings = self.feed.readings.borrow().clone();
        self.history.borrow_mut().remember(&readings);
        if !self.shell.minimized() {
            self.show_readings(&readings);
        }
    }

    fn show_readings(&self, readings: &Snapshot) {
        view::show(&self.ui, readings);
        view::redraw_sparklines(&self.ui);
    }

    /// Has the services checked on the collector's thread. The answer
    /// comes back as ``WM_SERVICES``.
    fn check_services(&self) {
        if self.checking.replace(true) {
            return;
        }
        let client = self.feed.client.clone();
        let report = self.report.clone();
        let hwnd = self.shell.hwnd as usize;
        self.feed.runtime.spawn(async move {
            let found = probes::run(&client).await.probes;
            // SAFETY: as for ``WM_READINGS``. If the window has gone, so
            // has the other end of ``report``, and both simply fail.
            if report.send(found).is_ok() {
                unsafe { PostMessageW(hwnd as HWND, WM_SERVICES, 0, 0) };
            }
        });
    }

    /// Does what one of the layout's buttons asked for.
    fn asked(&self, ask: Ask) {
        match ask {
            Ask::Minimize => {
                // SAFETY: a plain call on this thread's own window.
                unsafe { ShowWindow(self.shell.hwnd, SW_MINIMIZE) };
            }
            Ask::Full => crate::tray::open_full(),
            Ask::Settings => crate::tray::open_settings(),
            Ask::Link => {
                // The addresses are the service checks', from the
                // settings file; only web addresses go to the browser.
                let link = self.link.borrow_mut().take();
                if let Some(link) = link
                    && (link.starts_with("https://") || link.starts_with("http://"))
                {
                    open_external(&link);
                }
            }
            Ask::Theme => {
                self.fit();
                let dark = self.ui.global::<Theme>().get_dark();
                crate::window::set_theme(dark);
                crate::settings::theme_changed(dark);
            }
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: called by the system on the tray thread for its own window;
    // the pointers in ``lparam`` are valid for the messages they come with.
    unsafe {
        if let Some(answer) = shell::frame_message(hwnd, msg, wparam, lparam, MIN_SIZE) {
            return answer;
        }
        // The close button puts Vigil back in the tray; Quit is in the
        // tray menu.
        if msg == WM_CLOSE {
            close();
            return 0;
        }
        let Some(open) = open().filter(|open| open.shell.hwnd == hwnd) else {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        };
        let bar = (open.ui.get_bar_height(), open.ui.get_caption_until());
        if let Some(answer) = open.shell.handle(msg, wparam, lparam, bar) {
            return answer;
        }
        match msg {
            WM_SIZE => {
                let was = open.shell.minimized();
                if open.shell.sized(wparam) {
                    open.fit();
                    if was {
                        let readings = open.feed.readings.borrow().clone();
                        open.show_readings(&readings);
                        open.check_services();
                    }
                }
                0
            }
            WM_DPICHANGED => {
                frame::follow_scaling(hwnd, lparam);
                open.fit();
                0
            }
            WM_TIMER => {
                match wparam {
                    SERVICES_TIMER if !open.shell.minimized() => open.check_services(),
                    PAUSED_TIMER if !open.ui.get_live() => open.take_readings(),
                    _ => {}
                }
                0
            }
            WM_READINGS => {
                if open.ui.get_live() {
                    open.take_readings();
                }
                0
            }
            WM_SERVICES => {
                open.checking.set(false);
                while let Ok(found) = open.checked.try_recv() {
                    view::show_services(&open.ui, &found);
                }
                0
            }
            WM_ASKED => {
                if let Some(ask) = Ask::from_wparam(wparam) {
                    open.asked(ask);
                }
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::mpsc as std_mpsc;

    use tokio::sync::{oneshot, watch};
    use windows_sys::Win32::Foundation::{HWND, POINT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HTCAPTION, HTCLIENT, HTTOP, IsIconic, IsWindow, SendMessageW, WM_LBUTTONDOWN, WM_LBUTTONUP,
        WM_MOUSEMOVE, WM_NCHITTEST,
    };

    use super::super::tests::snapshot;
    use super::{Ask, Feed, WindowLook, open, raise, shut};
    use crate::native::shell::{lparam, pump};

    #[test]
    fn every_ask_survives_being_posted() {
        for ask in [
            Ask::Minimize,
            Ask::Full,
            Ask::Settings,
            Ask::Link,
            Ask::Theme,
        ] {
            assert_eq!(
                Ask::from_wparam(ask as usize).map(|a| a as usize),
                Some(ask as usize)
            );
        }
        assert!(Ask::from_wparam(0).is_none());
        assert!(Ask::from_wparam(99).is_none());
    }

    /// The picture as it stands, brought up to date first.
    fn picture(hwnd: HWND) -> Vec<u32> {
        let open = open().expect("the window is open");
        assert_eq!(open.shell.hwnd, hwnd);
        open.shell.picture().0
    }

    /// The real window, start to finish, with made-up readings: it opens,
    /// paints, says which of its parts drag and resize it, takes new
    /// readings from another thread and a press of its minimise button,
    /// and closes. Nothing is read from or written to the settings folder.
    #[test]
    fn the_window_opens_paints_follows_readings_and_takes_the_mouse() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        // Stands in for the collector's thread.
        let (handle_tx, handle_rx) = std_mpsc::channel();
        let (stop, stopped) = oneshot::channel::<()>();
        let collector = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .expect("a runtime");
            handle_tx
                .send(runtime.handle().clone())
                .expect("a listener");
            runtime.block_on(async {
                let _ = stopped.await;
            });
        });
        let (readings, _) = watch::channel(Arc::new(snapshot(0)));
        let feed = Feed {
            readings: readings.subscribe(),
            runtime: handle_rx.recv().expect("the runtime's handle"),
            client: crate::server::probes_client().expect("a client"),
        };

        let look = WindowLook {
            dark: true,
            live: true,
        };
        let open = raise(&feed, look, None).expect("the window opens");
        let hwnd = open.shell.hwnd;
        let scale = open.shell.scale();
        drop(open);

        // It is painted, to the size of the window, bar and all.
        let first = picture(hwnd);
        let (_, width, height) = super::open().expect("still open").shell.picture();
        assert!(width >= 300 && height >= 250, "{width} x {height}");
        assert_eq!(first.len(), (width * height) as usize);
        let at = |pixels: &[u32], x: f32, y: f32| {
            let pixel = pixels[((y * scale) as u32 * width + (x * scale) as u32) as usize];
            [(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]
        };
        let bar = at(&first, 60.0, 16.0);
        let page = at(&first, 3.0, 200.0);
        assert!(bar.iter().all(|c| *c < 0x60), "{bar:?}");
        assert_ne!(bar, page);

        // The bar drags the window, its buttons are the layout's own, and
        // the very top resizes it.
        let layout_width = width as f32 / scale;
        let part = |x: f32, y: f32| {
            let mut at = POINT {
                x: (x * scale) as i32,
                y: (y * scale) as i32,
            };
            // SAFETY: plain calls on this thread's own window.
            unsafe {
                ClientToScreen(hwnd, &mut at);
                SendMessageW(hwnd, WM_NCHITTEST, 0, lparam(at.x as f32, at.y as f32)) as u32
            }
        };
        assert_eq!(part(60.0, 16.0), HTCAPTION);
        assert_eq!(part(layout_width - 20.0, 16.0), HTCLIENT);
        assert_eq!(part(100.0, 200.0), HTCLIENT);
        assert_eq!(part(100.0, 1.0), HTTOP);

        // New readings arrive from the other thread and change the picture.
        readings.send_replace(Arc::new(snapshot(7)));
        assert!(
            pump(|| picture(hwnd) != first),
            "new readings were never shown"
        );

        // The minimise button, third from the right, is pressed.
        let press = lparam((layout_width - 100.0) * scale, 16.0 * scale);
        for (msg, wparam) in [(WM_MOUSEMOVE, 0), (WM_LBUTTONDOWN, 1), (WM_LBUTTONUP, 0)] {
            // SAFETY: messages to this thread's own window.
            unsafe { SendMessageW(hwnd, msg, wparam as WPARAM, press) };
        }
        // SAFETY: a plain query of this thread's own window.
        assert!(
            pump(|| unsafe { IsIconic(hwnd) } != 0),
            "the minimise button did nothing"
        );

        assert!(shut().is_some());
        assert!(super::open().is_none());
        // SAFETY: a query about a handle, which may be stale.
        assert_eq!(unsafe { IsWindow(hwnd) }, 0);
        let _ = stop.send(());
        collector.join().expect("the runtime's thread ends");
    }
}
