//! Which part of the window is under the pointer.
//!
//! The compact window has no title bar from Windows; the layout draws its
//! own. Windows still does the moving and resizing, once it is told what
//! each place stands for: the bar is what the window is dragged by, and a
//! thin strip along the very top resizes it, as the top border of an
//! ordinary window would. (The other three borders are outside the
//! picture and Windows' own.)

/// How far down from the top edge counts as the edge, in layout pixels.
const EDGE: f32 = 4.0;
/// How far in from a side the top edge counts as the corner.
const CORNER: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The layout's own: a button, a switch, a card.
    Inside,
    /// The bar, where it has nothing to press. Drags the window.
    Bar,
    Top,
    TopLeft,
    TopRight,
}

/// The part at ``(x, y)`` in a window ``width`` wide, all in layout
/// pixels. ``bar`` is the bar's height and ``controls`` where its switches
/// and buttons begin; left of there it is bare apart from the logo.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn part_at(x: f32, y: f32, width: f32, bar: f32, controls: f32) -> Part {
    if y < EDGE {
        return if x < CORNER {
            Part::TopLeft
        } else if x >= width - CORNER {
            Part::TopRight
        } else {
            Part::Top
        };
    }
    if y < bar && x < controls {
        Part::Bar
    } else {
        Part::Inside
    }
}

#[cfg(test)]
mod tests {
    use super::{Part, part_at};

    #[test]
    fn the_bar_drags_and_its_controls_do_not() {
        let at = |x, y| part_at(x, y, 420.0, 33.0, 250.0);
        assert_eq!(at(100.0, 16.0), Part::Bar);
        assert_eq!(at(249.0, 32.0), Part::Bar);
        assert_eq!(at(250.0, 16.0), Part::Inside);
        assert_eq!(at(400.0, 16.0), Part::Inside);
        assert_eq!(at(100.0, 33.0), Part::Inside);
        assert_eq!(at(100.0, 300.0), Part::Inside);
    }

    #[test]
    fn the_very_top_resizes_even_over_the_buttons() {
        let at = |x, y| part_at(x, y, 420.0, 33.0, 250.0);
        assert_eq!(at(200.0, 0.0), Part::Top);
        assert_eq!(at(400.0, 3.9), Part::Top);
        assert_eq!(at(3.0, 2.0), Part::TopLeft);
        assert_eq!(at(415.0, 2.0), Part::TopRight);
        assert_eq!(at(400.0, 4.0), Part::Inside);
    }
}
