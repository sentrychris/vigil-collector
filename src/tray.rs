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
//!    shows how many dashboards are connected and the version.
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
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, DIB_RGB_COLORS, DeleteObject, HBITMAP,
};
use windows_sys::Win32::System::Console::{
    GetConsoleMode, GetConsoleProcessList, GetConsoleWindow, GetStdHandle, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, GetSystemMetricsForDpi,
    SetThreadDpiAwarenessContext,
};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
    GetSystemMetrics, LR_DEFAULTCOLOR, MB_ICONERROR, MB_OK, MENUITEMINFOW, MF_GRAYED, MF_SEPARATOR,
    MF_STRING, MIIM_BITMAP, MSG, MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SM_CXSMICON, SW_HIDE, SetForegroundWindow, SetMenuItemInfoW,
    ShowWindow, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP, WM_CLOSE, WM_COMMAND,
    WM_CONTEXTMENU, WM_DESTROY, WM_ENDSESSION, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
};

/// Set on the tray process so it knows it is the copy, not the launcher.
const CHILD_ENV: &str = "VIGIL_TRAY_CHILD";
/// Set on the tray process when the launcher's console shows colour, so
/// the log lines it relays keep their colours.
pub const COLOR_ENV: &str = "VIGIL_FORCE_COLOR";
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
const WM_TRAY: u32 = WM_APP + 1;

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
            let caption = wide("Vigil Collector");
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

    let child = tray_command().and_then(|mut cmd| {
        if color {
            cmd.env(COLOR_ENV, "1");
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
    /// ``connections`` is the server's count of open WebSocket sessions,
    /// read each time the menu opens.
    pub fn start(
        tooltip: String,
        events: UnboundedSender<TrayEvent>,
        connections: Arc<AtomicUsize>,
    ) -> anyhow::Result<Self> {
        let (ready_tx, ready_rx) = std_mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("tray".into())
            .spawn(move || run(tooltip, events, connections, ready_tx))?;
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
    tooltip: String,
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

        let instance = GetModuleHandleW(std::ptr::null());
        let class = wide(CLASS_NAME);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let title = wide("Vigil Collector");
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

        let size = GetSystemMetrics(SM_CXSMICON).max(16);
        let hicon = match crate::ico::pick(ICON, size as u32) {
            Some(range) => CreateIconFromResourceEx(
                ICON[range.clone()].as_ptr(),
                range.len() as u32,
                1,
                0x0003_0000,
                size,
                size,
                LR_DEFAULTCOLOR,
            ),
            None => std::ptr::null_mut(),
        };

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
        let tip: Vec<u16> = tooltip.encode_utf16().take(icon.szTip.len() - 1).collect();
        icon.szTip[..tip.len()].copy_from_slice(&tip);

        STATE.set(Some(State {
            icon,
            events,
            taskbar_created: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            connections,
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

/// Shows the menu at the pointer. The menu posts WM_COMMAND back to the
/// window when an item is chosen.
///
/// ```text
/// Restart
/// Quit
/// ─────────────────────────────────────
/// ● 1 connected
/// Vigil Collector - Monitoring - v0.2.0
/// ```
unsafe fn show_menu(hwnd: HWND) {
    let Some(connections) =
        STATE.with_borrow(|s| s.as_ref().map(|s| s.connections.load(Ordering::Acquire)))
    else {
        return;
    };
    let restart = wide("Restart");
    let quit = wide("Quit");
    let connected = wide(format!("{connections} connected"));
    let title = wide(format!(
        "Vigil Collector - Monitoring - v{}",
        env!("CARGO_PKG_VERSION")
    ));
    // SAFETY (caller): called on the tray thread with its live window.
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        AppendMenuW(menu, MF_STRING, ID_RESTART, restart.as_ptr());
        AppendMenuW(menu, MF_STRING, ID_QUIT, quit.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, ID_CONNECTIONS, connected.as_ptr());
        AppendMenuW(menu, MF_STRING | MF_GRAYED, ID_VERSION, title.as_ptr());

        // The dot is sized for the menu's DPI, in the column where menus
        // put item icons.
        let size = GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForWindow(hwnd)).max(16);
        let dot = dot_bitmap(size);
        if !dot.is_null() {
            let item = MENUITEMINFOW {
                cbSize: size_of::<MENUITEMINFOW>() as u32,
                fMask: MIIM_BITMAP,
                hbmpItem: dot,
                ..Default::default()
            };
            SetMenuItemInfoW(menu, ID_CONNECTIONS as u32, 0, &item);
        }

        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        // Without this the menu stays open when you click elsewhere.
        SetForegroundWindow(hwnd);
        TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, std::ptr::null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        if !dot.is_null() {
            DeleteObject(dot);
        }
    }
}

/// A 32-bit bitmap of the green dot, ``size`` pixels square, which menus
/// draw with its transparency. Null if it couldn't be made.
fn dot_bitmap(size: i32) -> HBITMAP {
    let pixels = dot_pixels(size as usize);
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            // Negative: rows run top to bottom, like ``pixels``.
            biHeight: -size,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        },
        ..Default::default()
    };
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
                if event == WM_CONTEXTMENU || event == WM_RBUTTONUP {
                    show_menu(hwnd);
                }
                0
            }
            WM_COMMAND => {
                match wparam & 0xFFFF {
                    ID_RESTART => send(TrayEvent::Restart),
                    ID_QUIT => send(TrayEvent::Quit),
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
    use super::{DOT_RGB, dot_pixels, strip_ansi};

    #[test]
    fn strips_colour_codes() {
        assert_eq!(
            strip_ansi("\x1b[2m[2026-11-11]\x1b[0m \x1b[32m INFO\x1b[0m Listening"),
            "[2026-11-11]  INFO Listening"
        );
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
