//! The clipboard, for Copy, Cut and Paste in a text field.
//!
//! On Windows this is the system's. Elsewhere nothing opens a window, so
//! it is a piece of text kept by the thread, which is all the tests need.

#[cfg(not(windows))]
mod kept {
    use std::cell::RefCell;

    thread_local! {
        static TEXT: RefCell<Option<String>> = const { RefCell::new(None) };
    }

    pub fn set(text: &str) {
        TEXT.set(Some(text.to_string()));
    }

    pub fn get() -> Option<String> {
        TEXT.with_borrow(Clone::clone)
    }
}

#[cfg(not(windows))]
pub use kept::{get, set};

#[cfg(windows)]
mod system {
    use windows_sys::Win32::Foundation::{GlobalFree, HANDLE};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
        OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
    };

    /// Text as UTF-16 with a NUL at the end (``CF_UNICODETEXT``).
    const UNICODE_TEXT: u32 = 13;

    /// Closes the clipboard when dropped, however the function is left.
    struct Opened;

    impl Opened {
        fn new() -> Option<Opened> {
            // SAFETY: a plain call. No window is named as the owner, which
            // the clipboard allows.
            (unsafe { OpenClipboard(std::ptr::null_mut()) } != 0).then_some(Opened)
        }
    }

    impl Drop for Opened {
        fn drop(&mut self) {
            // SAFETY: only made once the clipboard was opened.
            unsafe { CloseClipboard() };
        }
    }

    pub fn set(text: &str) {
        let units: Vec<u16> = text.encode_utf16().chain([0]).collect();
        let Some(_opened) = Opened::new() else {
            return;
        };
        // SAFETY: the block is allocated with room for every unit, filled
        // while locked, and handed to the clipboard, which then owns it.
        // If the clipboard won't take it, it is freed here instead.
        unsafe {
            EmptyClipboard();
            let block = GlobalAlloc(GMEM_MOVEABLE, units.len() * size_of::<u16>());
            if block.is_null() {
                return;
            }
            let to = GlobalLock(block).cast::<u16>();
            if to.is_null() {
                GlobalFree(block);
                return;
            }
            std::ptr::copy_nonoverlapping(units.as_ptr(), to, units.len());
            GlobalUnlock(block);
            if SetClipboardData(UNICODE_TEXT, block as HANDLE).is_null() {
                GlobalFree(block);
            }
        }
    }

    pub fn get() -> Option<String> {
        // SAFETY: a plain query.
        if unsafe { IsClipboardFormatAvailable(UNICODE_TEXT) } == 0 {
            return None;
        }
        let _opened = Opened::new()?;
        // SAFETY: the clipboard's text is a NUL-terminated block that
        // stays valid while the clipboard is open and the block locked.
        unsafe {
            let block = GetClipboardData(UNICODE_TEXT);
            if block.is_null() {
                return None;
            }
            let from = GlobalLock(block).cast::<u16>();
            if from.is_null() {
                return None;
            }
            let mut length = 0;
            while *from.add(length) != 0 {
                length += 1;
            }
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(from, length));
            GlobalUnlock(block);
            Some(text)
        }
    }
}

#[cfg(windows)]
pub use system::{get, set};
