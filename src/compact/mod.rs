//! The compact Vigil window, drawn without a browser.
//!
//! The dashboard at its usual small size: one tight column of cards, the
//! service checks and the busiest processes. It used to be a web page in
//! a WebView2 control like the rest of the dashboard; this is the same
//! picture laid out and painted by Slint (https://slint.dev) on the
//! processor, into an ordinary window, with no browser processes behind
//! it and a repaint only when a reading changes. The full dashboard and
//! the Settings page are still the web page, in their own window
//! (``window.rs``).
//!
//! - ``ui/compact.slint`` is the layout.
//! - ``view`` words the readings and keeps the sparklines' history.
//! - ``spark`` draws the sparklines' outlines.
//! - ``backdrop`` paints the page behind the cards.
//! - ``surface`` is where Slint paints: a block of pixels in memory.
//! - ``bar`` says which part of the window the pointer is over.
//! - ``host`` (Windows only) is the window those pixels are shown in.
//!
//! Only Windows opens the window, but everything except ``host`` builds
//! and is tested on every platform: the tests draw it into a picture.

mod backdrop;
mod bar;
mod spark;
mod surface;
mod view;

#[cfg(windows)]
mod host;
#[cfg(windows)]
// ``theme_changed`` is for the full dashboard's page, which a MinGW build
// may not have (see window_none.rs).
#[allow(unused_imports)]
pub use host::{Feed, close, show, theme_changed};

/// What Slint makes of ``ui/compact.slint`` (see build.rs).
#[allow(clippy::all, unreachable_pub, dead_code)]
mod ui {
    slint::include_modules!();
}

#[cfg(test)]
mod tests;
