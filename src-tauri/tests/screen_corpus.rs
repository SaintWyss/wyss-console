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
        let repaint = t.repaint(10_000).unwrap();
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

// ---- mutation fuzzing on real streams ---------------------------------------------------------

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// A corpus stream with a few bytes flipped, dropped, inserted, repeated or cut, as a corrupted
/// or hostile program might write it.
fn mutate(rng: &mut Rng, original: &[u8]) -> Vec<u8> {
    let mut bytes = original.to_vec();
    for _ in 0..1 + rng.below(12) {
        if bytes.is_empty() {
            break;
        }
        let at = rng.below(bytes.len());
        match rng.below(6) {
            0 => bytes[at] ^= 1 << rng.below(8),
            1 => {
                bytes.remove(at);
            }
            2 => bytes.insert(at, rng.next() as u8),
            3 => {
                let end = (at + 1 + rng.below(64)).min(bytes.len());
                let chunk = bytes[at..end].to_vec();
                bytes.splice(at..at, chunk);
            }
            4 => bytes.truncate(at.max(1)),
            _ => {
                let esc = [0x1b, b'[', b'?', b'1', b'0', b'4', b'9', b'h'];
                bytes.splice(at..at, esc);
            }
        }
    }
    bytes
}

fn check_bounds(t: &Terminal, name: &str, seed: u64) {
    let (rows, cols) = (t.rows(), t.cols());
    for alt in [false, true] {
        let b = t.buffer(alt);
        assert_eq!(b.rows.len(), rows, "{name} seed {seed}");
        assert!(
            b.rows.iter().all(|r| r.cells.len() == cols),
            "{name} seed {seed}"
        );
        assert!(
            b.cursor.row < rows && b.cursor.col < cols,
            "{name} seed {seed}"
        );
        assert!(b.top <= b.bottom && b.bottom < rows, "{name} seed {seed}");
    }
    assert!(t.scrollback_len() <= Limits::default().scrollback_lines);
    assert!(t.scrollback_cells() <= Limits::default().scrollback_cells);
    assert!(!t.title().chars().any(char::is_control));
}

fn mutation_run(iterations: u64) -> u64 {
    let streams: Vec<(String, Vec<u8>, usize, usize)> = names()
        .into_iter()
        .map(|n| {
            let meta: serde_json::Value = serde_json::from_slice(
                &std::fs::read(dir().join(format!("{n}.ref.json"))).unwrap(),
            )
            .unwrap();
            let bytes = std::fs::read(dir().join(format!("{n}.bin"))).unwrap();
            (
                n,
                bytes,
                meta["rows"].as_u64().unwrap() as usize,
                meta["cols"].as_u64().unwrap() as usize,
            )
        })
        .collect();
    let mut total = 0u64;
    for seed in 1..=iterations {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let (name, original, rows, cols) = &streams[rng.below(streams.len())];
        let bytes = mutate(&mut rng, original);
        let mut t = Terminal::new(*rows, *cols, limits());
        let mut at = 0;
        while at < bytes.len() {
            let n = 1 + rng.below(700);
            t.feed(&bytes[at..(at + n).min(bytes.len())]);
            at += n;
        }
        check_bounds(&t, name, seed);
        // The repaint of whatever came out is itself valid and redraws the same screen.
        let r = t.repaint(500).unwrap();
        assert!(r.bytes.len() <= wyss_console::screen::repaint::MAX_REPAINT);
        let mut again = Terminal::new(*rows, *cols, limits());
        again.feed(&r.bytes);
        check_bounds(&again, name, seed);
        assert_eq!(again.text(), t.text(), "{name} seed {seed}");
        assert_eq!(again.modes(), t.modes(), "{name} seed {seed}");
        total += bytes.len() as u64;
    }
    total
}

#[test]
fn mutated_real_streams_keep_every_bound_and_still_round_trip() {
    mutation_run(600);
}

/// `cargo test --release --test screen_corpus -- --ignored long_mutation_fuzz --nocapture`
#[test]
#[ignore = "long: run in release for the acceptance record"]
fn long_mutation_fuzz() {
    let n: u64 = std::env::var("FUZZ_ITERATIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100_000);
    let started = std::time::Instant::now();
    let bytes = mutation_run(n);
    eprintln!(
        "fuzz: {n} mutated streams, {bytes} bytes, {:?}",
        started.elapsed()
    );
}
