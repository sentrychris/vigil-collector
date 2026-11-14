//! Stands in for ``window.rs`` in Windows builds without the full
//! dashboard's window (MinGW, unless the ``window-on-mingw`` feature is
//! on). ``show`` always fails, so the full dashboard opens in the browser,
//! as it does where WebView2 is missing. The compact window and Settings
//! are in these builds like any other.

pub fn show(_origin: &str, _token: &str) -> Result<(), String> {
    Err("this build doesn't include it".into())
}

pub fn close() {}

pub fn set_theme(_dark: bool) {}
