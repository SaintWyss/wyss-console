//! Mouse selection, as pure logic over the grid the widget last painted (one `Vec<Cell>` per viewport
//! row). Coordinates are viewport (row, col). A selection is a snapshot of screen positions, not a
//! reference into ghostty's page list: the widget drops it as soon as one of its rows is redrawn with
//! new content (see `draw` in term.rs), so it can never stay painted over text it would not copy.
//!
//! Convention: both ends are INCLUSIVE cells. A wide character is selected whole whichever of its two
//! cells (the glyph cell or the spacer) is hit. The text of a selection joins rows with '\n' (soft
//! wraps are not rejoined), skips spacers and drops the trailing spaces of every row.

use crate::ghostty::{Cell, Wide};

/// Characters that end a word, besides whitespace and empty cells.
const SEPARATORS: &str = "()[]{}<>'\"`|;,";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Pos {
    pub row: usize,
    pub col: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Cell,
    Word,
    Line,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Selection {
    pub anchor: Pos,
    pub head: Pos,
    pub mode: Mode,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Space,
    Sep,
    Word,
}

/// Orders two positions (start, end) in reading order.
pub fn normalize(a: Pos, b: Pos) -> (Pos, Pos) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Columns covered by the character at `col`: both cells of a wide character, else just `col`.
fn glyph_span(row: &[Cell], col: usize) -> (usize, usize) {
    let col = col.min(row.len().saturating_sub(1));
    match row.get(col).map(|c| c.wide) {
        Some(Wide::Spacer) if col > 0 && row[col - 1].wide == Wide::Double => (col - 1, col),
        Some(Wide::Double) if col + 1 < row.len() => (col, col + 1),
        _ => (col, col),
    }
}

fn class(row: &[Cell], col: usize) -> Class {
    let col = if row[col].wide == Wide::Spacer && col > 0 { col - 1 } else { col };
    match row[col].text().chars().next() {
        None => Class::Space,
        Some(c) if c.is_whitespace() => Class::Space,
        Some(c) if SEPARATORS.contains(c) => Class::Sep,
        Some(_) => Class::Word,
    }
}

/// Double-click expansion: the run of word characters (or of blanks) around `col`; a separator is
/// selected alone.
pub fn word_span(row: &[Cell], col: usize) -> (usize, usize) {
    if row.is_empty() {
        return (0, 0);
    }
    let col = col.min(row.len() - 1);
    let c = class(row, col);
    if c == Class::Sep {
        return glyph_span(row, col);
    }
    let (mut a, mut b) = glyph_span(row, col);
    while a > 0 && class(row, a - 1) == c {
        a -= 1;
    }
    while b + 1 < row.len() && class(row, b + 1) == c {
        b += 1;
    }
    (a, b)
}

/// Triple-click expansion: the whole row.
pub fn line_span(cols: usize) -> (usize, usize) {
    (0, cols.saturating_sub(1))
}

/// Inclusive columns of `row` covered by the selection `start..=end` (already ordered).
pub fn row_range(start: Pos, end: Pos, row: usize, cols: usize) -> Option<(usize, usize)> {
    if cols == 0 || row < start.row || row > end.row {
        return None;
    }
    let c0 = if row == start.row { start.col } else { 0 };
    let c1 = if row == end.row { end.col } else { cols - 1 };
    (c0 <= c1).then_some((c0.min(cols - 1), c1.min(cols - 1)))
}

impl Selection {
    /// Ordered, inclusive, expanded bounds of the selection over `grid`.
    pub fn bounds(&self, grid: &[Vec<Cell>]) -> (Pos, Pos) {
        let last = grid.len().saturating_sub(1);
        let clamp = |p: Pos| Pos { row: p.row.min(last), col: p.col };
        let (a, h) = (clamp(self.anchor), clamp(self.head));
        let span = |p: Pos| -> (Pos, Pos) {
            let row: &[Cell] = grid.get(p.row).map_or(&[], |r| r.as_slice());
            let (c0, c1) = match self.mode {
                Mode::Cell => glyph_span(row, p.col),
                Mode::Word => word_span(row, p.col),
                Mode::Line => line_span(row.len()),
            };
            (Pos { row: p.row, col: c0 }, Pos { row: p.row, col: c1 })
        };
        let ((a0, a1), (h0, h1)) = (span(a), span(h));
        (normalize(a0, h0).0, normalize(a1, h1).1)
    }
}

/// The selected text: rows joined with '\n', wide characters once, trailing spaces removed per row.
pub fn extract(grid: &[Vec<Cell>], start: Pos, end: Pos) -> String {
    let mut out = String::new();
    let end_row = end.row.saturating_add(1);
    for (r, row) in grid.iter().enumerate().take(end_row).skip(start.row) {
        let Some((c0, c1)) = row_range(start, end, r, row.len()) else { continue };
        let mut line = String::new();
        for cell in &row[c0..=c1] {
            if cell.wide == Wide::Spacer {
                continue;
            }
            line.push_str(if cell.text().is_empty() { " " } else { cell.text() });
        }
        if r > start.row {
            out.push('\n');
        }
        out.push_str(line.trim_end_matches(' '));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ghostty::{RenderState, Terminal};

    fn row(s: &str, cols: usize) -> Vec<Cell> {
        let mut v = Vec::new();
        for ch in s.chars() {
            let t = ch.to_string();
            if ch.len_utf8() > 2 {
                v.push(Cell::plain(&t, Wide::Double));
                v.push(Cell::plain("", Wide::Spacer));
            } else if ch == ' ' {
                v.push(Cell::plain("", Wide::Narrow));
            } else {
                v.push(Cell::plain(&t, Wide::Narrow));
            }
        }
        while v.len() < cols {
            v.push(Cell::plain("", Wide::Narrow));
        }
        v
    }
    fn p(row: usize, col: usize) -> Pos {
        Pos { row, col }
    }

    #[test]
    fn selecting_an_empty_grid_returns_no_text() {
        assert_eq!(extract(&[], p(0, 0), p(0, 0)), "");
    }

    #[test]
    fn normalize_orders_reading_order() {
        assert_eq!(normalize(p(2, 1), p(0, 5)), (p(0, 5), p(2, 1)));
        assert_eq!(normalize(p(1, 3), p(1, 2)), (p(1, 2), p(1, 3)));
        assert_eq!(normalize(p(1, 3), p(1, 3)), (p(1, 3), p(1, 3)));
    }

    #[test]
    fn row_range_follows_lines_like_a_terminal() {
        let (s, e) = (p(1, 4), p(3, 2));
        assert_eq!(row_range(s, e, 0, 10), None);
        assert_eq!(row_range(s, e, 1, 10), Some((4, 9)), "first row: from the start to the end of the row");
        assert_eq!(row_range(s, e, 2, 10), Some((0, 9)), "middle rows are complete");
        assert_eq!(row_range(s, e, 3, 10), Some((0, 2)), "last row: up to the end cell, inclusive");
        assert_eq!(row_range(s, e, 4, 10), None);
        assert_eq!(row_range(p(1, 4), p(1, 6), 1, 10), Some((4, 6)), "single row");
    }

    #[test]
    fn word_expansion_uses_the_separator_set() {
        let r = row("foo(bar_baz, \"x y\") ok", 30);
        assert_eq!(word_span(&r, 1), (0, 2), "foo");
        assert_eq!(word_span(&r, 3), (3, 3), "a separator is alone");
        assert_eq!(word_span(&r, 6), (4, 10), "bar_baz, underscores are word characters");
        assert_eq!(word_span(&r, 11), (11, 11), ", is a separator");
        assert_eq!(word_span(&r, 12), (12, 12), "single blank");
        assert_eq!(word_span(&r, 18), (18, 18), ") is a separator");
        assert_eq!(word_span(&r, 13), (13, 13), "\" is a separator");
        assert_eq!(word_span(&r, 21), (20, 21), "ok");
        assert_eq!(word_span(&r, 27), (22, 29), "blank run to the end of the row");
        let w = row("a日本b c", 12);
        assert_eq!(word_span(&w, 2), (0, 5), "wide chars belong to the word; hitting a spacer works too");
        assert_eq!(word_span(&w, 3), (0, 5));
        assert_eq!(word_span(&[], 3), (0, 0));
    }

    #[test]
    fn line_expansion_covers_the_row() {
        assert_eq!(line_span(80), (0, 79));
        assert_eq!(line_span(0), (0, 0));
    }

    #[test]
    fn text_is_built_from_rows_without_padding() {
        let g = vec![row("hola mundo", 20), row("segunda línea  ", 20), row("日本a", 20)];
        let s = Selection { anchor: p(0, 5), head: p(2, 2), mode: Mode::Cell };
        let (a, b) = s.bounds(&g);
        assert_eq!((a, b), (p(0, 5), p(2, 3)), "the end snaps to the second cell of the wide char");
        assert_eq!(extract(&g, a, b), "mundo\nsegunda línea\n日本");
        // Drag backwards and from the spacer cell: same text.
        let s = Selection { anchor: p(2, 1), head: p(0, 5), mode: Mode::Cell };
        let (a, b) = s.bounds(&g);
        assert_eq!(extract(&g, a, b), "mundo\nsegunda línea\n日");
        let w = Selection { anchor: p(1, 9), head: p(1, 9), mode: Mode::Word };
        let (a, b) = w.bounds(&g);
        assert_eq!(extract(&g, a, b), "línea");
        let l = Selection { anchor: p(0, 3), head: p(1, 0), mode: Mode::Line };
        let (a, b) = l.bounds(&g);
        assert_eq!(extract(&g, a, b), "hola mundo\nsegunda línea");
    }

    /// Same scenario through the real libghostty terminal and render state.
    #[test]
    fn text_from_the_real_terminal() {
        let mut t = Terminal::new(20, 5, 100, Box::new(|_| {}));
        t.write("hola mundo\r\nsegunda línea  \r\n日本a".as_bytes());
        let mut rs = RenderState::new();
        rs.update(&t);
        let mut grid: Vec<Vec<Cell>> = Vec::new();
        rs.for_rows(true, |_, cells| grid.push(cells.to_vec()));
        assert_eq!(grid.len(), 5);
        let s = Selection { anchor: p(0, 5), head: p(2, 2), mode: Mode::Cell };
        let (a, b) = s.bounds(&grid);
        assert_eq!(extract(&grid, a, b), "mundo\nsegunda línea\n日本");
        let w = Selection { anchor: p(0, 1), head: p(0, 1), mode: Mode::Word };
        let (a, b) = w.bounds(&grid);
        assert_eq!(extract(&grid, a, b), "hola");
    }
}
