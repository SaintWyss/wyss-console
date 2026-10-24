//! The screen a terminal's output draws: a byte-driven VT state machine over two buffers
//! (normal and alternate), a cursor, modes, scroll regions and a bounded scrollback.
//!
//! The bytes are untrusted data. Nothing here acts on them: it keeps no clipboard, opens no URL,
//! answers no query (the page's own terminal does that for the live stream) and cannot change the
//! size (one authority, the host's PTY). A sequence it will not honour is *counted* by a closed
//! name (`Refusals`), never stored, never echoed. Every buffer is bounded by construction.

use super::width;
use std::collections::VecDeque;

pub const MAX_ROWS: usize = 512;
pub const MAX_COLS: usize = 512;
/// Longest title kept (characters). Longer ones are cut, control characters dropped.
pub const MAX_TITLE: usize = 128;
/// Longest operating-system-command payload looked at; the rest is skipped and counted.
const MAX_OSC: usize = 1024;
/// A device-control or application string is skipped; one longer than this is abandoned.
const MAX_STRING: usize = 4096;
const MAX_PARAMS: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Color {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

pub mod attr {
    pub const BOLD: u8 = 1;
    pub const DIM: u8 = 2;
    pub const ITALIC: u8 = 4;
    pub const UNDERLINE: u8 = 8;
    pub const BLINK: u8 = 16;
    pub const INVERSE: u8 = 32;
    pub const HIDDEN: u8 = 64;
    pub const STRIKE: u8 = 128;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    pub attrs: u8,
}

/// One position of the screen. `width` is 1, 2 (the first half of a wide character) or 0 (its
/// second half, which holds nothing).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub ch: char,
    /// One combining mark kept with the character; `'\0'` when none.
    pub mark: char,
    pub width: u8,
    pub style: Style,
}

