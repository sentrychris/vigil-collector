//! Windows system tray mode.
//!
//! Started normally (double-clicked, or from a terminal), the collector
//! moves into the background and puts an icon in the notification area,
//! with a right-click menu to restart or quit it. It works like this:
//!
//! 1. The process that was started is the *launcher*. It starts a copy of
//!    the program with no console, passes it the same arguments, and relays
//!    its log output from a pipe. Once the copy is listening (it logs
//!    "Listening on ..." and then signals the launcher), the launcher
//!    exits, so a terminal gets its prompt back with the address on screen. Double-clicked, the launcher
//!    hides its console window straight away and shows a message box only
//!    if the copy fails to start.
//! 2. The copy is the *tray process*. It runs the collector as usual and
//!    owns the icon. Its log still goes to the log file. The menu also
//!    shows how many dashboards are connected and the version, and
//!    double-clicking the icon opens the dashboard in the browser.
//!
//! Only one copy runs per computer (see ``Instance``). Started again while
//! one is running, it opens that copy's dashboard (double-clicked) or says
//! where it is (from a terminal) and exits.
//!
//! ``--foreground`` skips all of this and runs in the console as before,
//! with Ctrl-C to stop. So do services and ``SYSTEM`` scheduled tasks
//! (session 0, which has no desktop to put an icon on) and runs whose
//! output is redirected to a file or pipe.

use std::cell::RefCell;
use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc as std_mpsc;
use std::thread::JoinHandle;

use tokio::sync::mpsc::UnboundedSender;

use crate::TrayEvent;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM,
    LRESULT, POINT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, DIB_RGB_COLORS, DeleteObject, GetDC,
    GetDIBits, HBITMAP, ReleaseDC,
};
use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows_sys::Win32::System::Console::{
    GetConsoleMode, GetConsoleProcessList, GetConsoleWindow, GetStdHandle, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows_sys::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcessId, OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE,
};
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, GetSystemMetricsForDpi,
    SetThreadDpiAwarenessContext,
};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW, ShellExecuteW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, FindWindowW, GetCursorPos,
    GetIconInfo, GetMessageW, GetSystemMetrics, GetWindowTextW, HICON, ICONINFO, LR_DEFAULTCOLOR,
    MB_ICONERROR, MB_OK, MENUITEMINFOW, MF_GRAYED, MF_SEPARATOR, MF_STRING, MIIM_BITMAP, MSG,
    MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
    SM_CXSMICON, SW_HIDE, SW_SHOWNORMAL, SetForegroundWindow, SetMenuDefaultItem, SetMenuItemInfoW,
    ShowWindow, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP, WM_CLOSE, WM_COMMAND,
    WM_CONTEXTMENU, WM_DESTROY, WM_ENDSESSION, WM_LBUTTONDBLCLK, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
};

/// Set on the tray process so it knows it is the copy, not the launcher.
const CHILD_ENV: &str = "VIGIL_TRAY_CHILD";
/// Set on the tray process when the launcher's console shows colour, so
/// the log lines it relays keep their colours.
pub const COLOR_ENV: &str = "VIGIL_FORCE_COLOR";
/// Set on the tray process when it was double-clicked: if the port is
/// taken, it moves on to the next free one (see ``server::bind``).
/// Restarts inherit it.
const FREE_PORT_ENV: &str = "VIGIL_FIND_FREE_PORT";
/// Printed by the tray process once it is listening, so the launcher
/// knows when to exit even with logging turned off. Never shown.
const READY_LINE: &str = "vigil-collector:listening";

