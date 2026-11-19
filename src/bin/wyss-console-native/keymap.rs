//! Console shortcuts use Ctrl+Shift with a letter, arrow, comma, period or digit.
//! Other chords reach the terminal. Ctrl+Shift+C and +V
//! (interrupt and paste, handled by the terminal) and +U (an input method's unicode entry) are not
//! taken. The chord is recognised by the physical key (the X11/Wayland keycode, evdev + 8), so a
//! dead key or a keyboard layout does not change it.

use crate::layout::{Dir, Way};
use crate::t;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Way(Way),
    Next,
    Previous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    NewTab,
    Split(Dir),
    ClosePane,
    /// Search the scrollback of the focused pane.
    Search,
    Focus(Focus),
    Tab(i32),
    /// Go to the n-th tab (1 to 9).
    Jump(usize),
}

pub fn action(code: u32, ctrl: bool, shift: bool, alt: bool, meta: bool) -> Option<Action> {
    if !ctrl || !shift || alt || meta {
        return None;
    }
    Some(match code {
        28 => Action::NewTab,                      // T
        40 => Action::Split(Dir::Row),             // D
        26 => Action::Split(Dir::Col),             // E
        25 => Action::ClosePane,                   // W
        41 => Action::Search,                      // F
        113 => Action::Focus(Focus::Way(Way::Left)),
        114 => Action::Focus(Focus::Way(Way::Right)),
        111 => Action::Focus(Focus::Way(Way::Up)),
        116 => Action::Focus(Focus::Way(Way::Down)),
        60 => Action::Focus(Focus::Next),          // .
        59 => Action::Focus(Focus::Previous),      // ,
        117 => Action::Tab(1),                     // PageDown
        112 => Action::Tab(-1),                    // PageUp
        10..=18 => Action::Jump((code - 9) as usize), // 1..9
        _ => return None,
    })
}

pub fn from_gtk(code: u32, mods: gtk::gdk::ModifierType) -> Option<Action> {
    use gtk::gdk::ModifierType as M;
    action(code, mods.contains(M::CONTROL_MASK), mods.contains(M::SHIFT_MASK), mods.contains(M::ALT_MASK), mods.intersects(M::SUPER_MASK | M::META_MASK))
}

/// What each chord does, for the legend and the tooltips.
pub const LEGEND: [(&str, &str); 12] = [
    ("Ctrl+Shift+T", t!("New terminal in a new tab")),
    ("Ctrl+Shift+D", t!("Split the pane side by side")),
    ("Ctrl+Shift+E", t!("Split the pane, one above the other")),
    ("Ctrl+Shift+W", t!("Close the pane")),
    ("Ctrl+Shift+F", t!("Search the pane’s scrollback")),
    ("Ctrl+Shift+Arrows", t!("Move to the pane in that direction")),
    ("Ctrl+Shift+. and ,", t!("Next and previous pane")),
    ("Ctrl+Shift+PageDown, PageUp", t!("Next and previous tab")),
    ("Ctrl+Shift+1 to 9", t!("Go to that tab")),
    ("Ctrl+C", t!("Copy the selection (never interrupts)")),
    ("Ctrl+Shift+C", t!("Interrupt the program (Ctrl+C in a normal terminal)")),
    ("Ctrl+V, Ctrl+Shift+V", t!("Paste")),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(code: u32) -> Option<Action> {
        action(code, true, true, false, false)
    }

    #[test]
    fn the_leader_chords_map_to_their_actions() {
        assert_eq!(chord(28), Some(Action::NewTab));
        assert_eq!(chord(40), Some(Action::Split(Dir::Row)));
        assert_eq!(chord(26), Some(Action::Split(Dir::Col)));
        assert_eq!(chord(25), Some(Action::ClosePane));
        assert_eq!(chord(41), Some(Action::Search), "Ctrl+Shift+F");
        assert_eq!(chord(113), Some(Action::Focus(Focus::Way(Way::Left))));
        assert_eq!(chord(114), Some(Action::Focus(Focus::Way(Way::Right))));
        assert_eq!(chord(111), Some(Action::Focus(Focus::Way(Way::Up))));
        assert_eq!(chord(116), Some(Action::Focus(Focus::Way(Way::Down))));
        assert_eq!(chord(60), Some(Action::Focus(Focus::Next)));
        assert_eq!(chord(59), Some(Action::Focus(Focus::Previous)));
        assert_eq!(chord(117), Some(Action::Tab(1)));
        assert_eq!(chord(112), Some(Action::Tab(-1)));
    }

    #[test]
    fn digits_one_to_nine_jump_to_that_tab() {
        for n in 1..=9u32 {
            assert_eq!(chord(9 + n), Some(Action::Jump(n as usize)));
        }
        assert_eq!(chord(19), None, "0 is not a tab");
        assert_eq!(chord(9), None, "Escape is not a tab");
    }

    #[test]
    fn copy_interrupt_and_paste_are_left_to_the_terminal() {
        assert_eq!(chord(54), None, "Ctrl+Shift+C");
        assert_eq!(chord(55), None, "Ctrl+Shift+V");
        assert_eq!(chord(30), None, "Ctrl+Shift+U (IME unicode)");
    }

    #[test]
    fn the_chord_needs_ctrl_and_shift_and_nothing_else() {
        assert_eq!(action(28, true, false, false, false), None);
        assert_eq!(action(28, false, true, false, false), None);
        assert_eq!(action(28, false, false, false, false), None);
        assert_eq!(action(28, true, true, true, false), None, "Alt");
        assert_eq!(action(28, true, true, false, true), None, "Super");
    }

    #[test]
    fn gtk_modifiers_are_read_and_lock_keys_do_not_matter() {
        use gtk::gdk::ModifierType as M;
        assert_eq!(from_gtk(28, M::CONTROL_MASK | M::SHIFT_MASK), Some(Action::NewTab));
        assert_eq!(from_gtk(28, M::CONTROL_MASK | M::SHIFT_MASK | M::LOCK_MASK), Some(Action::NewTab), "Caps Lock");
        assert_eq!(from_gtk(28, M::CONTROL_MASK | M::SHIFT_MASK | M::ALT_MASK), None);
        assert_eq!(from_gtk(28, M::CONTROL_MASK), None);
    }

    #[test]
    fn the_legend_lists_every_leader_action() {
        let keys: Vec<&str> = LEGEND.iter().map(|(k, _)| *k).collect();
        for k in ["Ctrl+Shift+T", "Ctrl+Shift+D", "Ctrl+Shift+E", "Ctrl+Shift+W", "Ctrl+Shift+F", "Ctrl+C", "Ctrl+Shift+C"] {
            assert!(keys.contains(&k), "{k}");
        }
    }
}
