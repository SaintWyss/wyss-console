use super::*;

/// A minimal unified-diff applier: the oracle that the diff is exact.
fn apply(old: Option<&str>, diff: &str) -> String {
    let old_lines: Vec<&str> = lines(old.unwrap_or(""));
    let mut out = String::new();
    let mut pos = 0usize;
    let mut it = diff.lines().skip(2).peekable();
    let raw: Vec<&str> = diff.split_inclusive('\n').skip(2).collect();
    let _ = &mut it;
    let mut k = 0;
    while k < raw.len() {
        let h = raw[k];
        assert!(h.starts_with("@@ "), "{h:?}");
        let nums: Vec<usize> = h[3..]
            .split(' ')
            .take(2)
            .flat_map(|p| {
                p[1..]
                    .split(',')
                    .map(|n| n.parse::<usize>().unwrap())
                    .collect::<Vec<_>>()
            })
            .collect();
        let (astart, alen) = (nums[0], nums[1]);
        let target = if alen == 0 { astart } else { astart - 1 };
        while pos < target {
            out.push_str(old_lines[pos]);
            pos += 1;
        }
        k += 1;
        while k < raw.len() && !raw[k].starts_with("@@ ") {
            let l = raw[k];
            let body = &l[1..];
            let mut text = body.to_owned();
            if raw
                .get(k + 1)
                .is_some_and(|n| n.starts_with("\\ No newline"))
            {
                text = text.trim_end_matches('\n').to_owned();
                k += 1;
            }
            match &l[..1] {
                " " => {
                    assert_eq!(old_lines[pos], text);
                    out.push_str(&text);
                    pos += 1;
                }
                "-" => {
                    assert_eq!(old_lines[pos], text);
                    pos += 1;
                }
                "+" => out.push_str(&text),
                other => panic!("{other:?}"),
            }
            k += 1;
        }
    }
    while pos < old_lines.len() {
        out.push_str(old_lines[pos]);
        pos += 1;
    }
    out
}

fn check(old: Option<&str>, new: &str) -> String {
    let d = unified("/w/f.txt", old, new);
    assert_eq!(apply(old, &d), new, "diff was:\n{d}");
    d
}

#[test]
fn a_new_file_is_all_additions() {
    let d = check(None, "one\ntwo\n");
    assert!(
        d.starts_with("--- /dev/null\n+++ b/w/f.txt\n@@ -0,0 +1,2 @@\n+one\n+two\n"),
        "{d}"
    );
}

#[test]
fn an_unchanged_file_has_no_hunks() {
    let d = unified("/w/f", Some("a\nb\n"), "a\nb\n");
    assert_eq!(d, "--- a/w/f\n+++ b/w/f\n");
}

#[test]
fn a_one_line_change_keeps_three_lines_of_context() {
    let old: String = (1..=20).map(|i| format!("line {i}\n")).collect();
    let new = old.replace("line 10\n", "LINE TEN\n");
    let d = check(Some(&old), &new);
    assert!(d.contains("@@ -7,7 +7,7 @@"), "{d}");
    assert!(d.contains("-line 10\n+LINE TEN\n"));
}

#[test]
fn distant_changes_make_separate_hunks_and_close_ones_merge() {
    let old: String = (1..=40).map(|i| format!("l{i}\n")).collect();
    let far = old.replace("l3\n", "X\n").replace("l38\n", "Y\n");
    assert_eq!(check(Some(&old), &far).matches("@@ ").count(), 2);
    let near = old.replace("l3\n", "X\n").replace("l6\n", "Y\n");
    assert_eq!(check(Some(&old), &near).matches("@@ ").count(), 1);
}

#[test]
fn missing_final_newlines_are_marked_and_round_trip() {
    for (old, new) in [
        ("a\nb", "a\nb\n"),
        ("a\nb\n", "a\nb"),
        ("a\nb", "a\nc"),
        ("", "x"),
        ("x", ""),
        ("a", "a"),
    ] {
        check(Some(old), new);
    }
    assert!(unified("/f", Some("a"), "b").contains("\\ No newline at end of file"));
}

#[test]
fn deleting_everything_and_replacing_everything() {
    check(Some("a\nb\nc\n"), "");
    check(Some("a\nb\nc\n"), "x\ny\n");
    check(Some(""), "only\n");
}

#[test]
fn multibyte_and_crlf_text_round_trips() {
    check(Some("héllo\r\nwörld\r\n"), "héllo\r\nWÖRLD\r\n");
    check(Some("日本語\n"), "日本語\n中文\n");
}

#[test]
fn generated_edits_always_round_trip() {
    // A small deterministic generator: random line sets, random edits.
    let mut seed = 0x9e3779b97f4a7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..300 {
        let n = (next() % 30) as usize;
        let old: Vec<String> = (0..n).map(|_| format!("v{}\n", next() % 6)).collect();
        let mut new = old.clone();
        for _ in 0..(next() % 5) {
            match next() % 3 {
                0 if !new.is_empty() => {
                    let i = (next() as usize) % new.len();
                    new.remove(i);
                }
                1 => {
                    let i = (next() as usize) % (new.len() + 1);
                    new.insert(i, format!("v{}\n", next() % 6));
                }
                _ if !new.is_empty() => {
                    let i = (next() as usize) % new.len();
                    new[i] = format!("v{}\n", next() % 6);
                }
                _ => (),
            }
        }
        check(Some(&old.concat()), &new.concat());
    }
}

#[test]
fn a_huge_comparison_falls_back_to_a_whole_file_replacement_that_is_still_exact() {
    let old: String = (0..3000).map(|i| format!("a{i}\n")).collect();
    let new: String = (0..3000).map(|i| format!("b{i}\n")).collect();
    const { assert!(3000usize * 3000 > MAX_CELLS) };
    let d = check(Some(&old), &new);
    assert_eq!(d.matches("\n-a").count(), 3000);
}

#[test]
fn the_diff_is_deterministic() {
    let a = unified("/f", Some("a\nb\n"), "a\nc\n");
    assert_eq!(a, unified("/f", Some("a\nb\n"), "a\nc\n"));
}