impl Cell {
    pub const BLANK: Cell = Cell {
        ch: ' ',
        mark: '\0',
        width: 1,
        style: Style {
            fg: Color::Default,
            bg: Color::Default,
            attrs: 0,
        },
    };
    fn blank(style: Style) -> Cell {
        Cell {
            style: Style {
                fg: Color::Default,
                bg: style.bg,
                attrs: 0,
            },
            ..Cell::BLANK
        }
    }
    pub fn is_blank(&self) -> bool {
        *self == Cell::BLANK
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row {
    pub cells: Vec<Cell>,
    /// The line continued onto the next one because it ran out of columns.
    pub wrapped: bool,
}

impl Row {
    fn new(cols: usize) -> Row {
        Row {
            cells: vec![Cell::BLANK; cols],
            wrapped: false,
        }
    }
    fn blank(cols: usize, style: Style) -> Row {
        Row {
            cells: vec![Cell::blank(style); cols],
            wrapped: false,
        }
    }
    /// A wide character is always whole: no orphaned half either way.
    fn whole(&mut self) {
        let n = self.cells.len();
        for i in 0..n {
            let c = self.cells[i];
            if c.width == 0 && (i == 0 || self.cells[i - 1].width != 2) {
                self.cells[i] = Cell::blank(c.style);
            } else if c.width == 2 && (i + 1 >= n || self.cells[i + 1].width != 0) {
                self.cells[i] = Cell {
                    width: 1,
                    ch: ' ',
                    mark: '\0',
                    ..c
                };
            }
        }
    }
    fn trimmed(&self) -> Row {
        let mut cells = self.cells.clone();
        while cells.last().is_some_and(Cell::is_blank) {
            cells.pop();
        }
        Row {
            cells,
            wrapped: self.wrapped,
        }
    }
}

pub mod mode {
    pub const APP_CURSOR: u32 = 1 << 0;
    pub const ORIGIN: u32 = 1 << 1;
    pub const AUTOWRAP: u32 = 1 << 2;
    pub const CURSOR_VISIBLE: u32 = 1 << 3;
    pub const MOUSE_1000: u32 = 1 << 4;
    pub const MOUSE_1002: u32 = 1 << 5;
    pub const MOUSE_1003: u32 = 1 << 6;
    pub const FOCUS_1004: u32 = 1 << 7;
    pub const MOUSE_UTF8: u32 = 1 << 8;
    pub const MOUSE_SGR: u32 = 1 << 9;
    pub const MOUSE_URXVT: u32 = 1 << 10;
    pub const BRACKETED_PASTE: u32 = 1 << 11;
    pub const REVERSE_VIDEO: u32 = 1 << 12;
    pub const INSERT: u32 = 1 << 13;
    pub const NEWLINE: u32 = 1 << 14;
    pub const APP_KEYPAD: u32 = 1 << 15;
    pub const CURSOR_BLINK: u32 = 1 << 16;
    pub const DEFAULT: u32 = AUTOWRAP | CURSOR_VISIBLE;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    /// The last column was written: the next printable wraps first.
    pub pending_wrap: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Saved {
    cursor: Cursor,
    pen: Style,
    origin: bool,
    charset: [Charset; 2],
    shifted: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Charset {
    Ascii,
    LineDrawing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Buffer {
    pub rows: Vec<Row>,
    pub cursor: Cursor,
    pub top: usize,
    pub bottom: usize,
    saved: Option<SavedBox>,
}
// `Saved` is private: the buffer keeps it behind a newtype so equality ignores nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SavedBox(Saved);

impl Buffer {
    /// The saved cursor (DECSC): where it was, the pen, and whether the origin mode was on.
    pub fn saved_state(&self) -> Option<(Cursor, Style, bool)> {
        self.saved.map(|SavedBox(s)| (s.cursor, s.pen, s.origin))
    }
    fn new(rows: usize, cols: usize) -> Buffer {
        Buffer {
            rows: (0..rows).map(|_| Row::new(cols)).collect(),
            cursor: Cursor::default(),
            top: 0,
            bottom: rows - 1,
            saved: None,
        }
    }
}

/// What a sequence asked for that this screen will not do. Each is only counted.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Refusals {
    /// OSC 52: a program asking to write the clipboard.
    pub clipboard: u64,
    /// OSC 8: a hyperlink (nothing is ever opened from output).
    pub hyperlink: u64,
    /// CSI t: moving, resizing or reporting the window; DECCOLM: changing the width.
    pub window_op: u64,
    /// A request for an answer (device attributes, colour queries, status reports).
    pub query: u64,
    /// A sequence longer than the bound, skipped.
    pub oversized: u64,
    /// Device-control, application and privacy strings (sixel, tmux passthrough, kitty graphics).
    pub string: u64,
    /// Bytes that were not valid UTF-8.
    pub invalid_utf8: u64,
}

/// Where in the byte stream the screen's knowledge starts and what it is missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct History {
    /// Lines that scrolled out of the kept scrollback.
    pub lines_evicted: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Which width table decides how many columns a character takes.
    pub width: width::Table,
    pub scrollback_lines: usize,
    /// Cells all kept scrollback may hold together.
    pub scrollback_cells: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            width: width::Table::default(),
            scrollback_lines: 2_000,
            scrollback_cells: 256 * 1024,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Ground,
    Escape,
    EscapeIntermediate,
    Csi,
    CsiIgnore,
    Osc,
    /// DCS, APC, PM, SOS: skipped to the string terminator.
    Skip,
    /// An ESC inside a string: `\` ends it, anything else starts a new sequence.
    SkipEscape,
    OscEscape,
}

#[derive(Clone)]
pub struct Terminal {
    pub(crate) rows: usize,
    pub(crate) cols: usize,
    pub(crate) primary: Buffer,
    pub(crate) alternate: Buffer,
    pub(crate) alt_active: bool,
    pub(crate) pen: Style,
    pub(crate) modes: u32,
    pub(crate) charset: [Charset; 2],
    pub(crate) shifted: bool,
    pub(crate) cursor_style: u8,
    pub(crate) title: String,
    pub(crate) scrollback: VecDeque<Row>,
    scrollback_cells: usize,
    pub(crate) limits: Limits,
    pub(crate) history: History,
    pub(crate) refusals: Refusals,
    pub(crate) tabs: Vec<bool>,
    pub(crate) generation: u64,
    /// Bytes fed since the terminal was made.
    pub(crate) fed: u64,
    last: Option<char>,
    // parser
    state: State,
    params: [u32; MAX_PARAMS],
    colon: [bool; MAX_PARAMS],
    nparams: usize,
    have_param: bool,
    private: u8,
    intermediate: u8,
    osc: Vec<u8>,
    osc_over: bool,
    skipped: usize,
    utf8: [u8; 4],
    utf8_len: u8,
    utf8_need: u8,
}

fn tab_stops(cols: usize) -> Vec<bool> {
    (0..cols).map(|c| c % 8 == 0).collect()
}

impl Terminal {
    pub fn new(rows: usize, cols: usize, limits: Limits) -> Terminal {
        let rows = rows.clamp(1, MAX_ROWS);
        let cols = cols.clamp(1, MAX_COLS);
        Terminal {
            rows,
            cols,
            primary: Buffer::new(rows, cols),
            alternate: Buffer::new(rows, cols),
            alt_active: false,
            pen: Style::default(),
            modes: mode::DEFAULT,
            charset: [Charset::Ascii; 2],
            shifted: false,
            cursor_style: 0,
            title: String::new(),
            scrollback: VecDeque::new(),
            scrollback_cells: 0,
            limits,
            history: History::default(),
            refusals: Refusals::default(),
            tabs: tab_stops(cols),
            generation: 0,
            fed: 0,
            last: None,
            state: State::Ground,
            params: [0; MAX_PARAMS],
            colon: [false; MAX_PARAMS],
            nparams: 0,
            have_param: false,
            private: 0,
            intermediate: 0,
            osc: Vec::new(),
            osc_over: false,
            skipped: 0,
            utf8: [0; 4],
            utf8_len: 0,
            utf8_need: 0,
        }
    }

    pub fn rows(&self) -> usize {
        self.rows
    }
    pub fn cols(&self) -> usize {
        self.cols
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn bytes_fed(&self) -> u64 {
        self.fed
    }
    pub fn modes(&self) -> u32 {
        self.modes
    }
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn refusals(&self) -> Refusals {
        self.refusals
    }
    pub fn history(&self) -> History {
        self.history
    }
    pub fn alt_active(&self) -> bool {
        self.alt_active
    }
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }
    pub fn scrollback_cells(&self) -> usize {
        self.scrollback_cells
    }
    pub fn cursor(&self) -> Cursor {
        self.buf().cursor
    }
    pub fn pen(&self) -> Style {
        self.pen
    }
    pub fn charset_g0(&self) -> Charset {
        self.charset[0]
    }
    pub fn cursor_style(&self) -> u8 {
        self.cursor_style
    }
    pub fn region(&self) -> (usize, usize) {
        (self.buf().top, self.buf().bottom)
    }
    pub fn buffer(&self, alternate: bool) -> &Buffer {
        if alternate {
            &self.alternate
        } else {
            &self.primary
        }
    }
    pub fn scrollback(&self) -> impl Iterator<Item = &Row> {
        self.scrollback.iter()
    }

    fn buf(&self) -> &Buffer {
        if self.alt_active {
            &self.alternate
        } else {
            &self.primary
        }
    }
    fn buf_mut(&mut self) -> &mut Buffer {
        if self.alt_active {
            &mut self.alternate
        } else {
            &mut self.primary
        }
    }
    fn has(&self, flag: u32) -> bool {
        self.modes & flag != 0
    }
    fn set(&mut self, flag: u32, on: bool) {
        if on {
            self.modes |= flag;
        } else {
            self.modes &= !flag;
        }
    }

    /// The visible text, one string per row, trailing blanks trimmed (the reference corpus
    /// compares this with what tmux reports).
    pub fn text(&self) -> Vec<String> {
        self.buf().rows.iter().map(row_text).collect()
    }

    // ---------------------------------------------------------------- resize

    /// Takes the new size the PTY has. Content is cropped or padded, not reflowed: the program
    /// redraws on the size change it is sent. Lines pushed off the top go to the scrollback.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        let rows = rows.clamp(1, MAX_ROWS);
        let cols = cols.clamp(1, MAX_COLS);
        if rows == self.rows && cols == self.cols {
            return;
        }
        let alt = self.alt_active;
        for which in [false, true] {
            self.alt_active = which;
            if !which {
                // Shrinking: the lines above the cursor's new place scroll into the history.
                let excess = self
                    .primary
                    .cursor
                    .row
                    .saturating_add(1)
                    .saturating_sub(rows);
                for _ in 0..excess {
                    if self.primary.rows.is_empty() {
                        break;
                    }
                    let row = self.primary.rows.remove(0);
                    self.push_history(row);
                    self.primary.cursor.row -= 1;
                }
            }
            let b = self.buf_mut();
            b.rows.truncate(rows.max(1));
            while b.rows.len() < rows {
                b.rows.push(Row::new(cols));
            }
            for r in &mut b.rows {
                r.cells.truncate(cols);
                r.cells.resize(cols, Cell::BLANK);
                r.whole();
            }
            b.top = 0;
            b.bottom = rows - 1;
            b.cursor.row = b.cursor.row.min(rows - 1);
            b.cursor.col = b.cursor.col.min(cols - 1);
            b.cursor.pending_wrap = false;
            b.saved = None;
        }
        self.alt_active = alt;
        self.rows = rows;
        self.cols = cols;
        self.tabs = tab_stops(cols);
        self.generation += 1;
    }

    // ---------------------------------------------------------------- history

    fn push_history(&mut self, row: Row) {
        let row = row.trimmed();
        self.scrollback_cells += row.cells.len() + 1;
        self.scrollback.push_back(row);
        while self.scrollback.len() > self.limits.scrollback_lines
            || self.scrollback_cells > self.limits.scrollback_cells
        {
            match self.scrollback.pop_front() {
                Some(old) => {
                    self.scrollback_cells -= old.cells.len() + 1;
                    self.history.lines_evicted += 1;
                }
                None => break,
            }
        }
    }

    // ---------------------------------------------------------------- scrolling

    fn scroll_up(&mut self, n: usize) {
        let (top, bottom) = (self.buf().top, self.buf().bottom);
        let n = n.min(bottom - top + 1);
        let keep = !self.alt_active && top == 0 && bottom == self.rows - 1;
        let (cols, pen) = (self.cols, self.pen);
        for _ in 0..n {
            let row = {
                let b = self.buf_mut();
                let row = b.rows.remove(top);
                b.rows.insert(bottom, Row::blank(cols, pen));
                row
            };
            if keep {
                self.push_history(row);
            }
        }
    }
    fn scroll_down(&mut self, n: usize) {
        let (top, bottom) = (self.buf().top, self.buf().bottom);
        let n = n.min(bottom - top + 1);
        let (cols, pen) = (self.cols, self.pen);
        let b = self.buf_mut();
        for _ in 0..n {
            b.rows.remove(bottom);
            b.rows.insert(top, Row::blank(cols, pen));
        }
    }
    fn linefeed(&mut self) {
        let (row, bottom) = (self.buf().cursor.row, self.buf().bottom);
        if row == bottom {
            self.scroll_up(1);
        } else if row + 1 < self.rows {
            self.buf_mut().cursor.row += 1;
        }
        self.buf_mut().cursor.pending_wrap = false;
    }
    fn reverse_index(&mut self) {
        let (row, top) = (self.buf().cursor.row, self.buf().top);
        if row == top {
            self.scroll_down(1);
        } else if row > 0 {
            self.buf_mut().cursor.row -= 1;
        }
        self.buf_mut().cursor.pending_wrap = false;
    }

    // ---------------------------------------------------------------- printing

    fn print(&mut self, ch: char) {
        let ch = if self.charset[usize::from(self.shifted)] == Charset::LineDrawing {
            line_drawing(ch)
        } else {
            ch
        };
        let w = width::of(self.limits.width, ch);
        if w == 0 {
            self.combine(ch);
            return;
        }
        let w = usize::from(w);
        let cols = self.cols;
        if self.buf().cursor.pending_wrap {
            if self.has(mode::AUTOWRAP) {
                let row = self.buf().cursor.row;
                self.buf_mut().rows[row].wrapped = true;
                self.buf_mut().cursor.col = 0;
                self.linefeed();
            }
            self.buf_mut().cursor.pending_wrap = false;
        }
        if w == 2 && self.buf().cursor.col + 1 >= cols {
            if cols < 2 {
                return;
            }
            if self.has(mode::AUTOWRAP) {
                let (row, col, pen) = (self.buf().cursor.row, self.buf().cursor.col, self.pen);
                self.buf_mut().rows[row].cells[col] = Cell::blank(pen);
                self.buf_mut().rows[row].whole();
                self.buf_mut().rows[row].wrapped = true;
                self.buf_mut().cursor.col = 0;
                self.linefeed();
            } else {
                self.buf_mut().cursor.col = cols - 2;
            }
        }
        let (row, col, pen) = (self.buf().cursor.row, self.buf().cursor.col, self.pen);
        if self.has(mode::INSERT) {
            let r = &mut self.buf_mut().rows[row];
            r.cells.truncate(cols - w);
            for _ in 0..w {
                r.cells.insert(col, Cell::blank(pen));
            }
        }
        let r = &mut self.buf_mut().rows[row];
        r.cells[col] = Cell {
            ch,
            mark: '\0',
            width: w as u8,
            style: pen,
        };
        if w == 2 {
            r.cells[col + 1] = Cell {
                ch: ' ',
                mark: '\0',
                width: 0,
                style: pen,
            };
        }
        r.whole();
        self.last = Some(ch);
        let next = col + w;
        let b = self.buf_mut();
        if next >= cols {
            b.cursor.col = cols - 1;
            b.cursor.pending_wrap = true;
        } else {
            b.cursor.col = next;
        }
    }

    fn combine(&mut self, mark: char) {
        let cur = self.buf().cursor;
        let col = if cur.pending_wrap {
            cur.col
        } else if cur.col > 0 {
            cur.col - 1
        } else {
            return;
        };
        let r = &mut self.buf_mut().rows[cur.row];
        let col = if r.cells[col].width == 0 && col > 0 {
            col - 1
        } else {
            col
        };
        if r.cells[col].mark == '\0' {
            r.cells[col].mark = mark;
        }
    }

    // ---------------------------------------------------------------- erase, insert, delete

    fn erase_cells(&mut self, row: usize, from: usize, to: usize) {
        let pen = self.pen;
        let r = &mut self.buf_mut().rows[row];
        let to = to.min(r.cells.len());
        for c in from.min(to)..to {
            r.cells[c] = Cell::blank(pen);
        }
        r.whole();
    }
    /// The column erasing and editing work from: a cursor that is waiting to wrap is logically
    /// past the last column (as in xterm.js and tmux), so erasing to the end of the line leaves
    /// the character just written alone.
    fn edit_col(&self) -> usize {
        let c = self.buf().cursor;
        if c.pending_wrap { self.cols } else { c.col }
    }
    fn erase_display(&mut self, how: u32) {
        let cur = self.buf().cursor;
        let at = self.edit_col();
        let (rows, cols) = (self.rows, self.cols);
        let pen = self.pen;
        match how {
            0 => {
                self.erase_cells(cur.row, at, cols);
                for r in cur.row + 1..rows {
                    self.buf_mut().rows[r] = Row::blank(cols, pen);
                }
            }
            1 => {
                for r in 0..cur.row {
                    self.buf_mut().rows[r] = Row::blank(cols, pen);
                }
                self.erase_cells(cur.row, 0, at + 1);
            }
            2 => {
                for r in 0..rows {
                    self.buf_mut().rows[r] = Row::blank(cols, pen);
                }
            }
            3 => {
                self.scrollback.clear();
                self.scrollback_cells = 0;
            }
            _ => {}
        }
    }
    fn erase_line(&mut self, how: u32) {
        let cur = self.buf().cursor;
        let cols = self.cols;
        let at = self.edit_col();
        match how {
            0 => self.erase_cells(cur.row, at, cols),
            1 => self.erase_cells(cur.row, 0, at + 1),
            2 => self.erase_cells(cur.row, 0, cols),
            _ => {}
        }
    }
    fn insert_cells(&mut self, n: usize) {
        let (cur, cols, pen) = (self.buf().cursor, self.cols, self.pen);
        if cur.pending_wrap {
            self.buf_mut().cursor.pending_wrap = false;
            return;
        }
        let n = n.min(cols - cur.col);
        let r = &mut self.buf_mut().rows[cur.row];
        r.cells.truncate(cols - n);
        for _ in 0..n {
            r.cells.insert(cur.col, Cell::blank(pen));
        }
        r.whole();
        self.buf_mut().cursor.pending_wrap = false;
    }
    fn delete_cells(&mut self, n: usize) {
        let (cur, cols, pen) = (self.buf().cursor, self.cols, self.pen);
        if cur.pending_wrap {
            self.buf_mut().cursor.pending_wrap = false;
            return;
        }
        let n = n.min(cols - cur.col);
        let r = &mut self.buf_mut().rows[cur.row];
        r.cells.drain(cur.col..cur.col + n);
        r.cells.resize(cols, Cell::blank(pen));
        r.whole();
        self.buf_mut().cursor.pending_wrap = false;
    }
    fn insert_lines(&mut self, n: usize) {
        let (row, top, bottom) = (self.buf().cursor.row, self.buf().top, self.buf().bottom);
        if row < top || row > bottom {
            return;
        }
        let n = n.min(bottom - row + 1);
        let (cols, pen) = (self.cols, self.pen);
        let b = self.buf_mut();
        for _ in 0..n {
            b.rows.remove(bottom);
            b.rows.insert(row, Row::blank(cols, pen));
        }
        b.cursor.col = 0;
        b.cursor.pending_wrap = false;
    }
    fn delete_lines(&mut self, n: usize) {
        let (row, top, bottom) = (self.buf().cursor.row, self.buf().top, self.buf().bottom);
        if row < top || row > bottom {
            return;
        }
        let n = n.min(bottom - row + 1);
        let (cols, pen) = (self.cols, self.pen);
        let b = self.buf_mut();
        for _ in 0..n {
            b.rows.remove(row);
            b.rows.insert(bottom, Row::blank(cols, pen));
        }
        b.cursor.col = 0;
        b.cursor.pending_wrap = false;
    }

    // ---------------------------------------------------------------- cursor

    fn move_to(&mut self, row: usize, col: usize) {
        let (top, bottom) = (self.buf().top, self.buf().bottom);
        let (rows, cols) = (self.rows, self.cols);
        let row = if self.has(mode::ORIGIN) {
            (top + row).min(bottom)
        } else {
            row.min(rows - 1)
        };
        let b = self.buf_mut();
        b.cursor.row = row;
        b.cursor.col = col.min(cols - 1);
        b.cursor.pending_wrap = false;
    }
    fn home(&mut self) {
        self.move_to(0, 0);
    }
    fn save_cursor(&mut self) {
        let saved = Saved {
            cursor: self.buf().cursor,
            pen: self.pen,
            origin: self.has(mode::ORIGIN),
            charset: self.charset,
            shifted: self.shifted,
        };
        self.buf_mut().saved = Some(SavedBox(saved));
    }
    fn restore_cursor(&mut self) {
        match self.buf().saved {
            Some(SavedBox(s)) => {
                self.pen = s.pen;
                self.set(mode::ORIGIN, s.origin);
                self.charset = s.charset;
                self.shifted = s.shifted;
                let (rows, cols) = (self.rows, self.cols);
                let b = self.buf_mut();
                b.cursor = s.cursor;
                b.cursor.row = b.cursor.row.min(rows - 1);
                b.cursor.col = b.cursor.col.min(cols - 1);
            }
            None => {
                self.pen = Style::default();
                self.set(mode::ORIGIN, false);
                self.home();
            }
        }
    }

    fn tab_forward(&mut self, n: usize) {
        let cols = self.cols;
        let mut col = self.buf().cursor.col;
        for _ in 0..n {
            col += 1;
            while col < cols - 1 && !self.tabs[col] {
                col += 1;
            }
            col = col.min(cols - 1);
        }
        let b = self.buf_mut();
        b.cursor.col = col;
        b.cursor.pending_wrap = false;
    }
    fn tab_back(&mut self, n: usize) {
        let mut col = self.buf().cursor.col;
        for _ in 0..n {
            col = col.saturating_sub(1);
            while col > 0 && !self.tabs[col] {
                col -= 1;
            }
        }
        let b = self.buf_mut();
        b.cursor.col = col;
        b.cursor.pending_wrap = false;
    }

    // ---------------------------------------------------------------- alternate screen

    fn enter_alternate(&mut self, save: bool, clear: bool) {
        if self.alt_active {
            return;
        }
        if save {
            self.save_cursor();
        }
        self.alt_active = true;
        if clear {
            let (rows, cols) = (self.rows, self.cols);
            self.alternate = Buffer::new(rows, cols);
        }
    }
    fn leave_alternate(&mut self, restore: bool, clear: bool) {
        if !self.alt_active {
            return;
        }
        if clear {
            let (rows, cols) = (self.rows, self.cols);
            self.alternate = Buffer::new(rows, cols);
        }
        self.alt_active = false;
        if restore {
            self.restore_cursor();
        }
    }

    // ---------------------------------------------------------------- reset

    fn soft_reset(&mut self) {
        self.pen = Style::default();
        self.modes = (self.modes
            & (mode::MOUSE_1000
                | mode::MOUSE_1002
                | mode::MOUSE_1003
                | mode::FOCUS_1004
                | mode::MOUSE_SGR
                | mode::MOUSE_UTF8
                | mode::MOUSE_URXVT
                | mode::BRACKETED_PASTE))
            | mode::DEFAULT;
        self.charset = [Charset::Ascii; 2];
        self.shifted = false;
        self.cursor_style = 0;
        let rows = self.rows;
        for b in [&mut self.primary, &mut self.alternate] {
            b.top = 0;
            b.bottom = rows - 1;
            b.saved = None;
        }
    }
    fn hard_reset(&mut self) {
        let (rows, cols, limits) = (self.rows, self.cols, self.limits);
        let (refusals, history, generation, fed) =
            (self.refusals, self.history, self.generation, self.fed);
        *self = Terminal::new(rows, cols, limits);
        self.refusals = refusals;
        self.history = history;
        self.generation = generation + 1;
        self.fed = fed;
    }

    // ---------------------------------------------------------------- the byte stream

    pub fn feed(&mut self, bytes: &[u8]) {
        self.fed += bytes.len() as u64;
        let mut i = 0;
        while i < bytes.len() {
            // The common case, printable ASCII in the ground state, without the dispatch.
            if self.state == State::Ground && self.utf8_need == 0 {
                let start = i;
                while i < bytes.len() && (0x20..0x7F).contains(&bytes[i]) {
                    i += 1;
                }
                for &b in &bytes[start..i] {
                    self.print(b as char);
                }
                if i >= bytes.len() {
                    break;
                }
            }
            self.byte(bytes[i]);
            i += 1;
        }
    }

    fn byte(&mut self, b: u8) {
        // CAN and SUB abandon whatever sequence was in progress.
        if matches!(b, 0x18 | 0x1A) {
            self.state = State::Ground;
            self.utf8_need = 0;
            return;
        }
        match self.state {
            State::Ground => self.ground(b),
            State::Escape => self.escape(b),
            State::EscapeIntermediate => {
                if (0x20..=0x2F).contains(&b) {
                    self.intermediate = b;
                } else if (0x30..=0x7E).contains(&b) {
                    self.esc_dispatch(b);
                    self.state = State::Ground;
                } else if b == 0x1B {
                    self.begin_escape();
                }
            }
            State::Csi => self.csi(b),
            State::CsiIgnore => match b {
                0x40..=0x7E => self.state = State::Ground,
                0x1B => self.begin_escape(),
                _ => {}
            },
            State::Osc => match b {
                0x07 => self.osc_end(),
                0x1B => self.state = State::OscEscape,
                0x20..=0xFF => {
                    if self.osc.len() < MAX_OSC {
                        self.osc.push(b);
                    } else {
                        self.osc_over = true;
                    }
                }
                _ => {}
            },
            State::OscEscape => {
                if b == b'\\' {
                    self.osc_end();
                } else {
                    // The string was cut off by a new sequence: nothing of it is used.
                    self.osc_over = true;
                    self.state = State::Ground;
                    self.osc.clear();
                    self.begin_escape();
                    self.escape(b);
                }
            }
            State::Skip => match b {
                0x1B => self.state = State::SkipEscape,
                0x07 => self.state = State::Ground,
                _ => {
                    self.skipped += 1;
                    if self.skipped > MAX_STRING {
                        self.refusals.oversized += 1;
                        self.state = State::Ground;
                    }
                }
            },
            State::SkipEscape => {
                if b == b'\\' {
                    self.state = State::Ground;
                } else {
                    self.state = State::Ground;
                    self.begin_escape();
                    self.escape(b);
                }
            }
        }
    }

    fn begin_escape(&mut self) {
        self.state = State::Escape;
        self.intermediate = 0;
        self.utf8_need = 0;
    }

    fn ground(&mut self, b: u8) {
        if self.utf8_need > 0 {
            if b & 0xC0 == 0x80 {
                self.utf8[usize::from(self.utf8_len)] = b;
                self.utf8_len += 1;
                if self.utf8_len == self.utf8_need {
                    let s = &self.utf8[..usize::from(self.utf8_len)];
                    let ch = std::str::from_utf8(s)
                        .ok()
                        .and_then(|s| s.chars().next())
                        .unwrap_or_else(|| {
                            self.refusals.invalid_utf8 += 1;
                            '\u{FFFD}'
                        });
                    self.utf8_need = 0;
                    self.print(ch);
                }
                return;
            }
            // A lead byte was cut short: one replacement, and this byte is read again.
            self.refusals.invalid_utf8 += 1;
            self.utf8_need = 0;
            self.print('\u{FFFD}');
        }
        match b {
            0x00..=0x1F | 0x7F => self.control(b),
            0x20..=0x7E => self.print(b as char),
            0xC2..=0xDF => self.utf8_start(b, 2),
            0xE0..=0xEF => self.utf8_start(b, 3),
            0xF0..=0xF4 => self.utf8_start(b, 4),
            _ => {
                self.refusals.invalid_utf8 += 1;
                self.print('\u{FFFD}');
            }
        }
    }
    fn utf8_start(&mut self, b: u8, need: u8) {
        self.utf8[0] = b;
        self.utf8_len = 1;
        self.utf8_need = need;
    }

    fn control(&mut self, b: u8) {
        match b {
            0x08 => {
                let c = self.buf().cursor.col;
                let buf = self.buf_mut();
                buf.cursor.col = c.saturating_sub(1);
                buf.cursor.pending_wrap = false;
            }
            0x09 => self.tab_forward(1),
            0x0A..=0x0C => {
                self.linefeed();
                if self.has(mode::NEWLINE) {
                    self.buf_mut().cursor.col = 0;
                }
            }
            0x0D => {
                let buf = self.buf_mut();
                buf.cursor.col = 0;
                buf.cursor.pending_wrap = false;
            }
            0x0E => self.shifted = true,
            0x0F => self.shifted = false,
            0x1B => self.begin_escape(),
            _ => {}
        }
    }

    fn escape(&mut self, b: u8) {
        match b {
            b'[' => {
                self.state = State::Csi;
                self.params = [0; MAX_PARAMS];
                self.colon = [false; MAX_PARAMS];
                self.nparams = 0;
                self.have_param = false;
                self.private = 0;
                self.intermediate = 0;
            }
            b']' => {
                self.state = State::Osc;
                self.osc.clear();
                self.osc_over = false;
            }
            b'P' | b'_' | b'^' | b'X' => {
                self.state = State::Skip;
                self.skipped = 0;
                self.refusals.string += 1;
            }
            0x20..=0x2F => {
                self.intermediate = b;
                self.state = State::EscapeIntermediate;
            }
            0x1B => {}
            0x30..=0x7E => {
                self.esc_dispatch(b);
                self.state = State::Ground;
            }
            0x00..=0x1F => self.control(b),
            _ => self.state = State::Ground,
        }
    }

    fn esc_dispatch(&mut self, b: u8) {
        match (self.intermediate, b) {
            (0, b'7') => self.save_cursor(),
            (0, b'8') => self.restore_cursor(),
            (0, b'D') => self.linefeed(),
            (0, b'E') => {
                self.linefeed();
                self.buf_mut().cursor.col = 0;
            }
            (0, b'H') => {
                let c = self.buf().cursor.col;
                self.tabs[c] = true;
            }
            (0, b'M') => self.reverse_index(),
            (0, b'c') => self.hard_reset(),
            (0, b'=') => self.set(mode::APP_KEYPAD, true),
            (0, b'>') => self.set(mode::APP_KEYPAD, false),
            (b'(', c) => self.charset[0] = designate(c),
            (b')', c) => self.charset[1] = designate(c),
            (b'#', b'8') => {
                let (rows, cols) = (self.rows, self.cols);
                for r in 0..rows {
                    let mut row = Row::new(cols);
                    row.cells.iter_mut().for_each(|c| c.ch = 'E');
                    self.buf_mut().rows[r] = row;
                }
                self.home();
            }
            _ => {}
        }
        self.intermediate = 0;
    }

    fn csi(&mut self, b: u8) {
        match b {
            b'0'..=b'9' => {
                if self.nparams < MAX_PARAMS {
                    let p = &mut self.params[self.nparams];
                    *p = p
                        .saturating_mul(10)
                        .saturating_add(u32::from(b - b'0'))
                        .min(65_535);
                    self.have_param = true;
                }
            }
            b';' | b':' => {
                if self.nparams < MAX_PARAMS {
                    self.colon[self.nparams] = b == b':';
                    self.nparams += 1;
                    self.have_param = false;
                } else {
                    self.state = State::CsiIgnore;
                    self.refusals.oversized += 1;
                }
            }
            b'<'..=b'?' => self.private = b,
            0x20..=0x2F => self.intermediate = b,
            0x40..=0x7E => {
                if self.have_param || self.nparams > 0 {
                    self.nparams = (self.nparams + 1).min(MAX_PARAMS);
                }
                self.csi_dispatch(b);
                self.state = State::Ground;
            }
            0x1B => self.begin_escape(),
            0x00..=0x1F => self.control(b),
            _ => self.state = State::CsiIgnore,
        }
    }

    fn param(&self, i: usize, default: u32) -> u32 {
        if i < self.nparams && self.params[i] != 0 {
            self.params[i]
        } else {
            default
        }
    }
    fn raw(&self, i: usize) -> Option<u32> {
        (i < self.nparams).then_some(self.params[i])
    }

    fn csi_dispatch(&mut self, f: u8) {
        let n = self.param(0, 1) as usize;
        let cur = self.buf().cursor;
        match (self.private, self.intermediate, f) {
            (0, 0, b'@') => self.insert_cells(n),
            (0, 0, b'A') => {
                let top = if cur.row >= self.buf().top {
                    self.buf().top
                } else {
                    0
                };
                let row = cur.row.saturating_sub(n).max(top);
                self.set_row(row);
            }
            (0, 0, b'B') => {
                let bottom = if cur.row <= self.buf().bottom {
                    self.buf().bottom
                } else {
                    self.rows - 1
                };
                let row = (cur.row + n).min(bottom);
                self.set_row(row);
            }
            (0, 0, b'e') => {
                let row = (cur.row + n).min(self.rows - 1);
                self.set_row(row);
            }
            (0, 0, b'C' | b'a') => {
                let col = (cur.col + n).min(self.cols - 1);
                self.set_col(col);
            }
            (0, 0, b'D') => {
                let col = cur.col.saturating_sub(n);
                self.set_col(col);
            }
            (0, 0, b'E') => {
                let row = (cur.row + n).min(self.buf().bottom.max(cur.row));
                self.set_row(row);
                self.set_col(0);
            }
            (0, 0, b'F') => {
                let row = cur.row.saturating_sub(n).max(if cur.row >= self.buf().top {
                    self.buf().top
                } else {
                    0
                });
                self.set_row(row);
                self.set_col(0);
            }
            (0, 0, b'G' | b'`') => self.set_col(n - 1),
            (0, 0, b'H' | b'f') => {
                let r = self.param(0, 1) as usize - 1;
                let c = self.param(1, 1) as usize - 1;
                self.move_to(r, c);
            }
            (0, 0, b'I') => self.tab_forward(n),
            (0, 0, b'J') => self.erase_display(self.raw(0).unwrap_or(0)),
            (0, 0, b'K') => self.erase_line(self.raw(0).unwrap_or(0)),
            (0, 0, b'L') => self.insert_lines(n),
            (0, 0, b'M') => self.delete_lines(n),
            (0, 0, b'P') => self.delete_cells(n),
            (0, 0, b'S') => self.scroll_up(n),
            (0, 0, b'T') if self.nparams <= 1 => self.scroll_down(n),
            (0, 0, b'X') => {
                let cols = self.cols;
                let at = self.edit_col();
                self.erase_cells(cur.row, at, (at + n).min(cols));
            }
            (0, 0, b'Z') => self.tab_back(n),
            (0, 0, b'b') => {
                if let Some(ch) = self.last {
                    for _ in 0..n.min(MAX_COLS * 4) {
                        self.print(ch);
                    }
                }
            }
            (0, 0, b'c') | (b'>' | b'=', 0, b'c') => self.refusals.query += 1,
            (0, 0, b'd') => {
                let r = n - 1;
                let col = cur.col;
                self.move_to(r, col);
            }
            (0, 0, b'g') => match self.raw(0).unwrap_or(0) {
                0 => self.tabs[cur.col] = false,
                3 => self.tabs.iter_mut().for_each(|t| *t = false),
                _ => {}
            },
            (0, 0, b'h') => self.set_modes(false, true),
            (0, 0, b'l') => self.set_modes(false, false),
            (b'?', 0, b'h') => self.set_modes(true, true),
            (b'?', 0, b'l') => self.set_modes(true, false),
            (0, 0, b'm') => self.sgr(),
            (0, 0, b'n') | (b'?', 0, b'n') => self.refusals.query += 1,
            (0, 0, b'r') => {
                let rows = self.rows;
                let t = self.param(0, 1) as usize;
                let bm = self.param(1, rows as u32) as usize;
                let bm = bm.min(rows);
                if t >= 1 && t < bm {
                    let b = self.buf_mut();
                    b.top = t - 1;
                    b.bottom = bm - 1;
                    self.move_to(0, 0);
                }
            }
            (0, 0, b's') => self.save_cursor(),
            (0, 0, b'u') => self.restore_cursor(),
            (0, 0, b't') => self.refusals.window_op += 1,
            (0, b' ', b'q') => self.cursor_style = (self.raw(0).unwrap_or(0).min(6)) as u8,
            (0, b'!', b'p') => self.soft_reset(),
            _ => {}
        }
    }

    fn set_row(&mut self, row: usize) {
        let b = self.buf_mut();
        b.cursor.row = row;
        b.cursor.pending_wrap = false;
    }
    fn set_col(&mut self, col: usize) {
        let cols = self.cols;
        let b = self.buf_mut();
        b.cursor.col = col.min(cols - 1);
        b.cursor.pending_wrap = false;
    }

    fn set_modes(&mut self, private: bool, on: bool) {
        for i in 0..self.nparams.max(1) {
            let n = self.params[i];
            if !private {
                match n {
                    4 => self.set(mode::INSERT, on),
                    20 => self.set(mode::NEWLINE, on),
                    _ => {}
                }
                continue;
            }
            match n {
                1 => self.set(mode::APP_CURSOR, on),
                3 => self.refusals.window_op += 1,
                5 => self.set(mode::REVERSE_VIDEO, on),
                6 => {
                    self.set(mode::ORIGIN, on);
                    self.home();
                }
                7 => self.set(mode::AUTOWRAP, on),
                12 => self.set(mode::CURSOR_BLINK, on),
                25 => self.set(mode::CURSOR_VISIBLE, on),
                47 | 1047 => {
                    if on {
                        self.enter_alternate(false, false);
                    } else {
                        self.leave_alternate(false, n == 1047);
                    }
                }
                1048 => {
                    if on {
                        self.save_cursor();
                    } else {
                        self.restore_cursor();
                    }
                }
                1049 => {
                    if on {
                        self.enter_alternate(true, true);
                    } else {
                        self.leave_alternate(true, false);
                    }
                }
                // One tracking mode at a time: the last one set wins; a reset ends only itself.
                1000 | 1002 | 1003 => {
                    let flag = match n {
                        1000 => mode::MOUSE_1000,
                        1002 => mode::MOUSE_1002,
                        _ => mode::MOUSE_1003,
                    };
                    if on {
                        self.modes &= !(mode::MOUSE_1000 | mode::MOUSE_1002 | mode::MOUSE_1003);
                    }
                    self.set(flag, on);
                }
                1004 => self.set(mode::FOCUS_1004, on),
                1005 => self.set(mode::MOUSE_UTF8, on),
                1006 => self.set(mode::MOUSE_SGR, on),
                1015 => self.set(mode::MOUSE_URXVT, on),
                2004 => self.set(mode::BRACKETED_PASTE, on),
                _ => {}
            }
        }
    }

    fn sgr(&mut self) {
        if self.nparams == 0 {
            self.pen = Style::default();
            return;
        }
        let mut i = 0;
        while i < self.nparams {
            let p = self.params[i];
            match p {
                0 => self.pen = Style::default(),
                1 => self.pen.attrs |= attr::BOLD,
                2 => self.pen.attrs |= attr::DIM,
                3 => self.pen.attrs |= attr::ITALIC,
                4 | 21 => {
                    if p == 4 && self.colon[i] && i + 1 < self.nparams && self.params[i + 1] == 0 {
                        self.pen.attrs &= !attr::UNDERLINE;
                        i += 1;
                    } else {
                        self.pen.attrs |= attr::UNDERLINE;
                        if p == 4 && self.colon[i] {
                            i += 1;
                        }
                    }
                }
                5 | 6 => self.pen.attrs |= attr::BLINK,
                7 => self.pen.attrs |= attr::INVERSE,
                8 => self.pen.attrs |= attr::HIDDEN,
                9 => self.pen.attrs |= attr::STRIKE,
                22 => self.pen.attrs &= !(attr::BOLD | attr::DIM),
                23 => self.pen.attrs &= !attr::ITALIC,
                24 => self.pen.attrs &= !attr::UNDERLINE,
                25 => self.pen.attrs &= !attr::BLINK,
                27 => self.pen.attrs &= !attr::INVERSE,
                28 => self.pen.attrs &= !attr::HIDDEN,
                29 => self.pen.attrs &= !attr::STRIKE,
                30..=37 => self.pen.fg = Color::Indexed((p - 30) as u8),
                39 => self.pen.fg = Color::Default,
                40..=47 => self.pen.bg = Color::Indexed((p - 40) as u8),
                49 => self.pen.bg = Color::Default,
                90..=97 => self.pen.fg = Color::Indexed((p - 90 + 8) as u8),
                100..=107 => self.pen.bg = Color::Indexed((p - 100 + 8) as u8),
                38 | 48 => {
                    let (color, used) = self.extended_color(i);
                    if let Some(c) = color {
                        if p == 38 {
                            self.pen.fg = c;
                        } else {
                            self.pen.bg = c;
                        }
                    }
                    i += used;
                }
                _ => {}
            }
            i += 1;
        }
    }

    /// `38;5;n`, `38;2;r;g;b`, and the colon forms `38:5:n`, `38:2::r:g:b`, `38:2:r:g:b`.
    fn extended_color(&self, i: usize) -> (Option<Color>, usize) {
        let get = |k: usize| (i + k < self.nparams).then(|| self.params[i + k]);
        match get(1) {
            Some(5) => match get(2) {
                Some(n) => (Some(Color::Indexed(n.min(255) as u8)), 2),
                None => (None, self.nparams.saturating_sub(i + 1)),
            },
            Some(2) => {
                // The colon form carries an empty colour-space slot before the components.
                let skip = usize::from(self.colon[i] && self.nparams >= i + 6);
                match (get(2 + skip), get(3 + skip), get(4 + skip)) {
                    (Some(r), Some(g), Some(b)) => (
                        Some(Color::Rgb(
                            r.min(255) as u8,
                            g.min(255) as u8,
                            b.min(255) as u8,
                        )),
                        4 + skip,
                    ),
                    _ => (None, self.nparams.saturating_sub(i + 1)),
                }
            }
            _ => (None, 0),
        }
    }

    fn osc_end(&mut self) {
        self.state = State::Ground;
        if self.osc_over {
            self.refusals.oversized += 1;
            self.osc.clear();
            return;
        }
        let payload = std::mem::take(&mut self.osc);
        let split = payload.iter().position(|&b| b == b';');
        let (code, rest) = match split {
            Some(p) => (&payload[..p], &payload[p + 1..]),
            None => (&payload[..], &payload[0..0]),
        };
        match code {
            b"0" | b"2" => {
                let text = String::from_utf8_lossy(rest);
                self.title = text
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(MAX_TITLE)
                    .collect();
            }
            b"1" => {}
            b"52" => self.refusals.clipboard += 1,
            b"8" => self.refusals.hyperlink += 1,
            b"4" | b"10" | b"11" | b"12" | b"104" | b"110" | b"111" | b"112"
                if rest == b"?" || rest.ends_with(b";?") =>
            {
                self.refusals.query += 1;
            }
            _ => {}
        }
    }
}

fn designate(c: u8) -> Charset {
    if c == b'0' {
        Charset::LineDrawing
    } else {
        Charset::Ascii
    }
}

fn line_drawing(c: char) -> char {
    match c {
        '`' => '◆',
        'a' => '▒',
        'f' => '°',
        'g' => '±',
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'o' => '⎺',
        'p' => '⎻',
        'q' => '─',
        'r' => '⎼',
        's' => '⎽',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'y' => '≤',
        'z' => '≥',
        '{' => 'π',
        '|' => '≠',
        '}' => '£',
        '~' => '·',
        c => c,
    }
}

pub fn row_text(row: &Row) -> String {
    let mut out = String::new();
    for c in &row.cells {
        if c.width == 0 {
            continue;
        }
        out.push(c.ch);
        if c.mark != '\0' {
            out.push(c.mark);
        }
    }
    out.truncate(out.trim_end_matches(' ').len());
    out
}
