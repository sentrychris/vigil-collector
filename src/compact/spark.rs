//! The outlines of the compact window's sparklines: a minute of readings
//! as a smooth line, newest at the right edge, and the area under it.
//!
//! The same picture as web/src/components/compact/SparkLine.vue draws
//! with d3: a monotone curve through the points (one that never
//! overshoots a reading), in a box whose top follows the tallest reading
//! unless the series has a fixed one. The line is written as an SVG path,
//! which is what Slint's ``Path`` takes. The tint under it fades towards
//! the bottom, which Slint's software renderer can't do to a path, so it
//! is painted here, pixel by pixel, into a small picture.

use std::fmt::Write;

/// How many readings a sparkline keeps: a minute's worth at the usual one
/// a second.
pub const HISTORY: usize = 60;
/// Room left above the tallest and below the lowest reading, so the line's
/// own thickness isn't cut off.
const PAD: f32 = 2.3;

/// Where the top of a sparkline's scale is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scale {
    /// A fixed top, for readings that have one (a percent).
    Top(f64),
    /// The tallest reading and a little more, but never below this, so an
    /// idle line stays flat instead of being magnified noise.
    AtLeast(f64),
}

impl Scale {
    /// The top of the scale for these readings. ``together`` are the
    /// series drawn in the same box, which share a scale.
    pub fn top(self, together: &[&[f64]]) -> f64 {
        match self {
            Scale::Top(top) => top,
            Scale::AtLeast(floor) => {
                let tallest = together
                    .iter()
                    .flat_map(|series| series.iter())
                    .fold(0.0_f64, |tallest, v| tallest.max(*v));
                (tallest * 1.15).max(floor)
            }
        }
    }
}

/// Where each reading goes in a box ``width`` by ``height``: one step
/// apart from the right edge leftwards, and up from the bottom by its
/// share of ``top``.
fn points(values: &[f64], top: f64, width: f32, height: f32) -> Vec<(f32, f32)> {
    let step = width / (HISTORY - 1) as f32;
    let count = values.len();
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let share = if top > 0.0 && value.is_finite() {
                (value.clamp(0.0, top) / top) as f32
            } else {
                0.0
            };
            (
                width - (count - 1 - index) as f32 * step,
                height - PAD - share * (height - 2.0 * PAD),
            )
        })
        .collect()
}

/// One stretch of the curve between two neighbouring readings: where it
/// starts and ends, and its slope at each end.
#[derive(Clone, Copy, Debug)]
struct Segment {
    from: (f32, f32),
    to: (f32, f32),
    slopes: (f32, f32),
}

impl Segment {
    /// The heights of the two handles that give the stretch its slopes,
    /// a third of the way in from each end.
    fn handles(&self) -> (f32, f32) {
        let dx = (self.to.0 - self.from.0) / 3.0;
        (
            self.from.1 + dx * self.slopes.0,
            self.to.1 - dx * self.slopes.1,
        )
    }

