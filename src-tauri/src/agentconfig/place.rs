//! Where a source is, and whether its place can be trusted.
//!
//! The rules are those of `control/paths.rs` applied to a provider's directory (07 §4.2),
//! written again here because this module and the control module do not reference each
//! other (TS-SEC-03). The difference is what a shared directory means: the authority
//! refuses it, the configuration engine reads it and never writes it, because the user's
//! own `~/.claude` may be shared with a group.
//!
//! A place is judged one directory at a time, through descriptors: each directory is
//! opened below the one before it without following a link, and what is judged is the
//! descriptor that was opened. A link or directory swapped in between two steps is
//! therefore never followed.
use super::model::{PathTrust, ReadOnlyReason, SourceFailure, UnsupportedReason, UntrustedReason};
use super::roots::ApprovedRoots;
use nix::{
    errno::Errno,
    fcntl::{AtFlags, OFlag, openat, readlinkat},
    sys::stat::{Mode, SFlag, fstat, fstatat},
    unistd::geteuid,
};
use std::{
    ffi::{OsStr, OsString},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    os::unix::ffi::OsStrExt,
    path::{Component, Path, PathBuf},
};

/// The ownership and permission facts of one directory or file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Facts {
    pub uid: u32,
    pub mode: u32,
    pub nlink: u64,
}

/// A directory on the way to a source. Root's sticky directories (`/tmp`) are allowed, as
/// in `control/paths.rs`; any other directory another user can write is not.
pub(crate) fn judge_directory(facts: Facts, me: u32) -> Result<PathTrust, UntrustedReason> {
    if facts.uid != 0 && facts.uid != me {
        return Err(UntrustedReason::ForeignOwner);
    }
    if facts.uid == 0 && facts.mode & 0o1000 != 0 {
        return Ok(PathTrust::Trusted);
    }
    if facts.mode & 0o002 != 0 {
        return Err(UntrustedReason::WritableByOthers);
    }
    if facts.mode & 0o020 != 0 {
        return Ok(PathTrust::ReadOnly(ReadOnlyReason::GroupWritable));
    }
    Ok(PathTrust::Trusted)
}

/// The file itself: this user's or root's, not writable by everyone. A file with a second
/// name could be changed through it, so it is read and never written.
pub(crate) fn judge_file(facts: Facts, me: u32) -> Result<PathTrust, UntrustedReason> {
    if facts.uid != 0 && facts.uid != me {
        return Err(UntrustedReason::ForeignOwner);
    }
    if facts.mode & 0o002 != 0 {
        return Err(UntrustedReason::WritableByOthers);
    }
    if facts.mode & 0o020 != 0 {
        return Ok(PathTrust::ReadOnly(ReadOnlyReason::GroupWritable));
    }
    if facts.nlink > 1 {
        return Ok(PathTrust::ReadOnly(ReadOnlyReason::MultipleNames));
    }
    Ok(PathTrust::Trusted)
}

/// The weaker of two judgements; the first reason found is kept.
pub(crate) const fn weaker(a: PathTrust, b: PathTrust) -> PathTrust {
    match a {
        PathTrust::ReadOnly(_) => a,
        PathTrust::Trusted => b,
    }
}

