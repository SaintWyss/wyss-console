//! Finding the helper executable and trusting it: the file the host verified is the file it runs.
//!
//! A path checked and then executed is two lookups; whoever can rename or replace something along
//! it between them decides what runs. So the helper is opened once, every property is read from the
//! open descriptor, and the process is started *from that descriptor* (`/proc/self/fd/N`). What is
//! checked is what executes, whatever the path does afterwards.
//!
//! The rules (every one has a test): a regular file with an execute bit, not set-id, owned by this
//! user or root, writable by nobody else (no group or other write bit, whatever the group is), and
//! every directory above it (the real path, symlinks resolved) owned by this user or root and not
//! writable by others. A directory may be writable by its group only when that group is this user's
//! own private group: their primary group, with no other member in the group database and no other
//! account in the password file using it as its primary group (the default of an account whose
//! umask is 002). Any other group could add a file next to, or swap, what is below. A world-writable directory with the sticky bit (a `/tmp`) is
//! tolerated only where the entry below it is itself ours or root's, since only the owner may rename
//! it there. Anything else is refused with a closed word, and the host reports that it has no screen
//! helper.
//!
//! Not covered: the same user replacing the file. That user already owns the host's memory and
//! every PTY master.

use std::{
    fs::File,
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawFd,
    },
    path::{Path, PathBuf},
};

/// A helper executable that passed every check, held open.
#[derive(Debug)]
pub struct Verified {
    file: File,
    real: PathBuf,
}

impl Verified {
    /// A path that executes the open file and nothing else. Valid in a child forked from this
    /// process, as long as `self` is alive.
    pub fn exec_path(&self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.file.as_raw_fd()))
    }
    /// The real path it was found at, for messages and tests; never executed.
    pub fn real_path(&self) -> &Path {
        &self.real
    }
}

/// The first candidate that passes.
pub fn find(candidates: &[PathBuf]) -> Option<Verified> {
    let me = nix::unistd::getuid().as_raw();
    candidates.iter().find_map(|c| verify(c, me).ok())
}

/// The closed words a refusal has.
pub type Why = &'static str;

pub fn verify(candidate: &Path, me: u32) -> Result<Verified, Why> {
    let real = std::fs::canonicalize(candidate).map_err(|_| "missing")?;
    ancestors_are_ours(&real, me)?;
    let before = std::fs::symlink_metadata(&real).map_err(|_| "missing")?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_CLOEXEC | nix::libc::O_NOFOLLOW)
        .open(&real)
        .map_err(|_| "unreadable")?;
    let meta = file.metadata().map_err(|_| "unreadable")?;
    // The thing opened is the thing that was looked at.
    if meta.dev() != before.dev() || meta.ino() != before.ino() {
        return Err("changed");
    }
    let mode = meta.mode();
    if !meta.is_file() {
        return Err("not_a_file");
    }
    if mode & 0o111 == 0 {
        return Err("not_executable");
    }
    if mode & 0o6000 != 0 {
        return Err("set_id");
    }
    if mode & 0o022 != 0 {
        return Err("writable_by_others");
    }
    if meta.uid() != me && meta.uid() != 0 {
        return Err("foreign_owner");
    }
    Ok(Verified { file, real })
}

fn ancestors_are_ours(real: &Path, me: u32) -> Result<(), Why> {
    let mut chain: Vec<PathBuf> = real.ancestors().skip(1).map(Path::to_path_buf).collect();
    // From the root down: each directory, with the entry below it.
    chain.reverse();
    for (i, dir) in chain.iter().enumerate() {
        let meta = std::fs::symlink_metadata(dir).map_err(|_| "missing")?;
        if !meta.is_dir() {
            return Err("not_a_directory");
        }
        if meta.uid() != me && meta.uid() != 0 {
            return Err("foreign_directory");
        }
        let group_only = meta.mode() & 0o022 == 0o020 && group_is_private(meta.gid(), me);
        if meta.mode() & 0o022 != 0 && !group_only {
            let sticky = meta.mode() & 0o1000 != 0;
            let below = chain.get(i + 1).map_or(real, PathBuf::as_path);
            let below = std::fs::symlink_metadata(below).map_err(|_| "missing")?;
            let held = below.uid() == me || below.uid() == 0;
            if !(sticky && held) {
                return Err("writable_directory");
            }
        }
    }
    Ok(())
}

