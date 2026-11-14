//! A window of Vigil's own on Windows: an ordinary window of the tray
//! thread's that shows the picture Slint paints (``surface.rs``) and
//! passes the mouse and the keyboard on to it.
//!
//! Each repaint is one copy of the picture to the screen through GDI.
//! Slint only paints when something on it changed, and while the window
//! is minimised nothing is laid out or painted at all.
//!
//! Like the full dashboard's window (``window.rs``), a shell has no title
//! bar from Windows: the layout draws its own, and says which part of it
//! the window is dragged by. It lives on the tray thread, and everything
//! here must be called on that thread.
//!
//! This is what the compact window and Settings have in common. Each
//! keeps a ``Shell`` beside its own layout, hands it the messages every
//! such window answers the same way (``handle``), and answers the rest
//! itself.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{LogicalPosition, SharedString};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, DIB_RGB_COLORS, EndPaint, InvalidateRect,
    PAINTSTRUCT, ScreenToClient, SetDIBitsToDevice,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, HICON, HTCAPTION,
    HTCLIENT, HTTOP, HTTOPLEFT, HTTOPRIGHT, IDC_ARROW, KillTimer, LoadCursorW, RegisterClassW,
    SIZE_MINIMIZED, SetTimer, WM_CHAR, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN, WM_KEYUP,
    WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCALCSIZE,
    WM_NCHITTEST, WM_PAINT, WM_SETFOCUS, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WNDCLASSW, WNDPROC,
};

use super::bar::{self, Part};
use super::keys;
use super::surface::{self, Frame, Surface};
use crate::config::WindowPlace;
use crate::frame;
use crate::tray::wide;

/// Sent to a window when the pointer leaves it, once asked for with
/// ``TrackMouseEvent``.
const WM_MOUSELEAVE: u32 = 0x02A3;

/// Runs Slint's clock while something on the window is moving. A window
/// with timers of its own numbers them from ``FIRST_OWN_TIMER``.
const FRAME_TIMER: usize = 1;
pub const FIRST_OWN_TIMER: usize = 2;

/// About sixty pictures a second while something is moving.
const FRAME: Duration = Duration::from_millis(16);
/// How far one notch of the mouse wheel scrolls, in layout pixels.
const WHEEL_NOTCH: f32 = 48.0;
/// What Windows calls one notch.
const WHEEL_DELTA: f32 = 120.0;

// The keys that change what others mean (winuser.h).
const VK_CONTROL: i32 = 0x11;
const VK_MENU: i32 = 0x12;

/// What a kind of window is made with.
pub struct Make<'a> {
    pub class: &'a str,
    pub title: &'a str,
    /// The window's style, such as ``WS_OVERLAPPEDWINDOW``.
    pub style: u32,
    pub procedure: WNDPROC,
    /// Where it was when it was last closed, if it has been open before.
    pub place: Option<WindowPlace>,
    /// Its first size, in pixels at 100% scaling.
    pub size: (i32, i32),
}

/// One of Vigil's own windows while it is open: the window, where Slint
/// paints, and the picture so far.
pub struct Shell {
    pub hwnd: HWND,
    pub surface: Rc<Surface>,
    /// The picture, kept between repaints.
    frame: RefCell<Frame>,
    icons: [HICON; 2],
    minimized: Cell<bool>,
    /// Whether Windows has been asked to say when the pointer leaves.
    watching_pointer: Cell<bool>,
    /// The first half of a typed character that arrives in two (see
    /// ``keys::typed``).
    half_typed: Cell<Option<u16>>,
}

