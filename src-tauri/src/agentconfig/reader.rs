//! The bounded, read-only reader of 07 §4.2. It opens nothing for writing, holds no
//! descriptor after it returns, reaches only the approved roots, and stops at every bound
//! of C-11 before it allocates past it. A failure is a closed word; no failure carries a
//! byte of the file.
use super::model::{Bounds, ContentHash, InvalidBounds, PathTrust, SourceFailure, TooLarge};
use super::place::{self, Want};
use super::roots::ApprovedRoots;
use nix::{dir::Dir, sys::stat::fstat};
use std::{
    ffi::OsString,
    fmt,
    fs::File,
    os::fd::{AsRawFd, IntoRawFd, OwnedFd},
    os::unix::ffi::OsStrExt,
    os::unix::fs::FileExt,
    path::{Path, PathBuf},
};

/// The bytes of a source, read twice and found equal.
pub struct ReadFile {
    bytes: Vec<u8>,
    hash: ContentHash,
    trust: PathTrust,
    links: Vec<PathBuf>,
}

impl ReadFile {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn hash(&self) -> ContentHash {
        self.hash
    }
    pub fn trust(&self) -> PathTrust {
        self.trust
    }
    /// The links that were followed to reach the file, the first one being the path
    /// that was asked for. Empty when it was not a link.
    pub fn links(&self) -> &[PathBuf] {
        &self.links
    }
}

/// Never prints the content: a debug line of a read source names its size and hash.
impl fmt::Debug for ReadFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReadFile")
            .field("len", &self.bytes.len())
            .field("hash", &self.hash)
            .field("trust", &self.trust)
            .field("links", &self.links)
            .finish()
    }
}

/// What a file's descriptor said about it before and after a read. Equal on both sides
/// means nothing wrote to it in between that the kernel recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Identity {
    dev: u64,
    ino: u64,
    size: i64,
    mtime: (i64, i64),
    ctime: (i64, i64),
}

fn identity(fd: &OwnedFd) -> Result<Identity, SourceFailure> {
    let stat = fstat(fd.as_raw_fd()).map_err(|_| SourceFailure::Unreadable)?;
    Ok(Identity {
        dev: stat.st_dev,
        ino: stat.st_ino,
        size: stat.st_size,
        mtime: (stat.st_mtime, stat.st_mtime_nsec),
        ctime: (stat.st_ctime, stat.st_ctime_nsec),
    })
}

/// Every byte from the start, never more than `limit` plus one: one more than the bound
/// is how a file that grew past it during the read is seen.
fn read_from_start(file: &File, limit: u64) -> Result<Vec<u8>, SourceFailure> {
    let ceiling = limit.saturating_add(1);
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let have = bytes.len() as u64;
        let room = ceiling.saturating_sub(have);
        if room == 0 {
            return Ok(bytes);
        }
        let want = chunk.len().min(usize::try_from(room).unwrap_or(usize::MAX));
        let read = file
            .read_at(&mut chunk[..want], have)
            .map_err(|_| SourceFailure::Unreadable)?;
        if read == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

/// Runs `attempt` until it returns a settled value, at most `attempts` times. `None`
/// means the file changed while it was being read.
pub(crate) fn settle<T>(
    attempts: usize,
    mut attempt: impl FnMut() -> Result<Option<T>, SourceFailure>,
) -> Result<T, SourceFailure> {
    for _ in 0..attempts {
        if let Some(value) = attempt()? {
            return Ok(value);
        }
    }
    Err(SourceFailure::Unstable)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    Link,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: OsString,
    pub kind: EntryKind,
}

/// The names in one directory, sorted, with the kind the directory records for each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub entries: Vec<Entry>,
    pub trust: PathTrust,
    pub links: Vec<PathBuf>,
}

/// The only way to read a provider's files. It holds the approved roots and the bounds,
/// so no read exists without either; it takes no path from the window, only from an
/// adapter that derived it from the roots.
#[derive(Debug, Clone)]
pub struct ConfigReader {
    roots: ApprovedRoots,
    bounds: Bounds,
}

impl ConfigReader {
    pub fn bounds(&self) -> &Bounds {
        &self.bounds
    }

    pub(super) fn roots(&self) -> &ApprovedRoots {
        &self.roots
    }

    pub fn new(roots: ApprovedRoots, bounds: Bounds) -> Result<Self, InvalidBounds> {
        Ok(Self {
            roots,
            bounds: bounds.validated()?,
        })
    }