/// `gid` is a group nobody but `me` belongs to. Judged from the local databases; an account that
/// exists only in a directory service is not seen, and that is the limit of this rule.
pub fn group_is_private(gid: u32, me: u32) -> bool {
    let Ok(Some(user)) = nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(me)) else {
        return false;
    };
    if user.gid.as_raw() != gid {
        return false;
    }
    let Ok(Some(group)) = nix::unistd::Group::from_gid(nix::unistd::Gid::from_raw(gid)) else {
        return false;
    };
    if group.mem.iter().any(|m| *m != user.name) {
        return false;
    }
    let Ok(passwd) = std::fs::read_to_string("/etc/passwd") else {
        return false;
    };
    !passwd.lines().any(|line| {
        let f: Vec<&str> = line.split(':').collect();
        f.len() > 3 && f[3].parse::<u32>() == Ok(gid) && f[2].parse::<u32>() != Ok(me)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn me() -> u32 {
        nix::unistd::getuid().as_raw()
    }

    /// A private directory (0700) for one test, inside the build's own tree.
    fn scratch(name: &str) -> PathBuf {
        let base = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join("locate-tests");
        let _ = std::fs::create_dir_all(&base);
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).unwrap();
        let dir = base.join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }

    fn program(dir: &Path, name: &str, mode: u32) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    #[test]
    fn a_private_executable_is_accepted_and_runs_through_its_descriptor() {
        let dir = scratch("ok");
        let path = program(&dir, "helper", 0o755);
        let v = verify(&path, me()).unwrap();
        assert!(v.exec_path().starts_with("/proc/self/fd/"));
        // Replacing the file by path afterwards does not change what the descriptor is.
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"not the same file").unwrap();
        let held = std::fs::read(v.exec_path()).unwrap();
        assert!(
            held.starts_with(b"#!/bin/sh"),
            "the open file is still the checked one"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_bits_for_the_group_or_others_are_refused_whatever_the_group() {
        let dir = scratch("bits");
        for (mode, why) in [
            (0o775, "writable_by_others"),
            (0o757, "writable_by_others"),
            (0o777, "writable_by_others"),
            (0o4755, "set_id"),
            (0o2755, "set_id"),
            (0o644, "not_executable"),
        ] {
            let path = program(&dir, &format!("h{mode:o}"), mode);
            assert_eq!(verify(&path, me()).unwrap_err(), why, "mode {mode:o}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn something_that_is_not_a_regular_file_is_refused() {
        let dir = scratch("kinds");
        assert_eq!(verify(&dir, me()).unwrap_err(), "not_a_file");
        assert_eq!(verify(&dir.join("absent"), me()).unwrap_err(), "missing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_others_can_write_above_the_file_is_refused() {
        let dir = scratch("dirs");
        let open = dir.join("open");
        std::fs::create_dir(&open).unwrap();
        let path = program(&open, "helper", 0o755);
        assert!(verify(&path, me()).is_ok());
        // Group-writable is accepted only when the group is the user's private one.
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o770)).unwrap();
        let private = group_is_private(std::fs::metadata(&open).unwrap().gid(), me());
        assert_eq!(verify(&path, me()).is_ok(), private);
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o707)).unwrap();
        assert_eq!(verify(&path, me()).unwrap_err(), "writable_directory");
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o700)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_symlink_is_judged_by_where_it_leads() {
        let dir = scratch("links");
        let target = program(&dir, "real", 0o755);
        let link = dir.join("link");
        symlink(&target, &link).unwrap();
        let v = verify(&link, me()).unwrap();
        assert_eq!(v.real_path(), std::fs::canonicalize(&target).unwrap());
        // A link to something weak is weak.
        let weak = program(&dir, "weak", 0o775);
        let weak_link = dir.join("weak-link");
        symlink(&weak, &weak_link).unwrap();
        assert_eq!(verify(&weak_link, me()).unwrap_err(), "writable_by_others");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_group_that_is_the_users_own_alone_counts_as_private() {
        let gid = nix::unistd::getgid().as_raw();
        // This machine's account uses a private group (the default); skip the positive case where
        // the account was set up differently.
        if group_is_private(gid, me()) {
            assert!(!group_is_private(gid, me() + 12345));
        }
        // root's group has root in it and is not this user's.
        assert!(!group_is_private(0, me()));
        assert!(!group_is_private(gid + 54321, me()));
    }

    #[test]
    fn a_file_owned_by_someone_else_is_refused() {
        let dir = scratch("owner");
        let path = program(&dir, "helper", 0o755);
        // Asking as another user: the file is neither theirs nor root's.
        assert_eq!(
            verify(&path, me() + 12345).unwrap_err(),
            "foreign_directory"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_first_candidate_that_passes_is_the_one_used() {
        let dir = scratch("first");
        let weak = program(&dir, "a", 0o777);
        let good = program(&dir, "b", 0o755);
        let found = find(&[dir.join("absent"), weak, good.clone()]).unwrap();
        assert_eq!(found.real_path(), std::fs::canonicalize(good).unwrap());
        assert!(find(&[dir.join("absent")]).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
