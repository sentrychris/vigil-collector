//! The Settings window on Windows: one of Vigil's own windows
//! (``native/shell.rs``) showing ``ui/settings.slint``, and the doing of
//! what its buttons ask.
//!
//! It lives on the tray thread, and everything here must be called on
//! that thread. Changing a setting or stopping the server means waiting
//! on the supervisor, which is done on the collector's thread; how it
//! went is posted back to the window. Once a second, while it isn't
//! minimised, the window looks at where things stand and at the log.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::mpsc as std_mpsc;
use std::time::Instant;

use slint::ComponentHandle;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetWindowRect, HTCLIENT, IDC_HAND, IDC_IBEAM, LoadCursorW, PostMessageW,
    SW_MINIMIZE, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SetCursor, SetTimer, SetWindowPos,
    ShowWindow, WM_APP, WM_CLOSE, WM_DPICHANGED, WM_SETCURSOR, WM_SIZE, WM_TIMER, WS_MAXIMIZEBOX,
    WS_OVERLAPPEDWINDOW,
};

use super::view::{Job, Outcome, Request, Section, View, Work};
use crate::config::{self, WindowKind, WindowLook, WindowPlace};
use crate::frame;
use crate::native::shell::{self, FIRST_OWN_TIMER, Make, Shell};
use crate::native::ui::{SettingsWindow, Theme};
use crate::native::{self, surface};
use crate::server::control::Panel;

const CLASS_NAME: &str = "VigilSettingsWindow";
/// The window's first size and its smallest, in pixels at 100% scaling.
const DEFAULT_SIZE: (i32, i32) = (420, 480);
const MIN_SIZE: (i32, i32) = (360, 320);
/// The first time, it opens this far right of and below the middle of
/// the screen, which is where the compact window opens: beside it rather
/// than squarely over it.
const FIRST_NUDGE: i32 = 40;

/// Posted by the layout's own buttons: a ``Request`` is waiting in
/// ``Open::asked``. Posted rather than acted on at once because the press
/// arrives while Slint is in the middle of handling the mouse, which is
/// no time to minimise the window under it or open another.
const WM_ASKED: u32 = WM_APP + 1;
/// Posted by the collector's thread: a job has come back.
const WM_FINISHED: u32 = WM_APP + 2;
const LOOK_TIMER: usize = FIRST_OWN_TIMER;
/// How often the window looks at where things stand and at the log.
const LOOK_EVERY_MS: u32 = 1000;

/// How a job went, on its way back from the collector's thread.
struct Finished {
    section: Section,
    done: &'static str,
    result: Result<Outcome, String>,
}

