//! Read-only project information and the agent's local panel channel.
mod git;
mod manifests;
mod markdown;
mod markdown_blocks;
mod store;

pub use git::{Branch, Commit, Error as GitError, Git, clean_origin, git, head_ref};
pub use markdown::{Block, Inline, markdown};
pub use store::{AgentSection, cache_path, command, sections, watch_path};

use manifests::parse;
use nix::fcntl::{OFlag, openat};
use nix::sys::stat::Mode;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

const FILE_LIMIT: usize = 1 << 20;
const READ_LIMIT: usize = 32 << 20;
const MANIFESTS: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "Makefile",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub root: PathBuf,
    pub has_git: bool,
}

pub fn detect(cwd: &Path) -> Option<Project> {
    let home = std::env::var_os("HOME");
    detect_from(cwd, home.as_deref().map(Path::new))
}

fn detect_from(cwd: &Path, home: Option<&Path>) -> Option<Project> {
    let cwd = cwd.canonicalize().ok()?;
    let canonical_home = home.and_then(|path| path.canonicalize().ok());
    let home = canonical_home.as_deref().or(home);
    if let Some(relative) = home.and_then(|path| cwd.strip_prefix(path).ok())
        && relative.components().next().is_some_and(|part| {
            matches!(
                part.as_os_str().to_str(),
                Some(".cache" | ".local" | ".config")
            )
        })
    {
        return None;
    }
    // An external project can be detected without walking into a shared ancestor of HOME.
    let ancestors = cwd.ancestors().take_while(|dir| {
        dir.parent().is_some() && home.is_none_or(|home| *dir == home || !home.starts_with(dir))
    });
    detect_among(ancestors, home)
}

fn has_git_marker(root: &Path) -> bool {
    root.join(".git")
        .symlink_metadata()
        .is_ok_and(|m| m.is_dir() || m.is_file() || m.is_symlink())
}

fn detect_among<'a>(
    ancestors: impl Iterator<Item = &'a Path>,
    home: Option<&Path>,
) -> Option<Project> {
    let mut manifest = None;
    for dir in ancestors {
        if has_git_marker(dir) {
            return Some(Project {
                root: dir.into(),
                has_git: true,
            });
        }
        if Some(dir) != home
            && manifest.is_none()
            && MANIFESTS
                .iter()
                .any(|name| dir.join(name).symlink_metadata().is_ok_and(|m| m.is_file()))
        {
            manifest = Some(dir);
        }
    }
    manifest.map(|dir| Project {
        root: dir.into(),
        has_git: false,
    })
}

/// Resolve each component relative to a directory descriptor. O_NOFOLLOW also protects against a
/// link substituted between directory enumeration and the open. O_NONBLOCK rejects pipes safely.
const OPEN_FLAGS: OFlag = OFlag::O_RDONLY
    .union(OFlag::O_CLOEXEC)
    .union(OFlag::O_NOFOLLOW)
    .union(OFlag::O_NONBLOCK);

fn open_relative_fd(root: &Path, path: &Path, directory: bool) -> io::Result<File> {
    let fd = nix::fcntl::open(root, OPEN_FLAGS | OFlag::O_DIRECTORY, Mode::empty())?;
    // SAFETY: open and openat return fresh, owned descriptors on success.
    let mut dir = unsafe { File::from_raw_fd(fd) };
    let mut parts = path.components().peekable();
    while let Some(part) = parts.next() {
        let Component::Normal(name) = part else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid file path",
            ));
        };
        let extra = if directory || parts.peek().is_some() {
            OFlag::O_DIRECTORY
        } else {
            OFlag::empty()
        };
        let fd = openat(
            Some(dir.as_raw_fd()),
            name,
            OPEN_FLAGS | extra,
            Mode::empty(),
        )?;
        dir = unsafe { File::from_raw_fd(fd) };
    }
    Ok(dir)
}

pub(crate) fn open_relative(root: &Path, path: &Path) -> io::Result<File> {
    let file = open_relative_fd(root, path, false)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    Ok(file)
}

fn read_dir(root: &Path, relative: &Path) -> io::Result<fs::ReadDir> {
    let dir = open_relative_fd(root, relative, true)?;
    fs::read_dir(format!("/proc/self/fd/{}", dir.as_raw_fd()))
}

fn bounded(file: &mut impl Read, limit: usize) -> io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    let truncated = bytes.len() > limit;
    bytes.truncate(limit);
    Ok((bytes, truncated))
}

pub struct Scan<'a> {
    deadline: Instant,
    cancelled: &'a dyn Fn() -> bool,
    hashes: Cell<usize>,
    workflows: Cell<usize>,
    limited: Cell<bool>,
}

