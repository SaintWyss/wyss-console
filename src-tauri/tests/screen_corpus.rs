//! M4 reference-screen corpus: real programs and scripted sequences, recorded with tmux as the
//! reference implementation (`benchmarks/screen_corpus.py`). Every stream is replayed through the
//! Wyss screen engine and compared with what tmux made of the same bytes: visible text, cursor,
//! modes, region and, through tmux's own escape rendering of the screen, every cell's attributes.

use std::path::PathBuf;
use wyss_console::screen::{Limits, Terminal, mode, width::Table};

/// tmux counts emoji and recent wide ranges as wide, so the comparison is made under the table that
/// does the same; the table the window itself draws with is checked against its source separately.
fn limits() -> Limits {
    Limits {
        width: Table::Modern,
        ..Limits::default()
    }
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/screen")
}

fn names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir())
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            let n = p.file_name()?.to_str()?.to_owned();
            n.strip_suffix(".bin").map(str::to_owned)
        })
        .collect();
    names.sort();
    names
}

fn load(name: &str) -> (Terminal, serde_json::Value, String) {
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir().join(format!("{name}.ref.json"))).unwrap())
            .unwrap();
    let rows = meta["rows"].as_u64().unwrap() as usize;
    let cols = meta["cols"].as_u64().unwrap() as usize;
    let mut t = Terminal::new(rows, cols, limits());
    // Fed in the uneven pieces a PTY delivers, never in one: the result is the same.
    let bytes = std::fs::read(dir().join(format!("{name}.bin"))).unwrap();
    for piece in bytes.chunks(97) {
        t.feed(piece);
    }
    let sgr = std::fs::read_to_string(dir().join(format!("{name}.ref.sgr"))).unwrap();
    (t, meta, sgr)
}

fn flag(meta: &serde_json::Value, key: &str) -> bool {
    meta[key].as_u64().unwrap() != 0
}

#[test]
fn the_corpus_is_not_empty_and_has_real_programs() {
    let names = names();
    for required in [
        "vim-edit",
        "htop",
        "less-utf8",
        "tmux-nested",
        "top",
        "bash-session",
        "python-curses",
        "unicode-wide",
    ] {
        assert!(names.iter().any(|n| n == required), "missing {required}");
    }
}

/// tmux's rendering of its screen (`capture-pane -e`), replayed into an empty terminal: the
/// reference's cells, characters and attributes.
fn reference_screen(meta: &serde_json::Value, sgr: &str) -> Terminal {
    let rows = meta["rows"].as_u64().unwrap() as usize;
    let cols = meta["cols"].as_u64().unwrap() as usize;
    let mut reference = Terminal::new(rows, cols, limits());
    let lines: Vec<&str> = sgr.trim_end_matches('\n').split('\n').collect();
    // tmux's escape rendering switches to its ACS set with SO and never designates it.
    reference.feed(b"\x1b)0");
    reference.feed(lines.join("\r\n").as_bytes());
    reference
}