/// Names that only a credential has. Such a file is never opened, whatever links lead to
/// it (06 §4.2, CTL-06). A name is judged whole: `environment.md` and `secret-handling`
/// are not credentials, and a Markdown or text file named `secrets` is a note, not a
/// store (its text still goes through redaction).
pub(crate) fn is_credential_file(name: &OsStr) -> bool {
    let name = name.as_bytes().to_ascii_lowercase();
    let stem = |prefix: &[u8]| {
        name == prefix || (name.starts_with(prefix) && name.get(prefix.len()) == Some(&b'.'))
    };
    let prose = [&b".md"[..], b".markdown", b".txt", b".rst"]
        .iter()
        .any(|suffix| name.ends_with(suffix));
    stem(b".env")
        || (!prose && (stem(b"credentials") || stem(b"secret") || stem(b"secrets")))
        || stem(b".credentials")
        || [
            &b"auth.json"[..],
            b"token",
            b"token.json",
            b"tokens.json",
            b"oauth_tokens.json",
            b".netrc",
            b"_netrc",
            b".npmrc",
            b".pypirc",
            b".dockercfg",
            b".git-credentials",
        ]
        .contains(&name.as_slice())
        || name.starts_with(b"id_rsa")
        || name.starts_with(b"id_dsa")
        || name.starts_with(b"id_ecdsa")
        || name.starts_with(b"id_ed25519")
        || [
            &b".pem"[..],
            b".key",
            b".p12",
            b".pfx",
            b".jks",
            b".keystore",
            b".kdbx",
        ]
        .iter()
        .any(|suffix| name.ends_with(suffix))
}

/// An absolute path with no `..`, no `.` after the start and no control character, cut
/// into its names.
pub(crate) fn names(path: &Path) -> Result<Vec<&OsStr>, SourceFailure> {
    let not_plain = SourceFailure::UntrustedPath {
        reason: UntrustedReason::NotPlain,
    };
    if !path.is_absolute() || path.as_os_str().as_bytes().iter().any(u8::is_ascii_control) {
        return Err(not_plain);
    }
    let mut names = Vec::new();
    for component in path.components() {
        match component {
            Component::RootDir => (),
            Component::Normal(name) => names.push(name),
            _ => return Err(not_plain),
        }
    }
    Ok(names)
}

/// Where a link points, as an absolute path. Its `..` are resolved by name: no directory
/// on the way can be a link (it would be refused), so by name and on disk agree.
pub(crate) fn link_target(link: &Path, target: &OsStr) -> Result<PathBuf, SourceFailure> {
    let target = Path::new(target);
    let joined = if target.is_absolute() {
        target.to_owned()
    } else {
        link.parent().unwrap_or(Path::new("/")).join(target)
    };
    let mut resolved = PathBuf::from("/");
    for component in joined.components() {
        match component {
            Component::RootDir | Component::CurDir => (),
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(name) => resolved.push(name),
            Component::Prefix(_) => {
                return Err(SourceFailure::UntrustedPath {
                    reason: UntrustedReason::NotPlain,
                });
            }
        }
    }
    names(&resolved)?;
    Ok(resolved)
}