impl<'a> Scan<'a> {
    pub fn new(cancelled: &'a dyn Fn() -> bool) -> Self {
        Self {
            deadline: Instant::now() + Duration::from_millis(1500),
            cancelled,
            hashes: Cell::new(2000),
            workflows: Cell::new(200),
            limited: Cell::new(false),
        }
    }

    fn check(&self) -> Result<(), GitError> {
        if (self.cancelled)() || Instant::now() >= self.deadline {
            self.limited.set(true);
            Err(GitError::Timeout)
        } else {
            Ok(())
        }
    }

    fn take(&self, remaining: &Cell<usize>) -> Option<()> {
        self.check().ok()?;
        if let Some(next) = remaining.get().checked_sub(1) {
            remaining.set(next);
            Some(())
        } else {
            self.limited.set(true);
            None
        }
    }
}

struct Files<'a> {
    root: &'a Path,
    remaining: usize,
    limited: bool,
}

impl<'a> Files<'a> {
    fn new(root: &'a Path) -> Self {
        Self {
            root,
            remaining: READ_LIMIT,
            limited: false,
        }
    }

    fn read(&mut self, path: &Path) -> Option<(Vec<u8>, fs::Metadata)> {
        let file = open_relative(self.root, path).ok()?;
        let meta = file.metadata().ok()?;
        let limit = self.remaining.min(FILE_LIMIT);
        // Never read an oversized static file. If a normal file grows, charge all bytes read.
        if meta.len() > limit as u64 || limit == 0 {
            self.limited = true;
            return None;
        }
        let mut data = Vec::new();
        let result = file.take(limit as u64).read_to_end(&mut data);
        self.remaining -= data.len();
        result.ok()?;
        if data.len() as u64 != meta.len() {
            self.limited = true;
            return None;
        }
        Some((data, meta))
    }

    fn text(&mut self, path: &Path) -> Option<String> {
        let (data, _) = self.read(path)?;
        String::from_utf8(data).ok()
    }
}

#[derive(Default)]
pub struct Summary {
    pub name: String,
    pub readme: Option<String>,
    pub license: Option<String>,
    pub origin: Option<String>,
}

fn license(text: &str) -> Option<String> {
    if let Some(id) = text.lines().take(12).find_map(|l| {
        l.split_once("SPDX-License-Identifier:")
            .map(|(_, s)| s.trim())
    }) && !id.is_empty()
    {
        return Some(short(id));
    }
    let prefix = text
        .get(..text.floor_char_boundary(512))
        .unwrap_or(text)
        .to_ascii_lowercase();
    [
        ("mit license", "MIT"),
        ("permission is hereby granted, free of charge", "MIT"),
        ("apache license", "Apache-2.0"),
        ("mozilla public license", "MPL-2.0"),
        ("gnu general public license", "GPL-3.0-only"),
        ("boost software license", "BSL-1.0"),
        ("unlicense", "Unlicense"),
    ]
    .into_iter()
    .find(|(name, _)| {
        prefix.contains(name)
            && (*name != "gnu general public license" || prefix.contains("version 3"))
    })
    .map(|(_, id)| id.into())
}

pub fn overview(project: &Project) -> (Summary, Option<Result<Git, GitError>>, usize) {
    overview_with_scan(project, &Scan::new(&|| false))
}

pub fn overview_with_scan(
    project: &Project,
    scan: &Scan<'_>,
) -> (Summary, Option<Result<Git, GitError>>, usize) {
    let (mut summary, mut remaining) = summary(project, scan);
    let git = (project.has_git && has_git_marker(&project.root)).then(|| {
        let (origin, result) = git::snapshot(&project.root, &mut remaining, scan);
        summary.origin = origin;
        result
    });
    (summary, git, remaining)
}

impl Project {
    pub fn read_git(&self) -> Option<Result<Git, GitError>> {
        self.read_git_with_scan(&Scan::new(&|| false))
    }

    pub fn read_git_with_scan(&self, scan: &Scan<'_>) -> Option<Result<Git, GitError>> {
        (self.has_git && has_git_marker(&self.root)).then(|| {
            let mut remaining = READ_LIMIT;
            git::snapshot(&self.root, &mut remaining, scan).1
        })
    }
}

