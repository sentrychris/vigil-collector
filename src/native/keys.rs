//! Windows' key presses, as Slint wants them.
//!
//! Windows reports the keyboard twice over. ``WM_KEYDOWN`` and
//! ``WM_KEYUP`` name the key that moved (a "virtual key"), and
//! ``WM_CHAR`` follows with what it types, once the layout, Shift, dead
//! keys and the rest have been applied. Slint takes one stream of
//! presses, each a piece of text: what was typed, or a character it
//! reserves for a key that types nothing, like an arrow.
//!
//! So the keys that type nothing are passed on from ``WM_KEYDOWN`` and
//! ``WM_KEYUP`` (``named``), and everything typed from ``WM_CHAR``
//! (``typed``). The exception is a letter pressed with Ctrl, which
//! Windows types as a control character: Slint knows Ctrl+C by its ``c``,
//! so that comes from the key instead (``with_control``).

use slint::platform::Key;

// Virtual-key codes (winuser.h).
const VK_BACK: u32 = 0x08;
const VK_TAB: u32 = 0x09;
const VK_RETURN: u32 = 0x0D;
const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_MENU: u32 = 0x12;
const VK_ESCAPE: u32 = 0x1B;
const VK_PRIOR: u32 = 0x21;
const VK_NEXT: u32 = 0x22;
const VK_END: u32 = 0x23;
const VK_HOME: u32 = 0x24;
const VK_LEFT: u32 = 0x25;
const VK_UP: u32 = 0x26;
const VK_RIGHT: u32 = 0x27;
const VK_DOWN: u32 = 0x28;
const VK_DELETE: u32 = 0x2E;

/// The key behind a virtual-key code, if it is one that types nothing.
pub fn named(vk: u32) -> Option<Key> {
    Some(match vk {
        VK_BACK => Key::Backspace,
        VK_TAB => Key::Tab,
        VK_RETURN => Key::Return,
        VK_SHIFT => Key::Shift,
        VK_CONTROL => Key::Control,
        VK_MENU => Key::Alt,
        VK_ESCAPE => Key::Escape,
        VK_PRIOR => Key::PageUp,
        VK_NEXT => Key::PageDown,
        VK_END => Key::End,
        VK_HOME => Key::Home,
        VK_LEFT => Key::LeftArrow,
        VK_UP => Key::UpArrow,
        VK_RIGHT => Key::RightArrow,
        VK_DOWN => Key::DownArrow,
        VK_DELETE => Key::Delete,
        _ => return None,
    })
}

/// The letter of a key pressed with Ctrl held, as Slint knows shortcuts:
/// ``c`` for Ctrl+C. Only letters; anything else types as usual.
pub fn with_control(vk: u32) -> Option<char> {
    char::from_u32(vk)
        .filter(char::is_ascii_uppercase)
        .map(|letter| letter.to_ascii_lowercase())
}

/// What a ``WM_CHAR`` types, if it types anything. Its characters arrive
/// as UTF-16, so one outside the basic plane (an emoji, say) comes as two
/// messages: ``pending`` holds the first half until the second arrives.
/// Control characters are left out: the keys behind them are passed on by
/// name (Backspace, Tab, Enter, Escape), or are shortcuts.
pub fn typed(unit: u16, pending: &mut Option<u16>) -> Option<char> {
    let units = match (pending.take(), unit) {
        (_, 0xD800..=0xDBFF) => {
            *pending = Some(unit);
            return None;
        }
        (Some(high), 0xDC00..=0xDFFF) => vec![high, unit],
        _ => vec![unit],
    };
    char::decode_utf16(units)
        .next()?
        .ok()
        .filter(|typed| !typed.is_control())
}

#[cfg(test)]
mod tests {
    use super::{named, typed, with_control};
    use slint::platform::Key;

    #[test]
    fn keys_that_type_nothing_are_passed_on_by_name() {
        assert_eq!(named(0x08), Some(Key::Backspace));
        assert_eq!(named(0x25), Some(Key::LeftArrow));
        assert_eq!(named(0x2E), Some(Key::Delete));
        assert_eq!(named(0x10), Some(Key::Shift));
        // Letters, digits and the space bar type something.
        assert_eq!(named(u32::from(b'A')), None);
        assert_eq!(named(u32::from(b'7')), None);
        assert_eq!(named(0x20), None);
    }

    #[test]
    fn a_letter_with_ctrl_is_a_shortcut_by_its_small_letter() {
        assert_eq!(with_control(u32::from(b'C')), Some('c'));
        assert_eq!(with_control(u32::from(b'V')), Some('v'));
        assert_eq!(with_control(u32::from(b'1')), None);
        assert_eq!(with_control(0x25), None);
    }

    #[test]
    fn typing_passes_text_and_drops_control_characters() {
        let mut pending = None;
        assert_eq!(typed(u16::from(b'a'), &mut pending), Some('a'));
        assert_eq!(typed(0x00E9, &mut pending), Some('é'));
        // Backspace, Enter and Ctrl+C as WM_CHAR sends them.
        assert_eq!(typed(0x08, &mut pending), None);
        assert_eq!(typed(0x0D, &mut pending), None);
        assert_eq!(typed(0x03, &mut pending), None);
    }

    #[test]
    fn a_character_in_two_halves_is_put_together() {
        let mut pending = None;
        let mut units = [0; 2];
        '😀'.encode_utf16(&mut units);
        assert_eq!(typed(units[0], &mut pending), None);
        assert_eq!(typed(units[1], &mut pending), Some('😀'));
        assert_eq!(pending, None);
        // A first half that never gets its second is dropped.
        assert_eq!(typed(units[0], &mut pending), None);
        assert_eq!(typed(u16::from(b'x'), &mut pending), Some('x'));
    }
}
