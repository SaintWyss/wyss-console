//! Synthetic, test-only capture fixture behind the [`TrustedCapture`] boundary (docs 50 and 52).
//!
//! THIS IS NOT A TRUSTED CAPTURE SERVICE. It exists so the admission contract can be exercised
//! against real filesystem behavior (symlinks, hardlinks, special files, races) without a real
//! user's files. It claims no sandbox, namespace, mount or inode isolation, closes no FX-H01 gate,
//! and runs with the privileges of the test process.
//!
//! Containment of the fixture itself:
//! - It is compiled only into test builds (`cfg(test)`), so no shipped binary contains it and
//!   nothing in the control protocol, UI, agent launcher or provider code can reach it.
//! - It starts disabled. [`SyntheticCapture::disabled`] refuses every request with
//!   `ISOLATION_UNAVAILABLE`, and [`SyntheticCapture::enabled_for_synthetic_test`] is the only way
//!   to turn it on.
//! - Its only possible root is a [`SyntheticRoot`], a private directory this module creates under
//!   the system temporary directory. There is no constructor that accepts an existing path, so it
//!   cannot be pointed at a home directory, a project or a credential store.
//!
//! What it does, for each path the owner's selection names and only those paths:
//! - Opens the containing directory with `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS |
//!   RESOLVE_NO_MAGICLINKS | RESOLVE_NO_XDEV)` from a held root descriptor, and the leaf relative
//!   to that descriptor. A missing `openat2` fails closed. There is no silent path-based fallback.
//! - Classifies the leaf with `fstatat(AT_SYMLINK_NOFOLLOW)` before opening it, so FIFOs, sockets
//!   and devices are never opened. Regular files are opened `O_NOFOLLOW | O_NONBLOCK | O_NOCTTY`,
//!   then `fstat`ed and required to be the same inode that was classified.
//! - Reads at most the selection's declared size plus one byte, so an oversized file is bounded.
//! - Refuses regular files with `st_nlink != 1`. The selection carries no hardlink authorization
//!   yet, so any aliased inode fails closed (doc 51 CG-02).
//! - Reads symlinks with `readlinkat` as inert bytes. Targets are never resolved or followed.
//! - Detects change during capture: stat before and after, a second read compared byte for byte
//!   (timestamps alone are too coarse to catch a same-size overwrite), and a final re-resolution
//!   that must reach the same directory and leaf inodes.
//!
//! Capture reports what is on disk. Authorization stays in `admit`, which refuses a captured
//! record that differs from the selection. Returned records own their bytes.
use super::{
    B1Error, B1ErrorCode, B1Result, MAX_INPUT_BYTES, MAX_LINK_TARGET_BYTES,
    MAX_TOTAL_CONTENT_BYTES,
    admission::{SuppliedContent, SuppliedRecord, TrustedCapture},
    ensure,
    path::B1RelPath,
    source::{B1SourceSelectionV1, ChangeOperation, EXECUTABLE_MODE, OverlayContent, REGULAR_MODE},
};
use nix::{
    errno::Errno,
    fcntl::{AtFlags, OFlag, readlinkat},
    libc,
    sys::stat::{FileStat, fstat, fstatat},
};
use std::{
    ffi::CString,
    fs::{DirBuilder, File},
    io,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::fs::{DirBuilderExt, FileExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

#[cfg(test)]
mod tests;

const RESOLVE_NO_XDEV: u64 = 0x01;
const RESOLVE_NO_MAGICLINKS: u64 = 0x02;
const RESOLVE_NO_SYMLINKS: u64 = 0x04;
const RESOLVE_BENEATH: u64 = 0x08;
const CONFINED: u64 =
    RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS | RESOLVE_NO_XDEV;

/// `struct open_how` from `openat2(2)`.
#[repr(C)]
struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}

/// The only unsafe operations in this module: one syscall and one descriptor adoption.
fn openat2(dir: RawFd, name: &[u8], flags: OFlag) -> Result<OwnedFd, Errno> {
    let name = CString::new(name).map_err(|_| Errno::EINVAL)?;
    let how = OpenHow {
        flags: (flags | OFlag::O_CLOEXEC).bits() as u64,
        mode: 0,
        resolve: CONFINED,
    };
    // SAFETY: `name` and `how` outlive the call, the size matches `OpenHow`, and the kernel only
    // reads them. The return value is checked before use.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            libc::c_long::from(dir),
            name.as_ptr(),
            &how as *const OpenHow,
            std::mem::size_of::<OpenHow>(),
        )
    };
    if fd < 0 {
        return Err(Errno::last());
    }
    // SAFETY: `fd` is a fresh descriptor returned by the kernel that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as RawFd) })
}

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