fn summary(project: &Project, scan: &Scan<'_>) -> (Summary, usize) {
    let mut files = Files::new(&project.root);
    let readme = [
        "README.md",
        "README",
        "README.txt",
        "README.markdown",
        "readme.md",
    ]
    .into_iter()
    .take_while(|_| scan.check().is_ok())
    .find_map(|p| {
        let file = open_relative(&project.root, Path::new(p)).ok()?;
        let mut prefix = Vec::new();
        file.take(markdown::README_LIMIT as u64)
            .read_to_end(&mut prefix)
            .ok()?;
        files.remaining -= prefix.len();
        let text = match std::str::from_utf8(&prefix) {
            Ok(text) => text,
            Err(e) if e.error_len().is_none() && prefix.len() == markdown::README_LIMIT => {
                std::str::from_utf8(&prefix[..e.valid_up_to()]).ok()?
            }
            Err(_) => return None,
        };
        Some(markdown::readme_excerpt(text))
    })
    .flatten();
    let license = ["LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING"]
        .into_iter()
        .take_while(|_| scan.check().is_ok())
        .find_map(|p| license(&files.text(Path::new(p))?))
        .or_else(|| {
            MANIFESTS
                .iter()
                .take_while(|_| scan.check().is_ok())
                .find_map(|p| parse(p, &files.text(Path::new(p))?).license)
        });
    let overview = Summary {
        name: project
            .root
            .file_name()
            .unwrap_or(project.root.as_os_str())
            .to_string_lossy()
            .into_owned(),
        readme,
        license,
        origin: None,
    };
    (overview, files.remaining)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    pub file: String,
    pub package: Option<String>,
    pub version: Option<String>,
    pub edition: Vec<(String, String)>,
    pub dependencies: Vec<(String, String)>,
    pub runs: Vec<(String, String)>,
    pub license: Option<String>,
    pub quality: Vec<String>,
}

#[derive(Default)]
pub struct Inventory {
    pub manifests: Vec<Manifest>,
    pub languages: Vec<(&'static str, u64)>,
    pub frameworks: Vec<&'static str>,
    pub directories: Vec<(String, usize, usize)>,
    pub runs: Vec<(String, String)>,
    pub workflows: Vec<(String, String)>,
    pub quality: Vec<String>,
    pub limited: bool,
    pub listing_error: Option<git::Error>,
}

fn language(path: &Path) -> Option<&'static str> {
    Some(match path.extension()?.to_str()? {
        "rs" => "Rust",
        "c" | "h" => "C",
        "cpp" | "cc" | "cxx" | "hpp" => "C++",
        "py" | "pyi" => "Python",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "ts" | "tsx" | "mts" | "cts" => "TypeScript",
        "go" => "Go",
        "sh" | "bash" => "Shell",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "svelte" => "Svelte",
        "vue" => "Vue",
        "cs" => "C#",
        "rb" => "Ruby",
        "swift" => "Swift",
        "php" => "PHP",
        "css" | "scss" => "CSS",
        "html" => "HTML",
        "sql" => "SQL",
        "zig" => "Zig",
        "lua" => "Lua",
        _ => return None,
    })
}

fn walk(
    dir: &Path,
    prefix: &Path,
    depth: usize,
    budget: &mut usize,
    out: &mut Vec<PathBuf>,
    scan: &Scan<'_>,
) {
    if depth >= 32 || *budget == 0 || scan.check().is_err() {
        return;
    }
    for entry in read_dir(dir, prefix)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
    {
        if *budget == 0 || scan.check().is_err() {
            break;
        }
        *budget -= 1;
        let name = entry.file_name();
        if !matches!(
            name.to_str(),
            Some(".git" | "target" | "node_modules" | ".venv" | "dist" | "build")
        ) && let Ok(ty) = entry.file_type()
        {
            let path = prefix.join(name);
            if ty.is_file() {
                out.push(path);
            } else if ty.is_dir() {
                walk(dir, &path, depth + 1, budget, out, scan);
            }
        }
    }
}

pub fn inventory(project: &Project, remaining: usize, cancelled: impl Fn() -> bool) -> Inventory {
    inventory_with_scan(project, remaining, &Scan::new(&cancelled))
}

