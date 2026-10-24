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

/// The most a layout file may be; a longer one is not a layout.
const MOST: u64 = 1024 * 1024;

/// What was found at the layout's path, and which file it was.
struct Seen {
    kind: Existing,
    id: (u64, u64, u64, i64, i64),
}

fn identity(meta: &std::fs::Metadata) -> (u64, u64, u64, i64, i64) {
    use std::os::unix::fs::MetadataExt;
    (
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.mtime(),
        meta.mtime_nsec(),
    )
}

/// Looks at the file without following a link and without trusting its size: `None` when there is
/// nothing; a link, a special file or someone else's file is refused (and left alone); a file too
/// long to be a layout is `Unreadable` and is kept, not ignored.
fn observe(path: &Path) -> Result<Option<Seen>, Refused> {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Refused::Io),
    };
    if !meta.is_file() || meta.uid() != nix::unistd::geteuid().as_raw() {
        return Err(Refused::Io);
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC | nix::libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| Refused::Io)?;
    let opened = file.metadata().map_err(|_| Refused::Io)?;
    if opened.ino() != meta.ino() || opened.dev() != meta.dev() || !opened.is_file() {
        return Err(Refused::Io);
    }
    let id = identity(&opened);
    if opened.len() > MOST {
        return Ok(Some(Seen {
            kind: Existing::Unreadable,
            id,
        }));
    }
    let mut bytes = Vec::new();
    file.take(MOST + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Refused::Io)?;
    let kind = if bytes.len() as u64 > MOST {
        Existing::Unreadable
    } else {
        classify(&bytes)
    };
    Ok(Some(Seen { kind, id }))
}

/// One writer at a time, across processes: an exclusive lock on a file beside the layout. The lock
/// file holds nothing and is never the layout.
fn locked(path: &Path) -> Result<std::fs::File, Refused> {
    use std::os::unix::{fs::OpenOptionsExt, io::AsRawFd};
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(path.with_file_name(name))
        .map_err(|_| Refused::Io)?;
    // SAFETY: flock on a descriptor this function owns.
    if unsafe { nix::libc::flock(file.as_raw_fd(), nix::libc::LOCK_EX) } != 0 {
        return Err(Refused::Io);
    }
    Ok(file)
}

/// Writes `new` as the layout at `path` unless a newer layout is there. An unreadable (or too long)
/// one is kept aside first (`layout.json.unreadable-<seconds>[-n]`, never over an earlier one).
///
/// Writers that use this function exclude each other. A writer that does not know the lock (a
/// different program) can still change the file between the last look and the replacement; the look
/// is repeated just before replacing and the write is abandoned if the file is not the one that was
/// judged, which leaves a window of one `rename`, and that is the limit of this guarantee.
pub fn write(path: &Path, new: &[u8], now: u64) -> Result<(), Refused> {
    let _lock = locked(path)?;
    let seen = observe(path)?;
    match seen.as_ref().map(|s| &s.kind) {
        Some(Existing::Newer) => return Err(Refused::Newer),
        Some(Existing::Unreadable) => keep_aside(path, now)?,
        Some(Existing::Current) | None => {}
    }
    replace(path, new, seen.as_ref().map(|s| s.id))
}