/// A private, temporary directory created by this module. It is the only kind of root the fixture
/// accepts, and it is deleted when dropped. Deletion never follows symlinks.
pub(super) struct SyntheticRoot {
    path: PathBuf,
    fd: OwnedFd,
}

impl SyntheticRoot {
    pub(super) fn create() -> Self {
        let name = format!(
            "wyss-b1-synthetic-{}-{}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed),
            uuid::Uuid::new_v4().simple()
        );
        let path = std::env::temp_dir().join(name);
        DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .expect("create synthetic root");
        let directory = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags((OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW).bits())
            .open(&path)
            .expect("open synthetic root");
        Self {
            path,
            fd: OwnedFd::from(directory),
        }
    }

    /// Test-setup access only. The capture never uses this path.
    pub(super) fn at(&self, relative: &str) -> PathBuf {
        self.path.join(relative)
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SyntheticRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Points at which a test can change the filesystem while a capture is in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    /// The containing directory is open and the leaf is classified, before it is opened or read.
    AfterClassify,
    /// The first read is finished, before the change checks.
    AfterRead,
}

type Hook<'r> = Box<dyn Fn(Stage, &B1RelPath) + 'r>;

/// Disabled-by-default capture over a [`SyntheticRoot`]. See the module documentation.
pub(super) struct SyntheticCapture<'r> {
    root: &'r SyntheticRoot,
    enabled: bool,
    hook: Option<Hook<'r>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NodeKind {
    Regular,
    Symlink,
    Directory,
    Fifo,
    Socket,
    CharDevice,
    BlockDevice,
    Unknown,
}

pub(super) fn node_kind(mode: u32) -> NodeKind {
    match mode & libc::S_IFMT {
        libc::S_IFREG => NodeKind::Regular,
        libc::S_IFLNK => NodeKind::Symlink,
        libc::S_IFDIR => NodeKind::Directory,
        libc::S_IFIFO => NodeKind::Fifo,
        libc::S_IFSOCK => NodeKind::Socket,
        libc::S_IFCHR => NodeKind::CharDevice,
        libc::S_IFBLK => NodeKind::BlockDevice,
        _ => NodeKind::Unknown,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
}

impl Identity {
    fn of(stat: &FileStat) -> Self {
        Self {
            device: stat.st_dev as u64,
            inode: stat.st_ino as u64,
        }
    }
}

/// Everything about an open file that a concurrent writer would change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    identity: Identity,
    mode: u32,
    links: u64,
    size: i64,
    modified: (i64, i64),
    changed: (i64, i64),
}

