//! The task's tree and what comes out of it. `materialize` builds a fresh tree from an admitted
//! snapshot (new inodes, bytes copied, nothing linked to the source); `Workspace::export` reads it
//! back after the worker is gone, with no path followed and no node opened that could block, and
//! turns it into typed, inert records (`B1ExportRecordV1`) plus the content blobs; `quarantine`
//! publishes them once, to a store of their own that is not the source repository.
//!
//! The worker's `.git` is its own: the exporter never opens it, never runs git, and says that it
//! skipped it. A pending result is data for the owner to review, not an integration.

use crate::contracts::b1::{
    B1Error, B1ErrorCode, B1Result, MAX_ENTRIES, MAX_INPUT_BYTES, MAX_TOTAL_CONTENT_BYTES,
    Sha256Hex,
    admission::{AdmittedSnapshot, SnapshotContent, TaskContext},
    digest,
    path::{B1LinkTarget, B1RelPath, validate_case_distinct, validate_destination_set},
    records::{B1ExportEntryV1, B1ExportRecordV1, ExportOperation},
    source::{EXECUTABLE_MODE, OverlayContent, REGULAR_MODE},
};
use nix::{
    dir::Dir,
    errno::Errno,
    fcntl::{AtFlags, OFlag, openat, readlinkat},
    sys::stat::{FileStat, Mode, SFlag, fstat, fstatat, mkdirat},
    unistd::symlinkat,
};
use std::{
    collections::BTreeMap,
    ffi::CString,
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::ffi::OsStrExt,
    },
    path::{Path, PathBuf},
};

fn err(field: &'static str, code: B1ErrorCode) -> B1Error {
    B1Error::new(field, code)
}

/// What a node was when the tree was built.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Initial {
    Regular {
        sha256: Sha256Hex,
        size: u64,
        mode: u32,
    },
    Symlink {
        target: Vec<u8>,
    },
}

#[derive(Debug)]
pub struct Workspace {
    root: PathBuf,
    /// The directory itself, held open from the moment it was made: what is exported is this object,
    /// whatever happens to its path.
    root_fd: OwnedFd,
    dev: u64,
    ino: u64,
    initial: BTreeMap<B1RelPath, Initial>,
    base_admission: Sha256Hex,
    source_tree: Sha256Hex,
    context: TaskContext,
}

fn c(bytes: &[u8]) -> B1Result<CString> {
    CString::new(bytes).map_err(|_| err("path", B1ErrorCode::PathUnsafe))
}

fn open_dir_at(parent: RawFd, name: &[u8]) -> B1Result<OwnedFd> {
    let name = c(name)?;
    let fd = openat(
        Some(parent),
        name.as_c_str(),
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| err("path", B1ErrorCode::PathUnsafe))?;
    // SAFETY: a descriptor just opened and owned by nobody else.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn ensure_dir_at(parent: RawFd, name: &[u8]) -> B1Result<OwnedFd> {
    let cname = c(name)?;
    match mkdirat(
        Some(parent),
        cname.as_c_str(),
        Mode::from_bits_truncate(0o700),
    ) {
        Ok(()) | Err(Errno::EEXIST) => {}
        Err(_) => return Err(err("path", B1ErrorCode::PathUnsafe)),
    }
    open_dir_at(parent, name)
}