/// Window class of the tray's hidden window. Scripts can find the window
/// by this name and post a menu command to it (see ``ID_RESTART``).
const CLASS_NAME: &str = "VigilCollectorTray";
const ID_RESTART: usize = 1;
const ID_QUIT: usize = 2;
const ID_CONNECTIONS: usize = 3;
const ID_VERSION: usize = 4;
const ID_OPEN: usize = 5;
const ID_ADDRESS: usize = 6;
const WM_TRAY: u32 = WM_APP + 1;
/// Posted by a second copy to the running one: open the dashboard.
const WM_OPEN_DASHBOARD: u32 = WM_APP + 2;
/// Held by the running copy, whatever its mode. ``Global\`` makes it one
/// per computer rather than one per signed-in user, so a ``SYSTEM`` task
/// and a tray icon can't both run.
const INSTANCE_MUTEX: &str = "Global\\VigilCollector";
/// Start of the tray window's title, which is followed by the dashboard
/// URL so a second copy can say where the running one is.
const WINDOW_TITLE: &str = "Vigil ";

/// ``DETACHED_PROCESS``: the copy gets no console at all, so nothing
/// flashes on screen and closing the terminal doesn't stop it.
const DETACHED_PROCESS: u32 = 0x0000_0008;
/// Keeps Ctrl-C in the terminal from reaching the copy.
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

const ICON: &[u8] = include_bytes!("../web/public/favicon.ico");
/// The dot beside the connection count: the dashboard's "success" green.
const DOT_RGB: [u8; 3] = [0x41, 0xb8, 0x83];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Run in this console, as on every other platform.
    Foreground,
    /// Start the tray process and exit once it is listening.
    Launcher,
    /// This is the tray process.
    Tray,
}

pub fn mode(foreground: bool) -> Mode {
    if mode_is_tray() {
        return Mode::Tray;
    }
    if foreground || in_session_zero() || !stdout_is_console() {
        return Mode::Foreground;
    }
    Mode::Launcher
}

/// Services and tasks that run as ``SYSTEM`` live in session 0, which
/// has no desktop or notification area.
fn in_session_zero() -> bool {
    let mut session = 0u32;
    // SAFETY: valid out-pointer; on failure we assume a normal session.
    unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) != 0 && session == 0 }
}

fn stdout_is_console() -> bool {
    let mut mode = 0;
    // SAFETY: GetConsoleMode only writes the out-pointer, and fails for a
    // handle that isn't a console (a file, a pipe, or none at all).
    unsafe { GetConsoleMode(GetStdHandle(STD_OUTPUT_HANDLE), &mut mode) != 0 }
}

/// True when the console belongs to this process alone, which is the case
/// when it was double-clicked rather than started from a terminal.
fn owns_console() -> bool {
    let mut ids = [0u32; 2];
    // SAFETY: the buffer and its length match.
    unsafe { GetConsoleProcessList(ids.as_mut_ptr(), ids.len() as u32) == 1 }
}

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

fn tray_command() -> std::io::Result<Command> {
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.args(std::env::args_os().skip(1))
        .env(CHILD_ENV, "1")
        .stdin(Stdio::null())
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    Ok(cmd)
}

