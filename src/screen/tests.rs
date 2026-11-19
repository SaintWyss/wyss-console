use super::{Limits, Terminal, mode};

fn term(rows: usize, cols: usize) -> Terminal {
    Terminal::new(rows, cols, Limits::default())
}
fn run(t: &mut Terminal, bytes: &[u8]) {
    t.feed(bytes);
}

#[test]
fn text_wraps_scrolls_and_keeps_history() {
    let mut t = term(3, 5);
    run(&mut t, b"abcdefgh\r\nxyz\r\n1\r\n2");
    assert_eq!(t.text(), ["xyz", "1", "2"]);
    assert_eq!(t.scrollback_len(), 2);
    let lines: Vec<String> = t.scrollback().map(super::terminal::row_text).collect();
    assert_eq!(lines, ["abcde", "fgh"]);
}

#[test]
fn cursor_motion_erase_and_regions() {
    let mut t = term(5, 10);
    run(&mut t, b"\x1b[2;3Hhello\x1b[1;1HTOP\x1b[5;1Hbottom");
    assert_eq!(t.text(), ["TOP", "  hello", "", "", "bottom"]);
    run(&mut t, b"\x1b[2;4r\x1b[4;1H\n\n");
    // Scrolling inside rows 2..4 leaves rows 1 and 5 alone.
    assert_eq!(t.text()[0], "TOP");
    assert_eq!(t.text()[4], "bottom");
    run(&mut t, b"\x1b[r\x1b[2J\x1b[H");
    assert!(t.text().iter().all(String::is_empty));
}

#[test]
fn wide_characters_take_two_columns_and_never_split() {
    let mut t = term(2, 6);
    run(&mut t, "a世b".as_bytes());
    assert_eq!(t.text()[0], "a世b");
    assert_eq!(t.cursor().col, 4);
    // Overwriting half of a wide character removes the whole of it.
    run(&mut t, b"\x1b[1;3Hz");
    assert_eq!(t.text()[0], "a zb");
    // A wide character that does not fit wraps whole.
    let mut t = term(2, 3);
    run(&mut t, "ab世".as_bytes());
    assert_eq!(t.text(), ["ab", "世"]);
}

#[test]
fn combining_marks_stay_with_their_character() {
    let mut t = term(1, 8);
    run(&mut t, "e\u{301}x".as_bytes());
    assert_eq!(t.text()[0], "e\u{301}x");
    assert_eq!(t.cursor().col, 2);
}

#[test]
fn the_alternate_screen_keeps_the_normal_one_and_the_cursor() {
    let mut t = term(4, 10);
    run(&mut t, b"normal\x1b[?1049h\x1b[2Jalt");
    assert!(t.alt_active());
    assert_eq!(t.text()[0], "alt");
    run(&mut t, b"\x1b[?1049l");
    assert!(!t.alt_active());
    assert_eq!(t.text()[0], "normal");
    assert_eq!(t.cursor().col, 6);
}

#[test]
fn modes_are_tracked() {
    let mut t = term(2, 10);
    run(
        &mut t,
        b"\x1b[?1000h\x1b[?1006h\x1b[?2004h\x1b[?25l\x1b[?1h",
    );
    for m in [
        mode::MOUSE_1000,
        mode::MOUSE_SGR,
        mode::BRACKETED_PASTE,
        mode::APP_CURSOR,
    ] {
        assert!(t.modes() & m != 0);
    }
    assert!(t.modes() & mode::CURSOR_VISIBLE == 0);
    run(&mut t, b"\x1b[?1000l\x1b[?25h");
    assert!(t.modes() & mode::MOUSE_1000 == 0 && t.modes() & mode::CURSOR_VISIBLE != 0);
}

