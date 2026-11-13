//! Where Slint draws the compact window: into a block of pixels in
//! memory, with no windowing system or graphics card involved.
//!
//! Slint usually brings its own window (through winit) and a renderer
//! that wants OpenGL or better. Neither is used here. This is Slint's
//! other mode, meant for small devices: the program says how big the
//! picture is and passes on the mouse, and Slint's software renderer
//! paints the picture into a buffer it is handed. ``host.rs`` copies that
//! buffer to a window of its own on Windows; the tests write it to a PNG.
//!
//! Everything here belongs to one thread, the one that first calls
//! ``open``: on Windows the tray thread, in tests the test's own.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use slint::platform::software_renderer::{
    PremultipliedRgbaColor, RepaintBufferType, SoftwareRenderer, TargetPixel,
};
use slint::platform::{Platform, PlatformError, Renderer, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalSize, PhysicalSize, Window, WindowSize};

use super::ui::CompactWindow;

/// One pixel as Windows keeps them in a 32-bit bitmap: blue in the low
/// byte, then green, then red, and a top byte nothing reads. Slint paints
/// straight into these, so the picture goes to the screen without being
/// converted on the way.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Pixel(pub u32);

impl Pixel {
    #[cfg(test)]
    pub fn rgb(self) -> [u8; 3] {
        [(self.0 >> 16) as u8, (self.0 >> 8) as u8, self.0 as u8]
    }
}

impl TargetPixel for Pixel {
    fn blend(&mut self, color: PremultipliedRgbaColor) {
        // What is there shows through by the colour's transparency; the
        // colour itself already has its own opacity multiplied in.
        let through = u32::from(u8::MAX - color.alpha);
        let mix = |shift: u32, over: u8| {
            let under = (self.0 >> shift) & 0xff;
            ((under * through / 255) + u32::from(over)).min(255) << shift
        };
        self.0 = mix(16, color.red) | mix(8, color.green) | mix(0, color.blue);
    }

    fn from_rgb(red: u8, green: u8, blue: u8) -> Self {
        Pixel(u32::from(red) << 16 | u32::from(green) << 8 | u32::from(blue))
    }
}

/// The picture of the window: ``width * height`` pixels in rows from the
/// top. It is kept between frames, and Slint repaints only what changed.
#[derive(Default)]
pub struct Frame {
    pub pixels: Vec<Pixel>,
    pub width: u32,
    pub height: u32,
}

/// What Slint calls a window adapter: the window as far as Slint knows
/// one. It has a size and a scale, takes the mouse, and can be drawn.
pub struct Surface {
    window: Window,
    renderer: SoftwareRenderer,
    size: Cell<PhysicalSize>,
    /// Set when something on it changed, until it is next painted.
    stale: Cell<bool>,
    /// Called when it first goes stale, so whoever shows it can arrange a
    /// repaint.
    on_stale: RefCell<Option<Box<dyn Fn()>>>,
}

impl Surface {
    fn new() -> Rc<Self> {
        Rc::new_cyclic(|weak: &Weak<Self>| Self {
            window: Window::new(weak.clone()),
            renderer: SoftwareRenderer::new_with_repaint_buffer_type(
                RepaintBufferType::ReusedBuffer,
            ),
            size: Cell::default(),
            stale: Cell::new(true),
            on_stale: RefCell::new(None),
        })
    }

    /// Has ``notify`` called whenever the picture goes out of date.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn on_stale(&self, notify: impl Fn() + 'static) {
        *self.on_stale.borrow_mut() = Some(Box::new(notify));
    }

    /// Tells Slint the window's size in real pixels and how many of them
    /// make one of the layout's: 1.0 at 100% display scaling, 1.5 at 150%.
    pub fn resize(&self, width: u32, height: u32, scale: f32) {
        if self.window.scale_factor() != scale {
            self.window.dispatch_event(WindowEvent::ScaleFactorChanged {
                scale_factor: scale,
            });
        }
        self.size.set(PhysicalSize::new(width, height));
        self.window.dispatch_event(WindowEvent::Resized {
            size: LogicalSize::new(width as f32 / scale, height as f32 / scale),
        });
        self.window.request_redraw();
    }

    /// Passes a mouse or window event on to the layout.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn send(&self, event: WindowEvent) {
        self.window.dispatch_event(event);
    }

    #[cfg(test)]
    pub fn scale(&self) -> f32 {
        self.window.scale_factor()
    }

    /// Whether anything is moving (a switch sliding, say), which wants
    /// frame after frame until it settles.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn animating(&self) -> bool {
        self.window.has_active_animations()
    }

    /// Brings ``frame`` up to date if anything changed since it was last
    /// painted, and says whether it did. A frame of the wrong size is
    /// made again and painted whole.
    pub fn paint(&self, frame: &mut Frame) -> bool {
        let size = self.size.get();
        let fits = frame.width == size.width && frame.height == size.height;
        if fits && !self.stale.get() {
            return false;
        }
        self.stale.set(false);
        if size.width == 0 || size.height == 0 {
            return false;
        }
        if !fits {
            frame.width = size.width;
            frame.height = size.height;
            frame.pixels.clear();
            frame
                .pixels
                .resize(size.width as usize * size.height as usize, Pixel::default());
            // Nothing of the last picture is in the new buffer.
            self.renderer
                .set_repaint_buffer_type(RepaintBufferType::NewBuffer);
        }
        self.renderer.render(&mut frame.pixels, size.width as usize);
        self.renderer
            .set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
        true
    }
}

