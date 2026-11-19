use super::{OPEN_FLAGS, Project, bounded, detect, git, short};
use nix::fcntl::{OFlag, openat, renameat};
use nix::sys::stat::{Mode, mkdirat};
use nix::unistd::{UnlinkatFlags, geteuid, unlinkat};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SECTION_LIMIT: usize = 64 << 10;
const META_LIMIT: usize = 64 << 10;
const EXTRA_LIMIT: usize = 6;

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn cache_path(project: &Project) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".cache")))?;
    Some(
        base.join("wyss-console/panels")
            .join(hash(project.root.as_os_str().as_bytes())),
    )
}

pub fn watch_path(path: &Path) -> Option<PathBuf> {
    for (distance, candidate) in path.ancestors().enumerate() {
        let private = 3usize.saturating_sub(distance);
        match open_cache(candidate, private, false) {
            Ok(_) => return Some(candidate.to_path_buf()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }
    }
    None
}

#[derive(Clone, Deserialize, Serialize)]
struct Entry {
    root: PathBuf,
    head: Option<String>,
    written_at: u64,
    title: Option<String>,
    digest: String,
    truncated: bool,
}

#[derive(Deserialize, Serialize)]
struct Versioned {
    #[serde(flatten)]
    current: Entry,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous: Option<Entry>,
}

impl Versioned {
    fn matching(self, digest: &str) -> Option<Entry> {
        if self.current.digest == digest {
            Some(self.current)
        } else {
            self.previous.filter(|entry| entry.digest == digest)
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct AgentSection {
    pub name: String,
    pub title: Option<String>,
    pub head: Option<String>,
    pub written_at: u64,
    pub markdown: String,
    pub truncated: bool,
}

fn valid(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn owned_open(dir: &File, name: &std::ffi::OsStr, flags: OFlag, mode: Mode) -> io::Result<File> {
    let fd = openat(
        Some(dir.as_raw_fd()),
        name,
        OPEN_FLAGS | OFlag::O_NOCTTY | flags,
        mode,
    )?;
    // SAFETY: openat returned a fresh descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn open_cache(path: &Path, private: usize, create: bool) -> io::Result<File> {
    let relative = path
        .strip_prefix("/")
        .map_err(|_| invalid("invalid panel cache path"))?;
    let mut parts = relative.components();
    let mut left = parts.clone().count();
    let mut dir = File::open("/")?;
    for part in &mut parts {
        let Component::Normal(name) = part else {
            return Err(invalid("invalid panel cache path"));
        };
        let open = || owned_open(&dir, name, OFlag::O_DIRECTORY, Mode::empty());
        let next = match open() {
            Err(e) if e.kind() == io::ErrorKind::NotFound && create => {
                match mkdirat(Some(dir.as_raw_fd()), name, Mode::S_IRWXU) {
                    Ok(()) | Err(nix::errno::Errno::EEXIST) => {}
                    Err(e) => return Err(e.into()),
                }
                open()?
            }
            result => result?,
        };
        if left <= private {
            let meta = next.metadata()?;
            if meta.uid() != geteuid().as_raw() || meta.mode() & 0o7777 != 0o700 {
                return Err(invalid(
                    "panel cache directories must be private and owned by this user",
                ));
            }
        }
        dir = next;
        left -= 1;
    }
    Ok(dir)
}

fn private_file(dir: &File, name: &str, flags: OFlag) -> io::Result<File> {
    let file = owned_open(dir, name.as_ref(), flags, Mode::S_IRUSR | Mode::S_IWUSR)?;
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != geteuid().as_raw()
        || meta.mode() & 0o7777 != 0o600
        || meta.nlink() != 1
    {
        return Err(invalid(
            "panel cache file must be private and owned by this user",
        ));
    }
    Ok(file)
}

fn read_meta(dir: &File) -> BTreeMap<String, Versioned> {
    private_file(dir, "meta.json", OFlag::empty())
        .ok()
        .and_then(|mut f| bounded(&mut f, META_LIMIT).ok())
        .and_then(|(b, truncated)| {
            (!truncated)
                .then(|| serde_json::from_slice(&b).ok())
                .flatten()
        })
        .unwrap_or_default()
}

pub fn sections(project: &Project, dir: &Path) -> Vec<AgentSection> {
    let Ok(dir) = open_cache(dir, 3, false) else {
        return Vec::new();
    };
    let Ok(lock) = private_file(&dir, ".lock", OFlag::empty()) else {
        return Vec::new();
    };
    // The writer reads stdin before acquiring the lock; wait briefly for the atomic renames.
    let acquired = (0..20).any(|_| {
        if lock.try_lock_shared().is_ok() {
            true
        } else {
            std::thread::sleep(std::time::Duration::from_millis(5));
            false
        }
    });
    if !acquired {
        return Vec::new();
    }
    let mut entries: Vec<_> = read_meta(&dir).into_iter().collect();
    entries.sort_by_key(|(name, entry)| (name != "flujo", entry.current.written_at, name.clone()));
    let mut extra = 0;
    entries
        .into_iter()
        .filter_map(|(name, entry)| {
            if !valid(&name) || (name != "flujo" && extra >= EXTRA_LIMIT) {
                return None;
            }
            let mut file = private_file(&dir, &format!("{name}.md"), OFlag::empty()).ok()?;
            let (data, over) = bounded(&mut file, SECTION_LIMIT).ok()?;
            let entry = entry.matching(&hash(&data))?;
            if entry.root != project.root {
                return None;
            }
            extra += usize::from(name != "flujo");
            Some(AgentSection {
                name,
                title: entry.title.map(|s| short(&s)),
                head: entry.head,
                written_at: entry.written_at,
                markdown: String::from_utf8_lossy(&data).into_owned(),
                truncated: entry.truncated || over,
            })
        })
        .collect()
}

fn atomic(dir: &File, name: &str, data: &[u8]) -> io::Result<()> {
    let temp = format!(".{}.new", uuid::Uuid::new_v4());
    let fd = Some(dir.as_raw_fd());
    let mut file = owned_open(
        dir,
        temp.as_ref(),
        OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_WRONLY,
        Mode::S_IRUSR | Mode::S_IWUSR,
    )?;
    let result = (|| {
        file.write_all(data)?;
        file.sync_all()?;
        renameat(fd, temp.as_str(), fd, name)?;
        dir.sync_all()
    })();
    if result.is_err() {
        let _ = unlinkat(fd, temp.as_str(), UnlinkatFlags::NoRemoveDir);
    }
    result
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn save(
    dir: &Path,
    project: &Project,
    name: &str,
    title: Option<&str>,
    input: &mut impl Read,
) -> io::Result<()> {
    let (data, truncated) = bounded(input, SECTION_LIMIT)?;
    let available = String::from_utf8_lossy(&data)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace(|c: char| c.is_control() && !matches!(c, '\n' | '\t'), "�");
    let clipped = &available[..available.floor_char_boundary(SECTION_LIMIT)];
    let bytes = clipped.as_bytes();
    let entry = Entry {
        root: project.root.clone(),
        head: project.has_git.then(|| git::head(&project.root)).flatten(),
        written_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        title: title.map(short),
        digest: hash(bytes),
        truncated: truncated || bytes.len() < available.len(),
    };
    let dir = open_cache(dir, 3, true)?;
    let _guard = lock(&dir)?;
    let mut entries = read_meta(&dir);
    if name != "flujo"
        && !entries.contains_key(name)
        && entries.keys().filter(|n| *n != "flujo").count() >= EXTRA_LIMIT
    {
        return Err(invalid("section limit reached (six additional sections)"));
    }
    let filename = format!("{name}.md");
    let previous = entries.remove(name).and_then(|old| {
        let mut f = private_file(&dir, &filename, OFlag::empty()).ok()?;
        let (data, _) = bounded(&mut f, SECTION_LIMIT).ok()?;
        old.matching(&hash(&data))
    });
    entries.insert(
        name.into(),
        Versioned {
            current: entry,
            previous,
        },
    );
    // Commit the two files in this order. A reader after an interrupted replacement
    // can still match the old file to its metadata until the new file is renamed.
    atomic(&dir, "meta.json", &serde_json::to_vec(&entries)?)?;
    atomic(&dir, &filename, bytes)
}

fn lock(dir: &File) -> io::Result<File> {
    let lock = private_file(dir, ".lock", OFlag::O_RDWR | OFlag::O_CREAT)?;
    lock.lock()?;
    Ok(lock)
}

fn clear(dir: &Path, name: &str) -> io::Result<()> {
    let dir = match open_cache(dir, 3, false) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        result => result?,
    };
    let _guard = lock(&dir)?;
    let mut entries = read_meta(&dir);
    entries.remove(name);
    atomic(&dir, "meta.json", &serde_json::to_vec(&entries)?)?;
    match unlinkat(
        Some(dir.as_raw_fd()),
        format!("{name}.md").as_str(),
        UnlinkatFlags::NoRemoveDir,
    ) {
        Ok(()) => dir.sync_all(),
        Err(nix::errno::Errno::ENOENT) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub fn command(
    cwd: &Path,
    args: &[String],
    input: &mut impl Read,
    out: &mut impl Write,
) -> io::Result<()> {
    let args: Vec<_> = args.iter().map(String::as_str).collect();
    let (action, name, title) = match args.as_slice() {
        ["show"] => ("show", None, None),
        [action @ ("set" | "clear"), name] if valid(name) => (*action, Some(*name), None),
        ["set", name, "--title", title] if valid(name) => ("set", Some(*name), Some(*title)),
        _ => {
            return Err(invalid(
                "usage: wyss-panel set <section> [--title <text>] | clear <section> | show (section: [a-z0-9-]{1,32})",
            ));
        }
    };
    let project = detect(cwd).ok_or_else(|| invalid("no project found"))?;
    let dir = cache_path(&project).ok_or_else(|| invalid("no XDG cache directory"))?;
    match (action, name) {
        ("set", Some(name)) => save(&dir, &project, name, title, input),
        ("clear", Some(name)) => clear(&dir, name),
        _ => {
            for section in sections(&project, &dir) {
                writeln!(out, "[{}]\n{}", section.name, section.markdown)?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn panel_security_cache_rejects_symlink_ancestors_and_open_modes() {
        use std::os::unix::fs::symlink;
        let fixture = crate::project::tests::Fixture::new();
        let outside = fixture.0.join("outside");
        fs::create_dir(&outside).unwrap();
        let link = fixture.0.join("cache");
        symlink(&outside, &link).unwrap();
        let project = Project {
            root: fixture.0.clone(),
            has_git: false,
        };
        let set = |dir: &Path| save(dir, &project, "flujo", None, &mut &b"note"[..]);
        let redirected = set(&link.join("wyss-console/panels/id"));
        let broad = fixture.0.join("wyss-console");
        fs::create_dir(&broad).unwrap();
        fs::set_permissions(&broad, fs::Permissions::from_mode(0o755)).unwrap();
        let open_mode = set(&broad.join("panels/id"));
        assert!(redirected.is_err(), "followed a cache ancestor symlink");
        assert!(
            open_mode.is_err(),
            "accepted a public panel cache directory"
        );
        assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
    }

    #[test]
    fn panel_security_cache_writes_and_unlinks_through_the_open_directory() {
        let fixture = crate::project::tests::Fixture::new();
        let original = fixture.0.join("wyss-console/panels/id");
        let moved = original.with_file_name("moved");
        let dir = open_cache(&original, 3, true).unwrap();
        fs::rename(&original, &moved).unwrap();
        fs::create_dir(&original).unwrap();
        fs::write(original.join("flujo.md"), "replacement").unwrap();
        atomic(&dir, "flujo.md", b"pinned").unwrap();
        assert_eq!(
            fs::read_to_string(moved.join("flujo.md")).unwrap(),
            "pinned"
        );
        unlinkat(
            Some(dir.as_raw_fd()),
            "flujo.md",
            UnlinkatFlags::NoRemoveDir,
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(original.join("flujo.md")).unwrap(),
            "replacement"
        );
        assert!(!moved.join("flujo.md").exists());
    }

    #[test]
    fn writes_private_atomic_sections_and_clears_them() {
        let fixture = crate::project::tests::Fixture::new();
        let root = fixture.0.join("project");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("Makefile"), "test:\n").unwrap();
        let project = detect(&root).unwrap();
        let dir = fixture.0.join("wyss-console/panels/id");
        save(&dir, &project, "flujo", None, &mut &b"first"[..]).unwrap();
        let old = File::open(dir.join("flujo.md")).unwrap();
        let long = "á".repeat(SECTION_LIMIT);
        save(
            &dir,
            &project,
            "flujo",
            Some("Architecture"),
            &mut long.as_bytes(),
        )
        .unwrap();
        assert_ne!(
            old.metadata().unwrap().ino(),
            fs::metadata(dir.join("flujo.md")).unwrap().ino()
        );
        for name in [".lock", "meta.json", "flujo.md"] {
            assert_eq!(
                fs::metadata(dir.join(name)).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let data = sections(&project, &dir);
        assert_eq!(data[0].title.as_deref(), Some("Architecture"));
        assert_eq!(data[0].markdown.len(), SECTION_LIMIT);
        assert!(data[0].truncated);
        for i in 0..6 {
            save(
                &dir,
                &project,
                &format!("part-{i}"),
                None,
                &mut &b"data"[..],
            )
            .unwrap();
        }
        assert!(save(&dir, &project, "overflow", None, &mut &b""[..]).is_err());
        assert_eq!(sections(&project, &dir).len(), 7);
        let invalid = vec!["set".into(), "../escape".into()];
        assert_eq!(
            command(&root, &invalid, &mut &b""[..], &mut Vec::new())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        clear(&dir, "flujo").unwrap();
        assert_eq!(sections(&project, &dir).len(), 6);
        assert!(!dir.join("flujo.md").exists());
    }
}
