//! A tab's layout is a tree of splits whose leaves are terminals.
//! The layout operations work on data alone, so the shape of a tab can be
//! tested without a window: how a leaf is split, removed, resized and found, where each leaf sits
//! (as fractions of the tab's area), which pane is next to which, and how the layout of all tabs is
//! written down, read back and reconciled with the terminals the host still has.

use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// Side by side.
    Row,
    /// One above the other.
    Col,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Leaf(String),
    Split { dir: Dir, ratio: f64, a: Box<Node>, b: Box<Node> },
}

/// A pane is never given less than this share of a split.
pub const MIN_RATIO: f64 = 0.1;
/// A tab holds at most this many panes.
pub const MAX_PANES: usize = 16;
/// Splits nest at most this deep.
pub const MAX_DEPTH: usize = 6;
/// At most this many tabs are kept.
pub const MAX_TABS: usize = 16;
pub const LAYOUT_VERSION: u64 = 1;

pub fn leaf(id: &str) -> Node {
    Node::Leaf(id.to_owned())
}

fn clamp(r: f64) -> f64 {
    r.clamp(MIN_RATIO, 1.0 - MIN_RATIO)
}

pub fn leaves(node: &Node) -> Vec<String> {
    match node {
        Node::Leaf(id) => vec![id.clone()],
        Node::Split { a, b, .. } => {
            let mut v = leaves(a);
            v.extend(leaves(b));
            v
        }
    }
}

pub fn has(node: &Node, id: &str) -> bool {
    leaves(node).iter().any(|l| l == id)
}

fn depth(node: &Node) -> usize {
    match node {
        Node::Leaf(_) => 0,
        Node::Split { a, b, .. } => 1 + depth(a).max(depth(b)),
    }
}

/// `target` becomes a split of itself and `added`, `added` second. Refused (the same tree comes
/// back) when the tab is full, would nest too deep, or does not hold `target`.
pub fn split(root: &Node, target: &str, added: &str, dir: Dir) -> Node {
    if leaves(root).len() >= MAX_PANES || !has(root, target) {
        return root.clone();
    }
    fn go(n: &Node, target: &str, added: &str, dir: Dir) -> Node {
        match n {
            Node::Leaf(id) if id == target => Node::Split { dir, ratio: 0.5, a: Box::new(n.clone()), b: Box::new(leaf(added)) },
            Node::Leaf(_) => n.clone(),
            Node::Split { dir: d, ratio, a, b } => Node::Split { dir: *d, ratio: *ratio, a: Box::new(go(a, target, added, dir)), b: Box::new(go(b, target, added, dir)) },
        }
    }
    let result = go(root, target, added, dir);
    if depth(&result) > MAX_DEPTH {
        root.clone()
    } else {
        result
    }
}

/// The tree without `id`: its sibling takes the whole of the space. `None` when none is left.
pub fn remove(root: &Node, id: &str) -> Option<Node> {
    match root {
        Node::Leaf(l) => (l != id).then(|| root.clone()),
        Node::Split { dir, ratio, a, b } => match (remove(a, id), remove(b, id)) {
            (None, b) => b,
            (a, None) => a,
            (Some(a), Some(b)) => Some(Node::Split { dir: *dir, ratio: *ratio, a: Box::new(a), b: Box::new(b) }),
        },
    }
}

