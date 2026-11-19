//! Where a mouse-wheel turn goes: to the application (as mouse reports or arrow keys) or to the
//! scrollback. Same order as Ghostty: mouse tracking first, then the alternate screen (which has
//! no scrollback, so arrows), then local history. Shift always forces the scrollback.

/// Terminal modes that decide the wheel's target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modes {
    /// DEC 9 / 1000 / 1002 / 1003: the application wants mouse events.
    pub mouse: bool,
    /// DEC 1006: mouse reports in SGR form.
    pub sgr: bool,
    /// DEC 47 / 1047 / 1049: alternate screen is active.
    pub alt: bool,
    /// DECCKM: arrows are `ESC O x`.
    pub app_cursor: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Move the local scrollback by this many rows (negative = older).
    Scrollback(isize),
    /// Bytes for the application.
    Send(Vec<u8>),
}

/// `rows` < 0 means toward older output (wheel up). `col`/`row` are 0-based viewport cells.
pub fn action(m: Modes, shift: bool, rows: isize, col: usize, row: usize) -> Action {
    if rows == 0 || shift || !(m.mouse || m.alt) {
        return Action::Scrollback(rows);
    }
    let up = rows < 0;
    let n = rows.unsigned_abs();
    let mut out = Vec::new();
    if m.mouse {
        let button = if up { 64 } else { 65 };
        for _ in 0..n {
            if m.sgr {
                out.extend_from_slice(format!("\x1b[<{button};{};{}M", col + 1, row + 1).as_bytes());
            } else {
                // X10 encoding caps coordinates at 223.
                let c = (col + 1).min(223) as u8 + 32;
                let r = (row + 1).min(223) as u8 + 32;
                out.extend_from_slice(&[0x1b, b'[', b'M', 32 + button, c, r]);
            }
        }
    } else {
        let key: &[u8] = match (up, m.app_cursor) {
            (true, false) => b"\x1b[A",
            (true, true) => b"\x1bOA",
            (false, false) => b"\x1b[B",
            (false, true) => b"\x1bOB",
        };
        for _ in 0..n {
            out.extend_from_slice(key);
        }
    }
    Action::Send(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_screen_scrolls_history() {
        assert_eq!(action(Modes::default(), false, -3, 0, 0), Action::Scrollback(-3));
    }

    #[test]
    fn mouse_tracking_gets_sgr_wheel_reports() {
        let m = Modes { mouse: true, sgr: true, ..Default::default() };
        assert_eq!(action(m, false, -2, 4, 9), Action::Send(b"\x1b[<64;5;10M\x1b[<64;5;10M".to_vec()));
        assert_eq!(action(m, false, 1, 0, 0), Action::Send(b"\x1b[<65;1;1M".to_vec()));
    }

    #[test]
    fn mouse_tracking_without_sgr_uses_x10() {
        let m = Modes { mouse: true, ..Default::default() };
        assert_eq!(action(m, false, -1, 0, 0), Action::Send(vec![0x1b, b'[', b'M', 96, 33, 33]));
    }

    #[test]
    fn alternate_screen_gets_arrows() {
        let m = Modes { alt: true, ..Default::default() };
        assert_eq!(action(m, false, -2, 0, 0), Action::Send(b"\x1b[A\x1b[A".to_vec()));
        let m = Modes { alt: true, app_cursor: true, ..Default::default() };
        assert_eq!(action(m, false, 1, 0, 0), Action::Send(b"\x1bOB".to_vec()));
    }

    #[test]
    fn shift_forces_history() {
        let m = Modes { mouse: true, sgr: true, alt: true, app_cursor: false };
        assert_eq!(action(m, true, -3, 0, 0), Action::Scrollback(-3));
    }
}
