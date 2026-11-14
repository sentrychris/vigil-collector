//! The compact Vigil window, drawn without a browser.
//!
//! The dashboard at its usual small size: one tight column of cards, the
//! service checks and the busiest processes. It used to be a web page in
//! a WebView2 control like the rest of the dashboard; this is the same
//! picture laid out and painted by Slint (see ``native/``), with no
//! browser processes behind it and a repaint only when a reading
//! changes. The full dashboard is still the web page, in a window of its
//! own (``window.rs``).
//!
//! - ``ui/compact.slint`` is the layout.
//! - ``view`` words the readings and keeps the sparklines' history.
//! - ``spark`` draws the sparklines' outlines.
//! - ``host`` (Windows only) is the window it is shown in.
//!
//! Only Windows opens the window, but everything except ``host`` builds
//! and is tested on every platform: the tests draw it into a picture.

mod spark;
mod view;

#[cfg(windows)]
mod host;
#[cfg(windows)]
// ``theme_changed`` is for the full dashboard's page, which a MinGW build
// may not have (see window_none.rs).
#[allow(unused_imports)]
pub use host::{Feed, close, services_changed, show, theme_changed};

#[cfg(test)]
mod tests;