/// The window while it is open.
struct Open {
    shell: Shell,
    view: View,
    panel: Panel,
    /// The collector's thread, where jobs wait on the supervisor.
    runtime: tokio::runtime::Handle,
    /// What the layout has asked for and not yet been given.
    asked: Rc<RefCell<VecDeque<Request>>>,
    /// Where finished jobs arrive, and the end handed to each.
    finished: std_mpsc::Receiver<Finished>,
    report: std_mpsc::Sender<Finished>,
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

/// Shows the Settings window, creating it if it isn't open, and brings it
/// to the front. ``runtime`` is the collector's thread. An error means
/// there is no window to show.
pub fn show(panel: &Panel, runtime: &tokio::runtime::Handle) -> Result<(), String> {
    if let Some(open) = open() {
        frame::bring_forward(open.shell.hwnd);
        return Ok(());
    }
    raise(
        panel,
        runtime,
        config::window_look(),
        config::window_place(WindowKind::Settings),
        config::read_settings(),
    )?;
    Ok(())
}

/// Makes the window and puts it on the screen: at ``place`` if it has
/// been open before, in the theme the compact window was left in
/// (``look``, which also says whether that window follows the readings
/// live), with ``saved`` as what the settings file holds.
fn raise(
    panel: &Panel,
    runtime: &tokio::runtime::Handle,
    look: WindowLook,
    place: Option<WindowPlace>,
    saved: config::Settings,
) -> Result<Rc<Open>, String> {
    let (ui, surface) = surface::open(SettingsWindow::new)?;
    ui.global::<Theme>().set_dark(look.dark);

    // SAFETY: plain Win32 calls on this thread.
    let hwnd = unsafe {
        let hwnd = Shell::create(&Make {
            class: CLASS_NAME,
            title: "Vigil Settings",
            // Nothing in it gains from being maximised.
            style: WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX,
            procedure: Some(window_proc),
            place,
            size: DEFAULT_SIZE,
        })?;
        if place.is_none() {
            let mut at = RECT::default();
            if GetWindowRect(hwnd, &mut at) != 0 {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    at.left + FIRST_NUDGE,
                    at.top + FIRST_NUDGE,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
        hwnd
    };

    let asked = Rc::new(RefCell::new(VecDeque::new()));
    let view = View::new(ui, panel.status(), saved, {
        let asked = asked.clone();
        move |request| {
            asked.borrow_mut().push_back(request);
            // SAFETY: posting to this thread's own window; one that has
            // gone just fails.
            unsafe { PostMessageW(hwnd, WM_ASKED, 0, 0) };
        }
    });
    view.show_live(look.live);
    let (report, finished) = std_mpsc::channel();
    let open = Rc::new(Open {
        shell: Shell::new(hwnd, surface),
        view,
        panel: panel.clone(),
        runtime: runtime.clone(),
        asked,
        finished,
        report,
    });
    OPEN.set(Some(open.clone()));

    open.fit();
    open.look();
    // SAFETY: a plain call on this thread's own window.
    unsafe { SetTimer(hwnd, LOOK_TIMER, LOOK_EVERY_MS, None) };
    frame::bring_forward(hwnd);
    Ok(open)
}

/// The theme was changed in one of the other windows: follow it.
pub fn theme_changed(dark: bool) {
    if let Some(open) = open() {
        open.view.ui().global::<Theme>().set_dark(dark);
        open.fit();
    }
}

/// Closes the window, if it is open, and remembers where it was.
pub fn close() {
    if let Some(place) = shut() {
        config::remember_window_place(WindowKind::Settings, place);
    }
}

/// Takes the window off the screen and apart, and says where it was.
fn shut() -> Option<WindowPlace> {
    let open = OPEN.take()?;
    open.shell.destroy()
}

impl Open {
    /// Tells the layout the size and scaling the window has now, and
    /// paints the page behind the cards to fit (and in the theme of the
    /// moment, so this is also what follows a change of theme).
    fn fit(&self) {
        if let Some((width, height, scale)) = self.shell.fit() {
            native::paint_page(self.view.ui(), width, height, scale);
        }
    }

    /// Brings what the window shows up to date: where things stand, the
    /// log, and the notes that have been up long enough.
    fn look(&self) {
        self.view.show(self.panel.status());
        let (lines, newest) = crate::log::lines_after(self.view.newest_log());
        self.view.take_log(lines, newest);
        self.view.tidy(Instant::now());
    }

    /// Does what the layout asked for.
    fn asked(&self, request: Request) {
        match request {
            Request::Minimize => {
                // SAFETY: a plain call on this thread's own window.
                unsafe { ShowWindow(self.shell.hwnd, SW_MINIMIZE) };
            }
            Request::Close => {
                // SAFETY: posting to this thread's own window.
                unsafe { PostMessageW(self.shell.hwnd, WM_CLOSE, 0, 0) };
            }
            Request::Live(live) => crate::compact::set_live(live),
            // The addresses are the layout's own; only web addresses go
            // to the browser all the same.
            Request::Link(address) => {
                if address.starts_with("https://") {
                    crate::tray::open_external(&address);
                }
            }
            Request::Job(job) => self.start(job),
        }
    }

    /// Starts a job. What only shows something is done here and now; what
    /// waits on the supervisor is done on the collector's thread, and
    /// comes back as ``WM_FINISHED``.
    fn start(&self, job: Job) {
        let Job {
            section,
            done,
            work,
        } = job;
        let panel = self.panel.clone();
        let result = match work {
            Work::Open(target) => panel
                .open(target)
                .map(|()| Outcome::Done)
                .map_err(|refusal| refusal.message),
            Work::Quit => {
                // The section stays busy: the window goes with Vigil.
                panel.quit();
                return;
            }
            Work::Change(changes) => {
                let checks = section == Section::Checks;
                self.wait(section, done, async move {
                    let changed = panel.change(&changes).await;
                    changed
                        .map(|changed| (Outcome::Changed(Box::new(changed)), checks))
                        .map_err(|refusal| refusal.message)
                });
                return;
            }
            Work::Server(action) => {
                self.wait(section, done, async move {
                    let status = panel.server(action).await;
                    status
                        .map(|status| (Outcome::Status(Box::new(status)), false))
                        .map_err(|refusal| refusal.message)
                });
                return;
            }
        };
        self.view.finished(section, done, result, Instant::now());
    }

    /// Runs ``work`` on the collector's thread and has its answer posted
    /// back. With the answer comes whether the service checks changed.
    fn wait(
        &self,
        section: Section,
        done: &'static str,
        work: impl Future<Output = Result<(Outcome, bool), String>> + Send + 'static,
    ) {
        let report = self.report.clone();
        let hwnd = self.shell.hwnd as usize;
        self.runtime.spawn(async move {
            let (result, checks) = match work.await {
                Ok((outcome, checks)) => (Ok(outcome), checks),
                Err(why) => (Err(why), false),
            };
            let finished = Finished {
                section,
                done,
                result,
            };
            // SAFETY: posting to a window from another thread is allowed.
            // If the window has gone, so has the other end of ``report``,
            // and both simply fail.
            if report.send(finished).is_ok() {
                unsafe { PostMessageW(hwnd as HWND, WM_FINISHED, usize::from(checks), 0) };
            }
        });
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
        // Closing it leaves Vigil running in the tray.
        if msg == WM_CLOSE {
            close();
            return 0;
        }
        let Some(open) = open().filter(|open| open.shell.hwnd == hwnd) else {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        };
        let ui = open.view.ui();
        let bar = (ui.get_bar_height(), ui.get_caption_until());
        if let Some(answer) = open.shell.handle(msg, wparam, lparam, bar) {
            return answer;
        }
        match msg {
            WM_SIZE => {
                let was = open.shell.minimized();
                if open.shell.sized(wparam) {
                    open.fit();
                    if was {
                        open.look();
                    }
                }
                0
            }
            WM_DPICHANGED => {
                frame::follow_scaling(hwnd, lparam);
                open.fit();
                0
            }
            // Over somewhere to type, the pointer is the bar it is in
            // every text field.
            WM_SETCURSOR if (lparam & 0xffff) as u32 == HTCLIENT && ui.get_over_text() => {
                SetCursor(LoadCursorW(std::ptr::null_mut(), IDC_IBEAM));
                1
            }
            // And over a link, the hand.
            WM_SETCURSOR if (lparam & 0xffff) as u32 == HTCLIENT && ui.get_over_link() => {
                SetCursor(LoadCursorW(std::ptr::null_mut(), IDC_HAND));
                1
            }
            WM_TIMER => {
                if wparam == LOOK_TIMER && !open.shell.minimized() {
                    open.look();
                    open.shell.keep_time();
                }
                0
            }
            WM_ASKED => {
                loop {
                    // Taken one at a time, so nothing is borrowed while a
                    // request is seen to: it may bring another.
                    let next = open.asked.borrow_mut().pop_front();
                    let Some(request) = next else {
                        break;
                    };
                    open.asked(request);
                }
                open.shell.keep_time();
                0
            }
            WM_FINISHED => {
                while let Ok(finished) = open.finished.try_recv() {
                    open.view.finished(
                        finished.section,
                        finished.done,
                        finished.result,
                        Instant::now(),
                    );
                }
                if wparam != 0 {
                    // The compact window checks the new list at once.
                    crate::compact::services_changed();
                }
                open.shell.keep_time();
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc as std_mpsc;
    use std::time::Instant;

    use slint::ComponentHandle;
    use tokio::sync::{mpsc, oneshot, watch};
    use windows_sys::Win32::Foundation::{POINT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HTCAPTION, HTCLIENT, HTTOP, IsIconic, IsWindow, SendMessageW, WM_CHAR, WM_KEYDOWN,
        WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST,
    };

    use super::super::tests::settings;
    use super::{open, raise, shut};
    use crate::config::WindowLook;
    use crate::native::shell::{lparam, pump};
    use crate::native::ui::Form;
    use crate::server::control::Panel;
    use crate::supervisor::{Command, Status};

    // The keys pressed below (winuser.h).
    const VK_BACK: WPARAM = 0x08;
    const VK_END: WPARAM = 0x23;

    /// The real window, start to finish, with a stand-in for the
    /// supervisor: it opens, paints, says which of its parts drag and
    /// resize it, takes typing in a field and a press of Stop that is
    /// answered from another thread, minimises and closes. Nothing is
    /// read from or written to the settings folder.
    #[test]
    fn the_window_opens_takes_typing_and_has_the_server_stopped() {
        // Stands in for the collector's thread and the supervisor on it.
        let (handle_tx, handle_rx) = std_mpsc::channel();
        let (stop, stopped) = oneshot::channel::<()>();
        let (commands, mut asked) = mpsc::unbounded_channel::<Command>();
        let (status_tx, status) = watch::channel(Status {
            serving: true,
            listen: Some("localhost:4500".into()),
            url: Some("http://localhost:4500".into()),
            address: "localhost".into(),
            port: 4500,
            ..Status::default()
        });
        let collector = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .expect("a runtime");
            handle_tx
                .send(runtime.handle().clone())
                .expect("a listener");
            runtime.spawn(async move {
                while let Some(command) = asked.recv().await {
                    if let Command::Stop(reply) = command {
                        status_tx.send_modify(|status| {
                            status.serving = false;
                            status.listen = None;
                            status.url = None;
                        });
                        if let Some(reply) = reply {
                            let _ = reply.send(Ok(()));
                        }
                    }
                }
            });
            runtime.block_on(async {
                let _ = stopped.await;
            });
        });
        let panel = Panel {
            commands,
            status,
            active_ws: Arc::new(AtomicUsize::new(2)),
            max_ws: Arc::new(AtomicUsize::new(20)),
            started: Instant::now(),
        };
        let runtime = handle_rx.recv().expect("the runtime's handle");

        let look = WindowLook {
            dark: true,
            live: true,
        };
        let open_now = raise(&panel, &runtime, look, None, settings()).expect("the window opens");
        let hwnd = open_now.shell.hwnd;
        let scale = open_now.shell.scale();
        drop(open_now);
        let window = || open().expect("the window is open");
        let form_port = || window().view.ui().global::<Form>().get_port().to_string();

        // It is painted, to the size of the window, bar and all.
        let (first, width, height) = window().shell.picture();
        assert!(width >= 300 && height >= 250, "{width} x {height}");
        assert_eq!(first.len(), (width * height) as usize);
        let at = |pixels: &[u32], x: f32, y: f32| {
            let pixel = pixels[((y * scale) as u32 * width + (x * scale) as u32) as usize];
            [(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]
        };
        let bar = at(&first, 200.0, 16.0);
        assert!(bar.iter().all(|c| *c < 0x60), "{bar:?}");
        assert_ne!(bar, at(&first, 3.0, 300.0));

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
        assert_eq!(part(120.0, 16.0), HTCAPTION);
        assert_eq!(part(layout_width - 20.0, 16.0), HTCLIENT);
        assert_eq!(part(100.0, 200.0), HTCLIENT);
        assert_eq!(part(100.0, 1.0), HTTOP);

        let send = |msg: u32, wparam: WPARAM, lparam: isize| {
            // SAFETY: a message to this thread's own window.
            unsafe { SendMessageW(hwnd, msg, wparam, lparam) };
        };
        let click = |x: f32, y: f32| {
            let at = lparam(x * scale, y * scale);
            send(WM_MOUSEMOVE, 0, at);
            send(WM_LBUTTONDOWN, 1, at);
            send(WM_LBUTTONUP, 0, at);
        };

        // The port's field takes the keyboard: the pointer over it is
        // known to be over text, End and Backspace edit it, and what is
        // typed goes in. The character Windows sends along with
        // Backspace types nothing.
        assert_eq!(form_port(), "4500");
        click(60.0, 360.0);
        assert!(window().view.ui().get_over_text());
        for key in [VK_END, VK_BACK] {
            send(WM_KEYDOWN, key, 0);
            send(WM_KEYUP, key, 0);
        }
        send(WM_CHAR, 0x08, 0);
        assert_eq!(form_port(), "450");
        send(WM_CHAR, '9' as WPARAM, 0);
        send(WM_CHAR, 'x' as WPARAM, 0);
        assert_eq!(form_port(), "4509");
        assert_ne!(window().shell.picture().0, first);

        // Stop is pressed; the supervisor's answer comes back from the
        // other thread and is shown.
        click(45.0, 166.0);
        let stopped_serving = || {
            let window = window();
            let form = window.view.ui().global::<Form>();
            !form.get_serving() && form.get_server_note().text == "Stopped serving."
        };
        assert!(pump(stopped_serving), "stopping was never shown");
        assert!(!window().view.ui().global::<Form>().get_server_busy());

        // The minimise button, second from the right, is pressed.
        click(layout_width - 60.0, 16.0);
        // SAFETY: a plain query of this thread's own window.
        assert!(
            pump(|| unsafe { IsIconic(hwnd) } != 0),
            "the minimise button did nothing"
        );

        assert!(shut().is_some());
        assert!(open().is_none());
        // SAFETY: a query about a handle, which may be stale.
        assert_eq!(unsafe { IsWindow(hwnd) }, 0);
        let _ = stop.send(());
        collector.join().expect("the runtime's thread ends");
    }
}
