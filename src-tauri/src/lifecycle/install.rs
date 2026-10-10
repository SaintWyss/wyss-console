//! The two files of the managed installation, written so that a crash at any point
//! leaves either the old file or the new one, and never part of one.
use super::{
    Layout, LifecycleError as E, LifecycleResult,
    package::{Digest, read_executable},
};
use nix::{
    fcntl::{Flock, FlockArg},
    unistd::geteuid,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
    thread,
    time::{Duration, Instant},
};

/// First line of the unit this product writes. A unit at its path without it was written
/// by someone else and is never replaced.
pub const UNIT_MARKER: &str = "# Managed by Wyss Console (wyss-control.service, layout 1).";
/// A unit is a few hundred bytes. Nothing larger is read as one.
const MAX_UNIT: u64 = 64 * 1024;
/// How long one installer waits for another to finish.
const LOCK_LIMIT: Duration = Duration::from_secs(10);

/// What is at the place of one managed file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileState {
    Absent,
    /// Exactly what this build installs.
    Current,
    /// This product's, from another build.
    Outdated,
    /// Not this product's: a unit without its marker.
    Foreign,
    /// A link, another owner, another writer, a second name: not a file to trust or to
    /// replace.
    Unsafe,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Installed {
    pub binary: FileState,
    pub unit: FileState,
}
/// What an installation wrote.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Change {
    pub binary: bool,
    pub unit: bool,
}

/// The unit, whole. It depends on the path of the managed binary and on nothing else, so
/// two builds that install to the same place write the same bytes.
///
/// - `Type=exec`: starting fails when the binary cannot be executed.
/// - `Restart=on-failure` with a start limit: a crash is restarted, a crash loop ends.
///   Status 2 is the authority saying another start cannot succeed (it already runs, or
///   a path is unsafe) and is not retried. A clean exit is not restarted either.
/// - `KillMode=mixed`: stopping signals the authority alone, which stops its runtime in
///   order; what is left after the timeout is killed.
/// - No `[Install]` section: the unit cannot be enabled. It is started on demand.
/// - Hardening is what was tested not to break the `bwrap` isolation the authority runs
///   Codex in. `RestrictSUIDSGID` does break it and is absent on purpose; namespace and
///   file-system directives are absent because the authority has to reach the user's
///   workspaces and create namespaces itself.
/// - The environment is the service manager's, the same every user service has. The
///   desktop passes nothing. The dynamic loader's variables are removed.
pub fn unit_text(binary: &Path) -> LifecycleResult<String> {
    let path = binary.to_str().ok_or(E::UnsafePath)?;
    if !binary.is_absolute() || path.chars().any(char::is_control) {
        return Err(E::UnsafePath);
    }
    // systemd's quoting: a quoted word, with its own escapes and its two expansions off.
    let mut quoted = String::with_capacity(path.len() + 2);
    for c in path.chars() {
        match c {
            '\\' | '"' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '%' | '$' => {
                quoted.push(c);
                quoted.push(c);
            }
            c => quoted.push(c),
        }
    }
    Ok(format!(
        "{UNIT_MARKER}\n\
         # Written by the application and rewritten when it differs: edits do not last.\n\
         [Unit]\n\
         Description=Wyss Control Authority\n\
         StartLimitIntervalSec=120\n\
         StartLimitBurst=5\n\
         \n\
         [Service]\n\
         Type=exec\n\
         ExecStart=\"{quoted}\"\n\
         Restart=on-failure\n\
         RestartSec=1\n\
         RestartPreventExitStatus=2\n\
         KillMode=mixed\n\
         TimeoutStopSec=30\n\
         UMask=0077\n\
         NoNewPrivileges=yes\n\
         LockPersonality=yes\n\
         RestrictRealtime=yes\n\
         SystemCallArchitectures=native\n\
         UnsetEnvironment=LD_PRELOAD LD_LIBRARY_PATH LD_AUDIT LD_DEBUG LD_DEBUG_OUTPUT LD_PROFILE LD_PROFILE_OUTPUT GCONV_PATH\n"
    ))
}

fn missing(path: &Path) -> bool {
    matches!(fs::symlink_metadata(path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
}
/// A small text file of this user's that nobody else could have written.
fn read_owned_text(path: &Path) -> LifecycleResult<String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC | nix::libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| E::UnsafePath)?;
    let meta = file.metadata().map_err(|_| E::UnsafePath)?;
    if !meta.is_file()
        || meta.uid() != geteuid().as_raw()
        || meta.mode() & 0o022 != 0
        || meta.nlink() != 1
        || meta.len() > MAX_UNIT
    {
        return Err(E::UnsafePath);
    }
    let mut text = String::new();
    File::take(file, MAX_UNIT)
        .read_to_string(&mut text)
        .map_err(|_| E::UnsafePath)?;
    Ok(text)
}

