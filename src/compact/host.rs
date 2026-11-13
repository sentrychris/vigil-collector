//! The compact window on Windows: an ordinary window of the tray thread's
//! that shows the picture Slint paints (``surface.rs``) and passes the
//! mouse on to it.
//!
//! Each repaint is one copy of the picture to the screen through GDI.
//! Slint only paints when something on it changed, which while the window
//! just sits there is once per set of readings. Minimised, nothing is
//! laid out or painted at all.
//!
//! Like the full dashboard's window (``window.rs``) it has no title bar
//! from Windows and lives on the tray thread, and everything here must be
//! called on that thread. The readings come from the collector's own
//! thread: a small task there posts this window a message for each new
//! set, and the window then reads the newest one. The service checks run
//! on that thread too and come back the same way.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, DIB_RGB_COLORS, EndPaint, InvalidateRect,
    PAINTSTRUCT, ScreenToClient, SetDIBitsToDevice,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, HICON, HTCAPTION,
    HTCLIENT, HTTOP, HTTOPLEFT, HTTOPRIGHT, IDC_ARROW, KillTimer, LoadCursorW, PostMessageW,
    RegisterClassW, SIZE_MINIMIZED, SW_MINIMIZE, SetTimer, ShowWindow, WM_APP, WM_CLOSE,
    WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_NCCALCSIZE, WM_NCHITTEST, WM_PAINT, WM_SIZE, WM_TIMER, WNDCLASSW,
    WS_MAXIMIZEBOX, WS_OVERLAPPEDWINDOW,
};

use super::bar::{self, Part};
use super::surface::{self, Frame, Surface};
use super::ui::{Charts, CompactWindow, Host, Theme};
use super::view::{self, History};
use crate::config::{self, WindowKind, WindowLook, WindowPlace};
use crate::frame;
use crate::server::probes::{self, ProbeResult};
use crate::snapshot::Snapshot;
use crate::tray::{open_external, wide};
use crate::window::Page;

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
/// Sent to a window when the pointer leaves it, once asked for with
/// ``TrackMouseEvent``.
const WM_MOUSELEAVE: u32 = 0x02A3;

/// Runs Slint's clock while something on the window is moving.
const FRAME_TIMER: usize = 1;
const SERVICES_TIMER: usize = 2;
/// Brings the readings up to date now and then while they aren't being
/// followed live.
const PAUSED_TIMER: usize = 3;

/// About sixty pictures a second while something is moving.
const FRAME: Duration = Duration::from_millis(16);
/// How often the services are checked, and how often paused readings are
/// brought up to date: the web dashboard's own intervals.
const SERVICES_EVERY_MS: u32 = 30_000;
const PAUSED_EVERY_MS: u32 = 30_000;
/// How far one notch of the mouse wheel scrolls, in layout pixels.
const WHEEL_NOTCH: f32 = 48.0;
/// What Windows calls one notch.
const WHEEL_DELTA: f32 = 120.0;

/// What the layout's buttons can ask of the window.
#[derive(Clone, Copy)]
#[repr(usize)]
enum Ask {
    Minimize = 1,
    /// Open the full dashboard, in its own window.
    Full,
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
}

/// The window while it is open.
struct Open {
    hwnd: HWND,
    ui: CompactWindow,
    surface: Rc<Surface>,
    /// The picture, kept between repaints.
    frame: RefCell<Frame>,
    history: Rc<RefCell<History>>,
    feed: Feed,
    icons: [HICON; 2],
    /// The task that posts ``WM_READINGS``.
    forward: JoinHandle<()>,
    /// Where finished service checks arrive, and the end handed to each
    /// run of them.
    checked: std_mpsc::Receiver<Vec<ProbeResult>>,
    report: std_mpsc::Sender<Vec<ProbeResult>>,
    /// Whether a run of checks is out, so a slow one isn't doubled up.
    checking: Cell<bool>,
    minimized: Cell<bool>,
    /// Whether Windows has been asked to say when the pointer leaves.
    watching_pointer: Cell<bool>,
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
        frame::bring_forward(open.hwnd);
        return Ok(());
    }
    let open = raise(
        feed,
        config::window_look(),
        config::window_place(WindowKind::Compact),
    )?;
    open.check_services();
    // SAFETY: a plain call on this thread's own window.
    unsafe { SetTimer(open.hwnd, SERVICES_TIMER, SERVICES_EVERY_MS, None) };
    Ok(())
}

