//! What Vigil's own windows are drawn with: the compact dashboard
//! (``compact/``) and Settings (``settings/``).
//!
//! Neither is a web page. Each is laid out and painted by Slint
//! (https://slint.dev) on the processor, into an ordinary window, with no
//! browser processes behind it and a repaint only when something on it
//! changes. The layouts are in ``ui/``; this is the part they share.
//!
//! - ``surface`` is where Slint paints: a block of pixels in memory.
//! - ``backdrop`` paints the page behind a window's cards.
//! - ``bar`` says which part of a window the pointer is over.
//! - ``keys`` turns Windows' key presses into Slint's.
//! - ``clipboard`` is what Copy and Paste in a text field use.
//! - ``shell`` (Windows only) is the window those pixels are shown in.
//!
//! Only Windows opens the windows, but everything except ``shell`` builds
//! and is tested on every platform: the tests draw them into pictures.

pub mod backdrop;
pub mod bar;
pub mod clipboard;
pub mod keys;
pub mod surface;

#[cfg(windows)]
pub mod shell;

/// What Slint makes of ``ui/vigil.slint`` (see build.rs).
#[allow(clippy::all, unreachable_pub, dead_code)]
pub mod ui {
    slint::include_modules!();
}

use slint::{ComponentHandle, Global, Image, Rgb8Pixel, SharedPixelBuffer};

/// Paints the page behind a window's cards, for a window ``width`` by
/// ``height`` real pixels at ``scale`` of them to a layout pixel, in the
/// theme the layout is in. Call when any of those changes.
pub fn paint_page<'a, T>(ui: &'a T, width: u32, height: u32, scale: f32)
where
    T: ComponentHandle,
    ui::Theme<'a>: Global<'a, T>,
    ui::Page<'a>: Global<'a, T>,
{
    let dark = ui.global::<ui::Theme>().get_dark();
    let picture = backdrop::paint(width, height, scale, dark);
    let page = if picture.is_empty() {
        Image::default()
    } else {
        Image::from_rgb8(SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(
            &picture, width, height,
        ))
    };
    ui.global::<ui::Page>().set_picture(page);
}

/// Saves a window's picture as ``name``.png in the folder
/// ``VIGIL_PREVIEW_DIR`` names, if it is set: how to see a change to a
/// layout without a Windows machine.
#[cfg(test)]
pub fn save_preview(frame: &surface::Frame, name: &str) {
    let Some(dir) = std::env::var_os("VIGIL_PREVIEW_DIR") else {
        return;
    };
    let path = std::path::PathBuf::from(dir).join(format!("{name}.png"));
    let file = std::fs::File::create(&path).expect("the preview folder can be written to");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), frame.width, frame.height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let bytes: Vec<u8> = frame.pixels.iter().flat_map(|p| p.rgb()).collect();
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&bytes))
        .expect("the preview can be written");
}
