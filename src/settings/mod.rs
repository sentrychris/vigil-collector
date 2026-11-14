//! The Settings window, drawn without a browser.
//!
//! What Vigil serves and to whom, its log and its service checks: the
//! same settings, rules and wording as the Settings page of the web
//! dashboard, in a small window in the compact dashboard's style, laid
//! out and painted by Slint (see ``native/``). Both go through
//! ``server::control::Panel``, so both check and save alike.
//!
//! - ``ui/settings.slint`` is the layout.
//! - ``form`` is what the forms come to: what changed, what is wrong.
//! - ``log`` folds the log to the width of its box.
//! - ``view`` fills the layout in and turns its buttons into requests.
//! - ``host`` (Windows only) is the window, and does what is asked.
//!
//! Only Windows opens the window, but everything except ``host`` builds
//! and is tested on every platform: the tests draw it into a picture.

mod form;
mod log;
mod view;

#[cfg(windows)]
mod host;
#[cfg(windows)]
pub use host::{close, show, theme_changed};

#[cfg(test)]
mod tests;