    /// Reads one source file. A final link is followed hop by hop and every hop is
    /// checked against the roots and judged again; the file is read twice from one
    /// descriptor and accepted only if both reads and the descriptor's record agree. A
    /// file replaced while it is read is opened again.
    pub fn read_file(&self, path: &Path) -> Result<ReadFile, SourceFailure> {
        let bounds = &self.bounds;
        let too_large = SourceFailure::TooLarge {
            bound: TooLarge::File,
        };
        settle(bounds.read_attempts, || {
            let (fd, trust, links) = place::open(&self.roots, path, Want::File, bounds.link_hops)?;
            let before = identity(&fd)?;
            let size = u64::try_from(before.size).map_err(|_| SourceFailure::Unreadable)?;
            if size > bounds.file_bytes {
                return Err(too_large);
            }
            let file = File::from(fd);
            let first = read_from_start(&file, bounds.file_bytes)?;
            if first.len() as u64 > bounds.file_bytes {
                return Err(too_large);
            }
            let second = read_from_start(&file, bounds.file_bytes)?;
            let after = identity(&OwnedFd::from(file))?;
            let hash = ContentHash::of(&first);
            if before != after || first.len() as u64 != size || hash != ContentHash::of(&second) {
                return Ok(None);
            }
            Ok(Some(ReadFile {
                bytes: first,
                hash,
                trust,
                links,
            }))
        })
    }