/// The share of the split at `path` (`""` is the root, then `a` or `b` per level), clamped.
pub fn resize(root: &Node, path: &str, ratio: f64) -> Node {
    fn go(n: &Node, here: &str, path: &str, ratio: f64) -> Node {
        match n {
            Node::Leaf(_) => n.clone(),
            Node::Split { dir, ratio: r, a, b } => {
                if here == path {
                    return Node::Split { dir: *dir, ratio: clamp(ratio), a: a.clone(), b: b.clone() };
                }
                Node::Split { dir: *dir, ratio: *r, a: Box::new(go(a, &format!("{here}a"), path, ratio)), b: Box::new(go(b, &format!("{here}b"), path, ratio)) }
            }
        }
    }
    go(root, "", path, ratio)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Where every leaf sits, in reading order, as fractions of the tab's area (0 to 1).
pub fn arrange(root: &Node) -> Vec<(String, Rect)> {
    fn go(n: &Node, area: Rect, out: &mut Vec<(String, Rect)>) {
        match n {
            Node::Leaf(id) => out.push((id.clone(), area)),
            Node::Split { dir: Dir::Row, ratio, a, b } => {
                let w = area.w * ratio;
                go(a, Rect { w, ..area }, out);
                go(b, Rect { x: area.x + w, w: area.w - w, ..area }, out);
            }
            Node::Split { dir: Dir::Col, ratio, a, b } => {
                let h = area.h * ratio;
                go(a, Rect { h, ..area }, out);
                go(b, Rect { y: area.y + h, h: area.h - h, ..area }, out);
            }
        }
    }
    let mut out = Vec::new();
    go(root, Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }, &mut out);
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Way {
    Left,
    Right,
    Up,
    Down,
}

/// The pane next to `from` in a direction, by where the panes sit: the nearest whose edge touches
/// the way out and whose span overlaps `from`'s middle.
pub fn neighbour(root: &Node, from: &str, way: Way) -> Option<String> {
    const E: f64 = 1e-9;
    let panes = arrange(root);
    let here = panes.iter().find(|(id, _)| id == from)?.1;
    let (cx, cy) = (here.x + here.w / 2.0, here.y + here.h / 2.0);
    let mut best: Option<(&String, f64)> = None;
    for (id, r) in &panes {
        if id == from {
            continue;
        }
        let gap = match way {
            Way::Right => r.x - (here.x + here.w),
            Way::Left => here.x - (r.x + r.w),
            Way::Down => r.y - (here.y + here.h),
            Way::Up => here.y - (r.y + r.h),
        };
        let across = match way {
            Way::Left | Way::Right => cy >= r.y - E && cy <= r.y + r.h + E,
            Way::Up | Way::Down => cx >= r.x - E && cx <= r.x + r.w + E,
        };
        if gap >= -E && across && best.is_none_or(|(_, g)| gap < g) {
            best = Some((id, gap));
        }
    }
    best.map(|(id, _)| id.clone())
}

/// The next (`step` 1) or previous (-1) leaf in reading order, wrapping.
pub fn cycle(root: &Node, from: &str, step: i32) -> Option<String> {
    let all = leaves(root);
    let at = all.iter().position(|l| l == from)? as i32;
    let n = all.len() as i32;
    Some(all[((at + step).rem_euclid(n)) as usize].clone())
}

// ---- writing a layout down and reading it back --------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct SavedTab {
    /// The number in "Terminal N".
    pub n: u64,
    pub root: Node,
    /// The terminal that had the focus.
    pub focus: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SavedLayout {
    pub tabs: Vec<SavedTab>,
    pub active: usize,
}

fn node_json(n: &Node) -> Value {
    match n {
        Node::Leaf(id) => json!({"kind": "leaf", "id": id}),
        Node::Split { dir, ratio, a, b } => json!({
            "kind": "split",
            "dir": if *dir == Dir::Row { "row" } else { "col" },
            "ratio": ratio,
            "a": node_json(a),
            "b": node_json(b),
        }),
    }
}

pub fn to_json(l: &SavedLayout) -> Value {
    json!({
        "version": LAYOUT_VERSION,
        "active": l.active,
        "tabs": l.tabs.iter().map(|t| json!({"n": t.n, "focus": t.focus, "root": node_json(&t.root)})).collect::<Vec<_>>(),
    })
}

fn read_node(v: &Value, level: usize, seen: &mut Vec<String>) -> Option<Node> {
    if level > MAX_DEPTH {
        return None;
    }
    match v["kind"].as_str()? {
        "leaf" => {
            let id = v["id"].as_str().filter(|s| !s.is_empty() && s.len() <= 64)?;
            if seen.iter().any(|s| s == id) {
                return None;
            }
            seen.push(id.to_owned());
            Some(leaf(id))
        }
        "split" => {
            let dir = match v["dir"].as_str()? {
                "row" => Dir::Row,
                "col" => Dir::Col,
                _ => return None,
            };
            let ratio = v["ratio"].as_f64().filter(|r| r.is_finite())?;
            Some(Node::Split { dir, ratio: clamp(ratio), a: Box::new(read_node(&v["a"], level + 1, seen)?), b: Box::new(read_node(&v["b"], level + 1, seen)?) })
        }
        _ => None,
    }
}

/// A saved layout, or `None` when what was read is not one. Bounded in tabs, panes and nesting;
/// an id may appear only once; nothing in it is ever used as a command.
pub fn read_layout(v: &Value) -> Option<SavedLayout> {
    if v["version"].as_u64()? != LAYOUT_VERSION {
        return None;
    }
    let raw = v["tabs"].as_array().filter(|t| t.len() <= MAX_TABS)?;
    let mut seen: Vec<String> = Vec::new();
    let mut tabs = Vec::new();
    for (i, t) in raw.iter().enumerate() {
        let before = seen.len();
        let root = read_node(&t["root"], 0, &mut seen)?;
        if seen.len() - before > MAX_PANES {
            return None;
        }
        let focus = t["focus"].as_str().filter(|f| has(&root, f)).map(str::to_owned).unwrap_or_else(|| leaves(&root)[0].clone());
        tabs.push(SavedTab { n: t["n"].as_u64().filter(|n| *n > 0).unwrap_or(i as u64 + 1), root, focus });
    }
    let active = v["active"].as_u64().map(|a| a as usize).filter(|a| *a < tabs.len()).unwrap_or(0);
    Some(SavedLayout { tabs, active })
}

/// What opening the window shows: the saved layout without the terminals that are gone, plus a
/// new tab for each live terminal the layout did not know. No tab is left empty. `next_n` is the
/// number the next new tab takes.
pub fn restore(saved: Option<&SavedLayout>, alive: &[String]) -> (SavedLayout, u64) {
    let mut tabs: Vec<SavedTab> = Vec::new();
    let mut active_id: Option<usize> = None;
    if let Some(s) = saved {
        for (i, t) in s.tabs.iter().enumerate() {
            let mut root = Some(t.root.clone());
            for id in leaves(&t.root) {
                if !alive.contains(&id) {
                    root = root.and_then(|r| remove(&r, &id));
                }
            }
            if let Some(root) = root {
                let focus = if has(&root, &t.focus) { t.focus.clone() } else { leaves(&root)[0].clone() };
                if i == s.active {
                    active_id = Some(tabs.len());
                }
                tabs.push(SavedTab { n: t.n, root, focus });
            }
        }
    }
    let known: Vec<String> = tabs.iter().flat_map(|t| leaves(&t.root)).collect();
    let mut next_n = tabs.iter().map(|t| t.n).max().unwrap_or(0) + 1;
    for id in alive.iter().filter(|a| !known.contains(a)) {
        if tabs.len() >= MAX_TABS {
            break;
        }
        tabs.push(SavedTab { n: next_n, root: leaf(id), focus: id.clone() });
        next_n += 1;
    }
    let active = active_id.unwrap_or(0).min(tabs.len().saturating_sub(1));
    (SavedLayout { tabs, active }, next_n)
}

/// Layout file in the `gtk-term` state directory under `$XDG_STATE_HOME` (default `~/.local/state`).
pub fn state_path() -> Option<std::path::PathBuf> {
    let base = match std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
        Some(p) => std::path::PathBuf::from(p),
        None => std::path::PathBuf::from(std::env::var_os("HOME")?).join(".local/state"),
    };
    Some(base.join("gtk-term/layout.json"))
}