#[test]
fn sgr_colors_in_every_notation() {
    let mut t = term(1, 20);
    run(
        &mut t,
        b"\x1b[1;31mA\x1b[38;5;200mB\x1b[38;2;1;2;3mC\x1b[38:2::9:8:7mD\x1b[0mE",
    );
    let row = &t.buffer(false).rows[0].cells;
    assert_eq!(row[0].style.fg, super::Color::Indexed(1));
    assert_eq!(row[0].style.attrs & super::terminal::attr::BOLD, 1);
    assert_eq!(row[1].style.fg, super::Color::Indexed(200));
    assert_eq!(row[2].style.fg, super::Color::Rgb(1, 2, 3));
    assert_eq!(row[3].style.fg, super::Color::Rgb(9, 8, 7));
    assert_eq!(row[4].style, super::Style::default());
}

#[test]
fn hostile_sequences_are_counted_and_do_nothing() {
    let mut t = term(2, 20);
    run(
        &mut t,
        b"\x1b]52;c;aGVsbG8=\x07\x1b]8;;http://example.invalid\x07link\x1b]8;;\x07",
    );
    run(
        &mut t,
        b"\x1b[8;50;200t\x1b[?3h\x1b[c\x1b[6n\x1bPq#0;2;0;0;0\x1b\\\x1b_Gf=24\x1b\\",
    );
    let r = t.refusals();
    assert_eq!((r.clipboard, r.hyperlink), (1, 2), "open and close");
    assert!(r.window_op >= 2 && r.query >= 2 && r.string >= 2, "{r:?}");
    assert_eq!((t.rows(), t.cols()), (2, 20), "output cannot resize");
    assert_eq!(t.text()[0], "link");
}

#[test]
fn a_title_is_bounded_and_has_no_control_characters() {
    let mut t = term(2, 20);
    let mut seq = b"\x1b]2;".to_vec();
    seq.extend_from_slice(b"ok\x01\x1b[31m");
    seq.push(0x07);
    run(&mut t, &seq);
    assert!(!t.title().chars().any(char::is_control));
    run(&mut t, b"\x1b]0;");
    run(&mut t, &vec![b'x'; 100_000]);
    run(&mut t, b"\x07");
    assert!(
        t.title().is_empty(),
        "an oversized one is dropped, not truncated into place"
    );
    assert_eq!(t.refusals().oversized, 1);
}

#[test]
fn an_unterminated_string_cannot_swallow_the_screen_for_ever() {
    let mut t = term(2, 20);
    run(&mut t, b"\x1bP");
    run(&mut t, &vec![b'x'; 10_000]);
    run(&mut t, b"visible");
    assert!(
        t.text().iter().any(|r| r.contains("visible")),
        "{:?}",
        t.text()
    );
    assert_eq!(t.refusals().oversized, 1);
}

#[test]
fn invalid_utf8_becomes_replacement_characters_and_is_counted() {
    let mut t = term(1, 20);
    run(&mut t, b"a\xffb\xc3(c\xe2\x82");
    run(&mut t, b"d");
    assert_eq!(t.text()[0], "a\u{FFFD}b\u{FFFD}(c\u{FFFD}d");
    assert!(t.refusals().invalid_utf8 >= 3);
}

#[test]
fn a_sequence_split_across_feeds_is_the_same_as_one_feed() {
    let bytes = "ab\x1b[1;31mX\x1b[0m世\x1b]2;t\x07\x1b[2;1Hz".as_bytes();
    let mut whole = term(3, 12);
    run(&mut whole, bytes);
    for cut in 0..bytes.len() {
        let mut split = term(3, 12);
        run(&mut split, &bytes[..cut]);
        run(&mut split, &bytes[cut..]);
        assert_eq!(split.text(), whole.text(), "cut at {cut}");
        assert_eq!(split.title(), whole.title());
    }
}

#[test]
fn resize_keeps_the_screen_bounded_and_counts_a_generation() {
    let mut t = term(4, 10);
    run(&mut t, b"1\r\n2\r\n3\r\n4");
    let g = t.generation();
    t.resize(2, 6);
    assert_eq!((t.rows(), t.cols()), (2, 6));
    assert!(t.generation() > g);
    assert!(t.cursor().row < 2 && t.cursor().col < 6);
    assert_eq!(t.text(), ["3", "4"]);
    assert_eq!(t.scrollback_len(), 2);
    t.resize(2, 6);
    assert_eq!(t.generation(), g + 1, "the same size is not a change");
}