impl Shell {
    /// Makes the window, still hidden, at ``make.place`` or, with none,
    /// at its first size in the middle of the screen.
    ///
    /// # Safety
    /// Call on the tray thread.
    pub unsafe fn create(make: &Make) -> Result<HWND, String> {
        // SAFETY (caller): plain Win32 calls on this thread, with
        // NUL-terminated strings that outlive them.
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let class = wide(make.class);
            let wc = WNDCLASSW {
                lpfnWndProc: make.procedure,
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                // No background brush: the picture covers the whole window.
                ..Default::default()
            };
            // Fails harmlessly when the class exists from an earlier window.
            RegisterClassW(&wc);
            let title = wide(make.title);
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                make.style,
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
            frame::place(hwnd, make.place, make.size);
            Ok(hwnd)
        }
    }

    /// Joins a window from ``create`` to the surface its layout draws on:
    /// from here on a change to the layout repaints the window.
    pub fn new(hwnd: HWND, surface: Rc<Surface>) -> Shell {
        surface.on_stale(move || {
            // SAFETY: a plain call on this thread's own window; one that
            // has gone just fails.
            unsafe { InvalidateRect(hwnd, std::ptr::null(), 0) };
        });
        Shell {
            hwnd,
            surface,
            frame: RefCell::new(Frame::default()),
            // SAFETY: the window was made on this thread.
            icons: unsafe { frame::set_icons(hwnd) },
            minimized: Cell::new(false),
            watching_pointer: Cell::new(false),
            half_typed: Cell::new(None),
        }
    }

    /// Takes the window off the screen and apart, and says where it was.
    pub fn destroy(&self) -> Option<WindowPlace> {
        let place = frame::placement(self.hwnd).map(|place| WindowPlace {
            maximized: false,
            ..place
        });
        // SAFETY: the window and its icons belong to this thread and are
        // destroyed once: whoever holds the shell drops it after this.
        unsafe {
            DestroyWindow(self.hwnd);
            frame::destroy_icons(self.icons);
        }
        place
    }

    /// The window's scaling: 1.0 at 100%, 1.5 at 150%.
    pub fn scale(&self) -> f32 {
        // SAFETY: a plain query of this thread's own window.
        unsafe { GetDpiForWindow(self.hwnd) }.max(96) as f32 / 96.0
    }

    pub fn minimized(&self) -> bool {
        self.minimized.get()
    }

    /// Takes note of a ``WM_SIZE`` and says whether the window is showing
    /// (rather than minimised) after it.
    pub fn sized(&self, wparam: WPARAM) -> bool {
        let minimized = wparam == SIZE_MINIMIZED as WPARAM;
        self.minimized.set(minimized);
        !minimized
    }

    /// Tells the layout the size and scaling the window has now, and says
    /// what they are: its width and height in real pixels, and how many
    /// of those make a layout pixel.
    pub fn fit(&self) -> Option<(u32, u32, f32)> {
        let mut client = RECT::default();
        // SAFETY: a query of this thread's own window into a valid
        // out-pointer.
        if unsafe { GetClientRect(self.hwnd, &mut client) } == 0 {
            return None;
        }
        let scale = self.scale();
        let (width, height) = (client.right.max(0) as u32, client.bottom.max(0) as u32);
        self.surface.resize(width, height, scale);
        Some((width, height, scale))
    }

    /// Passes a mouse or keyboard event on to the layout.
    pub fn send(&self, event: WindowEvent) {
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
    /// timer is due (a text field's cursor blinking, say), and not at all
    /// while nothing is pending.
    pub fn keep_time(&self) {
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

    /// Which part of the window is at a place on the screen (see
    /// ``bar.rs``), as the answer ``WM_NCHITTEST`` wants. ``bar`` is the
    /// layout's bar: its height, and how far from the left it is bare.
    fn part_at(&self, mut at: POINT, bar: (f32, f32)) -> u32 {
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
            bar.0,
            bar.1,
        );
        match part {
            Part::Inside => HTCLIENT,
            Part::Bar => HTCAPTION,
            Part::Top => HTTOP,
            Part::TopLeft => HTTOPLEFT,
            Part::TopRight => HTTOPRIGHT,
        }
    }

    /// A key went down or came up: passes it on if it is one that types
    /// nothing (see ``keys.rs``), or a letter pressed with Ctrl.
    fn key(&self, vk: u32, down: bool, repeated: bool) {
        if let Some(key) = keys::named(vk) {
            let text = SharedString::from(key);
            self.send(match (down, repeated) {
                (true, false) => WindowEvent::KeyPressed { text },
                (true, true) => WindowEvent::KeyPressRepeated { text },
                (false, _) => WindowEvent::KeyReleased { text },
            });
        } else if down
            && held(VK_CONTROL)
            && !held(VK_MENU)
            && let Some(letter) = keys::with_control(vk)
        {
            self.type_character(letter);
        }
    }

    fn type_character(&self, typed: char) {
        let text = SharedString::from(typed);
        self.send(WindowEvent::KeyPressed { text: text.clone() });
        self.send(WindowEvent::KeyReleased { text });
    }

    /// Something was typed. With AltGr held (which Windows reports as
    /// Ctrl and Alt together) the layout would take it for a shortcut and
    /// type nothing, so for the length of the character it is told both
    /// are up.
    fn typed(&self, unit: u16) {
        let mut pending = self.half_typed.take();
        let typed = keys::typed(unit, &mut pending);
        self.half_typed.set(pending);
        let Some(typed) = typed else {
            return;
        };
        let alt_gr = held(VK_CONTROL) && held(VK_MENU);
        if alt_gr {
            self.modifiers(false, &[Key::Control, Key::Alt]);
        }
        self.type_character(typed);
        if alt_gr {
            self.modifiers(true, &[Key::Control, Key::Alt]);
        }
    }

    /// Tells the layout that ``keys`` went down or came up.
    fn modifiers(&self, down: bool, keys: &[Key]) {
        for key in keys {
            let text = SharedString::from(*key);
            self.send(if down {
                WindowEvent::KeyPressed { text }
            } else {
                WindowEvent::KeyReleased { text }
            });
        }
    }

    /// Answers the messages every window of Vigil's own answers the same
    /// way: painting, the mouse, the keyboard, and which part of the
    /// window is where. ``bar`` is the layout's bar (see ``part_at``).
    /// ``None`` leaves the message to the caller.
    ///
    /// # Safety
    /// Call only from the window procedure of this shell's window, with
    /// the message's own ``wparam`` and ``lparam``.
    pub unsafe fn handle(
        &self,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        bar: (f32, f32),
    ) -> Option<LRESULT> {
        let hwnd = self.hwnd;
        // SAFETY (caller): on the window's own thread, with pointers
        // valid for the messages they come with.
        unsafe {
            match msg {
                WM_PAINT => self.paint(),
                // The sides and bottom are Windows' own resize borders.
                // What is left is the picture, whose bar and top edge
                // stand in for a title bar and a top border.
                WM_NCHITTEST => {
                    let hit = DefWindowProcW(hwnd, msg, wparam, lparam);
                    return Some(if hit == HTCLIENT as LRESULT {
                        self.part_at(point(lparam), bar) as LRESULT
                    } else {
                        hit
                    });
                }
                WM_MOUSEMOVE => {
                    self.watch_pointer();
                    let at = point(lparam);
                    self.send(WindowEvent::PointerMoved {
                        position: self.position(at.x, at.y),
                    });
                }
                WM_LBUTTONDOWN => {
                    // Keeps the mouse while the button is down, so a
                    // press that slides off the window is still let go
                    // of.
                    SetCapture(hwnd);
                    let at = point(lparam);
                    self.send(WindowEvent::PointerPressed {
                        position: self.position(at.x, at.y),
                        button: PointerEventButton::Left,
                    });
                }
                WM_LBUTTONUP => {
                    let at = point(lparam);
                    self.send(WindowEvent::PointerReleased {
                        position: self.position(at.x, at.y),
                        button: PointerEventButton::Left,
                    });
                    ReleaseCapture();
                }
                WM_MOUSELEAVE => {
                    self.watching_pointer.set(false);
                    self.send(WindowEvent::PointerExited);
                }
                WM_MOUSEWHEEL => {
                    // The wheel's position is on the screen, not the
                    // window.
                    let mut at = point(lparam);
                    ScreenToClient(hwnd, &mut at);
                    let notches = f32::from((wparam >> 16) as u16 as i16) / WHEEL_DELTA;
                    self.send(WindowEvent::PointerScrolled {
                        position: self.position(at.x, at.y),
                        delta_x: 0.0,
                        delta_y: notches * WHEEL_NOTCH,
                    });
                }
                WM_KEYDOWN | WM_SYSKEYDOWN => {
                    // Bit 30 says the key was already down: it is held
                    // and repeating.
                    self.key(wparam as u32, true, lparam & (1 << 30) != 0);
                    // Alt with a key is also Windows' own (Alt+F4, the
                    // window menu), so those go on to it.
                    if msg == WM_SYSKEYDOWN {
                        return None;
                    }
                }
                WM_KEYUP | WM_SYSKEYUP => {
                    self.key(wparam as u32, false, false);
                    if msg == WM_SYSKEYUP {
                        return None;
                    }
                }
                WM_CHAR => self.typed(wparam as u16),
                WM_SETFOCUS => self.send(WindowEvent::WindowActiveChanged(true)),
                WM_KILLFOCUS => {
                    // Whatever was held is let go of somewhere this
                    // window won't hear about.
                    self.modifiers(false, &[Key::Shift, Key::Control, Key::Alt]);
                    self.send(WindowEvent::WindowActiveChanged(false));
                }
                WM_TIMER if wparam == FRAME_TIMER => {
                    surface::tick();
                    self.keep_time();
                }
                _ => return None,
            }
        }
        Some(0)
    }

    /// The picture as it stands, brought up to date first, and its width
    /// and height. Asked for directly, so it doesn't depend on the window
    /// being in view.
    #[cfg(test)]
    pub fn picture(&self) -> (Vec<u32>, u32, u32) {
        use windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW;
        // SAFETY: a message to this thread's own window.
        unsafe { SendMessageW(self.hwnd, WM_PAINT, 0, 0) };
        let frame = self.frame.borrow();
        (
            frame.pixels.iter().map(|pixel| pixel.0).collect(),
            frame.width,
            frame.height,
        )
    }
}