pub fn load(path: &std::path::Path) -> Option<SavedLayout> {
    read_layout(&serde_json::from_slice(&std::fs::read(path).ok()?).ok()?)
}

/// Writes the layout atomically (temp file + rename), folder 0700, file 0600.
pub fn save(path: &std::path::Path, l: &SavedLayout) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let dir = path.parent().ok_or_else(|| std::io::Error::other("no parent"))?;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let tmp = dir.join(format!(".layout.{}.tmp", std::process::id()));
    {
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(to_json(l).to_string().as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(root: &Node, target: &str, added: &str, dir: Dir) -> Node {
        split(root, target, added, dir)
    }

    /// a | (b over c)
    fn three() -> Node {
        let t = s(&leaf("a"), "a", "b", Dir::Row);
        s(&t, "b", "c", Dir::Col)
    }

    #[test]
    fn split_makes_the_added_leaf_second() {
        let t = s(&leaf("a"), "a", "b", Dir::Row);
        assert_eq!(leaves(&t), ["a", "b"]);
        assert_eq!(t, Node::Split { dir: Dir::Row, ratio: 0.5, a: Box::new(leaf("a")), b: Box::new(leaf("b")) });
    }

    #[test]
    fn split_of_an_unknown_target_changes_nothing() {
        let t = leaf("a");
        assert_eq!(s(&t, "zzz", "b", Dir::Row), t);
    }

    #[test]
    fn split_refuses_past_the_pane_limit() {
        // Always split the biggest pane: the tree stays balanced (depth 4 for 16 panes).
        let mut t = leaf("p0");
        for i in 1..MAX_PANES {
            let biggest = arrange(&t).into_iter().max_by(|x, y| (x.1.w * x.1.h).total_cmp(&(y.1.w * y.1.h)).then(y.0.cmp(&x.0))).unwrap().0;
            t = s(&t, &biggest, &format!("p{i}"), if i % 2 == 0 { Dir::Row } else { Dir::Col });
        }
        assert_eq!(leaves(&t).len(), MAX_PANES);
        assert_eq!(s(&t, "p0", "extra", Dir::Row), t);
    }

    #[test]
    fn split_refuses_past_the_depth_limit() {
        let mut t = leaf("p0");
        let mut last = "p0".to_owned();
        for i in 1..=MAX_DEPTH {
            let next = format!("p{i}");
            t = s(&t, &last, &next, Dir::Row);
            last = next;
        }
        assert_eq!(depth(&t), MAX_DEPTH);
        assert_eq!(s(&t, &last, "deeper", Dir::Row), t);
    }

    #[test]
    fn remove_gives_the_space_to_the_sibling() {
        assert_eq!(remove(&three(), "a").unwrap(), s(&leaf("b"), "b", "c", Dir::Col));
        assert_eq!(leaves(&remove(&three(), "b").unwrap()), ["a", "c"]);
        assert_eq!(remove(&leaf("a"), "a"), None);
        assert_eq!(remove(&leaf("a"), "x"), Some(leaf("a")));
    }

    #[test]
    fn resize_clamps_and_targets_the_path() {
        let t = three();
        let Node::Split { ratio, .. } = resize(&t, "", 0.7) else { panic!() };
        assert_eq!(ratio, 0.7);
        let Node::Split { ratio, .. } = resize(&t, "", 0.0) else { panic!() };
        assert_eq!(ratio, MIN_RATIO);
        let Node::Split { ratio, .. } = resize(&t, "", 5.0) else { panic!() };
        assert_eq!(ratio, 1.0 - MIN_RATIO);
        let Node::Split { b, ratio: outer, .. } = resize(&t, "b", 0.25) else { panic!() };
        assert_eq!(outer, 0.5);
        let Node::Split { ratio, .. } = *b else { panic!() };
        assert_eq!(ratio, 0.25);
    }

    #[test]
    fn arrange_gives_fractions_of_the_area() {
        let p = arrange(&three());
        let r = |id: &str| p.iter().find(|(i, _)| i == id).unwrap().1;
        assert_eq!(r("a"), Rect { x: 0.0, y: 0.0, w: 0.5, h: 1.0 });
        assert_eq!(r("b"), Rect { x: 0.5, y: 0.0, w: 0.5, h: 0.5 });
        assert_eq!(r("c"), Rect { x: 0.5, y: 0.5, w: 0.5, h: 0.5 });
    }

    #[test]
    fn neighbour_follows_where_panes_sit() {
        let t = three();
        assert_eq!(neighbour(&t, "a", Way::Right).as_deref(), Some("b"), "the middle of a is at 0.5: touches b's edge");
        assert_eq!(neighbour(&t, "b", Way::Left).as_deref(), Some("a"));
        assert_eq!(neighbour(&t, "c", Way::Left).as_deref(), Some("a"));
        assert_eq!(neighbour(&t, "b", Way::Down).as_deref(), Some("c"));
        assert_eq!(neighbour(&t, "c", Way::Up).as_deref(), Some("b"));
        assert_eq!(neighbour(&t, "a", Way::Left), None);
        assert_eq!(neighbour(&t, "a", Way::Up), None);
        assert_eq!(neighbour(&t, "c", Way::Right), None);
        assert_eq!(neighbour(&t, "zzz", Way::Right), None);
    }

    #[test]
    fn cycle_wraps_in_reading_order() {
        let t = three();
        assert_eq!(cycle(&t, "a", 1).as_deref(), Some("b"));
        assert_eq!(cycle(&t, "c", 1).as_deref(), Some("a"));
        assert_eq!(cycle(&t, "a", -1).as_deref(), Some("c"));
        assert_eq!(cycle(&t, "zzz", 1), None);
        assert_eq!(cycle(&leaf("a"), "a", 1).as_deref(), Some("a"));
    }

    fn saved() -> SavedLayout {
        SavedLayout {
            tabs: vec![SavedTab { n: 1, root: resize(&three(), "", 0.3), focus: "b".into() }, SavedTab { n: 4, root: leaf("d"), focus: "d".into() }],
            active: 1,
        }
    }

    #[test]
    fn a_layout_survives_writing_and_reading() {
        let l = saved();
        assert_eq!(read_layout(&to_json(&l)), Some(l));
    }

    #[test]
    fn reading_rejects_what_is_not_a_layout() {
        assert_eq!(read_layout(&json!(null)), None);
        assert_eq!(read_layout(&json!({"version": 2, "tabs": []})), None);
        assert_eq!(read_layout(&json!({"version": 1, "tabs": "x"})), None);
        // The same id twice (in one tab or across tabs) is refused.
        let dup = json!({"version": 1, "tabs": [{"root": {"kind":"leaf","id":"a"}}, {"root": {"kind":"leaf","id":"a"}}]});
        assert_eq!(read_layout(&dup), None);
        let bad_dir = json!({"version": 1, "tabs": [{"root": {"kind":"split","dir":"diag","ratio":0.5,"a":{"kind":"leaf","id":"a"},"b":{"kind":"leaf","id":"b"}}}]});
        assert_eq!(read_layout(&bad_dir), None);
        let too_many = json!({"version": 1, "tabs": (0..MAX_TABS + 1).map(|i| json!({"root": {"kind":"leaf","id": format!("t{i}")}})).collect::<Vec<_>>()});
        assert_eq!(read_layout(&too_many), None);
    }

    #[test]
    fn reading_repairs_focus_active_and_ratio() {
        let v = json!({"version": 1, "active": 9, "tabs": [{"focus": "nope", "root": {"kind":"split","dir":"row","ratio":0.0,"a":{"kind":"leaf","id":"a"},"b":{"kind":"leaf","id":"b"}}}]});
        let l = read_layout(&v).unwrap();
        assert_eq!(l.active, 0);
        assert_eq!(l.tabs[0].focus, "a");
        assert_eq!(l.tabs[0].n, 1);
        let Node::Split { ratio, .. } = &l.tabs[0].root else { panic!() };
        assert_eq!(*ratio, MIN_RATIO);
    }

    #[test]
    fn reading_bounds_the_depth() {
        let mut v = json!({"kind": "leaf", "id": "x0"});
        for i in 1..=MAX_DEPTH + 2 {
            v = json!({"kind": "split", "dir": "row", "ratio": 0.5, "a": v, "b": {"kind": "leaf", "id": format!("x{i}")}});
        }
        assert_eq!(read_layout(&json!({"version": 1, "tabs": [{"root": v}]})), None);
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn restore_drops_dead_terminals_and_keeps_the_shape() {
        let (l, next) = restore(Some(&saved()), &ids(&["a", "c", "d"]));
        assert_eq!(l.tabs.len(), 2);
        assert_eq!(leaves(&l.tabs[0].root), ["a", "c"]);
        assert_eq!(l.tabs[0].focus, "a", "the focused terminal died: the first one takes the focus");
        assert_eq!(l.active, 1);
        assert_eq!(next, 5);
    }

    #[test]
    fn restore_drops_a_tab_whose_terminals_are_all_gone_and_moves_active() {
        let (l, _) = restore(Some(&saved()), &ids(&["d"]));
        assert_eq!(l.tabs.len(), 1);
        assert_eq!(l.tabs[0].n, 4);
        assert_eq!(l.active, 0);
    }

    #[test]
    fn restore_puts_unknown_live_terminals_in_new_tabs() {
        let (l, next) = restore(Some(&saved()), &ids(&["a", "b", "c", "d", "x", "y"]));
        assert_eq!(l.tabs.len(), 4);
        assert_eq!(l.tabs[2].root, leaf("x"));
        assert_eq!((l.tabs[2].n, l.tabs[3].n, next), (5, 6, 7));
    }

    #[test]
    fn restore_without_a_saved_layout_makes_one_tab_per_live_terminal() {
        let (l, next) = restore(None, &ids(&["p", "q"]));
        assert_eq!(l.tabs.iter().map(|t| leaves(&t.root)[0].clone()).collect::<Vec<_>>(), ["p", "q"]);
        assert_eq!((l.active, next), (0, 3));
        let (empty, _) = restore(None, &[]);
        assert!(empty.tabs.is_empty());
    }

    #[test]
    fn save_and_load_round_trip_on_disk_with_private_modes() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("layout-test-{}", std::process::id()));
        let path = dir.join("gtk-term/layout.json");
        let _ = std::fs::remove_dir_all(&dir);
        save(&path, &saved()).unwrap();
        assert_eq!(load(&path), Some(saved()));
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load(&path), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