    /// Lists a directory such as a skills folder. Stops at one entry past the bound.
    pub fn list_directory(&self, path: &Path) -> Result<Listing, SourceFailure> {
        let bounds = &self.bounds;
        let (fd, trust, links) = place::open(&self.roots, path, Want::Directory, bounds.link_hops)?;
        let mut dir = Dir::from_fd(fd.into_raw_fd()).map_err(|_| SourceFailure::Unreadable)?;
        let mut entries = Vec::new();
        for entry in dir.iter() {
            let entry = entry.map_err(|_| SourceFailure::Unreadable)?;
            let name = entry.file_name().to_bytes();
            if name == b"." || name == b".." {
                continue;
            }
            if entries.len() >= bounds.listing {
                return Err(SourceFailure::TooLarge {
                    bound: TooLarge::Listing,
                });
            }
            use nix::dir::Type;
            let kind = match entry.file_type() {
                Some(Type::File) => EntryKind::File,
                Some(Type::Directory) => EntryKind::Directory,
                Some(Type::Symlink) => EntryKind::Link,
                _ => EntryKind::Other,
            };
            entries.push(Entry {
                name: std::ffi::OsStr::from_bytes(name).to_owned(),
                kind,
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Listing {
            entries,
            trust,
            links,
        })
    }
}

/// Counts what a JSON document holds from its structure alone, outside strings, and says
/// whether it is within the bounds. Stops at the first excess, so a document built to
/// exhaust the parser (a hundred thousand brackets, a million keys) is refused without
/// being parsed and without recursion. Every key and every value is an element; each is
/// introduced by `{`, `[`, `,` or `:` or is the root, so the count never falls short.
fn json_shape_within(bytes: &[u8], bounds: &Bounds) -> Result<(), SourceFailure> {
    let (mut depth, mut elements) = (0_usize, 1_usize);
    let (mut in_string, mut escaped) = (false, false);
    for &byte in bytes {
        if in_string {
            match (escaped, byte) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => in_string = false,
                _ => (),
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth = depth.saturating_add(1);
                elements = elements.saturating_add(1);
                if depth > bounds.depth {
                    return Err(SourceFailure::TooLarge {
                        bound: TooLarge::Depth,
                    });
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            b',' | b':' => elements = elements.saturating_add(1),
            _ => (),
        }
        if elements > bounds.elements {
            return Err(SourceFailure::TooLarge {
                bound: TooLarge::Elements,
            });
        }
    }
    Ok(())
}

/// Parses a JSON source within the depth and element bounds. An empty or blank file is
/// `None`: the source is there and declares nothing. A malformed one reports where
/// parsing stopped and nothing of what was there (TS-CFG-03).
pub fn parse_json(
    bytes: &[u8],
    bounds: &Bounds,
) -> Result<Option<serde_json::Value>, SourceFailure> {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    json_shape_within(bytes, bounds)?;
    serde_json::from_slice(bytes)
        .map(Some)
        .map_err(|error| SourceFailure::Malformed {
            line: u32::try_from(error.line()).unwrap_or(u32::MAX),
            column: u32::try_from(error.column()).unwrap_or(u32::MAX),
        })
}

/// JSON with comments and trailing commas, as OpenCode reads its configuration. Comments
/// and the commas before a closing bracket become spaces, so every byte keeps its line and
/// column and a failure points where the user's file is, then it is read as strict JSON.
pub fn parse_jsonc(
    bytes: &[u8],
    bounds: &Bounds,
) -> Result<Option<serde_json::Value>, SourceFailure> {
    parse_json(&strip_jsonc(bytes), bounds)
}

fn strip_jsonc(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let (mut i, len) = (0, bytes.len());
    // The last comma outside a string and not yet followed by a value.
    let mut comma: Option<usize> = None;
    while i < len {
        match bytes[i] {
            b'"' => {
                comma = None;
                i += 1;
                while i < len && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < len && bytes[i] != b'\n' {
                    out[i] = b' ';
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let end = bytes[i + 2..]
                    .windows(2)
                    .position(|w| w == b"*/")
                    .map_or(len, |at| i + 2 + at + 2);
                for byte in &mut out[i..end] {
                    if *byte != b'\n' {
                        *byte = b' ';
                    }
                }
                i = end;
            }
            b',' => {
                comma = Some(i);
                i += 1;
            }
            b']' | b'}' => {
                if let Some(at) = comma.take() {
                    out[at] = b' ';
                }
                i += 1;
            }
            byte if byte.is_ascii_whitespace() => i += 1,
            _ => {
                comma = None;
                i += 1;
            }
        }
    }
    out
}

/// Whether a parsed document is within the depth and element bounds, counted without
/// recursion. Used for formats whose parser cannot be asked to stop early; the file size
/// bound already limits what the parser has built.
fn value_within(value: &serde_json::Value, bounds: &Bounds) -> Result<(), SourceFailure> {
    use serde_json::Value;
    let mut elements = 0_usize;
    let mut pending = vec![(value, 1_usize)];
    while let Some((value, depth)) = pending.pop() {
        elements = elements.saturating_add(1);
        match value {
            Value::Array(items) => {
                if depth >= bounds.depth {
                    return Err(SourceFailure::TooLarge {
                        bound: TooLarge::Depth,
                    });
                }
                pending.extend(items.iter().map(|item| (item, depth + 1)));
            }
            Value::Object(entries) => {
                if depth >= bounds.depth {
                    return Err(SourceFailure::TooLarge {
                        bound: TooLarge::Depth,
                    });
                }
                // A key is an element too, as in the JSON pre-scan.
                elements = elements.saturating_add(entries.len());
                pending.extend(entries.values().map(|item| (item, depth + 1)));
            }
            _ => (),
        }
        if elements > bounds.elements {
            return Err(SourceFailure::TooLarge {
                bound: TooLarge::Elements,
            });
        }
    }
    Ok(())
}

/// Parses a TOML source into the same value tree JSON gives, so one projection and one
/// redaction serve both. Dates become their text; a repeated key is malformed, as TOML
/// says. An empty file is `None`. A malformed one reports where parsing stopped and
/// nothing of what was there.
pub fn parse_toml(
    bytes: &[u8],
    bounds: &Bounds,
) -> Result<Option<serde_json::Value>, SourceFailure> {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    let text = std::str::from_utf8(bytes).map_err(|error| {
        let before = &bytes[..error.valid_up_to()];
        position(before)
    })?;
    let value: serde_json::Value = toml::from_str(text).map_err(|error| {
        error
            .span()
            .map_or(SourceFailure::Malformed { line: 1, column: 1 }, |span| {
                position(&bytes[..span.start.min(bytes.len())])
            })
    })?;
    value_within(&value, bounds)?;
    Ok(Some(value))
}

/// Where a position falls, as the line and column of the text before it.
fn position(before: &[u8]) -> SourceFailure {
    let line = before.iter().filter(|&&byte| byte == b'\n').count();
    let column = before
        .iter()
        .rev()
        .take_while(|&&byte| byte != b'\n')
        .count();
    SourceFailure::Malformed {
        line: u32::try_from(line + 1).unwrap_or(u32::MAX),
        column: u32::try_from(column + 1).unwrap_or(u32::MAX),
    }
}

/// The read loop on its own, for the test that gives it the largest bound there is.
#[cfg(test)]
pub(crate) fn read_bytes_for_test(file: &File, limit: u64) -> Vec<u8> {
    read_from_start(file, limit).unwrap()
}
