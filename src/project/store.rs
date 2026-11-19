use super::{Project, bounded, detect, git, open_relative, short};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
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
    path.ancestors()
        .find(|p| p.symlink_metadata().is_ok_and(|m| m.is_dir()))
        .map(Path::to_path_buf)
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

fn read_meta(dir: &Path) -> BTreeMap<String, Versioned> {
    open_relative(dir, Path::new("meta.json"))
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
    let Ok(lock) = open_relative(dir, Path::new(".lock")) else {
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
    let mut entries: Vec<_> = read_meta(dir).into_iter().collect();
    entries.sort_by_key(|(name, entry)| (name != "flujo", entry.current.written_at, name.clone()));
    let mut extra = 0;
    entries
        .into_iter()
        .filter_map(|(name, entry)| {
            if !valid(&name) || (name != "flujo" && extra >= EXTRA_LIMIT) {
                return None;
            }
            let mut file = open_relative(dir, Path::new(&format!("{name}.md"))).ok()?;
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

fn atomic(dir: &Path, name: &str, data: &[u8]) -> io::Result<()> {
    let temp = dir.join(format!(".{}.new", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temp)?;
        f.write_all(data)?;
        f.sync_all()?;
        fs::rename(&temp, dir.join(name))?;
        File::open(dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
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
    let _guard = lock(dir)?;
    let mut entries = read_meta(dir);
    if name != "flujo"
        && !entries.contains_key(name)
        && entries.keys().filter(|n| *n != "flujo").count() >= EXTRA_LIMIT
    {
        return Err(invalid("section limit reached (six additional sections)"));
    }
    let filename = format!("{name}.md");
    let previous = entries.remove(name).and_then(|old| {
        let mut f = open_relative(dir, Path::new(&filename)).ok()?;
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
    atomic(dir, "meta.json", &serde_json::to_vec(&entries)?)?;
    atomic(dir, &filename, bytes)
}

fn lock(dir: &Path) -> io::Result<File> {
    fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(dir)?;
    if !dir.symlink_metadata()?.is_dir() {
        return Err(invalid("invalid panel cache directory"));
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .mode(0o600)
        .open(dir.join(".lock"))?;
    lock.lock()?;
    Ok(lock)
}

fn clear(dir: &Path, name: &str) -> io::Result<()> {
    if !dir.try_exists()? {
        return Ok(());
    }
    let _guard = lock(dir)?;
    let mut entries = read_meta(dir);
    entries.remove(name);
    atomic(dir, "meta.json", &serde_json::to_vec(&entries)?)?;
    match fs::remove_file(dir.join(format!("{name}.md"))) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
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
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    #[test]
    fn writes_private_atomic_sections_and_clears_them() {
        let fixture = crate::project::tests::Fixture::new();
        let root = fixture.0.join("project");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("Makefile"), "test:\n").unwrap();
        let project = detect(&root).unwrap();
        let dir = fixture.0.join("panels").join("id");
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
