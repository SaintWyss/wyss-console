//! Pure side of the search bar: where the highlights of a match go and what the counter says. The
//! matching itself is libghostty's (`ghostty::Search`); this only turns its answers into rows.

use crate::ghostty::Span;

/// The cells a match covers in each viewport row, as `(row, first column, last column)`, inclusive.
/// A match that wraps covers the rest of its first row, whole rows, and the start of its last row.
/// Cells outside the `rows` x `cols` viewport are cut off (a match that continues past the viewport
/// has `x1`/`y1` at their maximum).
pub fn row_spans(m: Span, rows: usize, cols: usize) -> Vec<(usize, usize, usize)> {
    if rows == 0 || cols == 0 || (m.y0 as usize) >= rows && m.y0 != 0 {
        return Vec::new();
    }
    let last_row = (m.y1 as usize).min(rows - 1);
    let mut out = Vec::new();
    for row in m.y0 as usize..=last_row {
        let from = if row == m.y0 as usize { m.x0 as usize } else { 0 };
        let to = if row == m.y1 as usize { (m.x1 as usize).min(cols - 1) } else { cols - 1 };
        if from <= to && from < cols {
            out.push((row, from, to));
        }
    }
    out
}

/// What the bar shows next to the field: nothing for an empty query, `No matches`, or `k of n`
/// (`k` counts from the newest match, like the Console's "1 of N").
pub fn counter(query_empty: bool, total: usize, selected: Option<usize>) -> Option<(usize, usize)> {
    if query_empty || total == 0 {
        return None;
    }
    Some((selected.map_or(1, |i| i + 1).min(total), total))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(x0: u16, y0: u32, x1: u16, y1: u32) -> Span {
        Span { x0, y0, x1, y1 }
    }

    #[test]
    fn a_match_on_one_row_is_one_range() {
        assert_eq!(row_spans(span(8, 3, 13, 3), 24, 80), vec![(3, 8, 13)]);
    }

    #[test]
    fn a_wrapped_match_covers_the_rest_of_its_row_whole_rows_and_the_start_of_the_last() {
        assert_eq!(row_spans(span(70, 2, 4, 4), 10, 80), vec![(2, 70, 79), (3, 0, 79), (4, 0, 4)]);
    }

    #[test]
    fn a_match_that_continues_past_the_viewport_is_cut_off_at_its_edge() {
        assert_eq!(row_spans(span(75, 8, u16::MAX, u32::MAX), 10, 80), vec![(8, 75, 79), (9, 0, 79)]);
        // Began above the viewport: it starts at its first cell.
        assert_eq!(row_spans(span(0, 0, 5, 1), 10, 80), vec![(0, 0, 79), (1, 0, 5)]);
        assert!(row_spans(span(0, 12, 5, 12), 10, 80).is_empty(), "a match below the viewport is not drawn");
        assert!(row_spans(span(0, 0, 5, 0), 0, 80).is_empty());
    }

    #[test]
    fn the_counter_counts_from_the_newest_and_hides_for_nothing() {
        assert_eq!(counter(true, 5, Some(0)), None, "an empty query shows nothing");
        assert_eq!(counter(false, 0, None), None, "no matches");
        assert_eq!(counter(false, 7, Some(0)), Some((1, 7)));
        assert_eq!(counter(false, 7, Some(6)), Some((7, 7)));
        assert_eq!(counter(false, 7, None), Some((1, 7)));
    }
}