/// Makes the window and puts it on the screen, showing the newest
/// readings: at ``place`` if it has been open before, looking as it was
/// left.
fn raise(feed: &Feed, look: WindowLook, place: Option<WindowPlace>) -> Result<Rc<Open>, String> {
    let (ui, surface) = surface::open()?;
    ui.global::<Theme>().set_dark(look.dark);
    ui.set_live(look.live);
    let history = Rc::new(RefCell::new(History::default()));
    view::draw_from(&ui, &history);

    // SAFETY: plain Win32 calls on this thread.
    let hwnd = unsafe { create(place)? };
    let link = Rc::new(RefCell::new(None));
    listen(&ui, hwnd, &link);
    surface.on_stale(move || {
        // SAFETY: a plain call on this thread's own window; one that has
        // gone just fails.
        unsafe { InvalidateRect(hwnd, std::ptr::null(), 0) };
    });

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
        hwnd,
        ui,
        surface,
        frame: RefCell::new(Frame::default()),
        history,
        feed: feed.clone(),
        // SAFETY: the window was made above, on this thread.
        icons: unsafe { frame::set_icons(hwnd) },
        forward,
        checked,
        report,
        checking: Cell::new(false),
        minimized: Cell::new(false),
        watching_pointer: Cell::new(false),
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

/// Makes the window, still hidden, at ``place`` or, with none, at its
/// first size in the middle of the screen.
///
/// # Safety
/// Call on the tray thread.
unsafe fn create(place: Option<WindowPlace>) -> Result<HWND, String> {
    // SAFETY (caller): plain Win32 calls on this thread, with
    // NUL-terminated strings that outlive them.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class = wide(CLASS_NAME);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            // No background brush: the picture covers the whole window.
            ..Default::default()
        };
        // Fails harmlessly when the class exists from an earlier window.
        RegisterClassW(&wc);
        let title = wide("Vigil");
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            // It has no use for being maximised: the full dashboard is
            // what a big window is for, and the bar has a button for it.
            WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return Err(format!(
                "couldn't create the window: {}",
                std::io::Error::last_os_error()
            ));
        }
        frame::place(hwnd, place, DEFAULT_SIZE);
        Ok(hwnd)
    }
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
    host.on_set_live({
        let ui = ui.as_weak();
        move |live| {
            if let Some(ui) = ui.upgrade() {
                ui.set_live(live);
                remember_look(&ui);
                if live {
                    // Catch up at once rather than at the next reading.
                    // SAFETY: as in ``ask``.
                    unsafe { PostMessageW(hwnd, WM_READINGS, 0, 0) };
                }
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
    let place = frame::placement(open.hwnd).map(|place| WindowPlace {
        maximized: false,
        ..place
    });
    // SAFETY: the window and its icons belong to this thread and are
    // destroyed once.
    unsafe {
        DestroyWindow(open.hwnd);
        frame::destroy_icons(open.icons);
    }
    place
}

impl Open {
    /// The window's scaling: 1.0 at 100%, 1.5 at 150%.
    fn scale(&self) -> f32 {
        // SAFETY: a plain query of this thread's own window.
        unsafe { GetDpiForWindow(self.hwnd) }.max(96) as f32 / 96.0
    }

    /// Tells the layout the size and scaling the window has now, and
    /// paints the page behind the cards to fit (and in the theme of the
    /// moment, so this is also what follows a change of theme).
    fn fit(&self) {
        let mut client = RECT::default();
        // SAFETY: a query of this thread's own window into a valid
        // out-pointer.
        if unsafe { GetClientRect(self.hwnd, &mut client) } == 0 {
            return;
        }
        let scale = self.scale();
        let (width, height) = (client.right.max(0) as u32, client.bottom.max(0) as u32);
        self.ui.global::<Charts>().set_scale(scale);
        self.surface.resize(width, height, scale);
        view::paint_page(&self.ui, width, height, scale);
    }

    /// Takes the newest readings: into the sparklines' history, and onto
    /// the window unless it is minimised (it catches up when it is put
    /// back).
    fn take_readings(&self) {
        let readings = self.feed.readings.borrow().clone();
        self.history.borrow_mut().remember(&readings);
        if !self.minimized.get() {
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
        let hwnd = self.hwnd as usize;
        self.feed.runtime.spawn(async move {
            let found = probes::run(&client).await.probes;
            // SAFETY: as for ``WM_READINGS``. If the window has gone, so
            // has the other end of ``report``, and both simply fail.
            if report.send(found).is_ok() {
                unsafe { PostMessageW(hwnd as HWND, WM_SERVICES, 0, 0) };
            }
        });
    }

    /// Passes a mouse event on to the layout.
    fn pointer(&self, event: WindowEvent) {
        surface::tick();
        self.surface.send(event);
        self.keep_time();
    }

    /// A position from a mouse message, which is in real pixels from the
    /// window's top left, as the layout counts it.
    fn position(&self, x: i32, y: i32) -> LogicalPosition {
        let scale = self.scale();
        LogicalPosition::new(x as f32 / scale, y as f32 / scale)
    }

    /// Asks Windows to say when the pointer leaves, which it does once
    /// per asking.
    fn watch_pointer(&self) {
        if self.watching_pointer.replace(true) {
            return;
        }
        let mut track = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: self.hwnd,
            dwHoverTime: 0,
        };
        // SAFETY: a whole, correctly sized structure about this thread's
        // own window.
        unsafe { TrackMouseEvent(&mut track) };
    }

    /// Runs the frame timer for as long as Slint has a use for it: at
    /// every frame while something is moving, otherwise when its next
    /// timer is due, and not at all while nothing is pending.
    fn keep_time(&self) {
        let wait = if self.surface.animating() {
            Some(FRAME)
        } else {
            surface::next_timer()
        };
        // SAFETY: plain calls on this thread's own window. Setting a
        // timer that is running starts it again with the new period.
        unsafe {
            match wait {
                Some(wait) => {
                    let ms = wait.as_millis().clamp(10, 60_000) as u32;
                    SetTimer(self.hwnd, FRAME_TIMER, ms, None);
                }
                None => {
                    KillTimer(self.hwnd, FRAME_TIMER);
                }
            }
        }
    }

    /// Brings the picture up to date and copies it to the screen.
    fn paint(&self) {
        let mut paint = PAINTSTRUCT::default();
        // SAFETY: ``BeginPaint`` and ``EndPaint`` bracket the painting of
        // this thread's own window. The bitmap header describes exactly
        // the pixels handed over: ``width * height`` of 32 bits, in rows
        // from the top.
        unsafe {
            let dc = BeginPaint(self.hwnd, &mut paint);
            surface::tick();
            if let Ok(mut picture) = self.frame.try_borrow_mut() {
                self.surface.paint(&mut picture);
                if !dc.is_null() && picture.width > 0 && picture.height > 0 {
                    let info = BITMAPINFO {
                        bmiHeader: BITMAPINFOHEADER {
                            biSize: size_of::<BITMAPINFOHEADER>() as u32,
                            biWidth: picture.width as i32,
                            // Negative: rows run top to bottom.
                            biHeight: -(picture.height as i32),
                            biPlanes: 1,
                            biBitCount: 32,
                            biCompression: BI_RGB,
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    SetDIBitsToDevice(
                        dc,
                        0,
                        0,
                        picture.width,
                        picture.height,
                        0,
                        0,
                        0,
                        picture.height,
                        picture.pixels.as_ptr().cast(),
                        &info,
                        DIB_RGB_COLORS,
                    );
                }
            }
            EndPaint(self.hwnd, &paint);
        }
        self.keep_time();
    }

    /// Does what one of the layout's buttons asked for.
    fn asked(&self, ask: Ask) {
        match ask {
            Ask::Minimize => {
                // SAFETY: a plain call on this thread's own window.
                unsafe { ShowWindow(self.hwnd, SW_MINIMIZE) };
            }
            Ask::Full => crate::tray::open_full(Page::Dashboard),
            Ask::Settings => crate::tray::open_full(Page::Settings),
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
                crate::window::set_theme(self.ui.global::<Theme>().get_dark());
            }
        }
    }

    /// Which part of the window is at a place on the screen (see
    /// ``bar.rs``), as the answer ``WM_NCHITTEST`` wants.
    fn part_at(&self, mut at: POINT) -> u32 {
        let mut client = RECT::default();
        // SAFETY: queries of this thread's own window into valid
        // out-pointers.
        unsafe {
            ScreenToClient(self.hwnd, &mut at);
            GetClientRect(self.hwnd, &mut client);
        }
        let scale = self.scale();
        let part = bar::part_at(
            at.x as f32 / scale,
            at.y as f32 / scale,
            client.right as f32 / scale,
            self.ui.get_bar_height(),
            self.ui.get_caption_until(),
        );
        match part {
            Part::Inside => HTCLIENT,
            Part::Bar => HTCAPTION,
            Part::Top => HTTOP,
            Part::TopLeft => HTTOPLEFT,
            Part::TopRight => HTTOPRIGHT,
        }
    }
}

/// The two halves of a message's ``lparam`` when it carries a position:
/// x in the low 16 bits and y in the next 16, each signed (displays left
/// of or above the primary one have negative coordinates).
fn point(lparam: LPARAM) -> POINT {
    POINT {
        x: i32::from(lparam as u16 as i16),
        y: i32::from((lparam >> 16) as u16 as i16),
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
        // The frame, which needs nothing of the window's own state and is
        // asked about before the window has any.
        match msg {
            // No title bar: the layout draws one.
            WM_NCCALCSIZE => return frame::without_title_bar(hwnd, wparam, lparam),
            WM_GETMINMAXINFO => {
                frame::no_smaller_than(hwnd, lparam, MIN_SIZE);
                return 0;
            }
            // The picture covers everything, so there is nothing to clear
            // first (and clearing would flicker).
            WM_ERASEBKGND => return 1,
            // The close button puts Vigil back in the tray; Quit is in
            // the tray menu.
            WM_CLOSE => {
                close();
                return 0;
            }
            _ => {}
        }
        let Some(open) = open().filter(|open| open.hwnd == hwnd) else {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        };
        match msg {
            WM_PAINT => {
                open.paint();
                0
            }
            WM_SIZE => {
                let minimized = wparam == SIZE_MINIMIZED as WPARAM;
                let was = open.minimized.replace(minimized);
                if !minimized {
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
            // The sides and bottom are Windows' own resize borders. What
            // is left is the picture, whose bar and top edge stand in for
            // a title bar and a top border.
            WM_NCHITTEST => {
                let hit = DefWindowProcW(hwnd, msg, wparam, lparam);
                if hit == HTCLIENT as LRESULT {
                    open.part_at(point(lparam)) as LRESULT
                } else {
                    hit
                }
            }
            WM_MOUSEMOVE => {
                open.watch_pointer();
                let at = point(lparam);
                open.pointer(WindowEvent::PointerMoved {
                    position: open.position(at.x, at.y),
                });
                0
            }
            WM_LBUTTONDOWN => {
                // Keeps the mouse while the button is down, so a press
                // that slides off the window is still let go of.
                SetCapture(hwnd);
                let at = point(lparam);
                open.pointer(WindowEvent::PointerPressed {
                    position: open.position(at.x, at.y),
                    button: PointerEventButton::Left,
                });
                0
            }
            WM_LBUTTONUP => {
                let at = point(lparam);
                open.pointer(WindowEvent::PointerReleased {
                    position: open.position(at.x, at.y),
                    button: PointerEventButton::Left,
                });
                ReleaseCapture();
                0
            }
            WM_MOUSELEAVE => {
                open.watching_pointer.set(false);
                open.pointer(WindowEvent::PointerExited);
                0
            }
            WM_MOUSEWHEEL => {
                // The wheel's position is on the screen, not the window.
                let mut at = point(lparam);
                ScreenToClient(hwnd, &mut at);
                let notches = f32::from((wparam >> 16) as u16 as i16) / WHEEL_DELTA;
                open.pointer(WindowEvent::PointerScrolled {
                    position: open.position(at.x, at.y),
                    delta_x: 0.0,
                    delta_y: notches * WHEEL_NOTCH,
                });
                0
            }
            WM_TIMER => {
                match wparam {
                    FRAME_TIMER => {
                        surface::tick();
                        open.keep_time();
                    }
                    SERVICES_TIMER if !open.minimized.get() => open.check_services(),
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
    use std::time::{Duration, Instant};

    use tokio::sync::{oneshot, watch};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, HTCAPTION, HTCLIENT, HTTOP, IsIconic, IsWindow, MSG, PM_REMOVE,
        PeekMessageW, SendMessageW, TranslateMessage, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
        WM_NCHITTEST, WM_PAINT,
    };

    use super::super::tests::snapshot;
    use super::{Ask, Feed, WindowLook, open, point, raise, shut};

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

    #[test]
    fn reads_positions_left_of_and_above_the_primary_display() {
        let at = point(0x0002_0003);
        assert_eq!((at.x, at.y), (3, 2));
        let at = point(0xfffe_ffff_u32 as isize);
        assert_eq!((at.x, at.y), (-1, -2));
    }

    /// Handles this thread's waiting messages until ``done`` says so, for
    /// a few seconds at most. Says whether it did.
    fn pump(done: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            // SAFETY: the usual message loop, on the thread whose windows
            // these are.
            unsafe {
                let mut msg: MSG = std::mem::zeroed();
                while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            if done() {
                return true;
            }
            if Instant::now() > deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The picture as it stands, brought up to date first. Asked for
    /// directly, so it doesn't depend on the window being in view.
    fn picture(hwnd: HWND) -> Vec<u32> {
        // SAFETY: a message to this thread's own window.
        unsafe { SendMessageW(hwnd, WM_PAINT, 0, 0) };
        let open = open().expect("the window is open");
        let frame = open.frame.borrow();
        frame.pixels.iter().map(|pixel| pixel.0).collect()
    }

    fn lparam(x: f32, y: f32) -> LPARAM {
        (((y as i32 as u32) << 16) | (x as i32 as u32 & 0xffff)) as LPARAM
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
        let hwnd = open.hwnd;
        let scale = open.scale();
        drop(open);

        // It is painted, to the size of the window, bar and all.
        let first = picture(hwnd);
        let (width, height) = {
            let open = super::open().expect("still open");
            let frame = open.frame.borrow();
            (frame.width, frame.height)
        };
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
