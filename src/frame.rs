//! What Vigil's desktop windows have in common: the ones it draws itself
//! (``native/shell.rs``: the compact dashboard and Settings) and the one
//! with the full dashboard (``window.rs``).
//!
//! None has a title bar from Windows. Each draws its own top bar and
//! leaves the rest of the frame to Windows: the resize borders at the
//! sides and bottom, the shadow and the rounded corners. Each also opens
//! where it was last closed.
//!
//! Everything here must be called on the thread that owns the window,
//! which is the tray thread.

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DestroyIcon, GetSystemMetrics, GetWindowPlacement, HICON, ICON_BIG, ICON_SMALL,
    IsIconic, IsZoomed, MINMAXINFO, SM_CXICON, SM_CXPADDEDBORDER, SM_CXSMICON, SM_CYSIZEFRAME,
    SPI_GETWORKAREA, SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW,
    SetForegroundWindow, SetWindowPlacement, SetWindowPos, ShowWindow, SystemParametersInfoW,
    WINDOWPLACEMENT, WM_NCCALCSIZE, WM_SETICON, WPF_RESTORETOMAXIMIZED,
};

use crate::config::WindowPlace;
use crate::tray::load_icon;

/// Shows the window, out of the taskbar if it was minimised, in front of
/// everything else.
pub fn bring_forward(hwnd: HWND) {
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

/// Puts a window that is still hidden where it was when it was last
/// closed (``saved``). The first time, sizes it to ``default`` (pixels at
/// 100% scaling) for its display's scaling and centres it on the primary
/// display's work area (the screen less the taskbar). Returns whether it
/// was maximised when it was closed.
///
/// # Safety
/// ``hwnd`` must be a live window of this thread.
pub unsafe fn place(hwnd: HWND, saved: Option<WindowPlace>, default: (i32, i32)) -> bool {
    // SAFETY (caller): ``hwnd`` is live; ``area`` is a valid out-pointer.
    unsafe {
        if let Some(saved) = saved
            && restore(hwnd, saved)
        {
            return saved.maximized;
        }
        let dpi = GetDpiForWindow(hwnd).max(96) as i32;
        let mut area = RECT::default();
        let have_area = SystemParametersInfoW(SPI_GETWORKAREA, 0, (&raw mut area).cast(), 0) != 0;
        let mut width = default.0 * dpi / 96;
        let mut height = default.1 * dpi / 96;
        if !have_area {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOMOVE | SWP_FRAMECHANGED,
            );
            return false;
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
            // Has the frame worked out again, now without its title bar.
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
        false
    }
}

/// Gives the window a remembered rectangle, still hidden. Windows moves it
/// back onto a screen if the display it was on has gone. False if the
/// rectangle isn't one a window could have had, or Windows won't take it.
///
/// # Safety
/// ``hwnd`` must be a live window of this thread.
unsafe fn restore(hwnd: HWND, saved: WindowPlace) -> bool {
    if !plausible(saved) {
        return false;
    }
    let placement = WINDOWPLACEMENT {
        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
        showCmd: SW_HIDE as u32,
        rcNormalPosition: RECT {
            left: saved.x,
            top: saved.y,
            right: saved.x.saturating_add(saved.width),
            bottom: saved.y.saturating_add(saved.height),
        },
        ..Default::default()
    };
    // SAFETY (caller): ``hwnd`` is live; ``placement`` is a whole,
    // correctly sized structure.
    unsafe {
        if SetWindowPlacement(hwnd, &placement) == 0 {
            return false;
        }
        // Has the frame worked out again, now without its title bar.
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
    true
}

/// Whether a remembered rectangle is one a window could have had. The
/// file it comes from can be edited by hand.
fn plausible(place: WindowPlace) -> bool {
    const LARGEST: i32 = 16_384;
    (1..=LARGEST).contains(&place.width) && (1..=LARGEST).contains(&place.height)
}

/// Where the window is, to remember for the next time it is opened: its
/// restored rectangle, and whether it is maximised. ``None`` if Windows
/// won't say.
pub fn placement(hwnd: HWND) -> Option<WindowPlace> {
    let mut placement = WINDOWPLACEMENT {
        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    // SAFETY: a query of this thread's own window into a correctly sized
    // structure.
    if unsafe { GetWindowPlacement(hwnd, &mut placement) } == 0 {
        return None;
    }
    let rect = placement.rcNormalPosition;
    // A minimised window goes back to being maximised if that's what it
    // was before.
    let maximized = placement.showCmd == SW_SHOWMAXIMIZED as u32
        || (placement.showCmd == SW_SHOWMINIMIZED as u32
            && placement.flags & WPF_RESTORETOMAXIMIZED != 0);
    Some(WindowPlace {
        x: rect.left,
        y: rect.top,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
        maximized,
    })
}

/// Answers ``WM_NCCALCSIZE`` for a window with no title bar. Windows works
/// out the client area as usual, which leaves the resize borders at the
/// sides and bottom, and then the top goes back to the window's top edge.
/// ``lparam`` points at the window's rectangle either way (the first of
/// three when ``wparam`` is set).
///
/// # Safety
/// Call only from the window procedure of ``hwnd``, with that message's
/// own ``wparam`` and ``lparam``.
pub unsafe fn without_title_bar(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY (caller): ``lparam`` is the message's pointer, valid for the
    // length of the call.
    unsafe {
        let rect = lparam as *mut RECT;
        if rect.is_null() {
            return DefWindowProcW(hwnd, WM_NCCALCSIZE, wparam, lparam);
        }
        let top = (*rect).top;
        let result = DefWindowProcW(hwnd, WM_NCCALCSIZE, wparam, lparam);
        // A maximised window hangs over every edge of the screen by its
        // border, top included.
        let overhang = if IsZoomed(hwnd) != 0 {
            let dpi = GetDpiForWindow(hwnd).max(96);
            GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi)
                + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
        } else {
            0
        };
        (*rect).top = top + overhang;
        result
    }
}

/// Answers ``WM_GETMINMAXINFO``: the window can't be dragged smaller than
/// ``smallest``, in pixels at 100% scaling.
///
/// # Safety
/// Call only from the window procedure of ``hwnd``, with that message's
/// own ``lparam``.
pub unsafe fn no_smaller_than(hwnd: HWND, lparam: LPARAM, smallest: (i32, i32)) {
    let info = lparam as *mut MINMAXINFO;
    if info.is_null() {
        return;
    }
    // SAFETY (caller): ``lparam`` is the message's pointer.
    unsafe {
        let dpi = GetDpiForWindow(hwnd).max(96) as i32;
        (*info).ptMinTrackSize = POINT {
            x: smallest.0 * dpi / 96,
            y: smallest.1 * dpi / 96,
        };
    }
}

/// Answers ``WM_DPICHANGED``, sent when the window is dragged to a display
/// with different scaling: takes the size Windows suggests, which keeps
/// the window the same size to the eye.
///
/// # Safety
/// Call only from the window procedure of ``hwnd``, with that message's
/// own ``lparam``.
pub unsafe fn follow_scaling(hwnd: HWND, lparam: LPARAM) {
    let rect = lparam as *const RECT;
    if rect.is_null() {
        return;
    }
    // SAFETY (caller): ``lparam`` is the message's pointer.
    unsafe {
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
}

/// Gives the window the pictures the taskbar and Alt-Tab show for it, the
/// same as the tray's. Hand them to ``destroy_icons`` once the window has
/// gone.
///
/// # Safety
/// ``hwnd`` must be a live window of this thread.
pub unsafe fn set_icons(hwnd: HWND) -> [HICON; 2] {
    // SAFETY (caller): plain calls on this thread's own window.
    unsafe {
        let icons = [
            load_icon(GetSystemMetrics(SM_CXICON).max(32)),
            load_icon(GetSystemMetrics(SM_CXSMICON).max(16)),
        ];
        for (kind, icon) in [(ICON_BIG, icons[0]), (ICON_SMALL, icons[1])] {
            if !icon.is_null() {
                SendMessageW(hwnd, WM_SETICON, kind as WPARAM, icon as LPARAM);
            }
        }
        icons
    }
}

/// # Safety
/// The icons must not be in use by a live window.
pub unsafe fn destroy_icons(icons: [HICON; 2]) {
    for icon in icons {
        if !icon.is_null() {
            // SAFETY (caller): made by ``load_icon`` and no longer used.
            unsafe { DestroyIcon(icon) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{WindowPlace, plausible};

    #[test]
    fn a_remembered_place_must_have_a_size() {
        let place = |width, height| WindowPlace {
            x: -1200,
            y: 40,
            width,
            height,
            maximized: false,
        };
        assert!(plausible(place(420, 580)));
        assert!(!plausible(place(0, 580)));
        assert!(!plausible(place(420, -3)));
        assert!(!plausible(place(1_000_000, 580)));
    }
}
