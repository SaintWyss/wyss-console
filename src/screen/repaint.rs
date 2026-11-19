//! The bytes that redraw a screen: written into an empty terminal of the same size they produce the
//! same visible state, modes and scrollback (the round-trip is a property test). What a window gets
//! when it attaches to a terminal that kept running without it.

use super::terminal::{Charset, Color, Row, Style, Terminal, attr, mode};

/// Most bytes a repaint may take. The scrollback shrinks first to fit, never the visible screen.
pub const MAX_REPAINT: usize = 4 * 1024 * 1024;

pub struct Repaint {
    pub bytes: Vec<u8>,
    /// Lines of history included, newest last.
    pub scrollback_lines: usize,
    /// History that was available but did not fit the request or the bound.
    pub scrollback_omitted: usize,
}

fn sgr(out: &mut Vec<u8>, style: Style) {
    out.extend_from_slice(b"\x1b[0");
    let flags = [
        (attr::BOLD, 1),
        (attr::DIM, 2),
        (attr::ITALIC, 3),
        (attr::UNDERLINE, 4),
        (attr::BLINK, 5),
        (attr::INVERSE, 7),
        (attr::HIDDEN, 8),
        (attr::STRIKE, 9),
    ];
    for (bit, code) in flags {
        if style.attrs & bit != 0 {
            out.extend_from_slice(format!(";{code}").as_bytes());
        }
    }
    for (color, base, bright, ext) in [(style.fg, 30, 90, 38), (style.bg, 40, 100, 48)] {
        match color {
            Color::Default => {}
            Color::Indexed(n) if n < 8 => {
                out.extend_from_slice(format!(";{}", base + n).as_bytes())
            }
            Color::Indexed(n) if n < 16 => {
                out.extend_from_slice(format!(";{}", bright + n - 8).as_bytes());
            }
            Color::Indexed(n) => out.extend_from_slice(format!(";{ext};5;{n}").as_bytes()),
            Color::Rgb(r, g, b) => {
                out.extend_from_slice(format!(";{ext};2;{r};{g};{b}").as_bytes());
            }
        }
    }
    out.push(b'm');
}

fn write_char(out: &mut Vec<u8>, ch: char, mark: char) {
    let mut buf = [0u8; 4];
    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
    if mark != '\0' {
        out.extend_from_slice(mark.encode_utf8(&mut buf).as_bytes());
    }
}

/// One row's cells as runs of one style, trailing default blanks left out.
fn paint_row(out: &mut Vec<u8>, row: &Row, current: &mut Style) {
    let end = row
        .cells
        .iter()
        .rposition(|c| !c.is_blank())
        .map_or(0, |i| i + 1);
    for cell in &row.cells[..end] {
        if cell.width == 0 {
            continue;
        }
        if cell.style != *current {
            sgr(out, cell.style);
            *current = cell.style;
        }
        write_char(out, cell.ch, cell.mark);
    }
}

/// The screen alone does not fit in a repaint (a 512×512 screen of cells that each change colour).
/// Nothing is cut: no repaint is made, and the window replays the output instead.
#[derive(Debug, PartialEq, Eq)]
pub struct TooLarge;

fn check(out: &[u8]) -> Result<(), TooLarge> {
    if out.len() > MAX_REPAINT {
        Err(TooLarge)
    } else {
        Ok(())
    }
}

impl Terminal {
    /// The bytes that redraw this screen, with up to `scrollback` lines of history before it. The
    /// size is checked as it is made, never after: the screen that cannot fit is refused, and the
    /// history shrinks first to leave it room.
    pub fn repaint(&self, scrollback: usize) -> Result<Repaint, TooLarge> {
        let (rows, _) = (self.rows(), self.cols());
        // A clean slate: normal screen, no region, no leftover modes, nothing on screen or in the
        // renderer's history.
        let mut header: Vec<u8> = Vec::with_capacity(512);
        header.extend_from_slice(b"\x1b[?1049l\x1b[?47l\x1b[0m\x1b(B\x0f\x1b[r\x1b[4l\x1b[20l");
        header.extend_from_slice(
            b"\x1b[?1l\x1b[?5l\x1b[?6l\x1b[?7h\x1b[?12l\x1b[?25h\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1004l\x1b[?1005l\x1b[?1006l\x1b[?1015l\x1b[?2004l\x1b>\x1b[0 q",
        );
        header.extend_from_slice(b"\x1b[H\x1b[2J\x1b[3J");

        // The visible screen first, because it is the part that must fit.
        let mut screen: Vec<u8> = Vec::with_capacity(8192);
        screen.extend_from_slice(b"\x1b[0m\x1b[H\x1b[2J");
        // The normal screen is always drawn: it survives behind an alternate one.
        self.paint_buffer(&mut screen, false)?;
        if self.alt_active() {
            screen.extend_from_slice(b"\x1b[?47h\x1b[0m\x1b[H\x1b[2J");
            self.paint_buffer(&mut screen, true)?;
        }
        self.paint_modes(&mut screen);
        check(&screen)?;

        // History next, newest first, for as long as there is room; each line scrolls into the
        // renderer's own scrollback.
        let available = self.scrollback_len();
        let history: Vec<&Row> = self.scrollback().collect();
        let mut room = MAX_REPAINT.saturating_sub(header.len() + screen.len() + rows);
        let mut lines: Vec<Vec<u8>> = Vec::new();
        for line in history.iter().rev().take(scrollback.min(available)) {
            let mut one = Vec::new();
            let mut current = Style::default();
            paint_row(&mut one, line, &mut current);
            if current != Style::default() {
                one.extend_from_slice(b"\x1b[0m");
            }
            one.extend_from_slice(b"\r\n");
            if one.len() > room {
                break;
            }
            room -= one.len();
            lines.push(one);
        }
        let take = lines.len();
        let mut out = header;
        for one in lines.iter().rev() {
            out.extend_from_slice(one);
        }
        // The last lines written are still on screen: enough line feeds push them out too, so that
        // exactly the history lines are in the scrollback and only blank rows are left to clear.
        if take > 0 {
            out.extend(std::iter::repeat_n(b'\n', rows - 1));
        }
        out.extend_from_slice(&screen);
        check(&out)?;
        Ok(Repaint {
            bytes: out,
            scrollback_lines: take,
            scrollback_omitted: available - take,
        })
    }