/// Starts the tray process, relays its startup log, and exits once it is
/// listening (or with its error if it stops first).
pub fn launch(color: bool) -> ! {
    let double_clicked = owns_console();
    if double_clicked {
        // SAFETY: hides the console window this process was given; a null
        // handle (no window) is ignored by ShowWindow.
        unsafe { ShowWindow(GetConsoleWindow(), SW_HIDE) };
    }

    let fail = |message: String| -> ! {
        eprintln!("{message}");
        if double_clicked {
            let text = wide(message.trim());
            let caption = wide("Vigil");
            // SAFETY: both strings are NUL-terminated and outlive the call.
            unsafe {
                MessageBoxW(
                    std::ptr::null_mut(),
                    text.as_ptr(),
                    caption.as_ptr(),
                    MB_OK | MB_ICONERROR,
                )
            };
        }
        std::process::exit(1);
    };

    if Instance::exists() {
        if double_clicked && show_running() {
            std::process::exit(0);
        }
        fail(already_running_message());
    }

    let child = tray_command().and_then(|mut cmd| {
        if color {
            cmd.env(COLOR_ENV, "1");
        }
        if double_clicked {
            cmd.env(FREE_PORT_ENV, "1");
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
    });
    let mut child = match child {
        Ok(child) => child,
        Err(e) => fail(format!(
            "vigil-collector: couldn't start in the background: {e}"
        )),
    };

    // stderr carries the odd early warning (a log file that can't be
    // opened); collect it on its own thread so neither pipe can fill up.
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let stderr_reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });

    let mut seen = String::new();
    let mut lines = BufReader::new(child.stdout.take().expect("stdout is piped"));
    let mut out = std::io::stdout();
    let mut line = Vec::new();
    loop {
        line.clear();
        match lines.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&line);
        if text.trim_end() == READY_LINE {
            std::process::exit(0);
        }
        let _ = out.write_all(text.as_bytes());
        let _ = out.flush();
        seen.push_str(&text);
    }

    // The tray process stopped before it was listening.
    let status = child.wait();
    let errors = stderr_reader.join().unwrap_or_default();
    let mut message = strip_ansi(&seen);
    message.push_str(&errors);
    if message.trim().is_empty() {
        message = match status {
            Ok(s) => format!("vigil-collector stopped during startup ({s})."),
            Err(e) => format!("vigil-collector stopped during startup: {e}"),
        };
    }
    if !errors.is_empty() {
        eprint!("{errors}");
    }
    if double_clicked {
        fail(message);
    }
    std::process::exit(1);
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // CSI sequences: ESC [ ... final byte in @..~
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) && c != '[' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Tells the launcher that the server is listening. Does nothing outside
/// the tray process.
pub fn notify_listening() {
    if mode_is_tray() {
        let mut out = std::io::stdout();
        // The launcher may already be gone (after a restart there is
        // none), so a failed write is fine.
        let _ = writeln!(out, "{READY_LINE}");
        let _ = out.flush();
    }
}

fn mode_is_tray() -> bool {
    std::env::var_os(CHILD_ENV).is_some_and(|v| v == "1")
}

/// Proof that this is the only copy running: holds ``INSTANCE_MUTEX`` until
/// dropped (or the process exits).
pub struct Instance(HANDLE);

impl Instance {
    /// Claims the instance, or says where the running copy is.
    pub fn claim() -> Result<Self, String> {
        let name = wide(INSTANCE_MUTEX);
        // SAFETY: the name is NUL-terminated; the handle is closed on drop.
        unsafe {
            let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            // An existing mutex opens fine (with ERROR_ALREADY_EXISTS) if
            // it's ours to open; one made by another account (a SYSTEM
            // task) fails with access denied. Either way, it's taken.
            let taken = GetLastError();
            if handle.is_null() || taken == ERROR_ALREADY_EXISTS {
                if !handle.is_null() {
                    CloseHandle(handle);
                }
                return Err(already_running_message());
            }
            Ok(Self(handle))
        }
    }

    /// Whether another copy holds the instance, without claiming it.
    fn exists() -> bool {
        let name = wide(INSTANCE_MUTEX);
        // SAFETY: as in ``claim``.
        unsafe {
            let handle = OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, name.as_ptr());
            if handle.is_null() {
                return GetLastError() == ERROR_ACCESS_DENIED;
            }
            CloseHandle(handle);
            true
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateMutexW and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

/// The running copy's tray window, if it has one (a ``--foreground`` copy
/// doesn't).
fn running_window() -> HWND {
    let class = wide(CLASS_NAME);
    // SAFETY: the class name is NUL-terminated.
    unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) }
}

/// The running copy's dashboard URL, read from its tray window's title.
fn running_url() -> Option<String> {
    let hwnd = running_window();
    if hwnd.is_null() {
        return None;
    }
    let mut buf = [0u16; 256];
    // SAFETY: the buffer and its length match.
    let len = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    let title = String::from_utf16_lossy(&buf[..len.max(0) as usize]);
    title
        .strip_prefix(WINDOW_TITLE)
        .filter(|url| !url.is_empty())
        .map(str::to_string)
}

/// Asks the running copy to open its dashboard. False if it has no tray
/// window to ask.
fn show_running() -> bool {
    let hwnd = running_window();
    // SAFETY: posting to another process's window is fine; a stale handle
    // just fails.
    !hwnd.is_null() && unsafe { PostMessageW(hwnd, WM_OPEN_DASHBOARD, 0, 0) } != 0
}