impl WindowAdapter for Surface {
    fn window(&self) -> &Window {
        &self.window
    }

    fn size(&self) -> PhysicalSize {
        self.size.get()
    }

    fn set_size(&self, size: WindowSize) {
        let scale = self.window.scale_factor();
        let size = size.to_physical(scale);
        self.resize(size.width, size.height, scale);
    }

    fn renderer(&self) -> &dyn Renderer {
        &self.renderer
    }

    fn request_redraw(&self) {
        if !self.stale.replace(true)
            && let Some(notify) = self.on_stale.borrow().as_ref()
        {
            notify();
        }
    }
}

/// Slint's idea of the system it runs on: here, only a clock and a way to
/// make the one kind of window above.
struct Headless {
    started: Instant,
}

thread_local! {
    /// Whether this thread has told Slint about ``Headless`` yet.
    static INSTALLED: Cell<bool> = const { Cell::new(false) };
    /// The surface Slint asked for most recently, for ``open`` to pick up.
    static NEWEST: RefCell<Option<Rc<Surface>>> = const { RefCell::new(None) };
}

impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        let surface = Surface::new();
        NEWEST.set(Some(surface.clone()));
        Ok(surface)
    }

    fn duration_since_start(&self) -> Duration {
        self.started.elapsed()
    }
}

/// Makes the compact window's layout and the surface it draws on. Nothing
/// is on any screen yet: that is up to the caller.
pub fn open() -> Result<(CompactWindow, Rc<Surface>), String> {
    if !INSTALLED.get() {
        slint::platform::set_platform(Box::new(Headless {
            started: Instant::now(),
        }))
        .map_err(|e| e.to_string())?;
        INSTALLED.set(true);
    }
    let ui = CompactWindow::new().map_err(|e| e.to_string())?;
    let surface = NEWEST
        .take()
        .ok_or_else(|| "the layout was made without a surface".to_string())?;
    ui.show().map_err(|e| e.to_string())?;
    Ok((ui, surface))
}

/// Moves Slint's clock on: timers that are due run, and anything moving
/// takes its next step. Call before painting or passing on an event.
pub fn tick() {
    slint::platform::update_timers_and_animations();
}

/// How long until Slint next has a timer to run, if it has any.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn next_timer() -> Option<Duration> {
    slint::platform::duration_until_next_timer_update()
}

#[cfg(test)]
mod tests {
    use super::Pixel;
    use slint::platform::software_renderer::{PremultipliedRgbaColor, TargetPixel};

    #[test]
    fn a_pixel_is_laid_out_as_windows_expects() {
        assert_eq!(Pixel::from_rgb(0x11, 0x22, 0x33), Pixel(0x0011_2233));
        assert_eq!(Pixel(0x0011_2233).rgb(), [0x11, 0x22, 0x33]);
    }

    #[test]
    fn blending_lets_the_old_colour_through_by_the_new_ones_transparency() {
        let mut pixel = Pixel::from_rgb(200, 100, 0);
        // Opaque replaces it.
        pixel.blend(PremultipliedRgbaColor {
            red: 1,
            green: 2,
            blue: 3,
            alpha: 255,
        });
        assert_eq!(pixel.rgb(), [1, 2, 3]);
        // Fully transparent leaves it.
        pixel.blend(PremultipliedRgbaColor {
            red: 0,
            green: 0,
            blue: 0,
            alpha: 0,
        });
        assert_eq!(pixel.rgb(), [1, 2, 3]);
        // Half white over black is mid grey.
        let mut pixel = Pixel::from_rgb(0, 0, 0);
        pixel.blend(PremultipliedRgbaColor {
            red: 128,
            green: 128,
            blue: 128,
            alpha: 128,
        });
        assert_eq!(pixel.rgb(), [128, 128, 128]);
    }
}