#[test]
fn visible_text_matches_tmux() {
    // tmux's plain `capture-pane -p` prints a line-drawing cell as the raw ACS letter; its escape
    // rendering carries the charset switch, so that rendering is the reference for characters.
    let mut failures = Vec::new();
    for name in names() {
        let (t, meta, sgr) = load(&name);
        let reference = reference_screen(&meta, &sgr).text();
        for (i, (g, e)) in t.text().iter().zip(&reference).enumerate() {
            if g != e {
                failures.push(format!("{name} row {i}:\n  wyss: {g:?}\n  tmux: {e:?}"));
            }
        }
        // Where no line-drawing charset was used, tmux's plain text must agree as well.
        if !String::from_utf8_lossy(&std::fs::read(dir().join(format!("{name}.bin"))).unwrap())
            .contains("\x1b(0")
            && !name.contains("curses")
        {
            for (i, (g, e)) in t
                .text()
                .iter()
                .zip(meta["text"].as_array().unwrap())
                .enumerate()
            {
                if g != e.as_str().unwrap().trim_end() {
                    failures.push(format!(
                        "{name} plain row {i}:\n  wyss: {g:?}\n  tmux: {e:?}"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn cursor_modes_and_regions_match_tmux() {
    let mut failures = Vec::new();
    for name in names() {
        let (t, meta, _) = load(&name);
        let c = t.cursor();
        let x = if c.pending_wrap { c.col + 1 } else { c.col };
        let mut check = |what: &str, got: String, want: String| {
            if got != want {
                failures.push(format!("{name} {what}: wyss {got} tmux {want}"));
            }
        };
        check("cursor_x", x.to_string(), meta["cursor_x"].to_string());
        check("cursor_y", c.row.to_string(), meta["cursor_y"].to_string());
        check(
            "alternate",
            t.alt_active().to_string(),
            flag(&meta, "alternate_on").to_string(),
        );
        let m = t.modes();
        let has = |f: u32| (m & f != 0).to_string();
        check(
            "cursor_visible",
            has(mode::CURSOR_VISIBLE),
            flag(&meta, "cursor_visible").to_string(),
        );
        check(
            "origin",
            has(mode::ORIGIN),
            flag(&meta, "origin").to_string(),
        );
        check("wrap", has(mode::AUTOWRAP), flag(&meta, "wrap").to_string());
        check(
            "insert",
            has(mode::INSERT),
            flag(&meta, "insert").to_string(),
        );
        check(
            "keypad_cursor",
            has(mode::APP_CURSOR),
            flag(&meta, "keypad_cursor").to_string(),
        );
        check(
            "keypad",
            has(mode::APP_KEYPAD),
            flag(&meta, "keypad").to_string(),
        );
        let any = mode::MOUSE_1000 | mode::MOUSE_1002 | mode::MOUSE_1003;
        check("mouse_any", has(any), flag(&meta, "mouse_any").to_string());
        check(
            "mouse_standard",
            has(mode::MOUSE_1000),
            flag(&meta, "mouse_standard").to_string(),
        );
        check(
            "mouse_button",
            has(mode::MOUSE_1002),
            flag(&meta, "mouse_button").to_string(),
        );
        check(
            "mouse_sgr",
            has(mode::MOUSE_SGR),
            flag(&meta, "mouse_sgr").to_string(),
        );
        check(
            "mouse_utf8",
            has(mode::MOUSE_UTF8),
            flag(&meta, "mouse_utf8").to_string(),
        );
        let (top, bottom) = t.region();
        check(
            "region_upper",
            top.to_string(),
            meta["region_upper"].to_string(),
        );
        check(
            "region_lower",
            bottom.to_string(),
            meta["region_lower"].to_string(),
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_cells_attributes_match_tmux() {
    let mut failures = Vec::new();
    for name in names() {
        let (t, meta, sgr) = load(&name);
        let reference = reference_screen(&meta, &sgr);
        let (a, b) = (t.buffer(t.alt_active()), reference.buffer(false));
        'rows: for (r, (ra, rb)) in a.rows.iter().zip(&b.rows).enumerate() {
            for (c, (ca, cb)) in ra.cells.iter().zip(&rb.cells).enumerate() {
                if ca.ch != cb.ch || ca.width != cb.width || ca.style != cb.style {
                    failures.push(format!(
                        "{name} r{r} c{c}: wyss {:?} {:?} w{} / tmux {:?} {:?} w{}",
                        ca.ch, ca.style, ca.width, cb.ch, cb.style, cb.width
                    ));
                    continue 'rows;
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_repaint_of_every_corpus_screen_redraws_it_exactly() {
    for name in names() {
        let (t, meta, _) = load(&name);
        let rows = meta["rows"].as_u64().unwrap() as usize;
        let cols = meta["cols"].as_u64().unwrap() as usize;
        let repaint = t.repaint(10_000);
        let mut again = Terminal::new(rows, cols, limits());
        again.feed(&repaint.bytes);
        assert_eq!(again.text(), t.text(), "{name}");
        assert_eq!(again.cursor(), t.cursor(), "{name}");
        assert_eq!(again.modes(), t.modes(), "{name}");
        assert_eq!(again.alt_active(), t.alt_active(), "{name}");
        for alt in [false, true] {
            if alt && !t.alt_active() {
                continue;
            }
            // The cells; whether a line was wrapped is not something a repaint can carry.
            let cells = |t: &Terminal| -> Vec<Vec<wyss_console::screen::Cell>> {
                t.buffer(alt).rows.iter().map(|r| r.cells.clone()).collect()
            };
            assert_eq!(cells(&again), cells(&t), "{name} alt={alt}");
        }
    }
}