/// Whether a key is down as of the message being handled.
fn held(vk: i32) -> bool {
    // SAFETY: a plain query of the keyboard's state.
    unsafe { GetKeyState(vk) < 0 }
}

/// Answers what is asked about the frame of a window with no title bar
/// from Windows, which needs nothing of the window's own state and is
/// asked before the window has any. ``smallest`` is the least the window
/// can be dragged to, in pixels at 100% scaling. ``None`` leaves the
/// message to the caller.
///
/// # Safety
/// Call only from the window procedure of ``hwnd``, with the message's
/// own ``wparam`` and ``lparam``.
pub unsafe fn frame_message(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    smallest: (i32, i32),
) -> Option<LRESULT> {
    // SAFETY (caller): the pointers in ``lparam`` are valid for the
    // messages they come with.
    unsafe {
        match msg {
            // No title bar: the layout draws one.
            WM_NCCALCSIZE => Some(frame::without_title_bar(hwnd, wparam, lparam)),
            WM_GETMINMAXINFO => {
                frame::no_smaller_than(hwnd, lparam, smallest);
                Some(0)
            }
            // The picture covers everything, so there is nothing to clear
            // first (and clearing would flicker).
            WM_ERASEBKGND => Some(1),
            _ => None,
        }
    }
}

/// The two halves of a message's ``lparam`` when it carries a position:
/// x in the low 16 bits and y in the next 16, each signed (displays left
/// of or above the primary one have negative coordinates).
pub fn point(lparam: LPARAM) -> POINT {
    POINT {
        x: i32::from(lparam as u16 as i16),
        y: i32::from((lparam >> 16) as u16 as i16),
    }
}

/// The other way, for tests that send a window the mouse.
#[cfg(test)]
pub fn lparam(x: f32, y: f32) -> LPARAM {
    (((y as i32 as u32) << 16) | (x as i32 as u32 & 0xffff)) as LPARAM
}

/// Handles this thread's waiting messages until ``done`` says so, for a
/// few seconds at most. Says whether it did.
#[cfg(test)]
pub fn pump(done: impl Fn() -> bool) -> bool {
    use std::time::Instant;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
    };
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

#[cfg(test)]
mod tests {
    use super::point;

    #[test]
    fn reads_positions_left_of_and_above_the_primary_display() {
        let at = point(0x0002_0003);
        assert_eq!((at.x, at.y), (3, 2));
        let at = point(0xfffe_ffff_u32 as isize);
        assert_eq!((at.x, at.y), (-1, -2));
    }
}