fn already_running_message() -> String {
    match running_url() {
        Some(url) => format!(
            "Vigil is already running at {url}. Use its tray icon to restart \
             or quit it."
        ),
        None => "Vigil is already running on this computer.".into(),
    }
}

/// Whether a taken port should move this process on to the next free one.
pub fn find_free_port() -> bool {
    std::env::var_os(FREE_PORT_ENV).is_some_and(|v| v == "1")
}

/// Starts a fresh tray process with the same arguments. Used by Restart,
/// after this one has stopped listening.
pub fn respawn() -> std::io::Result<()> {
    tray_command()?
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

/// The tray icon. It lives on its own thread, which runs the Win32
/// message loop the icon and its menu need. Dropping it removes the icon.
pub struct Tray {
    hwnd: usize,
    thread: Option<JoinHandle<()>>,
}

impl Tray {
    /// ``url`` is the dashboard's address, shown in the tooltip and opened
    /// by a double-click. ``connections`` is the server's count of open
    /// WebSocket sessions, read each time the menu opens.
    pub fn start(
        url: String,
        events: UnboundedSender<TrayEvent>,
        connections: Arc<AtomicUsize>,
    ) -> anyhow::Result<Self> {
        let (ready_tx, ready_rx) = std_mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("tray".into())
            .spawn(move || run(url, events, connections, ready_tx))?;
        match ready_rx.recv() {
            Ok(Ok(hwnd)) => Ok(Self {
                hwnd,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(anyhow::anyhow!(e))
            }
            Err(_) => Err(anyhow::anyhow!("the tray thread stopped during startup")),
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        // SAFETY: the window belongs to the tray thread, which is still
        // running its message loop until it handles this WM_CLOSE.
        unsafe { PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct State {
    icon: NOTIFYICONDATAW,
    events: UnboundedSender<TrayEvent>,
    taskbar_created: u32,
    connections: Arc<AtomicUsize>,
    url: Vec<u16>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn send(event: TrayEvent) {
    STATE.with_borrow(|state| {
        if let Some(state) = state {
            let _ = state.events.send(event);
        }
    });
}

fn add_icon() {
    let icon = STATE.with_borrow(|state| state.as_ref().map(|s| s.icon));
    if let Some(icon) = icon {
        // SAFETY: the struct is fully initialised and its window is alive.
        unsafe {
            Shell_NotifyIconW(NIM_ADD, &icon);
            Shell_NotifyIconW(NIM_SETVERSION, &icon);
        }
    }
}

fn run(
    url: String,
    events: UnboundedSender<TrayEvent>,
    connections: Arc<AtomicUsize>,
    ready: std_mpsc::Sender<Result<usize, String>>,
) {
    // SAFETY: all calls below are plain Win32 calls on this thread, with
    // NUL-terminated strings that outlive them.
    unsafe {
        // Per-monitor DPI awareness for this thread only: the icon is
        // created at the size the taskbar really draws it, and the menu
        // isn't bitmap-stretched on scaled displays.
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // ShellExecuteW (double-click opens the dashboard) wants COM set up
        // on the calling thread, single-threaded as for any UI thread.
        CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);

        let instance = GetModuleHandleW(std::ptr::null());
        let class = wide(CLASS_NAME);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let title = wide(format!("{WINDOW_TITLE}{url}"));
        // A hidden top-level window rather than a message-only one: only
        // top-level windows receive the TaskbarCreated broadcast (sent when
        // Explorer restarts) and WM_ENDSESSION.
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            let _ = ready.send(Err(format!(
                "couldn't create the tray window: {}",
                std::io::Error::last_os_error()
            )));
            return;
        }

        let hicon = load_icon(GetSystemMetrics(SM_CXSMICON).max(16));

        let mut icon = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: WM_TRAY,
            hIcon: hicon,
            ..Default::default()
        };
        icon.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let tooltip = format!("Vigil\n{url}");
        let tip: Vec<u16> = tooltip.encode_utf16().take(icon.szTip.len() - 1).collect();
        icon.szTip[..tip.len()].copy_from_slice(&tip);

        STATE.set(Some(State {
            icon,
            events,
            taskbar_created: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            connections,
            url: wide(&url),
        }));

        if Shell_NotifyIconW(NIM_ADD, &icon) == 0 {
            STATE.set(None);
            DestroyWindow(hwnd);
            if !hicon.is_null() {
                DestroyIcon(hicon);
            }
            let _ = ready.send(Err("couldn't add the notification area icon".into()));
            return;
        }
        Shell_NotifyIconW(NIM_SETVERSION, &icon);
        let _ = ready.send(Ok(hwnd as usize));

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        STATE.set(None);
        if !hicon.is_null() {
            DestroyIcon(hicon);
        }
    }
}

/// The Vigil logo as an icon ``size`` pixels square, from the image in
/// ``favicon.ico`` that suits that size best. Null if it couldn't be made.
fn load_icon(size: i32) -> HICON {
    let Some(range) = crate::ico::pick(ICON, size as u32) else {
        return std::ptr::null_mut();
    };
    // SAFETY: the pointer and length describe one image inside ICON.
    unsafe {
        CreateIconFromResourceEx(
            ICON[range.clone()].as_ptr(),
            range.len() as u32,
            1,
            0x0003_0000,
            size,
            size,
            LR_DEFAULTCOLOR,
        )
    }
}

/// Shows the menu at the pointer. The menu posts WM_COMMAND back to the
/// window when an item is chosen.
///
/// ```text
/// [logo] Open dashboard              (bold: what a double-click does)
/// ──────────────────────────────────
///  (●)   Monitoring · 1 dashboard connected
///        http://127.0.0.1:4500       (greyed)
/// ──────────────────────────────────
///        Restart
///        Quit
/// ──────────────────────────────────
///        Vigil v0.2.0                (greyed)
/// ```
unsafe fn show_menu(hwnd: HWND) {
    let Some((connections, url)) = STATE.with_borrow(|s| {
        s.as_ref()
            .map(|s| (s.connections.load(Ordering::Acquire), s.url.clone()))
    }) else {
        return;
    };
    let open = wide("Open dashboard");
    let status = wide(status_text(connections));
    let restart = wide("Restart");
    let quit = wide("Quit");
    let version = wide(format!("Vigil v{}", env!("CARGO_PKG_VERSION")));
    // SAFETY (caller): called on the tray thread with its live window.
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        AppendMenuW(menu, MF_STRING, ID_OPEN, open.as_ptr());
        SetMenuDefaultItem(menu, ID_OPEN as u32, 0);
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        // Not greyed, or Windows draws the green dot grey too. Clicking it
        // does nothing.
        AppendMenuW(menu, MF_STRING, ID_CONNECTIONS, status.as_ptr());
        AppendMenuW(menu, MF_STRING | MF_GRAYED, ID_ADDRESS, url.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, ID_RESTART, restart.as_ptr());
        AppendMenuW(menu, MF_STRING, ID_QUIT, quit.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING | MF_GRAYED, ID_VERSION, version.as_ptr());

        // The logo and the dot go in the column where menus put item
        // icons, which also lines every item's text up after it. Both are
        // sized for the menu's DPI.
        let size = GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForWindow(hwnd)).max(16);
        let logo = logo_bitmap(size);
        let dot = bitmap_from_pixels(size, &dot_pixels(size as usize));
        for (id, bitmap) in [(ID_OPEN, logo), (ID_CONNECTIONS, dot)] {
            if !bitmap.is_null() {
                let item = MENUITEMINFOW {
                    cbSize: size_of::<MENUITEMINFOW>() as u32,
                    fMask: MIIM_BITMAP,
                    hbmpItem: bitmap,
                    ..Default::default()
                };
                SetMenuItemInfoW(menu, id as u32, 0, &item);
            }
        }

        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        // Without this the menu stays open when you click elsewhere.
        SetForegroundWindow(hwnd);
        TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, std::ptr::null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        for bitmap in [logo, dot] {
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
        }
    }
}

/// The menu's status line for ``connections`` open dashboards.
fn status_text(connections: usize) -> String {
    match connections {
        0 => "Monitoring \u{b7} no dashboards connected".into(),
        1 => "Monitoring \u{b7} 1 dashboard connected".into(),
        n => format!("Monitoring \u{b7} {n} dashboards connected"),
    }
}

/// Opens the dashboard in the default browser.
fn open_dashboard() {
    let Some(url) = STATE.with_borrow(|s| s.as_ref().map(|s| s.url.clone())) else {
        return;
    };
    let open = wide("open");
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            open.as_ptr(),
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// A 32-bit bitmap, ``size`` pixels square, from premultiplied BGRA
/// ``pixels`` in rows from the top. Menus draw it with its transparency.
/// Null if it couldn't be made.
fn bitmap_from_pixels(size: i32, pixels: &[u32]) -> HBITMAP {
    if pixels.len() != (size * size) as usize {
        return std::ptr::null_mut();
    }
    let info = bitmap_info(size);
    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    // SAFETY: ``bits`` receives a buffer of size * size 32-bit pixels,
    // which is exactly what's copied into it.
    unsafe {
        let bitmap = CreateDIBSection(
            std::ptr::null_mut(),
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        if !bitmap.is_null() && !bits.is_null() {
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u32>(), pixels.len());
        }
        bitmap
    }
}

/// Describes a top-down 32-bit bitmap ``size`` pixels square.
fn bitmap_info(size: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            // Negative: rows run top to bottom.
            biHeight: -size,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// The logo as a menu bitmap: the icon's colour pixels, read back from
/// the icon Windows made, with their alpha premultiplied as menus expect.
fn logo_bitmap(size: i32) -> HBITMAP {
    let hicon = load_icon(size);
    if hicon.is_null() {
        return std::ptr::null_mut();
    }
    let mut pixels = vec![0u32; (size * size) as usize];
    // SAFETY: GetIconInfo hands back two bitmaps this function owns and
    // deletes; GetDIBits writes at most size rows of size pixels into
    // ``pixels``, as ``info`` describes.
    let read = unsafe {
        let mut icon_info: ICONINFO = std::mem::zeroed();
        let ok = GetIconInfo(hicon, &mut icon_info) != 0 && !icon_info.hbmColor.is_null();
        let mut rows = 0;
        if ok {
            let mut info = bitmap_info(size);
            let dc = GetDC(std::ptr::null_mut());
            rows = GetDIBits(
                dc,
                icon_info.hbmColor,
                0,
                size as u32,
                pixels.as_mut_ptr().cast(),
                &mut info,
                DIB_RGB_COLORS,
            );
            ReleaseDC(std::ptr::null_mut(), dc);
        }
        for bitmap in [icon_info.hbmColor, icon_info.hbmMask] {
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
        }
        DestroyIcon(hicon);
        rows == size
    };
    // An icon without an alpha channel reads back fully transparent.
    if !read || pixels.iter().all(|p| p >> 24 == 0) {
        return std::ptr::null_mut();
    }
    let pixels: Vec<u32> = pixels.into_iter().map(premultiply).collect();
    bitmap_from_pixels(size, &pixels)
}

/// Premultiplies one BGRA pixel's colour by its alpha.
fn premultiply(pixel: u32) -> u32 {
    let a = pixel >> 24;
    let scale = |shift: u32| ((pixel >> shift & 0xff) * a / 255) << shift;
    a << 24 | scale(16) | scale(8) | scale(0)
}

/// The dot's pixels, row by row from the top, as premultiplied BGRA (the
/// layout ``AlphaBlend`` expects): a filled circle a little over half the
/// width, centred, with its edge smoothed by sampling each pixel 4 x 4.
fn dot_pixels(size: usize) -> Vec<u32> {
    const SAMPLES: usize = 4;
    let centre = size as f32 / 2.0;
    let radius = size as f32 * 0.28;
    let mut out = Vec::with_capacity(size * size);
    for y in 0..size {
        for x in 0..size {
            let mut inside = 0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = x as f32 + (sx as f32 + 0.5) / SAMPLES as f32 - centre;
                    let py = y as f32 + (sy as f32 + 0.5) / SAMPLES as f32 - centre;
                    if px * px + py * py <= radius * radius {
                        inside += 1;
                    }
                }
            }
            let alpha = (inside * 255 / (SAMPLES * SAMPLES)) as u32;
            let [r, g, b] = DOT_RGB.map(|c| u32::from(c) * alpha / 255);
            out.push(alpha << 24 | r << 16 | g << 8 | b);
        }
    }
    out
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let taskbar_created = STATE.with_borrow(|s| s.as_ref().map_or(0, |s| s.taskbar_created));
    // SAFETY: called by the system on the tray thread for its own window.
    unsafe {
        match msg {
            WM_TRAY => {
                // With NOTIFYICON_VERSION_4 the event is in the low word.
                let event = (lparam as u32) & 0xFFFF;
                match event {
                    WM_CONTEXTMENU | WM_RBUTTONUP => show_menu(hwnd),
                    WM_LBUTTONDBLCLK => open_dashboard(),
                    _ => {}
                }
                0
            }
            WM_OPEN_DASHBOARD => {
                open_dashboard();
                0
            }
            WM_COMMAND => {
                match wparam & 0xFFFF {
                    ID_RESTART => send(TrayEvent::Restart),
                    ID_QUIT => send(TrayEvent::Quit),
                    ID_OPEN => open_dashboard(),
                    _ => {}
                }
                0
            }
            // Signing out or shutting down: stop cleanly while there's time.
            WM_ENDSESSION if wparam != 0 => {
                send(TrayEvent::Quit);
                0
            }
            WM_CLOSE => {
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                let icon = STATE.with_borrow(|s| s.as_ref().map(|s| s.icon));
                if let Some(icon) = icon {
                    Shell_NotifyIconW(NIM_DELETE, &icon);
                }
                PostQuitMessage(0);
                0
            }
            // Explorer restarted and lost every icon: add ours back.
            m if m != 0 && m == taskbar_created => {
                add_icon();
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DOT_RGB, dot_pixels, premultiply, status_text, strip_ansi};

    #[test]
    fn strips_colour_codes() {
        assert_eq!(
            strip_ansi("\x1b[2m[2026-11-11]\x1b[0m \x1b[32m INFO\x1b[0m Listening"),
            "[2026-11-11]  INFO Listening"
        );
    }

    #[test]
    fn counts_dashboards_in_words() {
        assert_eq!(status_text(0), "Monitoring \u{b7} no dashboards connected");
        assert_eq!(status_text(1), "Monitoring \u{b7} 1 dashboard connected");
        assert_eq!(status_text(3), "Monitoring \u{b7} 3 dashboards connected");
    }

    #[test]
    fn premultiplies_colour_by_alpha() {
        assert_eq!(premultiply(0xff10_2030), 0xff10_2030);
        assert_eq!(premultiply(0x00ff_ffff), 0);
        assert_eq!(premultiply(0x80ff_0000), 0x8080_0000);
    }

    #[test]
    fn draws_a_round_green_dot() {
        let size = 16;
        let pixels = dot_pixels(size);
        assert_eq!(pixels.len(), size * size);
        // Corners are transparent, the centre is solid green.
        assert_eq!(pixels[0], 0);
        assert_eq!(pixels[size * size - 1], 0);
        let [r, g, b] = DOT_RGB.map(u32::from);
        assert_eq!(pixels[8 * size + 8], 0xff << 24 | r << 16 | g << 8 | b);
        // Premultiplied: no channel is brighter than the alpha.
        for p in pixels {
            let a = p >> 24;
            assert!((p >> 16 & 0xff) <= a && (p >> 8 & 0xff) <= a && (p & 0xff) <= a);
        }
    }
}
