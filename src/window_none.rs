//! Stands in for ``window.rs`` in Windows builds without the full
//! dashboard's window (MinGW, unless the ``window-on-mingw`` feature is
//! on). ``show`` always fails, so the full dashboard opens in the browser
//! and the settings file in its editor, as they do where WebView2 is
//! missing. The compact window is in these builds like any other.

/// Which page the window should be showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Dashboard,
    Settings,
}

pub fn show(_origin: &str, _token: &str, _page: Page) -> Result<(), String> {
    Err("this build doesn't include it".into())
}

pub fn close() {}

pub fn set_theme(_dark: bool) {}
