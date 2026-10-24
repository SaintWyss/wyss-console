//! The saved layout of tabs and panes (T2) and what a write may do to a file that is already there:
//! a layout written by a **newer** Console is never touched, and one that cannot be read is kept
//! beside it (renamed, not deleted) before a new one is written, so a usable, empty view comes back
//! without losing what was found. Only the layout is stored here: no terminal bytes, no command.

use std::path::{Path, PathBuf};

/// The layout version this Console reads and writes.
pub const VERSION: u64 = 1;
/// Kept corrupt layouts beside the live one; older ones are removed when a newer one is set aside.
const KEEP_ASIDE: usize = 3;

#[derive(Debug, PartialEq, Eq)]
pub enum Existing {
    Current,
    /// Written by a newer version: left exactly as it is.
    Newer,
    /// Not a layout this version can read.
    Unreadable,
}

pub fn classify(bytes: &[u8]) -> Existing {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return Existing::Unreadable;
    };
    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(v) if v > VERSION => Existing::Newer,
        Some(VERSION) if value.is_object() => Existing::Current,
        _ => Existing::Unreadable,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// A newer Console's layout is there and stays.
    Newer,
    Io,
}

/// Writes `new` as the layout at `path` unless a newer layout is there. An unreadable one is moved
/// aside first (`layout.json.unreadable-<seconds>`).
pub fn write(path: &Path, new: &[u8], now: u64) -> Result<(), Refused> {
    let existing = crate::native::store::read_bounded(path, 1024 * 1024);
    match existing.as_deref().map(classify) {
        Some(Existing::Newer) => return Err(Refused::Newer),
        Some(Existing::Unreadable) => {
            let aside = aside_name(path, now);
            std::fs::rename(path, &aside).map_err(|_| Refused::Io)?;
            trim_aside(path);
        }
        Some(Existing::Current) | None => {}
    }
    crate::native::store::write_atomic(path, new).map_err(|_| Refused::Io)
}

fn aside_name(path: &Path, now: u64) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".unreadable-{now}"));
    path.with_file_name(name)
}

fn trim_aside(path: &Path) {
    let Some(dir) = path.parent() else { return };
    let Some(base) = path.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let prefix = format!("{base}.unreadable-");
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut aside: Vec<(u64, PathBuf)> = read
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let n = e.file_name().into_string().ok()?;
            let stamp = n.strip_prefix(&prefix)?.parse::<u64>().ok()?;
            Some((stamp, e.path()))
        })
        .collect();
    aside.sort();
    while aside.len() > KEEP_ASIDE {
        let (_, old) = aside.remove(0);
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("wyss-layout-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
        d
    }

    #[test]
    fn a_file_is_classified_by_what_it_says_never_by_its_name() {
        assert_eq!(classify(br#"{"version":1,"tabs":[]}"#), Existing::Current);
        assert_eq!(
            classify(br#"{"version":2,"tabs":[],"future":{}}"#),
            Existing::Newer
        );
        assert_eq!(classify(br#"{"version":99}"#), Existing::Newer);
        for bad in [
            &b""[..],
            b"{",
            b"[]",
            b"null",
            br#"{"version":0}"#,
            br#"{"version":"1"}"#,
            br#"{"tabs":[]}"#,
            b"\xff\xfe",
        ] {
            assert_eq!(classify(bad), Existing::Unreadable, "{bad:?}");
        }
    }

    #[test]
    fn a_newer_layout_is_never_overwritten_and_stays_byte_for_byte() {
        let d = dir();
        let path = d.join("layout.json");
        let newer = br#"{"version":3,"tabs":[],"from":"a later Console"}"#;
        std::fs::write(&path, newer).unwrap();
        assert_eq!(
            write(&path, br#"{"version":1,"tabs":[]}"#, 10),
            Err(Refused::Newer)
        );
        assert_eq!(std::fs::read(&path).unwrap(), newer);
        assert_eq!(
            std::fs::read_dir(&d).unwrap().count(),
            1,
            "nothing was written beside it either"
        );
    }

    #[test]
    fn an_unreadable_layout_is_kept_aside_and_a_usable_one_written() {
        let d = dir();
        let path = d.join("layout.json");
        std::fs::write(&path, b"{ this is not json").unwrap();
        write(&path, br#"{"version":1,"tabs":[]}"#, 1_700_000_000).unwrap();
        assert_eq!(classify(&std::fs::read(&path).unwrap()), Existing::Current);
        assert_eq!(
            std::fs::read(d.join("layout.json.unreadable-1700000000")).unwrap(),
            b"{ this is not json"
        );
    }

    #[test]
    fn only_a_few_unreadable_layouts_are_kept() {
        let d = dir();
        let path = d.join("layout.json");
        for n in 1..=6u64 {
            std::fs::write(&path, format!("broken {n}")).unwrap();
            write(&path, br#"{"version":1,"tabs":[]}"#, 100 + n).unwrap();
        }
        let mut kept: Vec<String> = std::fs::read_dir(&d)
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.contains("unreadable"))
            .collect();
        kept.sort();
        assert_eq!(
            kept,
            [
                "layout.json.unreadable-104",
                "layout.json.unreadable-105",
                "layout.json.unreadable-106"
            ]
        );
    }

    #[test]
    fn a_current_or_missing_layout_is_simply_replaced() {
        let d = dir();
        let path = d.join("layout.json");
        write(&path, br#"{"version":1,"tabs":[1]}"#, 1).unwrap();
        write(&path, br#"{"version":1,"tabs":[2]}"#, 2).unwrap();
        assert!(
            String::from_utf8(std::fs::read(&path).unwrap())
                .unwrap()
                .contains("[2]")
        );
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
    }
}