#[test]
fn scrollback_is_bounded_by_lines_and_cells() {
    let mut t = Terminal::new(
        2,
        10,
        Limits {
            scrollback_lines: 5,
            scrollback_cells: 1_000,
            ..Limits::default()
        },
    );
    for n in 0..100 {
        run(&mut t, format!("line {n}\r\n").as_bytes());
    }
    assert_eq!(t.scrollback_len(), 5);
    assert!(t.history().lines_evicted >= 90);
    assert!(t.scrollback_cells() <= 1_000);
}

// ---- the round trip and the property tests ---------------------------------------------------

use super::terminal::{Row, row_text};

/// Everything a window sees of a screen: both buffers (the alternate one only while it is shown),
/// cursors, saved cursors, regions, pen, modes, title, cursor style and the scrollback's text and
/// styles. Tab stops and the shifted charset are not repainted and are not compared.
fn visible_state(t: &Terminal) -> Vec<String> {
    let mut out = Vec::new();
    let rows = |r: &[Row]| -> Vec<String> {
        r.iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|c| format!("{:?}{:?}{}{:?}", c.ch, c.mark, c.width, c.style))
                    .collect::<Vec<_>>()
                    .join("|")
            })
            .collect()
    };
    out.push(format!("size {}x{}", t.rows(), t.cols()));
    for alt in [false, true] {
        if alt && !t.alt_active() {
            continue;
        }
        let b = t.buffer(alt);
        out.push(format!(
            "buffer alt={alt} cursor={:?} region={},{} saved={:?}",
            b.cursor,
            b.top,
            b.bottom,
            b.saved_state()
        ));
        out.extend(rows(&b.rows));
    }
    out.push(format!(
        "alt={} modes={:#x} pen={:?} title={:?} cursor_style={} g0={:?}",
        t.alt_active(),
        t.modes(),
        t.pen(),
        t.title(),
        t.cursor_style(),
        t.charset_g0()
    ));
    for line in t.scrollback() {
        out.push(format!(
            "history {}",
            rows(std::slice::from_ref(&Row {
                cells: line.cells.clone(),
                wrapped: false
            }))[0]
        ));
    }
    out
}

