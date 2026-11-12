//! The Vigil window: the dashboard as a desktop program.
//!
//! One ordinary window with a WebView2 control filling it, showing the
//! same dashboard a browser would. It loads from the tray process's
//! private listener (see ``server::control``), which is also the only
//! place the Settings page is served.
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

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{BLACK_BRUSH, GetStockObject};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, GetSystemMetrics,
    HICON, ICON_BIG, ICON_SMALL, IDC_ARROW, IsIconic, LoadCursorW, MINMAXINFO, RegisterClassW,
    SM_CXICON, SM_CXSMICON, SPI_GETWORKAREA, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetWindowPos, ShowWindow,
    SystemParametersInfoW, WM_CLOSE, WM_DPICHANGED, WM_GETMINMAXINFO, WM_SETICON, WNDCLASSW,
    WS_OVERLAPPEDWINDOW,
};
use wry::raw_window_handle::{
    HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
};
use wry::{NewWindowResponse, WebContext, WebView, WebViewBuilder};

use crate::tray::{load_icon, open_external, wide};

const CLASS_NAME: &str = "VigilWindow";
/// The window's first size and its smallest, in pixels at 100% scaling.
const DEFAULT_SIZE: (i32, i32) = (1280, 880);
const MIN_SIZE: (i32, i32) = (520, 400);
/// The dashboard's dark page colour, shown until the page has drawn so
/// the window doesn't flash white.
const BACKGROUND: (u8, u8, u8, u8) = (0x0a, 0x0e, 0x17, 0xff);
/// Asks the page already showing to go to Settings, without loading it
/// again (``App.vue`` listens for this).
const GO_TO_SETTINGS: &str =
    "window.dispatchEvent(new CustomEvent('vigil:navigate', { detail: '/settings' }))";

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
        bring_forward(hwnd);
        return Ok(());
    }
    if CREATING.replace(true) {
        return Ok(());
    }
    let result = create(origin, token, page);
    CREATING.set(false);
    result
}

fn bring_forward(hwnd: HWND) {
    // SAFETY: plain calls on this thread's own window.
    unsafe {
        let show = if IsIconic(hwnd) != 0 {
            SW_RESTORE
        } else {
            SW_SHOW
        };
        ShowWindow(hwnd, show);
        SetForegroundWindow(hwnd);
    }
}

fn create(origin: &str, token: &str, page: Page) -> Result<(), String> {
    // SAFETY: plain Win32 calls on this thread, with NUL-terminated strings
    // that outlive them.
    let (hwnd, icons) = unsafe {
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
        place(hwnd);

        // The title bar and Alt-Tab use the same picture as the tray.
        let icons = [
            load_icon(GetSystemMetrics(SM_CXICON).max(32)),
            load_icon(GetSystemMetrics(SM_CXSMICON).max(16)),
        ];
        for (kind, icon) in [(ICON_BIG, icons[0]), (ICON_SMALL, icons[1])] {
            if !icon.is_null() {
                SendMessageW(hwnd, WM_SETICON, kind as WPARAM, icon as LPARAM);
            }
        }
        (hwnd, icons)
    };

    match webview(hwnd, origin, token, page) {
        Ok(webview) => {
            WINDOW.set(Some(Window {
                hwnd,
                webview,
                icons,
            }));
            bring_forward(hwnd);
            Ok(())
        }
        Err(e) => {
            // SAFETY: the window and icons were made above and nothing
            // else refers to them.
            unsafe {
                DestroyWindow(hwnd);
                destroy_icons(icons);
            }
            Err(e)
        }
    }
}

/// Sizes the window for its display's scaling and centres it on the
/// primary display's work area (the screen less the taskbar).
///
/// # Safety
/// ``hwnd`` must be a live window of this thread.
unsafe fn place(hwnd: HWND) {
    // SAFETY (caller): ``hwnd`` is live; ``area`` is a valid out-pointer.
    unsafe {
        let dpi = GetDpiForWindow(hwnd).max(96) as i32;
        let mut area = RECT::default();
        let have_area = SystemParametersInfoW(SPI_GETWORKAREA, 0, (&raw mut area).cast(), 0) != 0;
        let mut width = DEFAULT_SIZE.0 * dpi / 96;
        let mut height = DEFAULT_SIZE.1 * dpi / 96;
        if !have_area {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOMOVE,
            );
            return;
        }
        width = width.min(area.right - area.left);
        height = height.min(area.bottom - area.top);
        let x = area.left + (area.right - area.left - width) / 2;
        let y = area.top + (area.bottom - area.top - height) / 2;
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x,
            y,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
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
        .with_background_color(BACKGROUND)
        .with_devtools(cfg!(debug_assertions))
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
    drop(window.webview);
    // SAFETY: the window and its icons belong to this thread and are
    // destroyed once.
    unsafe {
        DestroyWindow(window.hwnd);
        destroy_icons(window.icons);
    }
}

/// # Safety
/// The icons must not be in use by a live window.
unsafe fn destroy_icons(icons: [HICON; 2]) {
    for icon in icons {
        if !icon.is_null() {
            // SAFETY (caller): made by ``load_icon`` and no longer used.
            unsafe { DestroyIcon(icon) };
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
            WM_GETMINMAXINFO => {
                let info = lparam as *mut MINMAXINFO;
                if !info.is_null() {
                    let dpi = GetDpiForWindow(hwnd).max(96) as i32;
                    (*info).ptMinTrackSize = POINT {
                        x: MIN_SIZE.0 * dpi / 96,
                        y: MIN_SIZE.1 * dpi / 96,
                    };
                }
                0
            }
            // Dragged to a display with different scaling: take the size
            // Windows suggests, which keeps the window the same size to
            // the eye.
            WM_DPICHANGED => {
                let rect = lparam as *const RECT;
                if !rect.is_null() {
                    let r = *rect;
                    SetWindowPos(
                        hwnd,
                        std::ptr::null_mut(),
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_web_address;

    #[test]
    fn only_http_addresses_go_to_the_browser() {
        assert!(is_web_address("https://versyx.digital/vigil/"));
        assert!(is_web_address("http://example.com"));
        assert!(!is_web_address("about:blank"));
        assert!(!is_web_address("file:///C:/Windows"));
    }
}
