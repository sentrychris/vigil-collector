//! The window for the full dashboard and the Settings page.
//!
//! One window with a WebView2 control filling it, showing the same
//! dashboard a browser would. It loads from the tray process's private
//! listener (see ``server::control``), which is also the only place the
//! Settings page is served. The window the tray icon opens is a different
//! one, the compact dashboard, which is drawn without a browser
//! (``compact/``) and has a button that opens this one.
//!
//! The window has no title bar of its own. The dashboard's top bar takes
//! its place: it draws the minimise, maximise and close buttons and is
//! what the window is dragged by (``SiteNavbar.vue``), and asks for each
//! of those through ``window.ipc.postMessage``. Everything else about the
//! window is still Windows' own: the resize borders at the sides and
//! bottom, the shadow and rounded corners, snapping and the keyboard
//! shortcuts.
//!
//! The window lives on the tray thread. That thread already has what
//! WebView2 asks for: a message loop, single-threaded COM, and per-monitor
//! DPI awareness. Everything here must be called on it.
//!
//! The WebView2 control is created when the window opens and destroyed
//! when it closes, so its browser processes (well over 100 MB) exist only
//! while there is something to look at. Sitting in the tray, Vigil costs
//! what it did before it had a window.
//!
//! WebView2 ships with Windows 11 and reaches Windows 10 through Windows
//! Update. Where it is missing, ``show`` fails and the tray opens the
//! dashboard in the browser instead.

use std::cell::{Cell, RefCell};
use std::num::NonZeroIsize;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{BLACK_BRUSH, GetStockObject};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, HICON, HTCAPTION,
    HTTOP, HTTOPLEFT, HTTOPRIGHT, IDC_ARROW, IsZoomed, LoadCursorW, PostMessageW, RegisterClassW,
    SIZE_MAXIMIZED, SIZE_MINIMIZED, SIZE_RESTORED, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE,
    ShowWindow, WM_APP, WM_CLOSE, WM_DPICHANGED, WM_GETMINMAXINFO, WM_NCCALCSIZE, WM_NCLBUTTONDOWN,
    WM_SIZE, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};
use wry::raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use wry::{NewWindowResponse, WebContext, WebView, WebViewBuilder};

use crate::config::{self, WindowKind};
use crate::frame;
use crate::tray::{open_external, wide};

const CLASS_NAME: &str = "VigilWindow";
/// The window's first size and its smallest, in pixels at 100% scaling.
/// It opens with room for the full dashboard, on a screen that has the
/// room. Made narrower than 900 the page changes to its compact layout
/// (see useCompactLayout.ts in web/), as it does in a browser.
const DEFAULT_SIZE: (i32, i32) = (1180, 760);
const MIN_SIZE: (i32, i32) = (360, 300);
/// The dashboard's dark page colour, shown until the page has drawn so
/// the window doesn't flash white.
const BACKGROUND: (u8, u8, u8, u8) = (0x0a, 0x0e, 0x17, 0xff);
/// Asks the page already showing to go to Settings, without loading it
/// again (``App.vue`` listens for this).
const GO_TO_SETTINGS: &str =
    "window.dispatchEvent(new CustomEvent('vigil:navigate', { detail: '/settings' }))";

/// Posted to the window by the page's title bar, with an ``Ask`` in
/// ``wparam``. Posted rather than acted on at once because the request
/// arrives inside a WebView2 callback, which is no place to move, resize
/// or close the window that control lives in.
const WM_PAGE_ASKED: u32 = WM_APP + 1;
/// Posted when the page's theme switch is used, with whether it is now
/// dark in ``wparam``.
const WM_PAGE_THEME: u32 = WM_APP + 2;
/// What the page sends before the theme's name when its switch is used.
const THEME_PREFIX: &str = "theme:";