/// Deterministic generator: no dependency, and every failure is reproduced by its seed.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const WORDS: &[&str] = &[
    "hello",
    "world",
    " ",
    "  ",
    "世界",
    "한글",
    "😀",
    "é",
    "e\u{301}",
    "─│┌┐",
    "x",
    "0123456789",
    "\r\n",
    "\r",
    "\n",
    "\t",
    "\x08",
    "tmux",
    "ñandú",
];
const CSI: &[&str] = &[
    "\x1b[H",
    "\x1b[2J",
    "\x1b[K",
    "\x1b[1K",
    "\x1b[2K",
    "\x1b[J",
    "\x1b[1J",
    "\x1b[3;5H",
    "\x1b[10;40H",
    "\x1b[A",
    "\x1b[3B",
    "\x1b[7C",
    "\x1b[2D",
    "\x1b[E",
    "\x1b[2F",
    "\x1b[9G",
    "\x1b[4d",
    "\x1b[2L",
    "\x1b[3M",
    "\x1b[2@",
    "\x1b[3P",
    "\x1b[2X",
    "\x1b[S",
    "\x1b[2T",
    "\x1b[2;6r",
    "\x1b[r",
    "\x1b[1;1r",
    "\x1b[0m",
    "\x1b[1m",
    "\x1b[2m",
    "\x1b[3m",
    "\x1b[4m",
    "\x1b[7m",
    "\x1b[9m",
    "\x1b[22m",
    "\x1b[31m",
    "\x1b[42m",
    "\x1b[91m",
    "\x1b[38;5;99m",
    "\x1b[48;2;10;20;30m",
    "\x1b[38:2::1:2:3m",
    "\x1b[39m",
    "\x1b[49m",
    "\x1b[?1h",
    "\x1b[?1l",
    "\x1b[?6h",
    "\x1b[?6l",
    "\x1b[?7l",
    "\x1b[?7h",
    "\x1b[?25l",
    "\x1b[?25h",
    "\x1b[?1000h",
    "\x1b[?1002h",
    "\x1b[?1006h",
    "\x1b[?1000l",
    "\x1b[?1004h",
    "\x1b[?2004h",
    "\x1b[?2004l",
    "\x1b[?1049h",
    "\x1b[?1049l",
    "\x1b[?47h",
    "\x1b[?47l",
    "\x1b[?1047h",
    "\x1b[?1047l",
    "\x1b[4h",
    "\x1b[4l",
    "\x1b[20h",
    "\x1b[20l",
    "\x1b7",
    "\x1b8",
    "\x1b[s",
    "\x1b[u",
    "\x1bM",
    "\x1bD",
    "\x1bE",
    "\x1bH",
    "\x1b[g",
    "\x1b[3g",
    "\x1b[3 q",
    "\x1b[0 q",
    "\x1b(0",
    "\x1b(B",
    "\x0e",
    "\x0f",
    "\x1b]2;a title\x07",
    "\x1b]0;other\x1b\\",
    "\x1b[3J",
    "\x1b[2b",
    "\x1b[I",
    "\x1b[Z",
    "\x1b[!p",
];
const HOSTILE: &[&str] = &[
    "\x1b]52;c;QUJD\x07",
    "\x1b]8;;http://x.invalid\x07",
    "\x1b[8;1;1t",
    "\x1b[?3h",
    "\x1b[c",
    "\x1b[6n",
    "\x1bPqabc\x1b\\",
    "\x1b_Gx\x1b\\",
    "\x1b^p\x1b\\",
    "\x1bXq\x1b\\",
    "\u{9b}31m",
    "\x1b[999999999999H",
    "\x1b[;;;;;;;;;;;;;;;;;;;;;;;;;;;;m",
    "\x1b[?9999h",
    "\x1b#8",
    "\x18",
    "\x1a",
    "\x1b[38;5m",
    "\x1b[38;2;1m",
];

fn stream(rng: &mut Rng, n: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..n {
        match rng.below(12) {
            0..=4 => out.extend_from_slice(WORDS[rng.below(WORDS.len())].as_bytes()),
            5..=9 => out.extend_from_slice(CSI[rng.below(CSI.len())].as_bytes()),
            10 => out.extend_from_slice(HOSTILE[rng.below(HOSTILE.len())].as_bytes()),
            _ => {
                for _ in 0..rng.below(6) {
                    out.push(rng.next() as u8);
                }
            }
        }
    }
    out
}

fn invariants(t: &Terminal) {
    assert!(t.rows() >= 1 && t.cols() >= 1);
    for alt in [false, true] {
        let b = t.buffer(alt);
        assert_eq!(b.rows.len(), t.rows());
        assert!(b.rows.iter().all(|r| r.cells.len() == t.cols()));
        assert!(
            b.cursor.row < t.rows() && b.cursor.col < t.cols(),
            "{:?}",
            b.cursor
        );
        assert!(b.top <= b.bottom && b.bottom < t.rows());
        for r in &b.rows {
            for (i, c) in r.cells.iter().enumerate() {
                if c.width == 0 {
                    assert!(i > 0 && r.cells[i - 1].width == 2, "orphaned half at {i}");
                }
                if c.width == 2 {
                    assert!(
                        i + 1 < r.cells.len() && r.cells[i + 1].width == 0,
                        "split wide at {i}"
                    );
                }
            }
        }
    }
    assert!(t.scrollback_len() <= Limits::default().scrollback_lines);
    assert!(t.scrollback_cells() <= Limits::default().scrollback_cells);
    assert!(t.title().chars().count() <= super::terminal::MAX_TITLE);
    assert!(!t.title().chars().any(char::is_control));
    // The text of every row can be produced without panicking.
    for r in &t.buffer(false).rows {
        let _ = row_text(r);
    }
}