pub fn inventory_with_scan(project: &Project, remaining: usize, scan: &Scan<'_>) -> Inventory {
    let mut inv = Inventory::default();
    let mut files = Files {
        remaining: remaining.min(READ_LIMIT),
        ..Files::new(&project.root)
    };
    for name in MANIFESTS.iter().take_while(|_| scan.check().is_ok()) {
        if let Some(text) = files.text(Path::new(name)) {
            let manifest = parse(name, &text);
            inv.quality.extend(manifest.quality.iter().cloned());
            inv.runs.extend(manifest.runs.iter().cloned());
            inv.manifests.push(manifest);
        }
    }
    for name in ["AGENTS.md", "README.md", "README", "README.markdown"]
        .into_iter()
        .take_while(|_| scan.check().is_ok())
    {
        if let Some(text) = files.text(Path::new(name)) {
            inv.runs.extend(
                markdown::commands(&text)
                    .into_iter()
                    .take(
                        10 - inv
                            .runs
                            .iter()
                            .filter(|(source, _)| source == "docs")
                            .count(),
                    )
                    .map(|line| ("docs".into(), line)),
            );
        }
    }
    let mut entries = 32768;
    for entry in fs::read_dir(&project.root)
        .into_iter()
        .flatten()
        .take_while(|_| scan.check().is_ok())
    {
        if entries == 0 {
            inv.limited = true;
            break;
        }
        entries -= 1;
        let Ok(entry) = entry else { continue };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if quality(&name) && entry.file_type().is_ok_and(|ty| ty.is_file()) {
            inv.quality.push(short(&name));
        }
        if name.ends_with(".sh")
            && entry.file_type().is_ok_and(|ty| ty.is_file())
            && entry
                .metadata()
                .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
        {
            inv.runs
                .push(("script".into(), format!("./{}", short(&name))));
        }
    }
    for entry in read_dir(&project.root, Path::new(".github/workflows"))
        .into_iter()
        .flatten()
    {
        if scan.take(&scan.workflows).is_none() {
            break;
        }
        let Ok(entry) = entry else { continue };
        let path = Path::new(".github/workflows").join(entry.file_name());
        if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("yml" | "yaml")
        ) && let Some(text) = files.text(&path)
        {
            inv.workflows.push(manifests::workflow(
                entry.file_name().to_string_lossy().as_ref(),
                &text,
            ));
        }
    }
    inv.workflows.sort();
    inv.quality.sort();
    inv.quality.dedup();
    inv.quality.truncate(32);

    let mut paths = Vec::new();
    if project.has_git && has_git_marker(&project.root) {
        match git::tracked(&project.root, scan) {
            Ok(bytes) => paths.extend(
                bytes
                    .split(|b| *b == 0)
                    .filter(|p| !p.is_empty())
                    .map(|p| PathBuf::from(std::ffi::OsStr::from_bytes(p))),
            ),
            Err(git::Error::Timeout | git::Error::OutputLimit) => {}
            Err(error) => inv.listing_error = Some(error),
        }
    } else {
        walk(
            &project.root,
            Path::new(""),
            0,
            &mut entries,
            &mut paths,
            scan,
        );
        inv.limited |= entries == 0;
    }
    paths.sort_unstable();
    paths.dedup();
    let mut langs = BTreeMap::new();
    let mut dirs = BTreeMap::<String, (usize, usize)>::new();
    for path in paths {
        if scan.check().is_err() {
            break;
        }
        if files.remaining == 0 {
            files.limited = true;
            break;
        }
        let Some((bytes, meta)) = files.read(&path) else {
            continue;
        };
        if let Some(lang) = language(&path) {
            *langs.entry(lang).or_insert(0) += meta.len();
        }
        let mut parts = path.components();
        if let Some(Component::Normal(top)) = parts.next()
            && parts.next().is_some()
        {
            let (count, lines) = dirs.entry(top.to_string_lossy().into_owned()).or_default();
            *count += 1;
            if !bytes.contains(&0) {
                *lines += bytes.iter().filter(|b| **b == b'\n').count()
                    + usize::from(bytes.last().is_some_and(|b| *b != b'\n'));
            }
        }
    }
    inv.directories = dirs
        .into_iter()
        .take(20)
        .map(|(name, (files, lines))| (short(&name), files, lines))
        .collect();
    inv.languages = langs.into_iter().filter(|(_, size)| *size > 0).collect();
    inv.languages
        .sort_by_key(|(_, size)| std::cmp::Reverse(*size));
    inv.frameworks = manifests::frameworks(&inv.manifests);
    for manifest in &mut inv.manifests {
        manifest.dependencies.truncate(10);
    }
    inv.limited |= files.limited || scan.limited.get();
    inv
}

/// Bound strings placed into individual widgets and hide control characters.
fn short(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(512)
        .collect()
}

#[cfg(test)]
mod tests;

fn quality(name: &str) -> bool {
    [
        "clippy.toml",
        ".clippy.toml",
        "rustfmt.toml",
        ".rustfmt.toml",
        ".eslintrc",
        "eslint.config",
        "ruff.toml",
        ".ruff.toml",
        ".prettierrc",
        "prettier.config",
        "jest.config",
        "vitest.config",
        "pytest.ini",
        ".pytest.ini",
        "tox.ini",
    ]
    .iter()
    .any(|key| name == *key || name.strip_prefix(key).is_some_and(|s| s.starts_with('.')))
}
