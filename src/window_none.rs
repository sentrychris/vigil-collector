//! Stands in for ``window.rs`` in Windows builds without the Vigil window
//! (MinGW, unless the ``window-on-mingw`` feature is on). ``show`` always
//! fails, so the tray opens the dashboard in the browser and the settings
//! file in its editor, as it does where WebView2 is missing.

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