    fn paint_buffer(&self, out: &mut Vec<u8>, alternate: bool) -> Result<(), TooLarge> {
        let buf = self.buffer(alternate);
        let mut current = Style::default();
        for (r, row) in buf.rows.iter().enumerate() {
            if row.cells.iter().all(|c| c.is_blank()) {
                continue;
            }
            out.extend_from_slice(format!("\x1b[{};1H", r + 1).as_bytes());
            paint_row(out, row, &mut current);
            check(out)?;
        }
        if current != Style::default() {
            out.extend_from_slice(b"\x1b[0m");
        }
        // The saved cursor (DECSC) first, under a full-screen region (where an origin-relative row
        // is an absolute one, so it can be anywhere), then the region, then the live cursor.
        out.extend_from_slice(b"\x1b[r");
        if let Some(saved) = buf.saved_state() {
            self.place(out, buf, saved.0, 0, saved.2);
            sgr(out, saved.1);
            out.extend_from_slice(b"\x1b7");
        }
        if buf.top != 0 || buf.bottom != self.rows() - 1 {
            out.extend_from_slice(format!("\x1b[{};{}r", buf.top + 1, buf.bottom + 1).as_bytes());
        }
        // The origin mode is the shown screen's; a normal screen behind an alternate one is placed
        // by absolute position.
        let origin = (alternate || !self.alt_active()) && self.modes & mode::ORIGIN != 0;
        self.place(out, buf, buf.cursor, buf.top, origin);
        // The pen the cursor writes with is last, so the next byte of output is styled right.
        out.extend_from_slice(b"\x1b[0m");
        if self.pen != Style::default() {
            sgr(out, self.pen);
        }
        check(out)
    }

    /// Moves the cursor to `cursor` (honouring the origin mode the position was stored under),
    /// leaving a pending wrap exactly as it was.
    fn place(
        &self,
        out: &mut Vec<u8>,
        buf: &super::terminal::Buffer,
        cursor: super::terminal::Cursor,
        top: usize,
        origin: bool,
    ) {
        out.extend_from_slice(if origin { b"\x1b[?6h" } else { b"\x1b[?6l" });
        let row = if origin {
            cursor.row.saturating_sub(top)
        } else {
            cursor.row
        };
        if cursor.pending_wrap && cursor.col + 1 == self.cols() {
            // Writing the last cell again puts the cursor back in the wrap-pending state.
            let cells = &buf.rows[cursor.row].cells;
            // The last character written: when it is wide, it starts one column earlier.
            let at = if cells[cursor.col].width == 0 && cursor.col > 0 {
                cursor.col - 1
            } else {
                cursor.col
            };
            let hold = cells[at];
            out.extend_from_slice(format!("\x1b[{};{}H", row + 1, at + 1).as_bytes());
            sgr(out, hold.style);
            write_char(out, hold.ch, hold.mark);
        } else {
            out.extend_from_slice(format!("\x1b[{};{}H", row + 1, cursor.col + 1).as_bytes());
        }
    }

    fn paint_modes(&self, out: &mut Vec<u8>) {
        let flags: [(u32, &str); 14] = [
            (mode::APP_CURSOR, "1"),
            (mode::REVERSE_VIDEO, "5"),
            (mode::AUTOWRAP, "7"),
            (mode::CURSOR_BLINK, "12"),
            (mode::CURSOR_VISIBLE, "25"),
            (mode::MOUSE_1000, "1000"),
            (mode::MOUSE_1002, "1002"),
            (mode::MOUSE_1003, "1003"),
            (mode::FOCUS_1004, "1004"),
            (mode::MOUSE_UTF8, "1005"),
            (mode::MOUSE_SGR, "1006"),
            (mode::MOUSE_URXVT, "1015"),
            (mode::BRACKETED_PASTE, "2004"),
            (0, ""),
        ];
        for (flag, code) in flags {
            if flag == 0 {
                continue;
            }
            let on = self.modes & flag != 0;
            out.extend_from_slice(
                format!("\x1b[?{}{}", code, if on { 'h' } else { 'l' }).as_bytes(),
            );
        }
        out.extend_from_slice(if self.modes & mode::APP_KEYPAD != 0 {
            b"\x1b="
        } else {
            b"\x1b>"
        });
        if self.modes & mode::INSERT != 0 {
            out.extend_from_slice(b"\x1b[4h");
        }
        if self.modes & mode::NEWLINE != 0 {
            out.extend_from_slice(b"\x1b[20h");
        }
        if self.charset_g0() == Charset::LineDrawing {
            out.extend_from_slice(b"\x1b(0");
        }
        out.extend_from_slice(format!("\x1b[{} q", self.cursor_style()).as_bytes());
        if !self.title().is_empty() {
            out.extend_from_slice(b"\x1b]2;");
            out.extend_from_slice(self.title().as_bytes());
            out.push(0x07);
        }
    }
}