/// What the page's title bar can ask of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
enum Ask {
    /// Is the window maximised? Answered with a ``vigil:window`` event.
    State = 1,
    Minimize,
    /// Maximise, or put back if it already is.
    Maximize,
    Close,
    /// The bar was pressed: move the window with the mouse.
    Drag,
    /// The top edge or a top corner was pressed: resize from there. The
    /// other edges are outside the page and Windows' own.
    ResizeTop,
    ResizeTopLeft,
    ResizeTopRight,
}

impl Ask {
    const ALL: [Ask; 8] = [
        Ask::State,
        Ask::Minimize,
        Ask::Maximize,
        Ask::Close,
        Ask::Drag,
        Ask::ResizeTop,
        Ask::ResizeTopLeft,
        Ask::ResizeTopRight,
    ];

    /// The word the page sends for it.
    fn word(self) -> &'static str {
        match self {
            Ask::State => "state",
            Ask::Minimize => "minimize",
            Ask::Maximize => "maximize",
            Ask::Close => "close",
            Ask::Drag => "drag",
            Ask::ResizeTop => "resize:top",
            Ask::ResizeTopLeft => "resize:top-left",
            Ask::ResizeTopRight => "resize:top-right",
        }
    }

    fn from_word(word: &str) -> Option<Ask> {
        Ask::ALL.into_iter().find(|ask| ask.word() == word)
    }

    fn from_wparam(wparam: WPARAM) -> Option<Ask> {
        Ask::ALL.into_iter().find(|ask| *ask as usize == wparam)
    }
}

/// The page's name for a theme.
fn theme_word(dark: bool) -> &'static str {
    if dark { "dark" } else { "light" }
}

/// The theme the page said it changed to, if that is what it said.
fn theme_from_page(message: &str) -> Option<bool> {
    match message.strip_prefix(THEME_PREFIX)? {
        "dark" => Some(true),
        "light" => Some(false),
        _ => None,
    }
}

/// Run in the page before any of its own scripts: starts it in the
/// compact window's theme, by putting that where the page keeps its
/// choice (web/src/stores/theme.ts). Once per window rather than once per
/// load, so reloading the page keeps a theme chosen since.
fn start_in_theme(dark: bool) -> String {
    let theme = theme_word(dark);
    format!(
        "try {{ if (!sessionStorage.getItem('vigil:themed')) {{ \
         sessionStorage.setItem('vigil:themed', '1'); \
         localStorage.setItem('theme', JSON.stringify({{ theme: '{theme}', active: '{theme}' }})); \
         }} }} catch (e) {{}}"
    )
}

/// Which page the window should be showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Dashboard,
    Settings,
}

struct Window {
    hwnd: HWND,
    webview: WebView,
    icons: [HICON; 2],
    /// False while the window is minimised and the control is hidden.
    page_shown: Cell<bool>,
}

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
    /// Set while the WebView2 control is being made. That runs the
    /// message loop, so a second double-click on the tray icon can arrive
    /// before the first has finished.
    static CREATING: Cell<bool> = const { Cell::new(false) };
}

/// The window's handle in the form ``wry`` takes.
struct Parent(HWND);