/// Writes the new file beside the layout and swaps it in, provided the layout is still the file that
/// was judged.
fn replace(
    path: &Path,
    new: &[u8],
    judged: Option<(u64, u64, u64, i64, i64)>,
) -> Result<(), Refused> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let dir = path.parent().ok_or(Refused::Io)?;
    let tmp = dir.join(format!(".tmp-{}", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(&tmp)
            .map_err(|_| Refused::Io)?;
        file.write_all(new).map_err(|_| Refused::Io)?;
        file.sync_all().map_err(|_| Refused::Io)?;
        // Not the file that was judged (or one appeared): not ours to replace.
        // (A file set aside is linked, not moved, so the layout is still where it was.)
        if std::fs::symlink_metadata(path).ok().map(|m| identity(&m)) != judged {
            return Err(Refused::Io);
        }
        std::fs::rename(&tmp, path).map_err(|_| Refused::Io)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Links the unreadable file under a name that is not taken. The live path is replaced by the
/// caller, so the file is never lost whichever step fails.
fn keep_aside(path: &Path, now: u64) -> Result<(), Refused> {
    for n in 0..1000u32 {
        let aside = aside_name(path, now, n);
        match std::fs::hard_link(path, &aside) {
            Ok(()) => {
                trim_aside(path);
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(Refused::Io),
        }
    }
    Err(Refused::Io)
}

fn aside_name(path: &Path, now: u64, n: u32) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    if n == 0 {
        name.push(format!(".unreadable-{now}"));
    } else {
        name.push(format!(".unreadable-{now}-{n}"));
    }
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
    let mut aside: Vec<((u64, u32), PathBuf)> = read
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let n = e.file_name().into_string().ok()?;
            let rest = n.strip_prefix(&prefix)?;
            let (stamp, counter) = match rest.split_once('-') {
                Some((stamp, counter)) => {
                    (stamp.parse::<u64>().ok()?, counter.parse::<u32>().ok()?)
                }
                None => (rest.parse::<u64>().ok()?, 0),
            };
            Some(((stamp, counter), e.path()))
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
        let beside: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n != "layout.json" && n != "layout.json.lock")
            .collect();
        assert!(
            beside.is_empty(),
            "nothing was written beside it either: {beside:?}"
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
    fn a_layout_too_long_to_be_one_is_kept_aside_not_taken_for_absent() {
        let d = dir();
        let path = d.join("layout.json");
        let big = vec![b' '; (MOST as usize) + 10];
        std::fs::write(&path, &big).unwrap();
        write(&path, br#"{"version":1,"tabs":[]}"#, 77).unwrap();
        assert_eq!(classify(&std::fs::read(&path).unwrap()), Existing::Current);
        assert_eq!(
            std::fs::read(d.join("layout.json.unreadable-77")).unwrap(),
            big
        );
    }

    #[test]
    fn a_set_aside_name_is_never_reused_so_an_earlier_one_is_never_lost() {
        let d = dir();
        let path = d.join("layout.json");
        for n in 0..3 {
            std::fs::write(&path, format!("broken {n}")).unwrap();
            write(&path, br#"{"version":1,"tabs":[]}"#, 500).unwrap();
        }
        let mut kept: Vec<(String, Vec<u8>)> = std::fs::read_dir(&d)
            .unwrap()
            .filter_map(|e| {
                let e = e.ok()?;
                let name = e.file_name().into_string().ok()?;
                name.contains("unreadable")
                    .then(|| (name, std::fs::read(e.path()).unwrap()))
            })
            .collect();
        kept.sort();
        assert_eq!(
            kept,
            [
                (
                    "layout.json.unreadable-500".to_owned(),
                    b"broken 0".to_vec()
                ),
                (
                    "layout.json.unreadable-500-1".to_owned(),
                    b"broken 1".to_vec()
                ),
                (
                    "layout.json.unreadable-500-2".to_owned(),
                    b"broken 2".to_vec()
                ),
            ]
        );
    }

    #[test]
    fn a_link_or_a_special_file_in_the_layouts_place_is_refused_and_left_alone() {
        let d = dir();
        let target = d.join("elsewhere.json");
        std::fs::write(&target, br#"{"version":9}"#).unwrap();
        let path = d.join("layout.json");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert_eq!(
            write(&path, br#"{"version":1,"tabs":[]}"#, 1),
            Err(Refused::Io)
        );
        assert!(
            std::fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read(&target).unwrap(), br#"{"version":9}"#);
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert_eq!(
            write(&path, br#"{"version":1,"tabs":[]}"#, 2),
            Err(Refused::Io)
        );
        assert!(path.is_dir());
    }

    #[test]
    fn writers_exclude_each_other_and_the_file_is_always_a_whole_layout() {
        let d = dir();
        let path = d.join("layout.json");
        let handles: Vec<_> = (0..8u64)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for j in 0..20u64 {
                        let body = format!(r#"{{"version":1,"tabs":[{}]}}"#, i * 100 + j);
                        write(&path, body.as_bytes(), 1).unwrap();
                        let seen = std::fs::read(&path).unwrap();
                        assert_eq!(classify(&seen), Existing::Current);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(classify(&std::fs::read(&path).unwrap()), Existing::Current);
        let strays: Vec<_> = std::fs::read_dir(&d)
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.starts_with(".tmp-") || n.contains("unreadable"))
            .collect();
        assert!(strays.is_empty(), "{strays:?}");
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
        assert_eq!(
            std::fs::read_dir(&d).unwrap().count(),
            2,
            "the layout and its lock"
        );
    }
}
