//! A unified diff of one whole file, for the approval preview of `write_file`.
//!
//! The preview is complete or it does not exist: callers refuse a proposal whose diff would not
//! fit the approval contract, they never show part of it. The diff is exact (applying it to the
//! old text yields the new text; the tests check that on generated inputs) and deterministic.
//! When the line-by-line comparison would be too large it falls back to replacing the whole
//! file, which is longer but equally exact.

const CONTEXT: usize = 3;
/// Largest `old_lines * new_lines` the comparison table may reach.
const MAX_CELLS: usize = 4_000_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Keep,
    Del,
    Add,
}

fn lines(s: &str) -> Vec<&str> {
    s.split_inclusive('\n').collect()
}

fn script<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(Op, &'a str)> {
    let (n, m) = (old.len(), new.len());
    // Trim the common head and tail first: most edits touch a small part of a file.
    let head = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (o, nw) = (&old[head..n - tail], &new[head..m - tail]);
    let mut out: Vec<(Op, &str)> = old[..head].iter().map(|l| (Op::Keep, *l)).collect();
    if o.is_empty() || nw.is_empty() || o.len().saturating_mul(nw.len()) > MAX_CELLS {
        out.extend(o.iter().map(|l| (Op::Del, *l)));
        out.extend(nw.iter().map(|l| (Op::Add, *l)));
    } else {
        // Longest common subsequence over the changed middle.
        let w = nw.len() + 1;
        let mut t = vec![0u32; (o.len() + 1) * w];
        for i in (0..o.len()).rev() {
            for j in (0..nw.len()).rev() {
                t[i * w + j] = if o[i] == nw[j] {
                    t[(i + 1) * w + j + 1] + 1
                } else {
                    t[(i + 1) * w + j].max(t[i * w + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < o.len() && j < nw.len() {
            if o[i] == nw[j] {
                out.push((Op::Keep, o[i]));
                i += 1;
                j += 1;
            } else if t[(i + 1) * w + j] >= t[i * w + j + 1] {
                out.push((Op::Del, o[i]));
                i += 1;
            } else {
                out.push((Op::Add, nw[j]));
                j += 1;
            }
        }
        out.extend(o[i..].iter().map(|l| (Op::Del, *l)));
        out.extend(nw[j..].iter().map(|l| (Op::Add, *l)));
    }
    out.extend(old[n - tail..].iter().map(|l| (Op::Keep, *l)));
    out
}

fn push_line(out: &mut String, prefix: char, line: &str) {
    out.push(prefix);
    out.push_str(line);
    if !line.ends_with('\n') {
        out.push_str("\n\\ No newline at end of file\n");
    }
}

/// `old` is `None` for a file that does not exist yet. Paths are shown as given.
pub fn unified(path: &str, old: Option<&str>, new: &str) -> String {
    let old_text = old.unwrap_or("");
    let (ol, nl) = (lines(old_text), lines(new));
    let ops = script(&ol, &nl);
    let mut out = String::new();
    out.push_str(&match old {
        Some(_) => format!("--- a{path}\n"),
        None => "--- /dev/null\n".to_owned(),
    });
    out.push_str(&format!("+++ b{path}\n"));
    // Indices of changed operations, then hunks of changes joined when their context touches.
    let changed: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, (op, _))| *op != Op::Keep)
        .map(|(i, _)| i)
        .collect();
    if changed.is_empty() {
        return out;
    }
    let mut hunks: Vec<(usize, usize)> = Vec::new();
    for &i in &changed {
        let (lo, hi) = (i.saturating_sub(CONTEXT), (i + CONTEXT + 1).min(ops.len()));
        match hunks.last_mut() {
            Some(last) if lo <= last.1 => last.1 = hi,
            _ => hunks.push((lo, hi)),
        }
    }
    // Line numbers before each operation.
    let mut before = Vec::with_capacity(ops.len() + 1);
    let (mut a, mut b) = (0usize, 0usize);
    for (op, _) in &ops {
        before.push((a, b));
        match op {
            Op::Keep => {
                a += 1;
                b += 1;
            }
            Op::Del => a += 1,
            Op::Add => b += 1,
        }
    }
    before.push((a, b));
    for (lo, hi) in hunks {
        let (a0, b0) = before[lo];
        let (a1, b1) = before[hi];
        let (alen, blen) = (a1 - a0, b1 - b0);
        let start = |s: usize, len: usize| if len == 0 { s } else { s + 1 };
        out.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            start(a0, alen),
            alen,
            start(b0, blen),
            blen
        ));
        for (op, line) in &ops[lo..hi] {
            push_line(
                &mut out,
                match op {
                    Op::Keep => ' ',
                    Op::Del => '-',
                    Op::Add => '+',
                },
                line,
            );
        }
    }
    out
}

#[cfg(test)]
mod tests;