impl HasWindowHandle for Parent {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let hwnd = NonZeroIsize::new(self.0 as isize).ok_or(HandleError::Unavailable)?;
        let raw = RawWindowHandle::Win32(Win32WindowHandle::new(hwnd));
        // SAFETY: the window outlives the handle, which is only used while
        // the WebView2 control is being built inside it.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

/// Shows the window on ``page``, creating it if it isn't open, and brings
/// it to the front. ``origin`` is the private listener
/// (``http://127.0.0.1:<port>``) and ``token`` its secret for this run.
/// An error means there is no window to show (most likely no WebView2 on
/// this computer) and the caller should fall back to the browser.
pub fn show(origin: &str, token: &str, page: Page) -> Result<(), String> {
    let existing = WINDOW.with_borrow(|w| w.as_ref().map(|w| w.hwnd));
    if let Some(hwnd) = existing {
        if page == Page::Settings {
            WINDOW.with_borrow(|w| {
                if let Some(w) = w {
                    let _ = w.webview.evaluate_script(GO_TO_SETTINGS);
                }
            });
        }
        frame::bring_forward(hwnd);
        return Ok(());
    }
    if CREATING.replace(true) {
        return Ok(());
    }
    let result = create(origin, token, page);
    CREATING.set(false);
    result
}

fn create(origin: &str, token: &str, page: Page) -> Result<(), String> {
    // SAFETY: plain Win32 calls on this thread, with NUL-terminated strings
    // that outlive them.
    let (hwnd, icons, maximized) = unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class = wide(CLASS_NAME);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: GetStockObject(BLACK_BRUSH),
            ..Default::default()
        };
        // Fails harmlessly when the class exists from an earlier window.
        RegisterClassW(&wc);
        let title = wide("Vigil");
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
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
        let maximized = frame::place(hwnd, config::window_place(WindowKind::Full), DEFAULT_SIZE);
        (hwnd, frame::set_icons(hwnd), maximized)
    };

    match webview(hwnd, origin, token, page) {
        Ok(webview) => {
            WINDOW.set(Some(Window {
                hwnd,
                webview,
                icons,
                page_shown: Cell::new(true),
            }));
            if maximized {
                // SAFETY: a plain call on this thread's own window.
                unsafe { ShowWindow(hwnd, SW_MAXIMIZE) };
            }
            frame::bring_forward(hwnd);
            Ok(())
        }
        Err(e) => {
            // SAFETY: the window and icons were made above and nothing
            // else refers to them.
            unsafe {
                DestroyWindow(hwnd);
                frame::destroy_icons(icons);
            }
            Err(e)
        }
    }
}

/// Remembers where the window is, for the next time it is opened.
fn remember_place(hwnd: HWND) {
    if let Some(place) = frame::placement(hwnd) {
        config::remember_window_place(WindowKind::Full, place);
    }
}