    /// The curve's height at ``x``, somewhere along the stretch. The
    /// handles sit at even thirds, so going along it is going along x.
    fn height_at(&self, x: f32) -> f32 {
        let span = self.to.0 - self.from.0;
        let t = if span > 0.0 {
            ((x - self.from.0) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (h0, h1) = self.handles();
        let u = 1.0 - t;
        u * u * u * self.from.1
            + 3.0 * u * u * t * h0
            + 3.0 * u * t * t * h1
            + t * t * t * self.to.1
    }
}

/// A smooth curve through ``points`` that keeps to their ups and downs:
/// between two readings it never goes above the higher or below the
/// lower. This is d3-shape's ``curveMonotoneX`` (after Steffen, 1990),
/// step for step, so the line is the web dashboard's. Fewer than two
/// points make no curve.
fn monotone(points: &[(f32, f32)]) -> Vec<Segment> {
    let (Some(&first), Some(&second)) = (points.first(), points.get(1)) else {
        return Vec::new();
    };
    if points.len() == 2 {
        let slope = (second.1 - first.1) / (second.0 - first.0);
        let slope = if slope.is_finite() { slope } else { 0.0 };
        return vec![Segment {
            from: first,
            to: second,
            slopes: (slope, slope),
        }];
    }
    // The slope at the middle of three points.
    let slope3 = |a: (f32, f32), b: (f32, f32), c: (f32, f32)| {
        let (h0, h1) = (b.0 - a.0, c.0 - b.0);
        let (s0, s1) = ((b.1 - a.1) / h0, (c.1 - b.1) / h1);
        let p = (s0 * h1 + s1 * h0) / (h0 + h1);
        let sign = |s: f32| if s < 0.0 { -1.0 } else { 1.0 };
        let t = (sign(s0) + sign(s1)) * s0.abs().min(s1.abs()).min(0.5 * p.abs());
        if t.is_finite() { t } else { 0.0 }
    };
    // The slope at one end of a stretch, given the slope at its other.
    let slope2 = |a: (f32, f32), b: (f32, f32), t: f32| {
        let h = b.0 - a.0;
        if h != 0.0 {
            (3.0 * (b.1 - a.1) / h - t) / 2.0
        } else {
            t
        }
    };
    let mut segments = Vec::with_capacity(points.len() - 1);
    let (mut from, mut to, mut slope) = (first, second, 0.0);
    for (index, &next) in points.iter().enumerate().skip(2) {
        let ahead = slope3(from, to, next);
        if index == 2 {
            slope = slope2(from, to, ahead);
        }
        segments.push(Segment {
            from,
            to,
            slopes: (slope, ahead),
        });
        (from, to, slope) = (to, next, ahead);
    }
    segments.push(Segment {
        from,
        to,
        slopes: (slope, slope2(from, to, slope)),
    });
    segments
}

/// The line through the readings as an SVG path in a box ``width`` by
/// ``height``, or nothing while there are fewer than two of them.
pub fn line(values: &[f64], top: f64, width: f32, height: f32) -> String {
    let segments = monotone(&points(values, top, width, height));
    let mut path = String::new();
    if let Some(first) = segments.first() {
        let _ = write!(path, "M{:.2} {:.2}", first.from.0, first.from.1);
    }
    for segment in &segments {
        let (h0, h1) = segment.handles();
        let dx = (segment.to.0 - segment.from.0) / 3.0;
        let _ = write!(
            path,
            "C{:.2} {h0:.2} {:.2} {h1:.2} {:.2} {:.2}",
            segment.from.0 + dx,
            segment.to.0 - dx,
            segment.to.0,
            segment.to.1
        );
    }
    path
}

/// How strong the tint under a line is at the line's highest point. It
/// fades evenly from there to nothing at the bottom of the box.
const TINT: f32 = 0.28;

/// The tint under the line, as a picture ``width`` by ``height`` pixels:
/// row after row from the top, four bytes a pixel (red, green, blue and
/// opacity, the colour not yet multiplied by the opacity). Empty while
/// there are fewer than two readings or no room to draw in.
pub fn tint(values: &[f64], top: f64, width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    let (w, h) = (width as f32, height as f32);
    let segments = monotone(&points(values, top, w, h));
    let (Some(first), Some(last)) = (segments.first(), segments.last()) else {
        return Vec::new();
    };
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let (left, right) = (first.from.0, last.to.0);
    // The fade runs from the line's highest point down to the bottom.
    let peak = segments
        .iter()
        .fold(h, |peak, s| peak.min(s.from.1).min(s.to.1));
    let fall = (h - peak).max(f32::EPSILON);

    let mut picture = vec![0_u8; width as usize * height as usize * 4];
    let mut segment = 0;
    for column in 0..width {
        let x = column as f32 + 0.5;
        if x < left || x > right {
            continue;
        }
        while segment + 1 < segments.len() && x > segments[segment].to.0 {
            segment += 1;
        }
        let line = segments[segment].height_at(x);
        for row in line.floor().max(0.0) as u32..height {
            // How much of this pixel lies under the line.
            let covered = (row as f32 + 1.0 - line).clamp(0.0, 1.0);
            let depth = ((row as f32 + 0.5 - peak) / fall).clamp(0.0, 1.0);
            let opacity = covered * TINT * (1.0 - depth);
            let at = (row as usize * width as usize + column as usize) * 4;
            picture[at..at + 3].copy_from_slice(&rgb);
            picture[at + 3] = (opacity * 255.0).round() as u8;
        }
    }
    picture
}

#[cfg(test)]
mod tests {
    use super::{HISTORY, Scale, line, points, tint};

    #[test]
    fn a_fixed_scale_ignores_the_readings() {
        assert_eq!(Scale::Top(100.0).top(&[&[250.0]]), 100.0);
    }

    #[test]
    fn a_following_scale_has_headroom_and_a_floor() {
        let scale = Scale::AtLeast(25.0);
        assert_eq!(scale.top(&[&[1.0, 2.0]]), 25.0);
        assert_eq!(scale.top(&[&[1.0, 2.0], &[40.0]]), 40.0 * 1.15);
        assert_eq!(scale.top(&[]), 25.0);
    }

    #[test]
    fn the_newest_reading_is_at_the_right_edge() {
        let placed = points(&[0.0, 50.0, 100.0], 100.0, 118.0, 26.0);
        assert_eq!(placed.len(), 3);
        assert_eq!(placed[2].0, 118.0);
        // One step apart, as if the minute were full.
        assert_eq!(placed[1].0, 118.0 - 118.0 / (HISTORY - 1) as f32);
        // Bottom, middle and top of the box, inside its padding.
        assert!((placed[0].1 - 23.7).abs() < 0.01);
        assert!((placed[1].1 - 13.0).abs() < 0.01);
        assert!((placed[2].1 - 2.3).abs() < 0.01);
    }

    #[test]
    fn readings_beyond_the_scale_stay_in_the_box() {
        let placed = points(&[-5.0, 500.0, f64::NAN], 100.0, 100.0, 26.0);
        assert!((placed[0].1 - 23.7).abs() < 0.01);
        assert!((placed[1].1 - 2.3).abs() < 0.01);
        assert!(placed[2].1.is_finite());
    }

    #[test]
    fn nothing_is_drawn_for_fewer_than_two_readings() {
        assert_eq!(line(&[], 100.0, 100.0, 26.0), "");
        assert_eq!(line(&[5.0], 100.0, 100.0, 26.0), "");
        assert!(tint(&[5.0], 100.0, 100, 26, [255, 0, 0]).is_empty());
    }

    #[test]
    fn two_readings_are_a_straight_line() {
        // From the bottom left corner of the last step to its top right:
        // the handles lie on the line between them.
        let path = line(&[0.0, 100.0], 100.0, 59.0 * 3.0, 26.0);
        assert_eq!(path, "M174.00 23.70C175.00 16.57 176.00 9.43 177.00 2.30");
    }

    #[test]
    fn a_flat_run_stays_flat() {
        // A monotone curve may not dip or bulge between equal readings.
        let path = line(&[10.0; 5], 100.0, 118.0, 26.0);
        let ys: Vec<&str> = path
            .split(['M', 'C', 'L', ' '])
            .filter(|s| !s.is_empty())
            .skip(1)
            .step_by(2)
            .collect();
        assert!(ys.len() > 5);
        assert!(ys.iter().all(|y| *y == ys[0]), "{path}");
    }

    #[test]
    fn the_tint_lies_under_the_line_and_fades_downwards() {
        // A level line a quarter of the way down a 60 by 40 box.
        let (width, height) = (60, 40);
        let picture = tint(&[75.0; 60], 100.0, width, height, [10, 20, 30]);
        let opacity = |x: usize, y: usize| picture[(y * width as usize + x) * 4 + 3];
        // Nothing above the line.
        assert_eq!(opacity(30, 2), 0);
        // Strongest just under it, weaker further down, gone at the bottom.
        assert!(opacity(30, 13) > opacity(30, 25));
        assert!(opacity(30, 25) > opacity(30, 38));
        assert!(opacity(30, 13) <= 72);
        assert!(opacity(30, 39) <= 1);
        // In the series' colour.
        let at = (13 * width as usize + 30) * 4;
        assert_eq!(&picture[at..at + 3], &[10, 20, 30]);
    }

    #[test]
    fn the_tint_starts_where_the_readings_do() {
        // Half a minute of readings fill the right half of the box.
        let picture = tint(&[50.0; 30], 100.0, 59, 20, [0, 0, 0]);
        let opacity = |x: usize, y: usize| picture[(y * 59 + x) * 4 + 3];
        assert_eq!(opacity(10, 15), 0);
        assert!(opacity(45, 12) > 0);
    }
}