/// What is at the end of a path.
#[derive(Debug)]
pub(crate) enum Reached {
    /// An open descriptor for what was asked for, and the judgement of its place.
    Node(OwnedFd, PathTrust),
    /// A symbolic link, not followed: its target as written, and the judgement of the
    /// place that holds it.
    Link(OsString, PathTrust),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Want {
    File,
    Directory,
}

fn facts(fd: &OwnedFd) -> Result<Facts, SourceFailure> {
    let stat = fstat(fd.as_raw_fd()).map_err(|_| SourceFailure::Unreadable)?;
    Ok(Facts {
        uid: stat.st_uid,
        mode: stat.st_mode,
        nlink: stat.st_nlink,
    })
}

fn kind(fd: &OwnedFd) -> Result<SFlag, SourceFailure> {
    let stat = fstat(fd.as_raw_fd()).map_err(|_| SourceFailure::Unreadable)?;
    Ok(SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT)
}

fn open_below(dir: Option<&OwnedFd>, name: &OsStr, flags: OFlag) -> Result<OwnedFd, Errno> {
    let fd = openat(
        dir.map(AsRawFd::as_raw_fd),
        name,
        flags | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC | OFlag::O_NOCTTY,
        Mode::empty(),
    )?;
    // SAFETY: `openat` just returned this descriptor and nothing else owns it.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Whether `name` in `dir` is a symbolic link, judged without following it.
fn is_link(dir: &OwnedFd, name: &OsStr) -> bool {
    fstatat(Some(dir.as_raw_fd()), name, AtFlags::AT_SYMLINK_NOFOLLOW)
        .map(|stat| SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT == SFlag::S_IFLNK)
        .unwrap_or(false)
}

fn open_failure(errno: Errno) -> SourceFailure {
    match errno {
        Errno::ENOENT => SourceFailure::Missing,
        _ => SourceFailure::Unreadable,
    }
}

/// Walks `path` from the root, one directory descriptor at a time, and opens what is at
/// its end without following a final link. A credential file is refused by its name
/// before anything is opened.
pub(crate) fn reach(path: &Path, want: Want) -> Result<Reached, SourceFailure> {
    let me = geteuid().as_raw();
    let names = names(path)?;
    let Some((last, ancestors)) = names.split_last() else {
        return Err(SourceFailure::Unsupported {
            reason: UnsupportedReason::NotRegularFile,
        });
    };
    if want == Want::File && is_credential_file(last) {
        return Err(SourceFailure::CredentialFile);
    }
    let untrusted = |reason| SourceFailure::UntrustedPath { reason };
    let directory = OFlag::O_RDONLY | OFlag::O_DIRECTORY;
    let mut dir = open_below(None, OsStr::new("/"), directory).map_err(open_failure)?;
    let mut trust = judge_directory(facts(&dir)?, me).map_err(untrusted)?;
    for name in ancestors {
        let next = match open_below(Some(&dir), name, directory) {
            Ok(next) => next,
            Err(Errno::ELOOP | Errno::ENOTDIR) if is_link(&dir, name) => {
                return Err(untrusted(UntrustedReason::AncestorLink));
            }
            Err(Errno::ENOTDIR) => return Err(untrusted(UntrustedReason::AncestorNotDirectory)),
            Err(errno) => return Err(open_failure(errno)),
        };
        trust = weaker(
            trust,
            judge_directory(facts(&next)?, me).map_err(untrusted)?,
        );
        dir = next;
    }
    #[cfg(test)]
    hook::before_final_open(path);
    let flags = match want {
        // Not blocking: a FIFO put where a file was expected must not hang the reader.
        Want::File => OFlag::O_RDONLY | OFlag::O_NONBLOCK,
        Want::Directory => directory,
    };
    let node = match open_below(Some(&dir), last, flags) {
        Ok(node) => node,
        Err(Errno::ELOOP | Errno::ENOTDIR) if is_link(&dir, last) => {
            let target = readlinkat(Some(dir.as_raw_fd()), *last).map_err(open_failure)?;
            return Ok(Reached::Link(target, trust));
        }
        Err(Errno::ENOTDIR) => {
            return Err(SourceFailure::Unsupported {
                reason: UnsupportedReason::NotRegularFile,
            });
        }
        Err(errno) => return Err(open_failure(errno)),
    };
    let expected = match want {
        Want::File => SFlag::S_IFREG,
        Want::Directory => SFlag::S_IFDIR,
    };
    if kind(&node)? != expected {
        return Err(SourceFailure::Unsupported {
            reason: UnsupportedReason::NotRegularFile,
        });
    }
    let own = match want {
        Want::File => judge_file(facts(&node)?, me),
        Want::Directory => judge_directory(facts(&node)?, me),
    };
    Ok(Reached::Node(node, weaker(trust, own.map_err(untrusted)?)))
}

/// Opens `path`, following a final link one hop at a time up to the bound; every hop is
/// checked against the approved roots, then walked and judged again from the root.
/// Returns the descriptor, the judgement of the weakest place on the way, and the links
/// that were followed, in order.
pub(crate) fn open(
    roots: &ApprovedRoots,
    path: &Path,
    want: Want,
    link_hops: usize,
) -> Result<(OwnedFd, PathTrust, Vec<PathBuf>), SourceFailure> {
    let mut current = path.to_owned();
    let mut links = Vec::new();
    let mut trust = PathTrust::Trusted;
    loop {
        // Before anything is opened or stat-ed: a place outside the list is not even
        // looked at, so its existence is not an answer either.
        names(&current)?;
        if roots.holding(&current).is_none() {
            return Err(SourceFailure::UntrustedPath {
                reason: UntrustedReason::OutsideRoots,
            });
        }
        if roots.in_credential_directory(&current, want == Want::Directory) {
            return Err(SourceFailure::CredentialFile);
        }
        match reach(&current, want)? {
            Reached::Node(fd, own) => return Ok((fd, weaker(trust, own), links)),
            Reached::Link(target, own) => {
                if links.len() >= link_hops {
                    return Err(SourceFailure::Unsupported {
                        reason: UnsupportedReason::LinkLimit,
                    });
                }
                trust = weaker(trust, own);
                let next = link_target(&current, &target)?;
                links.push(std::mem::replace(&mut current, next));
            }
        }
    }
}

/// The directory a file is in, opened by descriptor, for a write that renames beside it.
#[derive(Debug)]
pub(crate) struct Parent {
    pub dir: OwnedFd,
    pub name: OsString,
    /// Where the file really is, after the links that led to it.
    pub path: PathBuf,
    /// The weakest judgement of every place on the way and of the file and its directory.
    pub trust: PathTrust,
}

/// Resolves `path` as `open` does, then opens the directory its final file is in, walking
/// from the root one descriptor at a time and judging each. A write goes to the file the
/// links lead to and never replaces a link (07 §4.3).
pub(crate) fn parent_for_write(
    roots: &ApprovedRoots,
    path: &Path,
    link_hops: usize,
) -> Result<Parent, SourceFailure> {
    let (fd, mut trust, links) = open(roots, path, Want::File, link_hops)?;
    drop(fd);
    // The place the links lead to: the last hop's target, or the path itself.
    let mut current = path.to_owned();
    for _ in 0..=links.len() {
        match reach(&current, Want::File)? {
            Reached::Node(..) => break,
            Reached::Link(target, _) => current = link_target(&current, &target)?,
        }
    }
    let me = geteuid().as_raw();
    let all = names(&current)?;
    let Some((last, ancestors)) = all.split_last() else {
        return Err(SourceFailure::Unsupported {
            reason: UnsupportedReason::NotRegularFile,
        });
    };
    let untrusted = |reason| SourceFailure::UntrustedPath { reason };
    let directory = OFlag::O_RDONLY | OFlag::O_DIRECTORY;
    let mut dir = open_below(None, OsStr::new("/"), directory).map_err(open_failure)?;
    trust = weaker(trust, judge_directory(facts(&dir)?, me).map_err(untrusted)?);
    for name in ancestors {
        let next = match open_below(Some(&dir), name, directory) {
            Ok(next) => next,
            Err(Errno::ELOOP | Errno::ENOTDIR) => {
                return Err(untrusted(UntrustedReason::AncestorLink));
            }
            Err(errno) => return Err(open_failure(errno)),
        };
        trust = weaker(
            trust,
            judge_directory(facts(&next)?, me).map_err(untrusted)?,
        );
        dir = next;
    }
    Ok(Parent {
        dir,
        name: (*last).to_owned(),
        path: current,
        trust,
    })
}

/// A seam for tests to put something between the check of a place and the open of it.
#[cfg(test)]
pub(crate) mod hook {
    use std::{cell::RefCell, path::Path};

    type Action = Box<dyn FnMut(&Path)>;

    thread_local! {
        static BEFORE_FINAL_OPEN: RefCell<Option<Action>> = const { RefCell::new(None) };
    }

    pub(crate) fn set_before_final_open(action: impl FnMut(&Path) + 'static) {
        BEFORE_FINAL_OPEN.with(|slot| *slot.borrow_mut() = Some(Box::new(action)));
    }

    pub(crate) fn clear() {
        BEFORE_FINAL_OPEN.with(|slot| *slot.borrow_mut() = None);
    }

    pub(super) fn before_final_open(path: &Path) {
        BEFORE_FINAL_OPEN.with(|slot| {
            if let Some(action) = slot.borrow_mut().as_mut() {
                action(path);
            }
        });
    }
}