#[test]
fn random_streams_keep_every_invariant_and_survive_a_repaint_round_trip() {
    for seed in 1..=400u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let rows = 2 + rng.below(20);
        let cols = 3 + rng.below(60);
        let mut t = term(rows, cols);
        let count = 40 + rng.below(160);
        let bytes = stream(&mut rng, count);
        // Fed in uneven pieces, as a PTY delivers it.
        let mut at = 0;
        while at < bytes.len() {
            let n = 1 + rng.below(40);
            t.feed(&bytes[at..(at + n).min(bytes.len())]);
            at += n;
            invariants(&t);
        }
        let repaint = t.repaint(10_000).unwrap();
        let mut again = term(rows, cols);
        again.feed(&repaint.bytes);
        invariants(&again);
        let (a, b) = (visible_state(&t), visible_state(&again));
        if a != b {
            let at = a
                .iter()
                .zip(&b)
                .position(|(x, y)| x != y)
                .unwrap_or(a.len().min(b.len()));
            panic!(
                "seed {seed} {rows}x{cols}: first difference at line {at}\n  original: {:?}\n  repainted: {:?}",
                a.get(at),
                b.get(at)
            );
        }
    }
}

#[test]
fn pure_garbage_never_breaks_a_bound() {
    for seed in 1..=300u64 {
        let mut rng = Rng(seed.wrapping_mul(0xD1B5_4A32_D192_ED03) | 1);
        let mut t = term(1 + rng.below(30), 1 + rng.below(100));
        for _ in 0..20 {
            let n = rng.below(2_000);
            let bytes: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
            t.feed(&bytes);
            invariants(&t);
        }
        let r = t.repaint(100).unwrap();
        assert!(r.bytes.len() <= super::repaint::MAX_REPAINT);
    }
}

#[test]
fn a_repaint_contains_nothing_a_terminal_would_act_on() {
    let mut t = term(4, 20);
    t.feed(b"\x1b]52;c;QUJD\x07\x1b]8;;http://x.invalid\x07hi\x1b]2;a title\x07\x1b[8;1;1t\x1b[c");
    let bytes = t.repaint(10).unwrap().bytes;
    let text = String::from_utf8_lossy(&bytes);
    for forbidden in [
        "\x1b]52",
        "\x1b]8",
        "\x1b[8;",
        "\x1b[c",
        "\x1b[6n",
        "\x1bP",
        "x.invalid",
    ] {
        assert!(!text.contains(forbidden), "{forbidden:?} in the repaint");
    }
    assert!(text.contains("a title"));
}

#[test]
fn c1_controls_do_nothing_a_clipboard_write_included_and_never_reach_a_repaint() {
    // U+009D is OSC and U+009C is ST in the C1 set: a terminal that honours them would write the
    // clipboard from this. The engine counts it and the repaint carries none of it.
    let mut t = term(4, 30);
    t.feed("A\u{9d}52;c;SGVsbG8=\u{9c}B".as_bytes());
    assert_eq!(t.text()[0], "AB");
    assert_eq!(t.refusals().clipboard, 1);
    let bytes = t.repaint(10).unwrap().bytes;
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains('\u{9d}') && !text.contains('\u{9c}'));
    assert!(!text.contains("SGVsbG8="));
    // Every C1 character, alone, leaves no cell and no byte behind in a repaint.
    let mut t = term(4, 30);
    for c in 0x80u32..0xa0 {
        let ch = char::from_u32(c).unwrap();
        t.feed(format!("x{ch}y").as_bytes());
    }
    let bytes = t.repaint(10).unwrap().bytes;
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.chars().all(|c| !('\u{80}'..='\u{9f}').contains(&c)),
        "a C1 character reached the repaint"
    );
}