fn binary_state(layout: &Layout, digest: Option<Digest>) -> FileState {
    use crate::control::paths::owned_dir;
    let dir = layout.binary_dir();
    if missing(&dir) || missing(&layout.binary()) {
        // A directory that is there has to be safe even while it is empty.
        return if missing(&dir) || owned_dir(&dir, true).is_ok() {
            FileState::Absent
        } else {
            FileState::Unsafe
        };
    }
    if owned_dir(&dir, true).is_err() {
        return FileState::Unsafe;
    }
    match (read_executable(&layout.binary(), false), digest) {
        (Err(_), _) => FileState::Unsafe,
        (Ok(bytes), Some(digest)) if Digest::of(&bytes) == digest => FileState::Current,
        (Ok(_), _) => FileState::Outdated,
    }
}
fn unit_state(layout: &Layout) -> FileState {
    use crate::control::paths::owned_dir;
    let dir = layout.unit_dir();
    if missing(&dir) {
        return FileState::Absent;
    }
    if owned_dir(&dir, false).is_err() {
        return FileState::Unsafe;
    }
    if missing(&layout.unit()) {
        return FileState::Absent;
    }
    let (Ok(text), Ok(expected)) = (read_owned_text(&layout.unit()), unit_text(&layout.binary()))
    else {
        return FileState::Unsafe;
    };
    if text == expected {
        FileState::Current
    } else if text.lines().next() == Some(UNIT_MARKER) {
        FileState::Outdated
    } else {
        FileState::Foreign
    }
}

/// What is installed, read and changed by nothing. With no digest (a build that carries
/// no authority) a managed binary that is there is reported as another build's.
pub fn inspect(layout: &Layout, digest: Option<Digest>) -> Installed {
    Installed {
        binary: binary_state(layout, digest),
        unit: unit_state(layout),
    }
}

/// Writes `bytes` as `name` in `dir`: a new file under a name nothing else uses, flushed
/// to disk, then renamed over the old one, then the directory flushed. A reader, an
/// `exec` or a crash sees the old file or the new file. A binary that is executing keeps
/// the file it started from.
fn write_atomic(dir: &Path, name: &str, bytes: &[u8], mode: u32) -> LifecycleResult<()> {
    let temporary = dir.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
    let written = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(&temporary)?;
        file.write_all(bytes)?;
        // The mode asked for, whatever the process umask made of it.
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        file.sync_all()?;
        fs::rename(&temporary, dir.join(name))?;
        File::open(dir)?.sync_all()
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written.map_err(|_| E::Io)
}
/// Removes what an installer that never finished left behind. Only this product's own
/// temporary names, only regular files of this user. Called with the lock held, so no
/// file of a running installer is among them.
fn sweep(dir: &Path, name: &str) {
    let prefix = format!(".{name}.");
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let file = entry.file_name();
        let Some(file) = file.to_str() else { continue };
        if file.starts_with(&prefix)
            && file.ends_with(".tmp")
            && fs::symlink_metadata(entry.path())
                .is_ok_and(|m| m.is_file() && m.uid() == geteuid().as_raw())
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// One installer at a time, across every desktop of this user: an advisory lock on a file
/// in the managed directory. It is released when the handle is dropped, or when the
/// process holding it ends.
fn lock(dir: &Path) -> LifecycleResult<Flock<File>> {
    let path = dir.join(".install.lock");
    let deadline = Instant::now() + LOCK_LIMIT;
    loop {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(&path)
            .map_err(|_| E::UnsafePath)?;
        let meta = file.metadata().map_err(|_| E::UnsafePath)?;
        if !meta.is_file() || meta.uid() != geteuid().as_raw() || meta.nlink() != 1 {
            return Err(E::UnsafePath);
        }
        match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
            Ok(held) => return Ok(held),
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            Err(_) => return Err(E::Busy),
        }
    }
}

/// Installs the managed binary. For a caller that already holds what to install; the
/// lifecycle goes through [`ensure`].
pub fn binary(layout: &Layout, bytes: &[u8]) -> LifecycleResult<()> {
    let dir = layout.prepare_binary_dir()?;
    write_atomic(&dir, "wyss-control", bytes, 0o700)
}
/// Installs the unit for the managed binary.
pub fn unit(layout: &Layout) -> LifecycleResult<()> {
    let dir = layout.prepare_unit_dir()?;
    write_atomic(
        &dir,
        super::UNIT_NAME,
        unit_text(&layout.binary())?.as_bytes(),
        0o600,
    )
}

/// Brings the installation to what this build installs, and says what it wrote. Nothing
/// is written where it is already current. A path that is not safe and a unit somebody
/// else wrote are refused and left exactly as they are.
///
/// It never removes anything but its own unfinished temporary files, and it touches no
/// directory but the two it owns files in: canonical state is somewhere else and is not
/// read, moved or deleted by installing, repairing or updating.
pub fn ensure(layout: &Layout, bytes: &[u8], digest: Digest) -> LifecycleResult<Change> {
    if Digest::of(bytes) != digest {
        return Err(E::PackageInvalid);
    }
    let binary_dir = layout.prepare_binary_dir()?;
    let unit_dir = layout.prepare_unit_dir()?;
    let _held = lock(&binary_dir)?;
    sweep(&binary_dir, "wyss-control");
    sweep(&unit_dir, super::UNIT_NAME);
    // Read again under the lock: another installer may just have finished.
    let installed = inspect(layout, Some(digest));
    // Both are judged before either is written: a refusal leaves nothing half done.
    match (installed.binary, installed.unit) {
        (FileState::Unsafe | FileState::Foreign, _) | (_, FileState::Unsafe) => {
            return Err(E::UnsafePath);
        }
        (_, FileState::Foreign) => return Err(E::Foreign),
        _ => (),
    }
    let mut change = Change::default();
    if installed.binary != FileState::Current {
        binary(layout, bytes)?;
        change.binary = true;
    }
    if installed.unit != FileState::Current {
        unit(layout)?;
        change.unit = true;
    }
    Ok(change)
}
