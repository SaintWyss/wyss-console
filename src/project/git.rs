//! Git queries with no index writes, network access, hooks, or worktree filters.
use super::{Files, READ_LIMIT, Scan, bounded, open_relative_fd, short};
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use std::collections::BTreeMap;
use std::fs::{self, File, Metadata};
use std::io::{ErrorKind, Read, Seek, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, symlink};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const OUTPUT_LIMIT: usize = 256 << 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Command,
    Untrusted,
    Timeout,
    OutputLimit,
}

fn run(
    root: &Path,
    dir: &Path,
    config: &[String],
    args: &[&str],
    stdin: Stdio,
    scan: &Scan<'_>,
) -> Result<Vec<u8>, Error> {
    scan.check()?;
    let mut command = Command::new("/usr/bin/git");
    command.current_dir(root).env_clear();
    for name in ["HOME", "PATH", "LANG"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let mut child = command
        .envs([
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_OPTIONAL_LOCKS", "0"),
            ("GIT_NO_LAZY_FETCH", "1"),
            ("GIT_ALLOW_PROTOCOL", ""),
        ])
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.pager=cat",
            "-c",
            "diff.external=",
            "-c",
            "core.untrackedCache=false",
            "-c",
            "core.excludesFile=/dev/null",
            "-c",
            "include.path=/dev/null",
            "--no-optional-locks",
            "-c",
            "core.quotePath=false",
            "--no-pager",
            "--git-dir",
        ])
        .arg(dir)
        .arg("--work-tree=.")
        .args(config.iter().flat_map(|setting| ["-c", setting.as_str()]))
        .args(args)
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Error::Command)?;
    let mut pipe = child.stdout.take().expect("piped stdout");
    let result = (|| {
        fcntl(pipe.as_raw_fd(), FcntlArg::F_SETFL(OFlag::O_NONBLOCK))
            .map_err(|_| Error::Command)?;
        let mut output = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            scan.check()?;
            match pipe.read(&mut buf) {
                Ok(0) => {
                    if let Some(status) = child.try_wait().map_err(|_| Error::Command)? {
                        return status.success().then_some(output).ok_or(Error::Command);
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                Ok(n) if output.len() + n <= OUTPUT_LIMIT => output.extend_from_slice(&buf[..n]),
                Ok(_) => {
                    scan.limited.set(true);
                    return Err(Error::OutputLimit);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => return Err(Error::Command),
            }
        }
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Branch {
    Named(String),
    Detached(String),
}

fn absolute(base: &Path, path: &Path) -> Option<PathBuf> {
    let mut result = PathBuf::new();
    for part in base.join(path).components() {
        match part {
            Component::RootDir | Component::Normal(_) => result.push(part),
            Component::CurDir => {}
            Component::ParentDir if result.pop() => {}
            _ => return None,
        }
    }
    Some(result)
}

fn open_absolute(path: &Path, directory: bool) -> Result<File, Error> {
    let relative = path.strip_prefix("/").map_err(|_| Error::Untrusted)?;
    open_relative_fd(Path::new("/"), relative, directory).map_err(|_| Error::Untrusted)
}

fn descriptor_path(file: &File) -> PathBuf {
    PathBuf::from(format!(
        "/proc/{}/fd/{}",
        std::process::id(),
        file.as_raw_fd()
    ))
}

fn admin_text(dir: &File, name: &str) -> Result<String, Error> {
    let path = descriptor_path(dir).join(name);
    let mut file = open_absolute_file(&path).map_err(|_| Error::Untrusted)?;
    let (bytes, truncated) = bounded(&mut file, 4096).map_err(|_| Error::Untrusted)?;
    if truncated {
        return Err(Error::Untrusted);
    }
    String::from_utf8(bytes).map_err(|_| Error::Untrusted)
}

fn open_absolute_file(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::from(ErrorKind::InvalidData));
    }
    Ok(file)
}

struct Admin {
    dir: File,
    common: Option<File>,
}

impl Admin {
    fn open(root: &Path) -> Result<Self, Error> {
        let root = root.canonicalize().map_err(|_| Error::Untrusted)?;
        let checkout = open_absolute(&root, true)?;
        let marker = root.join(".git");
        let kind = descriptor_path(&checkout)
            .join(".git")
            .symlink_metadata()
            .map_err(|_| Error::Untrusted)?;
        let path = if kind.is_dir() {
            marker.clone()
        } else if kind.is_file() {
            let pointer = admin_text(&checkout, ".git")?;
            let target = pointer
                .trim()
                .strip_prefix("gitdir:")
                .ok_or(Error::Untrusted)?
                .trim();
            if target.is_empty() {
                return Err(Error::Untrusted);
            }
            absolute(&root, Path::new(target)).ok_or(Error::Untrusted)?
        } else {
            return Err(Error::Untrusted);
        };

        let worktrees = path.parent().filter(|p| p.ends_with(".git/worktrees"));
        if !path.starts_with(&root) && worktrees.is_none() {
            return Err(Error::Untrusted);
        }
        let dir = open_absolute(&path, true)?;
        let common = if let Some(worktrees) = worktrees {
            let back = admin_text(&dir, "gitdir")?;
            let common_dir = admin_text(&dir, "commondir")?;
            let expected = worktrees.parent().ok_or(Error::Untrusted)?;
            if absolute(&path, Path::new(back.trim())).as_deref() != Some(&marker)
                || absolute(&path, Path::new(common_dir.trim())).as_deref() != Some(expected)
            {
                return Err(Error::Untrusted);
            }
            Some(open_absolute(expected, true)?)
        } else {
            match descriptor_path(&dir).join("commondir").symlink_metadata() {
                Err(e) if e.kind() == ErrorKind::NotFound => {}
                _ => return Err(Error::Untrusted),
            }
            None
        };
        Ok(Self { dir, common })
    }

    fn common(&self) -> &File {
        self.common.as_ref().unwrap_or(&self.dir)
    }
}

pub fn head_ref(root: &Path) -> Option<Branch> {
    let admin = Admin::open(root).ok()?;
    branch(&admin_text(&admin.dir, "HEAD").ok()?)
}

fn branch(head: &str) -> Option<Branch> {
    let head = head.trim();
    match head.strip_prefix("ref: ") {
        Some(name) if !name.is_empty() => Some(Branch::Named(short(
            name.strip_prefix("refs/heads/").unwrap_or(name),
        ))),
        None if (7..=64).contains(&head.len()) && head.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Some(Branch::Detached(head.into()))
        }
        _ => None,
    }
}

fn valid_sha(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(super) fn head(root: &Path) -> Option<String> {
    let scan = Scan::new(&|| false);
    let repo = Repo::open(root, &scan).ok()?;
    repo.head()
}

pub fn clean_origin(value: &str) -> Option<String> {
    let value = value.trim();
    let (host, path) = if let Some((scheme, rest)) = value.split_once("://") {
        if !matches!(scheme, "http" | "https" | "ssh" | "git" | "git+ssh") {
            return None;
        }
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, path) = rest.split_at(end);
        (
            authority.rsplit('@').next()?,
            path.strip_prefix('/').unwrap_or(""),
        )
    } else {
        let (authority, path) = value.split_once(':')?;
        (authority.rsplit('@').next()?, path)
    };
    let host = host.split(':').next()?;
    if host.is_empty()
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
    {
        return None;
    }
    let path = path.split(['?', '#']).next()?;
    let segment = |part: &str| {
        !part.is_empty()
            && !matches!(part, "." | "..")
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    };
    if let Some((owner, repo)) = path.split_once('/')
        && segment(owner)
        && segment(repo)
        && segment(repo.strip_suffix(".git").unwrap_or(repo))
    {
        return Some(short(&format!(
            "{host}/{}",
            path.strip_suffix(".git").unwrap_or(path)
        )));
    }
    Some(short(host))
}

// Let Git parse a bounded snapshot without includes, then give repository commands a
// private GIT_DIR with only selected metadata and the settings they actually need.
struct Shadow {
    path: PathBuf,
    files: Vec<File>,
}

impl Shadow {
    fn new() -> Result<Self, Error> {
        let path = std::env::temp_dir().join(format!("wyss-panel-git-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|_| Error::Command)?;
        Ok(Self {
            path,
            files: Vec::new(),
        })
    }

    fn link(&mut self, source: &File, name: &Path, directory: bool) -> Result<(), Error> {
        let path = descriptor_path(source).join(name);
        let file = if directory {
            use std::os::unix::fs::OpenOptionsExt;
            fs::OpenOptions::new()
                .read(true)
                .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_DIRECTORY)
                .open(path)
        } else {
            open_absolute_file(&path)
        };
        let file = match file {
            Ok(file) => file,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(Error::Untrusted),
        };
        symlink(descriptor_path(&file), self.path.join(name)).map_err(|_| Error::Command)?;
        self.files.push(file);
        Ok(())
    }
}

impl Drop for Shadow {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct Repo<'a> {
    root: &'a Path,
    shadow: Shadow,
    branch: Branch,
    origin: Option<String>,
    config: Vec<String>,
    filemode: bool,
    scan: &'a Scan<'a>,
}

impl<'a> Repo<'a> {
    fn open(root: &'a Path, scan: &'a Scan<'a>) -> Result<Self, Error> {
        scan.check()?;
        let admin = Admin::open(root)?;
        let head = admin_text(&admin.dir, "HEAD").map_err(|_| Error::Command)?;
        let branch = branch(&head).ok_or(Error::Command)?;
        let mut shadow = Shadow::new()?;
        fs::write(shadow.path.join("HEAD"), head).map_err(|_| Error::Command)?;
        for dir in ["objects", "refs", "reftable"] {
            shadow.link(admin.common(), Path::new(dir), true)?;
        }
        for file in ["packed-refs", "shallow"] {
            shadow.link(admin.common(), Path::new(file), false)?;
        }
        fs::create_dir(shadow.path.join("info")).map_err(|_| Error::Command)?;
        if let Ok(info) = open_relative_fd(
            &descriptor_path(admin.common()).join("."),
            Path::new("info"),
            true,
        ) {
            // info/exclude is repository data. Lock it to the same descriptor as the other inputs.
            if let Ok(file) = open_absolute_file(&descriptor_path(&info).join("exclude")) {
                symlink(descriptor_path(&file), shadow.path.join("info/exclude"))
                    .map_err(|_| Error::Command)?;
                shadow.files.push(file);
            }
        }
        // Split-index reads can freshen sharedindex timestamps, so Git gets private copies.
        shadow.link(&admin.dir, Path::new("index"), false)?;
        let admin_path = descriptor_path(&admin.dir);
        let mut remaining = READ_LIMIT;
        for entry in fs::read_dir(&admin_path).map_err(|_| Error::Command)? {
            scan.check()?;
            let name = entry.map_err(|_| Error::Command)?.file_name();
            if name
                .to_str()
                .and_then(|name| name.strip_prefix("sharedindex."))
                .is_some_and(valid_sha)
            {
                let mut file =
                    open_absolute_file(&admin_path.join(&name)).map_err(|_| Error::Untrusted)?;
                let (bytes, truncated) =
                    bounded(&mut file, remaining).map_err(|_| Error::Command)?;
                if truncated {
                    scan.limited.set(true);
                    return Err(Error::OutputLimit);
                }
                remaining -= bytes.len();
                fs::write(shadow.path.join(name), bytes).map_err(|_| Error::Command)?;
            }
        }
        let mut origin = None;
        let mut config = Vec::new();
        let mut filemode = true;
        let mut object_format = None;
        let mut ref_storage = None;
        let config_path = descriptor_path(admin.common()).join("config");
        let raw = match open_absolute_file(&config_path) {
            Ok(mut file) => {
                let (bytes, truncated) =
                    bounded(&mut file, OUTPUT_LIMIT).map_err(|_| Error::Command)?;
                if truncated {
                    scan.limited.set(true);
                    return Err(Error::OutputLimit);
                }
                run(
                    root,
                    &shadow.path,
                    &[],
                    &["config", "--file", "-", "--no-includes", "--null", "--list"],
                    input(&bytes).ok_or(Error::Command)?.into(),
                    scan,
                )?
            }
            Err(e) if e.kind() == ErrorKind::NotFound => Vec::new(),
            Err(_) => return Err(Error::Untrusted),
        };
        for item in raw
            .split(|b| *b == 0)
            .filter_map(|b| std::str::from_utf8(b).ok())
        {
            let (key, value) = item.split_once('\n').unwrap_or((item, "true"));
            match key {
                "remote.origin.url" => origin = clean_origin(value),
                "core.filemode" => {
                    filemode = !matches!(
                        value.to_ascii_lowercase().as_str(),
                        "false" | "no" | "off" | "0" | ""
                    )
                }
                "extensions.objectformat" => object_format = Some(value),
                "extensions.refstorage" => ref_storage = Some(value),
                _ if upstream_setting(key) && value.len() <= 4096 => {
                    config.push(format!("{key}={value}"))
                }
                _ => {}
            }
        }
        let extensions = match (object_format, ref_storage) {
            (None | Some("sha1" | "sha256"), None | Some("files" | "reftable")) => {
                format!(
                    "[core]\n repositoryformatversion = 1\n[extensions]\n objectformat = {}\n refstorage = {}\n",
                    object_format.unwrap_or("sha1"),
                    ref_storage.unwrap_or("files")
                )
            }
            _ => return Err(Error::Command),
        };
        fs::write(shadow.path.join("config"), extensions).map_err(|_| Error::Command)?;
        Ok(Self {
            root,
            shadow,
            branch,
            origin,
            config,
            filemode,
            scan,
        })
    }

    fn run(&self, args: &[&str]) -> Result<Vec<u8>, Error> {
        self.run_with_input(args, Stdio::null())
    }

    fn run_with_input(&self, args: &[&str], stdin: Stdio) -> Result<Vec<u8>, Error> {
        run(
            self.root,
            &self.shadow.path,
            &self.config,
            args,
            stdin,
            self.scan,
        )
    }

    fn optional(&self, result: Result<Vec<u8>, Error>) -> Result<Option<Vec<u8>>, Error> {
        match result {
            Ok(bytes) => Ok(Some(bytes)),
            Err(Error::Timeout | Error::OutputLimit) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn head(&self) -> Option<String> {
        if let Branch::Detached(sha) = &self.branch
            && valid_sha(sha)
        {
            return Some(sha.clone());
        }
        let bytes = self.run(&["rev-parse", "--verify", "HEAD"]).ok()?;
        let sha = std::str::from_utf8(&bytes).ok()?.trim();
        valid_sha(sha).then(|| sha.into())
    }
}

fn upstream_setting(key: &str) -> bool {
    let Some((section, rest)) = key.split_once('.') else {
        return false;
    };
    let Some((name, key)) = rest.rsplit_once('.') else {
        return false;
    };
    matches!(
        (section, key),
        ("branch", "remote" | "merge") | ("remote", "fetch")
    ) && name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'))
}

pub(super) fn tracked(root: &Path, scan: &Scan<'_>) -> Result<Vec<u8>, Error> {
    Repo::open(root, scan)?.run(&["ls-files", "-z", "--cached"])
}

#[derive(PartialEq, Eq)]
pub struct Commit {
    pub sha: String,
    pub title: String,
    pub author: String,
    pub seconds: i64,
}

#[derive(PartialEq, Eq)]
pub struct Git {
    pub branch: Branch,
    pub head: Option<String>,
    pub modified: usize,
    pub new: usize,
    pub deleted: usize,
    pub upstream: Option<(usize, usize)>,
    pub commits: Vec<Commit>,
    pub local: Vec<String>,
    pub remote: Vec<String>,
    pub tags: Vec<String>,
    pub limited: bool,
}

fn record(by_path: &mut BTreeMap<Vec<u8>, u8>, status: u8, path: &[u8]) {
    let old = by_path.entry(path.to_vec()).or_insert(status);
    if status == b'D' || (*old != b'A' && *old != b'D') {
        *old = status;
    }
}

fn statuses(bytes: &[u8], by_path: &mut BTreeMap<Vec<u8>, u8>) {
    let mut entries = bytes.split(|b| *b == 0);
    while let (Some(status), Some(path)) = (entries.next(), entries.next()) {
        if let Some(status) = status.first().copied() {
            record(by_path, status, path);
        }
    }
}

fn input(data: &[u8]) -> Option<File> {
    use nix::sys::memfd::{MemFdCreateFlag, memfd_create};
    let mut input =
        File::from(memfd_create(c"wyss-panel-input", MemFdCreateFlag::MFD_CLOEXEC).ok()?);
    input.write_all(data).ok()?;
    input.rewind().ok()?;
    Some(input)
}

fn blob_matches(repo: &Repo<'_>, data: &[u8], oid: &str) -> Option<bool> {
    let output = repo
        .run_with_input(
            &["hash-object", "--no-filters", "--stdin"],
            input(data)?.into(),
        )
        .ok()?;
    Some(std::str::from_utf8(&output).ok()?.trim() == oid)
}

struct IndexStat<'a> {
    ctime: &'a str,
    mtime: &'a str,
    size: &'a str,
}

impl IndexStat<'_> {
    fn matches(&self, disk: &Metadata) -> bool {
        fn time(field: &str) -> Option<(i64, i64)> {
            let (sec, nano) = field.split_once(':')?;
            Some((sec.parse().ok()?, nano.parse().ok()?))
        }
        time(self.ctime) == Some((disk.ctime(), disk.ctime_nsec()))
            && time(self.mtime) == Some((disk.mtime(), disk.mtime_nsec()))
            && self.size.parse::<u64>().ok() == Some(disk.len())
    }
}

fn worktree_change(
    repo: &Repo<'_>,
    files: &mut Files<'_>,
    path: &Path,
    mode: &str,
    oid: &str,
    stat: IndexStat<'_>,
) -> Option<u8> {
    let parent = open_relative_fd(files.root, path.parent()?, true);
    let disk_path = match parent.as_ref() {
        Ok(dir) => {
            PathBuf::from(format!("/proc/self/fd/{}", dir.as_raw_fd())).join(path.file_name()?)
        }
        Err(e)
            if e.kind() == ErrorKind::NotFound
                || matches!(
                    e.raw_os_error(),
                    Some(nix::libc::ENOTDIR | nix::libc::ELOOP)
                ) =>
        {
            return Some(b'D');
        }
        Err(_) => return Some(b'M'),
    };
    let disk = match disk_path.symlink_metadata() {
        Ok(disk) => disk,
        Err(e) if e.kind() == ErrorKind::NotFound => return Some(b'D'),
        Err(_) => return Some(b'M'),
    };
    match mode {
        "100644" | "100755" if disk.is_file() => {
            if repo.filemode && (disk.mode() & 0o100 != 0) != (mode == "100755") {
                return Some(b'M');
            }
            if stat.matches(&disk) {
                return None;
            }
            repo.scan.take(&repo.scan.hashes)?;
            let same = files
                .read(path)
                .and_then(|(data, _)| blob_matches(repo, &data, oid));
            (same != Some(true)).then_some(b'M')
        }
        "120000" if disk.is_symlink() => {
            if stat.matches(&disk) {
                return None;
            }
            repo.scan.take(&repo.scan.hashes)?;
            let same = fs::read_link(&disk_path)
                .ok()
                .and_then(|link| blob_matches(repo, link.as_os_str().as_bytes(), oid));
            (same != Some(true)).then_some(b'M')
        }
        _ if disk.is_dir() => Some(b'D'),
        _ => Some(b'M'),
    }
}

fn index_changes(
    repo: &Repo<'_>,
    files: &mut Files<'_>,
    raw: &[u8],
    paths: &mut BTreeMap<Vec<u8>, u8>,
) -> Result<(), Error> {
    let mut raw = raw;
    while !raw.is_empty() {
        if repo.scan.check().is_err() || repo.scan.hashes.get() == 0 {
            repo.scan.limited.set(true);
            break;
        }
        let end = raw.iter().position(|b| *b == 0).ok_or(Error::Command)?;
        let (entry, tail) = raw.split_at(end);
        let tab = entry
            .iter()
            .position(|b| *b == b'\t')
            .ok_or(Error::Command)?;
        let path = &entry[tab + 1..];
        let mut fields = std::str::from_utf8(&entry[..tab])
            .map_err(|_| Error::Command)?
            .split_whitespace();
        let [Some(kind), Some(mode), Some(oid), Some(stage)] =
            std::array::from_fn(|_| fields.next())
        else {
            return Err(Error::Command);
        };
        // With --debug Git emits five stat lines after each NUL-delimited path.
        let mut offsets = tail[1..]
            .iter()
            .enumerate()
            .filter_map(|(i, b)| (*b == b'\n').then_some(i));
        let end = offsets.nth(4).ok_or(Error::Command)?;
        let mut stat = std::str::from_utf8(&tail[1..=end])
            .map_err(|_| Error::Command)?
            .lines();
        raw = &tail[end + 2..];
        if kind != "H" || stage != "0" || mode == "160000" {
            continue;
        }
        let ctime = stat
            .next()
            .and_then(|s| s.strip_prefix("  ctime: "))
            .ok_or(Error::Command)?;
        let mtime = stat
            .next()
            .and_then(|s| s.strip_prefix("  mtime: "))
            .ok_or(Error::Command)?;
        let size = stat
            .nth(2)
            .and_then(|s| s.strip_prefix("  size: "))
            .and_then(|s| s.split_whitespace().next())
            .ok_or(Error::Command)?;
        if let Some(status) = worktree_change(
            repo,
            files,
            Path::new(std::ffi::OsStr::from_bytes(path)),
            mode,
            oid,
            IndexStat { ctime, mtime, size },
        ) {
            record(paths, status, path);
        }
    }
    Ok(())
}

fn counts(
    repo: &Repo<'_>,
    files: &mut Files<'_>,
    has_head: bool,
) -> Result<(usize, usize, usize), Error> {
    // Plumbing avoids clean filters, submodule inspection, and external diff programs.
    let mut paths = BTreeMap::new();
    if has_head {
        if let Some(bytes) = repo.optional(repo.run(&[
            "diff-index",
            "--cached",
            "--name-status",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--ignore-submodules=dirty",
            "HEAD",
            "--",
        ]))? {
            statuses(&bytes, &mut paths);
        }
    } else if let Some(bytes) = repo.optional(repo.run(&["ls-files", "-z", "--cached"]))? {
        for path in bytes.split(|b| *b == 0).filter(|p| !p.is_empty()) {
            record(&mut paths, b'A', path);
        }
    }
    if let Some(bytes) =
        repo.optional(repo.run(&["ls-files", "-z", "--others", "--exclude-standard"]))?
    {
        for path in bytes.split(|b| *b == 0).filter(|p| !p.is_empty()) {
            paths.entry(path.to_vec()).or_insert(b'A');
        }
    }
    // diff-files/status can invoke a clean filter even for name-only output.
    if let Some(index) = repo.optional(repo.run(&["ls-files", "-v", "--stage", "--debug", "-z"]))? {
        index_changes(repo, files, &index, &mut paths)?;
    }
    Ok((
        paths.values().filter(|b| !matches!(b, b'A' | b'D')).count(),
        paths.values().filter(|b| **b == b'A').count(),
        paths.values().filter(|b| **b == b'D').count(),
    ))
}

pub fn git(root: &Path) -> Result<Git, Error> {
    let mut remaining = READ_LIMIT;
    snapshot(root, &mut remaining, &Scan::new(&|| false)).1
}

pub(super) fn snapshot(
    root: &Path,
    remaining: &mut usize,
    scan: &Scan<'_>,
) -> (Option<String>, Result<Git, Error>) {
    match Repo::open(root, scan) {
        Ok(repo) => (repo.origin.clone(), git_with_budget(&repo, remaining)),
        Err(e) => (None, Err(e)),
    }
}

fn git_with_budget(repo: &Repo<'_>, remaining: &mut usize) -> Result<Git, Error> {
    let branch = repo.branch.clone();
    let history = repo.run(&[
        "log",
        "-5",
        "-z",
        "--no-show-signature",
        "--no-notes",
        "--format=%H%x00%h%x00%s%x00%an%x00%at",
        "HEAD",
        "--",
    ]);
    let (head, commits) = match history {
        Ok(history) => {
            let fields: Vec<_> = history.split(|b| *b == 0).collect();
            let string = |b: &[u8]| short(&String::from_utf8_lossy(b));
            let head = fields
                .first()
                .map(|sha| string(sha))
                .filter(|sha| valid_sha(sha));
            let commits = fields
                .as_chunks::<5>()
                .0
                .iter()
                .map(|c| Commit {
                    sha: string(c[1]),
                    title: string(c[2]),
                    author: string(c[3]),
                    seconds: std::str::from_utf8(c[4])
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or_default(),
                })
                .collect();
            (head, commits)
        }
        Err(Error::Command) if repo.head().is_none() => (None, Vec::new()),
        Err(Error::Timeout | Error::OutputLimit) => (repo.head(), Vec::new()),
        Err(e) => return Err(e),
    };
    let mut files = Files {
        remaining: *remaining,
        ..Files::new(repo.root)
    };
    let changes = counts(repo, &mut files, head.is_some());
    *remaining = files.remaining;
    let (modified, new, deleted) = changes?;
    let upstream = match repo.run(&[
        "rev-list",
        "--count",
        "--left-right",
        "HEAD...@{upstream}",
        "--",
    ]) {
        Ok(raw) => {
            let mut n = std::str::from_utf8(&raw).unwrap_or("").split_whitespace();
            n.next()
                .and_then(|a| a.parse().ok())
                .zip(n.next().and_then(|b| b.parse().ok()))
        }
        Err(Error::Command | Error::Timeout | Error::OutputLimit) => None,
        Err(e) => return Err(e),
    };
    let refs = |kind: &str, sort: &str, limit: usize| -> Result<Vec<String>, Error> {
        let raw = repo
            .optional(repo.run(&[
                "for-each-ref",
                sort,
                &format!("--count={limit}"),
                "--format=%(refname:lstrip=2)%00%(symref)",
                kind,
            ]))?
            .unwrap_or_default();
        Ok(String::from_utf8_lossy(&raw)
            .lines()
            .filter_map(|l| {
                let (name, symref) = l.split_once('\0')?;
                symref.is_empty().then(|| short(name))
            })
            .collect())
    };
    let mut local = refs("refs/heads", "--sort=refname", 15)?;
    if let Branch::Named(name) = &branch
        && head.is_some()
    {
        local.retain(|n| n != name);
        local.insert(0, name.clone());
        local.truncate(15);
    }
    Ok(Git {
        branch,
        head,
        modified,
        new,
        deleted,
        upstream,
        commits,
        local,
        remote: refs("refs/remotes", "--sort=refname", 16)?
            .into_iter()
            .take(15)
            .collect(),
        tags: refs("refs/tags", "--sort=-creatordate", 5)?,
        limited: files.limited || repo.scan.limited.get(),
    })
}
