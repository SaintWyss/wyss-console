//! Git queries with no index writes, network access, hooks, or worktree filters.
use super::{Files, READ_LIMIT, open_relative_fd, read_small, short};
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use std::collections::BTreeMap;
use std::fs::{self, File, Metadata};
use std::io::{ErrorKind, Read, Seek, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const OUTPUT_LIMIT: usize = 256 << 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Command,
    Timeout,
    OutputLimit,
}

pub(super) fn run(root: &Path, args: &[&str]) -> Result<Vec<u8>, Error> {
    run_with_input(root, args, Stdio::null())
}

fn run_with_input(root: &Path, args: &[&str], stdin: Stdio) -> Result<Vec<u8>, Error> {
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
            "core.quotePath=false",
            "--no-pager",
            "--git-dir=.git",
            "--work-tree=.",
        ])
        .args(args)
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| Error::Command)?;
    let mut pipe = child.stdout.take().expect("piped stdout");
    let deadline = Instant::now() + Duration::from_secs(2);
    let result = (|| {
        fcntl(pipe.as_raw_fd(), FcntlArg::F_SETFL(OFlag::O_NONBLOCK))
            .map_err(|_| Error::Command)?;
        let mut output = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            match pipe.read(&mut buf) {
                Ok(0) => {
                    if let Some(status) = child.try_wait().map_err(|_| Error::Command)? {
                        return status.success().then_some(output).ok_or(Error::Command);
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                Ok(n) if output.len() + n <= OUTPUT_LIMIT => output.extend_from_slice(&buf[..n]),
                Ok(_) => return Err(Error::OutputLimit),
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

/// HEAD may live in another administrative directory for worktrees and submodules.
pub fn head_ref(root: &Path) -> Option<Branch> {
    let head = if root.join(".git").symlink_metadata().ok()?.is_dir() {
        read_small(root, Path::new(".git/HEAD"), 4096)?
    } else {
        let pointer = read_small(root, Path::new(".git"), 4096)?;
        let dir = root.join(PathBuf::from(
            pointer.trim().strip_prefix("gitdir:")?.trim(),
        ));
        read_small(&dir.canonicalize().ok()?, Path::new("HEAD"), 4096)?
    };
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
    if let Some(Branch::Detached(sha)) =
        head_ref(root).filter(|b| matches!(b, Branch::Detached(sha) if valid_sha(sha)))
    {
        return Some(sha);
    }
    let bytes = run(root, &["rev-parse", "--verify", "HEAD"]).ok()?;
    let sha = std::str::from_utf8(&bytes).ok()?.trim();
    valid_sha(sha).then(|| sha.into())
}

pub fn clean_origin(value: &str) -> Option<String> {
    let value = value.trim();
    let (host, path) = if let Some((scheme, rest)) = value.split_once("://") {
        if !matches!(scheme, "http" | "https" | "ssh" | "git" | "git+ssh") {
            return None;
        }
        let (authority, path) = rest.split_once('/')?;
        (authority.rsplit('@').next()?, path)
    } else {
        let (_, value) = value.rsplit_once('@').unwrap_or(("", value));
        value.split_once(':')?
    };
    let host = host.split(':').next()?;
    let path = path.split(['?', '#']).next()?.trim_matches('/');
    if host.is_empty()
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
        || !path.contains('/')
        || path
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '@')
    {
        return None;
    }
    Some(short(&format!(
        "{host}/{}",
        path.strip_suffix(".git").unwrap_or(path)
    )))
}

pub(super) fn origin(root: &Path) -> Option<String> {
    let bytes = run(
        root,
        &[
            "config",
            "--local",
            "--no-includes",
            "--get",
            "remote.origin.url",
        ],
    )
    .ok()?;
    clean_origin(std::str::from_utf8(&bytes).ok()?)
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

fn blob_matches(root: &Path, data: &[u8], oid: &str) -> Option<bool> {
    use nix::sys::memfd::{MemFdCreateFlag, memfd_create};
    let mut input =
        File::from(memfd_create(c"wyss-panel-blob", MemFdCreateFlag::MFD_CLOEXEC).ok()?);
    input.write_all(data).ok()?;
    input.rewind().ok()?;
    let output = run_with_input(
        root,
        &["hash-object", "--no-filters", "--stdin"],
        input.into(),
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
    files: &mut Files<'_>,
    path: &Path,
    mode: &str,
    oid: &str,
    stat: IndexStat<'_>,
    filemode: bool,
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
            if filemode && (disk.mode() & 0o100 != 0) != (mode == "100755") {
                return Some(b'M');
            }
            if stat.matches(&disk) {
                return None;
            }
            let same = files
                .read(path)
                .and_then(|(data, _)| blob_matches(files.root, &data, oid));
            (same != Some(true)).then_some(b'M')
        }
        "120000" if disk.is_symlink() => {
            if stat.matches(&disk) {
                return None;
            }
            let same = fs::read_link(&disk_path)
                .ok()
                .and_then(|link| blob_matches(files.root, link.as_os_str().as_bytes(), oid));
            (same != Some(true)).then_some(b'M')
        }
        _ if disk.is_dir() => Some(b'D'),
        _ => Some(b'M'),
    }
}

fn index_changes(
    files: &mut Files<'_>,
    raw: &[u8],
    paths: &mut BTreeMap<Vec<u8>, u8>,
    filemode: bool,
) -> Result<(), Error> {
    let mut raw = raw;
    while !raw.is_empty() {
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
            files,
            Path::new(std::ffi::OsStr::from_bytes(path)),
            mode,
            oid,
            IndexStat { ctime, mtime, size },
            filemode,
        ) {
            record(paths, status, path);
        }
    }
    Ok(())
}

fn counts(
    root: &Path,
    files: &mut Files<'_>,
    has_head: bool,
) -> Result<(usize, usize, usize), Error> {
    // Plumbing avoids clean filters, submodule inspection, and external diff programs.
    let mut paths = BTreeMap::new();
    if has_head {
        statuses(
            &run(
                root,
                &[
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
                ],
            )?,
            &mut paths,
        );
    } else {
        for path in run(root, &["ls-files", "-z", "--cached"])?
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
        {
            record(&mut paths, b'A', path);
        }
    }
    // diff-files/status can invoke a clean filter even for name-only output.
    // Read the index without asking Git to compare it with the worktree.
    let filemode = run(
        root,
        &[
            "config",
            "--local",
            "--no-includes",
            "--type=bool",
            "--get",
            "core.filemode",
        ],
    )
    .map_or(true, |value| value != b"false\n");
    let index = run(root, &["ls-files", "-v", "--stage", "--debug", "-z"])?;
    index_changes(files, &index, &mut paths, filemode)?;
    for path in run(root, &["ls-files", "-z", "--others", "--exclude-standard"])?
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
    {
        paths.entry(path.to_vec()).or_insert(b'A');
    }
    Ok((
        paths.values().filter(|b| !matches!(b, b'A' | b'D')).count(),
        paths.values().filter(|b| **b == b'A').count(),
        paths.values().filter(|b| **b == b'D').count(),
    ))
}

pub fn git(root: &Path) -> Result<Git, Error> {
    let mut remaining = READ_LIMIT;
    git_with_budget(root, &mut remaining)
}

pub(super) fn git_with_budget(root: &Path, remaining: &mut usize) -> Result<Git, Error> {
    let branch = head_ref(root).ok_or(Error::Command)?;
    let history = run(
        root,
        &[
            "log",
            "-5",
            "-z",
            "--no-show-signature",
            "--no-notes",
            "--format=%H%x00%h%x00%s%x00%an%x00%at",
            "HEAD",
            "--",
        ],
    );
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
        Err(Error::Command) if head(root).is_none() => (None, Vec::new()),
        Err(e) => return Err(e),
    };
    let mut files = Files {
        remaining: *remaining,
        ..Files::new(root)
    };
    let changes = counts(root, &mut files, head.is_some());
    *remaining = files.remaining;
    let (modified, new, deleted) = changes?;
    let upstream = match run(
        root,
        &[
            "rev-list",
            "--count",
            "--left-right",
            "HEAD...@{upstream}",
            "--",
        ],
    ) {
        Ok(raw) => {
            let mut n = std::str::from_utf8(&raw).unwrap_or("").split_whitespace();
            n.next()
                .and_then(|a| a.parse().ok())
                .zip(n.next().and_then(|b| b.parse().ok()))
        }
        Err(Error::Command) => None,
        Err(e) => return Err(e),
    };
    let refs = |kind: &str, sort: &str, limit: usize| -> Result<Vec<String>, Error> {
        let raw = run(
            root,
            &[
                "for-each-ref",
                sort,
                &format!("--count={limit}"),
                "--format=%(refname:lstrip=2)%00%(symref)",
                kind,
            ],
        )?;
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
    })
}
