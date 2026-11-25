//! Translation of non-text keys into the bytes a terminal program expects.
//! Text (including dead-key and IBus compositions) arrives through the input method instead.

use gtk::gdk::{Key, ModifierType};

/// Bytes for a key press that is not plain text, or `None` to let the input method handle it.
pub fn encode(key: Key, mods: ModifierType, app_cursor: bool) -> Option<Vec<u8>> {
    let ctrl = mods.contains(ModifierType::CONTROL_MASK);
    let alt = mods.contains(ModifierType::ALT_MASK);
    let arrow = |c: u8| {
        if app_cursor {
            vec![0x1b, b'O', c]
        } else {
            vec![0x1b, b'[', c]
        }
    };
    let seq: Vec<u8> = match key {
        // Shift+Enter is ESC CR so programs (Claude Code, readline) can tell it from Enter.
        Key::Return | Key::KP_Enter if mods.contains(ModifierType::SHIFT_MASK) => b"\x1b\r".to_vec(),
        Key::Return | Key::KP_Enter => vec![b'\r'],
        Key::BackSpace => vec![if ctrl { 0x08 } else { 0x7f }],
        Key::Tab => vec![b'\t'],
        Key::ISO_Left_Tab => b"\x1b[Z".to_vec(),
        Key::Escape => vec![0x1b],
        Key::Up => arrow(b'A'),
        Key::Down => arrow(b'B'),
        Key::Right => arrow(b'C'),
        Key::Left => arrow(b'D'),
        Key::Home => b"\x1b[H".to_vec(),
        Key::End => b"\x1b[F".to_vec(),
        Key::Insert => b"\x1b[2~".to_vec(),
        Key::Delete => b"\x1b[3~".to_vec(),
        Key::Page_Up => b"\x1b[5~".to_vec(),
        Key::Page_Down => b"\x1b[6~".to_vec(),
        _ if ctrl && !mods.contains(ModifierType::SHIFT_MASK) => {
            let c = key.to_lower().to_unicode()?;
            match c {
                // The clipboard controller handles Ctrl+C and Ctrl+V.
                'c' | 'v' => return None,
                'a'..='z' => vec![c as u8 - b'a' + 1],
                '@' | ' ' => vec![0],
                '[' => vec![0x1b],
                '\\' => vec![0x1c],
                ']' => vec![0x1d],
                _ => return None,
            }
        }
        _ if alt => {
            let c = key.to_unicode()?;
            let mut v = vec![0x1b];
            v.extend_from_slice(c.to_string().as_bytes());
            v
        }
        _ => return None,
    };
    Some(seq)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_letters_map_to_c0() {
        assert_eq!(encode(Key::c, ModifierType::CONTROL_MASK, false), None, "Ctrl+C must never interrupt");
        assert_eq!(encode(Key::v, ModifierType::CONTROL_MASK, false), None);
        assert_eq!(encode(Key::d, ModifierType::CONTROL_MASK, false), Some(vec![4]));
    }

    #[test]
    fn arrows_follow_cursor_mode() {
        assert_eq!(encode(Key::Up, ModifierType::empty(), false), Some(b"\x1b[A".to_vec()));
        assert_eq!(encode(Key::Up, ModifierType::empty(), true), Some(b"\x1bOA".to_vec()));
    }

    #[test]
    fn plain_text_goes_to_input_method() {
        assert_eq!(encode(Key::a, ModifierType::empty(), false), None);
        assert_eq!(encode(Key::dead_acute, ModifierType::empty(), false), None);
        // Ctrl+Shift is reserved for the window's own shortcuts.
        assert_eq!(encode(Key::T, ModifierType::CONTROL_MASK | ModifierType::SHIFT_MASK, false), None);
    }

    #[test]
    fn shift_enter_differs_from_enter() {
        assert_eq!(encode(Key::Return, ModifierType::empty(), false), Some(b"\r".to_vec()));
        assert_eq!(encode(Key::Return, ModifierType::SHIFT_MASK, false), Some(b"\x1b\r".to_vec()));
        assert_eq!(encode(Key::KP_Enter, ModifierType::SHIFT_MASK, false), Some(b"\x1b\r".to_vec()));
    }

    #[test]
    fn alt_prefixes_escape() {
        assert_eq!(encode(Key::b, ModifierType::ALT_MASK, false), Some(b"\x1bb".to_vec()));
    }
}
