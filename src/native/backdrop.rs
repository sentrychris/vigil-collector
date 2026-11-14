//! Paints the page behind the cards: the page colour with the web
//! dashboard's soft glows in its accent tones.
//!
//! Slint can draw these itself, as round gradients, but its software
//! renderer then works every glow out again for every pixel it repaints,
//! and with five of them overlapping that was most of the cost of showing
//! a reading. Painted here once per window size and handed over as a
//! picture, the page costs a copy.

/// A soft round glow: its colour at the centre, fading evenly to nothing
/// ``radius`` away.
struct Glow {
    /// The centre, as shares of the page's width and height.
    x: f32,
    y: f32,
    /// In layout pixels.
    radius: f32,
    /// Red, green, blue and how strong it is at the centre.
    tint: [u8; 4],
}

const fn glow(x: f32, y: f32, radius: f32, tint: u32) -> Glow {
    Glow {
        x,
        y,
        radius,
        tint: tint.to_be_bytes(),
    }
}

const PAGE_DARK: [u8; 3] = [0x0a, 0x0e, 0x17];
const PAGE_LIGHT: [u8; 3] = [0xe1, 0xe6, 0xed];

/// The glows of each theme, as ``body::before`` has them in the web
/// dashboard's stylesheets, scaled to this window.
const DARK: [Glow; 5] = [
    glow(0.10, 0.06, 600.0, 0x60a5_fa47),
    glow(0.92, 0.18, 540.0, 0xa78b_fa38),
    glow(0.55, 1.15, 660.0, 0x22d3_ee38),
    glow(0.25, 0.80, 412.0, 0xf59e_0b1f),
    glow(0.80, 0.65, 385.0, 0xf43f_5e1a),
];
const LIGHT: [Glow; 4] = [
    glow(0.12, 0.08, 585.0, 0x60a5_fa29),
    glow(0.88, 0.22, 520.0, 0xa78b_fa21),
    glow(0.55, 1.10, 650.0, 0x22d3_ee1f),
    glow(0.30, 0.85, 420.0, 0x10b9_8112),
];

/// The page for a window ``width`` by ``height`` real pixels, ``scale`` of
/// them to a layout pixel: red, green and blue for each pixel, in rows
/// from the top.
pub fn paint(width: u32, height: u32, scale: f32, dark: bool) -> Vec<u8> {
    let (page, glows): (_, &[Glow]) = if dark {
        (PAGE_DARK, &DARK)
    } else {
        (PAGE_LIGHT, &LIGHT)
    };
    // Each glow in real pixels: centre, radius, colour and strength.
    let glows: Vec<_> = glows
        .iter()
        .map(|glow| {
            let [r, g, b, a] = glow.tint.map(f32::from);
            (
                glow.x * width as f32,
                glow.y * height as f32,
                (glow.radius * scale).max(1.0),
                [r, g, b],
                a / 255.0,
            )
        })
        .collect();

    let mut out = Vec::with_capacity(width as usize * height as usize * 3);
    for y in 0..height {
        for x in 0..width {
            // The middle of the pixel.
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let mut colour = page.map(f32::from);
            for (cx, cy, radius, tint, strength) in &glows {
                let (dx, dy) = (px - cx, py - cy);
                let squared = dx * dx + dy * dy;
                if squared >= radius * radius {
                    continue;
                }
                let alpha = strength * (1.0 - squared.sqrt() / radius);
                for (channel, tint) in colour.iter_mut().zip(tint) {
                    *channel += (tint - *channel) * alpha;
                }
            }
            out.extend(colour.map(|channel| channel.round() as u8));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{PAGE_DARK, paint};

    fn at(picture: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * width + x) * 3) as usize;
        [picture[i], picture[i + 1], picture[i + 2]]
    }

    #[test]
    fn covers_the_window_and_is_the_page_colour_tinted() {
        let (width, height) = (420, 580);
        let picture = paint(width, height, 1.0, true);
        assert_eq!(picture.len(), (width * height * 3) as usize);
        // Near the blue glow's centre the page is bluer than it is bare,
        // and nowhere is it far from the page colour.
        let glowing = at(&picture, width, 42, 35);
        assert!(glowing[2] > PAGE_DARK[2] + 30, "{glowing:?}");
        for pixel in picture.as_chunks::<3>().0 {
            assert!(
                pixel
                    .iter()
                    .zip(PAGE_DARK)
                    .all(|(c, p)| c.abs_diff(p) < 0x60)
            );
        }
    }

    #[test]
    fn each_theme_has_its_own_page() {
        let light = paint(100, 100, 1.0, false);
        assert!(light.iter().all(|channel| *channel > 0x90));
        assert_ne!(light, paint(100, 100, 1.0, true));
    }

    #[test]
    fn a_denser_display_gets_the_same_page_in_more_pixels() {
        let once = paint(200, 300, 1.0, true);
        let twice = paint(400, 600, 2.0, true);
        // The same place on the page, give or take the half pixel between
        // where each samples it.
        let a = at(&once, 200, 50, 60);
        let b = at(&twice, 400, 100, 120);
        assert!(
            a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 1),
            "{a:?} {b:?}"
        );
    }

    #[test]
    fn a_window_with_no_size_has_no_page() {
        assert!(paint(0, 580, 1.0, true).is_empty());
    }
}