/// Does what the page's title bar asked for.
///
/// # Safety
/// ``hwnd`` must be this thread's live Vigil window.
unsafe fn page_asked(hwnd: HWND, ask: Ask) {
    // SAFETY (caller): plain calls on this thread's own window.
    unsafe {
        match ask {
            Ask::State => tell_page(hwnd),
            Ask::Minimize => {
                ShowWindow(hwnd, SW_MINIMIZE);
            }
            Ask::Maximize => {
                let show = if IsZoomed(hwnd) != 0 {
                    SW_RESTORE
                } else {
                    SW_MAXIMIZE
                };
                ShowWindow(hwnd, show);
            }
            Ask::Close => {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
            Ask::Drag => begin_move_or_size(hwnd, HTCAPTION),
            Ask::ResizeTop => begin_move_or_size(hwnd, HTTOP),
            Ask::ResizeTopLeft => begin_move_or_size(hwnd, HTTOPLEFT),
            Ask::ResizeTopRight => begin_move_or_size(hwnd, HTTOPRIGHT),
        }
    }
}

/// Hands a mouse press that happened in the page to Windows as if it had
/// been on the title bar (``HTCAPTION``) or an edge, so moving, resizing,
/// snapping and dragging a maximised window free all behave as they do
/// for any other window.
///
/// # Safety
/// ``hwnd`` must be this thread's live Vigil window.
unsafe fn begin_move_or_size(hwnd: HWND, hit: u32) {
    // SAFETY (caller): ``hwnd`` is live; ``at`` is a valid out-pointer.
    unsafe {
        let mut at = POINT::default();
        GetCursorPos(&mut at);
        // The page has the mouse while the button is down; Windows won't
        // start its own move until it is let go of.
        ReleaseCapture();
        PostMessageW(hwnd, WM_NCLBUTTONDOWN, hit as WPARAM, point_lparam(at));
    }
}

/// A screen position packed the way mouse messages carry it: x in the low
/// 16 bits and y in the next 16, each signed (displays left of or above
/// the primary one have negative coordinates).
fn point_lparam(at: POINT) -> LPARAM {
    let x = at.x as i16 as u16 as u32;
    let y = at.y as i16 as u16 as u32;
    ((y << 16) | x) as LPARAM
}

/// Tells the page whether the window is maximised, so its button can show
/// the right picture and its resize edge can step aside.
fn tell_page(hwnd: HWND) {
    // SAFETY: a plain query of this thread's own window.
    let maximized = unsafe { IsZoomed(hwnd) != 0 };
    let script = format!(
        "window.dispatchEvent(new CustomEvent('vigil:window', \
         {{ detail: {{ maximized: {maximized} }} }}))"
    );
    // Not there while the control is being made or taken apart; the page
    // asks again when it loads.
    WINDOW.with(|window| {
        if let Ok(window) = window.try_borrow()
            && let Some(window) = window.as_ref()
        {
            let _ = window.webview.evaluate_script(&script);
        }
    });
}

/// The compact window's theme switch was used: has the page follow, if
/// the window is open.
pub fn set_theme(dark: bool) {
    let script = format!(
        "window.dispatchEvent(new CustomEvent('vigil:theme', {{ detail: '{}' }}))",
        theme_word(dark)
    );
    WINDOW.with(|window| {
        if let Ok(window) = window.try_borrow()
            && let Some(window) = window.as_ref()
        {
            let _ = window.webview.evaluate_script(&script);
        }
    });
}

/// Shows or hides the WebView2 control. A minimised window's page still
/// counts as visible and goes on drawing its charts, sixty times a second,
/// for nobody; hiding the control is what tells the page to stop.
fn show_page(shown: bool) {
    WINDOW.with(|window| {
        if let Ok(window) = window.try_borrow()
            && let Some(window) = window.as_ref()
            && window.page_shown.replace(shown) != shown
        {
            let _ = window.webview.set_visible(shown);
        }
    });
}

/// Makes the WebView2 control inside ``hwnd`` and points it at the
/// private listener. Its first address carries the token, which the
/// listener swaps for a session cookie before sending it on to ``page``.
fn webview(hwnd: HWND, origin: &str, token: &str, page: Page) -> Result<WebView, String> {
    let next = match page {
        Page::Dashboard => "",
        Page::Settings => "&next=settings",
    };
    let url = format!("{origin}/control/session?token={token}{next}");

    // Links that leave the dashboard (the footer's, say) open in the
    // default browser rather than inside the window.
    let own = format!("{origin}/");
    let stays = move |url: &str| url.starts_with(&own) || !is_web_address(url);

    // WebView2 keeps a profile (cache, the dashboard's saved theme) in a
    // folder it must be able to write to. Its default is beside the
    // program, which fails under Program Files.
    let mut context = WebContext::new(Some(data_dir()));
    WebViewBuilder::new_with_web_context(&mut context)
        .with_url(url)
        .with_initialization_script(start_in_theme(config::window_look().dark))
        .with_background_color(BACKGROUND)
        .with_devtools(cfg!(debug_assertions))
        // The page's title bar. Only our own pages get this far: the
        // navigation handler below keeps every other site out.
        .with_ipc_handler({
            let hwnd = hwnd as usize;
            move |request| {
                let post = |msg: u32, wparam: WPARAM| {
                    // SAFETY: posting to this thread's own window; a
                    // window that has gone just drops the message.
                    unsafe { PostMessageW(hwnd as HWND, msg, wparam, 0) };
                };
                if let Some(ask) = Ask::from_word(request.body()) {
                    post(WM_PAGE_ASKED, ask as WPARAM);
                } else if let Some(dark) = theme_from_page(request.body()) {
                    post(WM_PAGE_THEME, WPARAM::from(dark));
                }
            }
        })
        .with_navigation_handler({
            let stays = stays.clone();
            move |url| {
                if stays(&url) {
                    return true;
                }
                open_external(&url);
                false
            }
        })
        .with_new_window_req_handler(move |url, _| {
            if is_web_address(&url) {
                open_external(&url);
            }
            NewWindowResponse::Deny
        })
        .build(&Parent(hwnd))
        .map_err(|e| e.to_string())
}

fn is_web_address(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// Where WebView2 keeps the window's profile:
/// ``%LOCALAPPDATA%\Vigil\WebView2``.
fn data_dir() -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(crate::config::settings_dir)
        .join("Vigil")
        .join("WebView2")
}

/// Closes the window, if it is open: the WebView2 control (and with it
/// the browser processes) first, then the window around it.
pub fn close() {
    let Some(window) = WINDOW.take() else {
        return;
    };
    remember_place(window.hwnd);
    drop(window.webview);
    // SAFETY: the window and its icons belong to this thread and are
    // destroyed once.
    unsafe {
        DestroyWindow(window.hwnd);
        frame::destroy_icons(window.icons);
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
        match msg {
            // The close button puts Vigil back in the tray; Quit is in
            // the tray menu. Ignored while the control is still being
            // made, when there is nothing consistent to take apart.
            WM_CLOSE => {
                if !CREATING.get() {
                    close();
                }
                0
            }
            // No title bar: the page draws one.
            WM_NCCALCSIZE => frame::without_title_bar(hwnd, wparam, lparam),
            WM_PAGE_ASKED => {
                if let Some(ask) = Ask::from_wparam(wparam)
                    && !CREATING.get()
                {
                    page_asked(hwnd, ask);
                }
                0
            }
            // The compact window keeps the same theme, now and next time.
            WM_PAGE_THEME => {
                crate::compact::theme_changed(wparam != 0);
                0
            }
            WM_SIZE => {
                if wparam == SIZE_MINIMIZED as WPARAM {
                    show_page(false);
                } else if wparam == SIZE_MAXIMIZED as WPARAM || wparam == SIZE_RESTORED as WPARAM {
                    show_page(true);
                    tell_page(hwnd);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_GETMINMAXINFO => {
                frame::no_smaller_than(hwnd, lparam, MIN_SIZE);
                0
            }
            WM_DPICHANGED => {
                frame::follow_scaling(hwnd, lparam);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Ask, POINT, is_web_address, point_lparam, start_in_theme, theme_from_page, theme_word,
    };

    #[test]
    fn themes_go_to_the_page_and_back_by_name() {
        assert_eq!(theme_from_page("theme:dark"), Some(true));
        assert_eq!(theme_from_page("theme:light"), Some(false));
        assert_eq!(theme_from_page("theme:pink"), None);
        assert_eq!(theme_from_page("close"), None);
        for dark in [true, false] {
            assert_eq!(
                theme_from_page(&format!("theme:{}", theme_word(dark))),
                Some(dark)
            );
        }
        assert!(start_in_theme(false).contains("{ theme: 'light', active: 'light' }"));
    }

    #[test]
    fn every_ask_survives_the_trip_from_the_page() {
        for ask in Ask::ALL {
            assert_eq!(Ask::from_word(ask.word()), Some(ask));
            assert_eq!(Ask::from_wparam(ask as usize), Some(ask));
        }
        assert_eq!(Ask::from_word("format-disk"), None);
        assert_eq!(Ask::from_wparam(0), None);
        assert_eq!(Ask::from_wparam(99), None);
    }

    #[test]
    fn packs_positions_left_of_and_above_the_primary_display() {
        assert_eq!(point_lparam(POINT { x: 3, y: 2 }), 0x0002_0003);
        assert_eq!(
            point_lparam(POINT { x: -1, y: -2 }),
            0xfffe_ffff_u32 as isize
        );
    }

    #[test]
    fn only_http_addresses_go_to_the_browser() {
        assert!(is_web_address("https://versyx.digital/vigil/"));
        assert!(is_web_address("http://example.com"));
        assert!(!is_web_address("about:blank"));
        assert!(!is_web_address("file:///C:/Windows"));
    }
}