/// Builds the tree of `snapshot` under a new private directory inside `parent`.
pub fn materialize(
    parent: &Path,
    snapshot: &AdmittedSnapshot,
    context: &TaskContext,
) -> B1Result<Workspace> {
    snapshot.verify(context)?;
    let live: Vec<_> = snapshot
        .entries()
        .iter()
        .filter(|e| !matches!(e.content(), SnapshotContent::Tombstone))
        .collect();
    validate_destination_set(live.iter().map(|e| e.path()))?;
    validate_case_distinct(live.iter().map(|e| e.path()))?;
    let parent = crate::b1::plan::pin_directory(parent)
        .map_err(|_| err("workspace", B1ErrorCode::IsolationUnavailable))?;
    let name = format!("task-{}", uuid::Uuid::new_v4().simple());
    let cname = c(name.as_bytes())?;
    // The directory is made through the descriptor of the parent that was checked.
    mkdirat(
        Some(parent.fd.as_raw_fd()),
        cname.as_c_str(),
        Mode::from_bits_truncate(0o700),
    )
    .map_err(|_| err("workspace", B1ErrorCode::IsolationUnavailable))?;
    let root = parent.real.join(&name);
    let root_fd = open_dir_at(parent.fd.as_raw_fd(), name.as_bytes())?;
    let meta = fstat(root_fd.as_raw_fd())
        .map_err(|_| err("workspace", B1ErrorCode::IsolationUnavailable))?;
    let built = build(root_fd.as_raw_fd(), &live);
    match built {
        Ok(initial) => Ok(Workspace {
            root,
            root_fd,
            dev: meta.st_dev,
            ino: meta.st_ino,
            initial,
            base_admission: digest::admission_record(snapshot.record())?,
            source_tree: snapshot.record().admitted_tree_sha256.clone(),
            context: context.clone(),
        }),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&root);
            Err(e)
        }
    }
}

fn build(
    root: RawFd,
    live: &[&crate::contracts::b1::admission::SnapshotEntry],
) -> B1Result<BTreeMap<B1RelPath, Initial>> {
    let rootfd = open_dir_at(root, b".")?;
    let mut initial = BTreeMap::new();
    for entry in live {
        let parts: Vec<&[u8]> = entry.path().components().collect();
        let (name, dirs) = parts
            .split_last()
            .ok_or(err("path", B1ErrorCode::PathUnsafe))?;
        let mut held: Vec<OwnedFd> = Vec::new();
        for part in dirs {
            let at = held.last().map_or(rootfd.as_raw_fd(), |f| f.as_raw_fd());
            held.push(ensure_dir_at(at, part)?);
        }
        let at = held.last().map_or(rootfd.as_raw_fd(), |f| f.as_raw_fd());
        let cname = c(name)?;
        match entry.content() {
            SnapshotContent::Regular {
                mode,
                bytes,
                sha256,
            } => {
                let executable = *mode == EXECUTABLE_MODE;
                let fd = openat(
                    Some(at),
                    cname.as_c_str(),
                    OFlag::O_WRONLY
                        | OFlag::O_CREAT
                        | OFlag::O_EXCL
                        | OFlag::O_NOFOLLOW
                        | OFlag::O_CLOEXEC,
                    Mode::from_bits_truncate(if executable { 0o700 } else { 0o600 }),
                )
                .map_err(|_| err("path", B1ErrorCode::SourceCollision))?;
                // SAFETY: a descriptor just opened and owned by nobody else.
                let mut file = unsafe { File::from_raw_fd(fd) };
                file.write_all(bytes)
                    .map_err(|_| err("content", B1ErrorCode::IsolationUnavailable))?;
                initial.insert(
                    entry.path().clone(),
                    Initial::Regular {
                        sha256: sha256.clone(),
                        size: bytes.len() as u64,
                        mode: *mode,
                    },
                );
            }
            SnapshotContent::Symlink { target } => {
                B1LinkTarget::from_bytes(target)?.validate_for(entry.path())?;
                let target_c = c(target)?;
                symlinkat(target_c.as_c_str(), Some(at), cname.as_c_str())
                    .map_err(|_| err("path", B1ErrorCode::SourceCollision))?;
                initial.insert(
                    entry.path().clone(),
                    Initial::Symlink {
                        target: target.clone(),
                    },
                );
            }
            SnapshotContent::Tombstone => {}
        }
    }
    // Independent objects: every file has exactly one name and the size that was written.
    verify_independent(
        rootfd.as_raw_fd(),
        &mut Vec::new(),
        &initial,
        0,
        &mut Budget::default(),
    )?;
    Ok(initial)
}

/// Everything the exporter will look at, counted over the whole tree and before it is read: a worker
/// that leaves millions of empty directories or names cannot make the supervisor spend more than this.
#[derive(Default)]
struct Budget {
    nodes: usize,
    name_bytes: usize,
}

/// Directories and files and links together; far above `MAX_ENTRIES` (which counts only what is exported).
const MAX_NODES: usize = 8 * MAX_ENTRIES;
const MAX_NAME_BYTES: usize = 1024 * 1024;