impl Stamp {
    fn of(stat: &FileStat) -> Self {
        Self {
            identity: Identity::of(stat),
            mode: stat.st_mode,
            links: stat.st_nlink as u64,
            size: stat.st_size,
            modified: (stat.st_mtime, stat.st_mtime_nsec),
            changed: (stat.st_ctime, stat.st_ctime_nsec),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Initial,
    Recheck,
}

fn changed() -> B1Error {
    B1Error::new("capture", B1ErrorCode::CaptureChanged)
}

fn unavailable() -> B1Error {
    B1Error::new("capture", B1ErrorCode::IsolationUnavailable)
}

/// Splits validated path bytes into the directory part and the single leaf name.
fn split(path: &B1RelPath) -> (&[u8], &[u8]) {
    let bytes = path.as_bytes();
    match bytes.iter().rposition(|&byte| byte == b'/') {
        Some(index) => (&bytes[..index], &bytes[index + 1..]),
        None => (&[], bytes),
    }
}

impl<'r> SyntheticCapture<'r> {
    pub(super) fn disabled(root: &'r SyntheticRoot) -> Self {
        Self {
            root,
            enabled: false,
            hook: None,
        }
    }

    pub(super) fn enabled_for_synthetic_test(mut self) -> Self {
        self.enabled = true;
        self
    }

    pub(super) fn with_hook(mut self, hook: impl Fn(Stage, &B1RelPath) + 'r) -> Self {
        self.hook = Some(Box::new(hook));
        self
    }

    fn notify(&self, stage: Stage, path: &B1RelPath) {
        if let Some(hook) = &self.hook {
            hook(stage, path);
        }
    }

    /// Opens the directory that holds `path`, confined beneath the root descriptor.
    pub(super) fn open_directory(&self, directory: &[u8], phase: Phase) -> B1Result<OwnedFd> {
        if directory.is_empty() {
            return self.root.fd.try_clone().map_err(|_| unavailable());
        }
        openat2(
            self.root.fd.as_raw_fd(),
            directory,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOCTTY,
        )
        .map_err(|errno| match errno {
            Errno::ENOSYS | Errno::EPERM => unavailable(),
            // A symlinked or escaping ancestor is an alias. If it appeared mid-capture, the
            // tree changed instead.
            Errno::ELOOP | Errno::EXDEV if phase == Phase::Initial => {
                B1Error::new("path", B1ErrorCode::HardlinkOrAliasUnsafe)
            }
            _ => changed(),
        })
    }

    fn read_regular(
        &self,
        parent: &OwnedFd,
        leaf: &[u8],
        classified: &FileStat,
        declared: Option<&OverlayContent>,
        path: &B1RelPath,
    ) -> B1Result<SuppliedContent> {
        // Never read more than the owner's selection declares, plus one byte so that a larger
        // file is visible to admission as a size mismatch.
        let limit = match declared {
            Some(OverlayContent::Regular { size, .. }) => (*size).min(MAX_INPUT_BYTES),
            _ => MAX_INPUT_BYTES,
        } + 1;
        let descriptor = openat2(
            parent.as_raw_fd(),
            leaf,
            OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_NOCTTY,
        )
        .map_err(|errno| match errno {
            Errno::ENOSYS | Errno::EPERM => unavailable(),
            _ => changed(),
        })?;
        let file = File::from(descriptor);
        let before = Stamp::of(&fstat(file.as_raw_fd()).map_err(|_| changed())?);
        ensure(
            node_kind(before.mode) == NodeKind::Regular
                && before.identity == Identity::of(classified),
            "capture",
            B1ErrorCode::CaptureChanged,
        )?;
        ensure(
            before.links == 1,
            "capture",
            B1ErrorCode::HardlinkOrAliasUnsafe,
        )?;
        let bytes = read_prefix(&file, limit).map_err(|_| changed())?;
        self.notify(Stage::AfterRead, path);
        let after = Stamp::of(&fstat(file.as_raw_fd()).map_err(|_| changed())?);
        ensure(after == before, "capture", B1ErrorCode::CaptureChanged)?;
        let again = read_prefix(&file, limit).map_err(|_| changed())?;
        ensure(again == bytes, "capture", B1ErrorCode::CaptureChanged)?;
        if declared.is_none_or(|content| !matches!(content, OverlayContent::Regular { .. })) {
            ensure(
                bytes.len() as u64 <= MAX_INPUT_BYTES,
                "size",
                B1ErrorCode::LimitExceeded,
            )?;
        }
        let mode = if before.mode & 0o100 != 0 {
            EXECUTABLE_MODE
        } else {
            REGULAR_MODE
        };
        Ok(SuppliedContent::Regular { mode, bytes })
    }

    fn read_symlink(
        &self,
        parent: &OwnedFd,
        leaf: &[u8],
        classified: &FileStat,
        path: &B1RelPath,
    ) -> B1Result<SuppliedContent> {
        use std::os::unix::ffi::OsStringExt;
        let read = || {
            readlinkat(Some(parent.as_raw_fd()), leaf)
                .map(OsStringExt::into_vec)
                .map_err(|_| changed())
        };
        let target = read()?;
        ensure(
            target.len() <= MAX_LINK_TARGET_BYTES,
            "link_target",
            B1ErrorCode::LimitExceeded,
        )?;
        self.notify(Stage::AfterRead, path);
        ensure(read()? == target, "capture", B1ErrorCode::CaptureChanged)?;
        let now = fstatat(Some(parent.as_raw_fd()), leaf, AtFlags::AT_SYMLINK_NOFOLLOW)
            .map_err(|_| changed())?;
        // Inode numbers can be reused by a delete and re-create, so a replacement with identical
        // target bytes is not always distinguishable. Its content then equals what was captured.
        ensure(
            Stamp::of(&now) == Stamp::of(classified),
            "capture",
            B1ErrorCode::CaptureChanged,
        )?;
        Ok(SuppliedContent::Symlink { target })
    }

    fn capture_one(
        &self,
        path: &B1RelPath,
        declared: Option<&OverlayContent>,
    ) -> B1Result<SuppliedContent> {
        let (directory, leaf) = split(path);
        let parent = self.open_directory(directory, Phase::Initial)?;
        let parent_identity = Identity::of(&fstat(parent.as_raw_fd()).map_err(|_| changed())?);
        let classified = fstatat(Some(parent.as_raw_fd()), leaf, AtFlags::AT_SYMLINK_NOFOLLOW)
            .map_err(|_| changed())?;
        self.notify(Stage::AfterClassify, path);
        let content = match node_kind(classified.st_mode) {
            NodeKind::Regular => self.read_regular(&parent, leaf, &classified, declared, path)?,
            NodeKind::Symlink => self.read_symlink(&parent, leaf, &classified, path)?,
            // Directories, FIFOs, sockets, devices and anything else are never opened.
            _ => return Err(B1Error::new("content", B1ErrorCode::InputKindUnsupported)),
        };
        // The name must still lead to the same directory and the same leaf that were read.
        let recheck = self.open_directory(directory, Phase::Recheck)?;
        let same_directory =
            Identity::of(&fstat(recheck.as_raw_fd()).map_err(|_| changed())?) == parent_identity;
        let leaf_now = fstatat(
            Some(recheck.as_raw_fd()),
            leaf,
            AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(|_| changed())?;
        ensure(
            same_directory && Identity::of(&leaf_now) == Identity::of(&classified),
            "capture",
            B1ErrorCode::CaptureChanged,
        )?;
        Ok(content)
    }
}

/// Reads up to `limit` bytes from offset zero, without moving a shared file offset.
fn read_prefix(file: &File, limit: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    while (bytes.len() as u64) < limit {
        let want = chunk.len().min((limit - bytes.len() as u64) as usize);
        match file.read_at(&mut chunk[..want], bytes.len() as u64) {
            Ok(0) => break,
            Ok(read) => bytes.extend_from_slice(&chunk[..read]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(bytes)
}

impl TrustedCapture for SyntheticCapture<'_> {
    fn capture(&self, selection: &B1SourceSelectionV1) -> B1Result<Vec<SuppliedRecord>> {
        ensure(self.enabled, "capture", B1ErrorCode::IsolationUnavailable)?;
        selection.validate()?;
        let mut records = Vec::new();
        let mut total = 0u64;
        for overlay in &selection.admitted_overlay {
            // A deletion has no content. Its path is never opened.
            if overlay.operation == ChangeOperation::Delete {
                continue;
            }
            let content = self.capture_one(&overlay.path, overlay.content.as_ref())?;
            total = total
                .checked_add(match &content {
                    SuppliedContent::Regular { bytes, .. } => bytes.len() as u64,
                    SuppliedContent::Symlink { target } => target.len() as u64,
                    SuppliedContent::Unsupported(_) => 0,
                })
                .filter(|sum| *sum <= MAX_TOTAL_CONTENT_BYTES)
                .ok_or(B1Error::new("captured_bytes", B1ErrorCode::LimitExceeded))?;
            records.push(SuppliedRecord {
                path: overlay.path.clone(),
                origin: overlay.origin,
                content,
            });
        }
        Ok(records)
    }
}