impl Budget {
    fn count(&mut self, name: &[u8]) -> B1Result {
        self.nodes += 1;
        self.name_bytes += name.len();
        if self.nodes > MAX_NODES || self.name_bytes > MAX_NAME_BYTES {
            return Err(err("entries", B1ErrorCode::LimitExceeded));
        }
        Ok(())
    }
}

/// The names in one directory, counted against the budget while they are read: the listing is
/// never collected past it.
fn names_in(d: &mut Dir, budget: &mut Budget) -> B1Result<Vec<Vec<u8>>> {
    let mut names = Vec::new();
    for entry in d.iter() {
        let entry = entry.map_err(|_| err("path", B1ErrorCode::ExportNodeUnsafe))?;
        let name = entry.file_name().to_bytes();
        if name == b"." || name == b".." {
            continue;
        }
        budget.count(name)?;
        names.push(name.to_vec());
    }
    Ok(names)
}

fn is(st: &FileStat, kind: SFlag) -> bool {
    SFlag::from_bits_truncate(st.st_mode & SFlag::S_IFMT.bits()) == kind
}

fn verify_independent(
    dir: RawFd,
    prefix: &mut Vec<u8>,
    initial: &BTreeMap<B1RelPath, Initial>,
    depth: usize,
    budget: &mut Budget,
) -> B1Result {
    if depth > 40 {
        return Err(err("path", B1ErrorCode::LimitExceeded));
    }
    let fd = open_dir_at(dir, b".")?;
    let mut d = Dir::from_fd(fd.as_raw_fd()).map_err(|_| err("path", B1ErrorCode::PathUnsafe))?;
    // `Dir` owns the descriptor now.
    std::mem::forget(fd);
    let names = names_in(&mut d, budget)?;
    for name in names {
        let cname = c(&name)?;
        let st = fstatat(
            Some(d.as_raw_fd()),
            cname.as_c_str(),
            AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(|_| err("path", B1ErrorCode::PathUnsafe))?;
        let mark = prefix.len();
        if !prefix.is_empty() {
            prefix.push(b'/');
        }
        prefix.extend_from_slice(&name);
        if is(&st, SFlag::S_IFDIR) {
            let sub = open_dir_at(d.as_raw_fd(), &name)?;
            verify_independent(sub.as_raw_fd(), prefix, initial, depth + 1, budget)?;
        } else if is(&st, SFlag::S_IFREG) {
            let path = B1RelPath::from_bytes(prefix)?;
            let expected = match initial.get(&path) {
                Some(Initial::Regular { size, .. }) => *size,
                _ => return Err(err("path", B1ErrorCode::UnselectedContent)),
            };
            if st.st_nlink != 1 || st.st_size as u64 != expected {
                return Err(err("path", B1ErrorCode::HardlinkOrAliasUnsafe));
            }
        }
        prefix.truncate(mark);
    }
    Ok(())
}

impl Workspace {
    pub fn path(&self) -> &Path {
        &self.root
    }

    /// (device, inode) of the directory, for a launch plan that must run in this object.
    pub fn identity(&self) -> (u64, u64) {
        (self.dev, self.ino)
    }

    /// Removes the tree. Only ever the directory this value created.
    pub fn remove(self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }

    /// Reads the tree and describes what changed since it was built. The worker must already be
    /// gone (the caller proves it); anything that could block or lead out of the tree makes the
    /// whole export fail with a closed code, and nothing is partially returned.
    pub fn export(&self) -> B1Result<Export> {
        let rootfd = open_dir_at(self.root_fd.as_raw_fd(), b".")?;
        let mut found: BTreeMap<B1RelPath, Found> = BTreeMap::new();
        let mut skipped_git = 0u32;
        let mut total = 0u64;
        scan(
            rootfd.as_raw_fd(),
            &mut Vec::new(),
            &mut found,
            &mut skipped_git,
            &mut total,
            0,
            &mut Budget::default(),
        )?;
        let mut entries: Vec<B1ExportEntryV1> = Vec::new();
        let mut blobs: BTreeMap<String, (Sha256Hex, Vec<u8>)> = BTreeMap::new();
        for (path, now) in &found {
            let operation = match (self.initial.get(path), now) {
                (None, _) => ExportOperation::Add,
                (
                    Some(Initial::Regular { sha256, mode, .. }),
                    Found::Regular { bytes, mode: m },
                ) => {
                    if *sha256 == digest::raw_sha256(bytes) && mode == m {
                        continue;
                    }
                    ExportOperation::Modify
                }
                (Some(Initial::Symlink { target }), Found::Symlink { target: t }) => {
                    if target == t {
                        continue;
                    }
                    ExportOperation::Modify
                }
                // A file became a link or a link became a file.
                (Some(_), _) => ExportOperation::Modify,
            };
            let content = match now {
                Found::Regular { bytes, mode } => {
                    let sha = digest::raw_sha256(bytes);
                    blobs.insert(sha.as_str().to_owned(), (sha.clone(), bytes.clone()));
                    OverlayContent::Regular {
                        content_sha256: sha,
                        size: bytes.len() as u64,
                        mode: *mode,
                    }
                }
                Found::Symlink { target } => OverlayContent::Symlink {
                    target: B1LinkTarget::from_bytes(target)?,
                },
            };
            entries.push(B1ExportEntryV1 {
                path: path.clone(),
                operation,
                content: Some(content),
            });
        }
        for path in self.initial.keys() {
            if !found.contains_key(path) {
                entries.push(B1ExportEntryV1 {
                    path: path.clone(),
                    operation: ExportOperation::Delete,
                    content: None,
                });
            }
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        let record = B1ExportRecordV1 {
            schema_version: crate::contracts::b1::SchemaV1,
            task_id: self.context.task_id().clone(),
            generation: self.context.generation(),
            base_admission_sha256: self.base_admission.clone(),
            entries,
            exporter_sha256: digest::raw_sha256(EXPORTER_IDENTITY),
            source_tree_sha256: self.source_tree.clone(),
        };
        record.validate()?;
        Ok(Export {
            record,
            blobs: blobs.into_values().collect(),
            skipped_git_directories: skipped_git,
        })
    }
}

/// Names this exporter in the record it produces.
const EXPORTER_IDENTITY: &[u8] = b"wyss-console.b1.exporter.v1";

#[derive(Debug)]
enum Found {
    Regular { bytes: Vec<u8>, mode: u32 },
    Symlink { target: Vec<u8> },
}

fn scan(
    dir: RawFd,
    prefix: &mut Vec<u8>,
    found: &mut BTreeMap<B1RelPath, Found>,
    skipped_git: &mut u32,
    total: &mut u64,
    depth: usize,
    budget: &mut Budget,
) -> B1Result {
    if depth > crate::contracts::b1::MAX_PATH_DEPTH {
        return Err(err("path", B1ErrorCode::LimitExceeded));
    }
    let fd = open_dir_at(dir, b".")?;
    let mut d = Dir::from_fd(fd.as_raw_fd()).map_err(|_| err("path", B1ErrorCode::PathUnsafe))?;
    std::mem::forget(fd);
    let mut names = names_in(&mut d, budget)?;
    names.sort();
    for name in names {
        let mark = prefix.len();
        if !prefix.is_empty() {
            prefix.push(b'/');
        }
        prefix.extend_from_slice(&name);
        let cname = c(&name)?;
        let st = fstatat(
            Some(d.as_raw_fd()),
            cname.as_c_str(),
            AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(|_| err("path", B1ErrorCode::ExportNodeUnsafe))?;
        if name.eq_ignore_ascii_case(b".git") {
            // The worker's own repository: never opened, never run, only counted.
            *skipped_git += 1;
        } else if is(&st, SFlag::S_IFDIR) {
            let sub = open_dir_at(d.as_raw_fd(), &name)?;
            scan(
                sub.as_raw_fd(),
                prefix,
                found,
                skipped_git,
                total,
                depth + 1,
                budget,
            )?;
        } else if is(&st, SFlag::S_IFREG) {
            if st.st_size < 0 || st.st_size as u64 > MAX_INPUT_BYTES {
                return Err(err("size", B1ErrorCode::LimitExceeded));
            }
            let fdr = openat(
                Some(d.as_raw_fd()),
                cname.as_c_str(),
                OFlag::O_RDONLY
                    | OFlag::O_NOFOLLOW
                    | OFlag::O_NONBLOCK
                    | OFlag::O_NOCTTY
                    | OFlag::O_CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| err("path", B1ErrorCode::ExportNodeUnsafe))?;
            // SAFETY: a descriptor just opened and owned by nobody else.
            let mut file = unsafe { File::from_raw_fd(fdr) };
            let opened =
                fstat(file.as_raw_fd()).map_err(|_| err("path", B1ErrorCode::ExportNodeUnsafe))?;
            // The thing opened is the thing that was classified.
            if !is(&opened, SFlag::S_IFREG)
                || opened.st_ino != st.st_ino
                || opened.st_dev != st.st_dev
            {
                return Err(err("path", B1ErrorCode::ExportNodeUnsafe));
            }
            let mut bytes = Vec::with_capacity(st.st_size as usize);
            (&mut file)
                .take(MAX_INPUT_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| err("path", B1ErrorCode::ExportNodeUnsafe))?;
            if bytes.len() as u64 != st.st_size as u64 {
                // It grew or shrank while it was being read: someone is still writing.
                return Err(err("path", B1ErrorCode::TaskTreeNotQuiesced));
            }
            *total += bytes.len() as u64;
            if *total > MAX_TOTAL_CONTENT_BYTES || found.len() >= MAX_ENTRIES {
                return Err(err("entries", B1ErrorCode::LimitExceeded));
            }
            let path = B1RelPath::from_bytes(prefix)
                .map_err(|_| err("path", B1ErrorCode::ExportNodeUnsafe))?;
            let mode = if st.st_mode & 0o111 != 0 {
                EXECUTABLE_MODE
            } else {
                REGULAR_MODE
            };
            found.insert(path, Found::Regular { bytes, mode });
        } else if is(&st, SFlag::S_IFLNK) {
            let target = readlinkat(Some(d.as_raw_fd()), cname.as_c_str())
                .map_err(|_| err("path", B1ErrorCode::ExportNodeUnsafe))?;
            let target = target.as_os_str().as_bytes().to_vec();
            let path = B1RelPath::from_bytes(prefix)
                .map_err(|_| err("path", B1ErrorCode::ExportNodeUnsafe))?;
            // Data, with the same rule as on the way in: relative, inside the tree, no `.git`.
            B1LinkTarget::from_bytes(&target)?
                .validate_for(&path)
                .map_err(|_| err("path", B1ErrorCode::ExportNodeUnsafe))?;
            if found.len() >= MAX_ENTRIES {
                return Err(err("entries", B1ErrorCode::LimitExceeded));
            }
            found.insert(path, Found::Symlink { target });
        } else {
            // A FIFO, socket or device: never opened, so it cannot block, and it is not data.
            return Err(err("path", B1ErrorCode::ExportNodeUnsafe));
        }
        prefix.truncate(mark);
    }
    Ok(())
}

/// What `Workspace::export` returns: the typed record, the content it refers to by digest, and how
/// many `.git` directories were not read.
#[derive(Debug)]
pub struct Export {
    pub record: B1ExportRecordV1,
    /// Exactly the contents the record names, once each, keyed by their digest.
    pub blobs: Vec<(Sha256Hex, Vec<u8>)>,
    pub skipped_git_directories: u32,
}

impl Export {
    /// The blobs are exactly the content the record names: every referenced digest present once,
    /// nothing else, each blob the digest and the size its entry says (the record is re-validated).
    /// Run before anything is written anywhere.
    pub fn verify(&self) -> B1Result {
        self.record.validate()?;
        let mut referenced: BTreeMap<&str, u64> = BTreeMap::new();
        for entry in &self.record.entries {
            if let Some(OverlayContent::Regular {
                content_sha256,
                size,
                ..
            }) = &entry.content
                && referenced
                    .insert(content_sha256.as_str(), *size)
                    .is_some_and(|earlier| earlier != *size)
            {
                return Err(err("blobs", B1ErrorCode::ExportDigestMismatch));
            }
        }
        let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for (sha, bytes) in &self.blobs {
            let Some(size) = referenced.get(sha.as_str()) else {
                // Not named by the record: it would be written for nothing, so it is refused.
                return Err(err("blobs", B1ErrorCode::ExportDigestMismatch));
            };
            if !seen.insert(sha.as_str()) {
                return Err(err("blobs", B1ErrorCode::ExportDigestMismatch));
            }
            digest::verify_content(bytes, sha, *size)?;
        }
        if seen.len() != referenced.len() {
            return Err(err("blobs", B1ErrorCode::ExportDigestMismatch));
        }
        Ok(())
    }
}

/// Publishes an export once. The store is a private directory of its own, never the source
/// repository's `.git`, never inside or around the paths in `apart_from` (the workspace, the
/// source), reached through descriptors that do not follow a link. A task and generation that were
/// published already are a conflict, not an overwrite. The record is written last and put in place
/// by a rename, so its presence means the publication is complete. Returns the digest of the record
/// that was stored.
pub fn quarantine(root: &Path, export: &Export, apart_from: &[&Path]) -> B1Result<Sha256Hex> {
    export.verify()?;
    let record_sha = digest::export_record(&export.record)?;
    let store = crate::b1::plan::pin_directory(root)
        .map_err(|_| err("quarantine", B1ErrorCode::QuarantineRecoveryNeeded))?;
    for other in apart_from {
        let other = std::fs::canonicalize(other).unwrap_or_else(|_| other.to_path_buf());
        if store.real.starts_with(&other) || other.starts_with(&store.real) {
            return Err(err("quarantine", B1ErrorCode::QuarantineRecoveryNeeded));
        }
    }
    let recovery = || err("quarantine", B1ErrorCode::QuarantineRecoveryNeeded);
    // <store>/<task>/<generation>: the task directory may exist, the generation may not.
    let task_name = export.record.task_id.as_str().as_bytes().to_vec();
    let task = match mkdirat(
        Some(store.fd.as_raw_fd()),
        c(&task_name)?.as_c_str(),
        Mode::from_bits_truncate(0o700),
    ) {
        Ok(()) | Err(Errno::EEXIST) => open_dir_at(store.fd.as_raw_fd(), &task_name)?,
        Err(_) => return Err(recovery()),
    };
    let generation = export.record.generation.to_string();
    // Creating the directory is the compare-and-swap: only one publisher gets it.
    match mkdirat(
        Some(task.as_raw_fd()),
        c(generation.as_bytes())?.as_c_str(),
        Mode::from_bits_truncate(0o700),
    ) {
        Ok(()) => {}
        Err(Errno::EEXIST) => return Err(err("quarantine", B1ErrorCode::QuarantineCasConflict)),
        Err(_) => return Err(recovery()),
    }
    let dir = open_dir_at(task.as_raw_fd(), generation.as_bytes())?;
    mkdirat(
        Some(dir.as_raw_fd()),
        c(b"blobs")?.as_c_str(),
        Mode::from_bits_truncate(0o700),
    )
    .map_err(|_| recovery())?;
    let blobs = open_dir_at(dir.as_raw_fd(), b"blobs")?;
    for (sha, bytes) in &export.blobs {
        write_new_at(blobs.as_raw_fd(), sha.as_str().as_bytes(), bytes).map_err(|_| recovery())?;
    }
    let json = serde_json::to_vec_pretty(&export.record).map_err(|_| recovery())?;
    write_new_at(dir.as_raw_fd(), b"export.json.partial", &json).map_err(|_| recovery())?;
    nix::fcntl::renameat(
        Some(dir.as_raw_fd()),
        "export.json.partial",
        Some(dir.as_raw_fd()),
        "export.json",
    )
    .map_err(|_| recovery())?;
    Ok(record_sha)
}

fn write_new_at(dir: RawFd, name: &[u8], bytes: &[u8]) -> std::io::Result<()> {
    let cname = CString::new(name).map_err(|_| std::io::Error::other("name"))?;
    let fd = openat(
        Some(dir),
        cname.as_c_str(),
        OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::from_bits_truncate(0o600),
    )
    .map_err(std::io::Error::from)?;
    // SAFETY: a descriptor just opened and owned by nobody else.
    let mut f = unsafe { File::from_raw_fd(fd) };
    f.write_all(bytes)?;
    f.sync_all()
}
