//! Who besides the owner can write a directory, read from the directory itself: its POSIX ACL
//! if it has one, its mode bits if it does not (07 §4.2).
//!
//! The rule the engine started with was "a group-writable directory is read and never
//! written". That is right for a directory a stranger could write, and wrong for two things
//! that are not strangers: the owner's own private group (a group of which the owner is the
//! only member changes nothing), and an account the owner has deliberately given access and
//! has *named* to the Console (`trusted-writers`). Everything else a named ACL entry or a
//! shared group allows stays read only. This module only says who can write; whether that is
//! acceptable is decided in `place.rs`.
use nix::libc;
use std::{fs, os::fd::AsRawFd};

const TAG_USER_OBJ: u16 = 0x01;
const TAG_USER: u16 = 0x02;
const TAG_GROUP_OBJ: u16 = 0x04;
const TAG_GROUP: u16 = 0x08;
const TAG_MASK: u16 = 0x10;
const TAG_OTHER: u16 = 0x20;
const WRITE: u16 = 0x02;

/// The principals other than the owner that can write a directory.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Writers {
    /// Users named in the ACL that can write (the owner is not listed).
    pub named: Vec<u32>,
    /// A group with more than the owner in it can write.
    pub group_shared: bool,
    /// Anyone can write.
    pub everyone: bool,
}

const ACCESS_ACL: &[u8] = b"system.posix_acl_access\0";
const DEFAULT_ACL: &[u8] = b"system.posix_acl_default\0";
const MOST_ACL_BYTES: usize = 4096;

/// What asking for one ACL attribute found. Not being able to ask is not the same as there being
/// nothing to find: only the kernel saying so (no attribute, or a file system that keeps none)
/// is an absence.
enum Attribute {
    Absent,
    Found(Vec<u8>),
    Unknown,
}

fn attribute(fd: &impl AsRawFd, name: &[u8]) -> Attribute {
    for _ in 0..2 {
        // SAFETY: `name` is NUL-terminated and the descriptor is open for the whole call.
        let size = unsafe {
            libc::fgetxattr(
                fd.as_raw_fd(),
                name.as_ptr().cast(),
                std::ptr::null_mut(),
                0,
            )
        };
        if size < 0 {
            return match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::ENODATA | libc::EOPNOTSUPP) => Attribute::Absent,
                _ => Attribute::Unknown,
            };
        }
        let Ok(size) = usize::try_from(size) else {
            return Attribute::Unknown;
        };
        if size > MOST_ACL_BYTES {
            return Attribute::Unknown;
        }
        let mut buffer = vec![0_u8; size];
        // SAFETY: the buffer is as long as the length passed, and the descriptor is open.
        let read = unsafe {
            libc::fgetxattr(
                fd.as_raw_fd(),
                name.as_ptr().cast(),
                buffer.as_mut_ptr().cast(),
                size,
            )
        };
        if let Ok(read) = usize::try_from(read) {
            buffer.truncate(read);
            return Attribute::Found(buffer);
        }
        // It grew between the two calls: look once more, then give up.
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::ERANGE) {
            return Attribute::Unknown;
        }
    }
    Attribute::Unknown
}

pub(crate) struct Entry {
    pub(crate) tag: u16,
    pub(crate) permissions: u16,
    pub(crate) id: u32,
}

pub(crate) fn parse(bytes: &[u8]) -> Option<Vec<Entry>> {
    // Version 2, then eight bytes an entry: tag, permissions, id, little-endian.
    if bytes.len() < 4
        || bytes.len() % 8 != 4
        || u32::from_le_bytes(bytes[..4].try_into().ok()?) != 2
    {
        return None;
    }
    let entries: Vec<Entry> = bytes[4..]
        .as_chunks::<8>()
        .0
        .iter()
        .map(|chunk| Entry {
            tag: u16::from_le_bytes([chunk[0], chunk[1]]),
            permissions: u16::from_le_bytes([chunk[2], chunk[3]]),
            id: u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]),
        })
        .collect();
    // A tag or a permission the kernel does not write is not an ACL this understands.
    entries
        .iter()
        .all(|e| {
            matches!(
                e.tag,
                TAG_USER_OBJ | TAG_USER | TAG_GROUP_OBJ | TAG_GROUP | TAG_MASK | TAG_OTHER
            ) && e.permissions & !0o7 == 0
        })
        .then_some(entries)
}

/// Whether the account databases are the local files only: a name service that reaches a
/// network directory could list members the files do not show.
fn accounts_are_local() -> bool {
    let Ok(conf) = fs::read_to_string("/etc/nsswitch.conf") else {
        return false;
    };
    ["passwd", "group"].iter().all(|database| {
        conf.lines()
            .map(|l| l.split('#').next().unwrap_or("").trim())
            .find_map(|l| l.strip_prefix(&format!("{database}:")))
            .is_some_and(|sources| {
                sources
                    .split_whitespace()
                    .filter(|s| !s.starts_with('['))
                    .all(|s| matches!(s, "files" | "compat" | "systemd"))
            })
    })
}

/// Whether `gid` is a group of which `me` is the only member: no other account lists it as a
/// supplementary group and none has it as its primary one. Read from the local account files
/// and, for the members the system itself reports, from the name service; when the name
/// service can reach anything but local files it cannot be proved, and the answer is no.
pub(crate) fn group_is_private(gid: u32, me: u32) -> bool {
    if !accounts_are_local() {
        return false;
    }
    let Ok(group) = fs::read_to_string("/etc/group") else {
        return false;
    };
    let Ok(passwd) = fs::read_to_string("/etc/passwd") else {
        return false;
    };
    let my_name = passwd
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            (f.get(2)?.parse::<u32>().ok()? == me).then(|| f[0].to_owned())
        })
        .next();
    let others_have_it_as_primary = passwd.lines().any(|l| {
        let f: Vec<&str> = l.split(':').collect();
        f.get(3).and_then(|g| g.parse::<u32>().ok()) == Some(gid)
            && f.get(2).and_then(|u| u.parse::<u32>().ok()) != Some(me)
    });
    let members_other_than_me = group.lines().any(|l| {
        let f: Vec<&str> = l.split(':').collect();
        f.get(2).and_then(|g| g.parse::<u32>().ok()) == Some(gid)
            && f.get(3).is_some_and(|m| {
                m.split(',')
                    .any(|name| !name.is_empty() && Some(name) != my_name.as_deref())
            })
    });
    let system_reports_others = match nix::unistd::Group::from_gid(nix::unistd::Gid::from_raw(gid))
    {
        Ok(Some(entry)) => entry
            .mem
            .iter()
            .any(|name| Some(name.as_str()) != my_name.as_deref()),
        _ => true,
    };
    !others_have_it_as_primary && !members_other_than_me && !system_reports_others
}

/// Who the entries of an ACL let write, on top of `found`.
fn from_entries(entries: &[Entry], gid: u32, me: u32, mut found: Writers) -> Writers {
    let mask = entries
        .iter()
        .find(|e| e.tag == TAG_MASK)
        .map(|e| e.permissions);
    let effective = |permissions: u16| mask.map_or(permissions, |m| permissions & m);
    for entry in entries {
        match entry.tag {
            TAG_OTHER if entry.permissions & WRITE != 0 => found.everyone = true,
            TAG_USER if effective(entry.permissions) & WRITE != 0 && entry.id != me => {
                found.named.push(entry.id);
            }
            TAG_GROUP_OBJ if effective(entry.permissions) & WRITE != 0 => {
                found.group_shared |= !group_is_private(gid, me);
            }
            TAG_GROUP if effective(entry.permissions) & WRITE != 0 => {
                found.group_shared |= !group_is_private(entry.id, me);
            }
            _ => (),
        }
    }
    found.named.sort_unstable();
    found.named.dedup();
    found
}

/// What stands for "somebody we cannot name can write": an ACL that could not be read or
/// understood is never taken to be the absence of one.
fn unverified() -> Writers {
    Writers {
        everyone: true,
        ..Writers::default()
    }
}

/// Who can write the directory behind `fd` (whose mode is `mode` and group `gid`).
pub(crate) fn writers(fd: &impl AsRawFd, mode: u32, gid: u32, me: u32) -> Writers {
    let found = Writers {
        everyone: mode & 0o002 != 0,
        ..Writers::default()
    };
    let entries = match attribute(fd, ACCESS_ACL) {
        Attribute::Found(bytes) => match parse(&bytes) {
            Some(entries) => entries,
            None => return unverified(),
        },
        // No ACL: the mode bits say it all.
        Attribute::Absent => {
            let mut found = found;
            found.group_shared = mode & 0o020 != 0 && !group_is_private(gid, me);
            return found;
        }
        Attribute::Unknown => return unverified(),
    };
    from_entries(&entries, gid, me, found)
}

/// Who the ACL a directory gives what is made in it (its default ACL) lets write. A directory
/// that hands another account write access to every file created in it has given it the project.
pub(crate) fn inherited(fd: &impl AsRawFd, gid: u32, me: u32) -> Writers {
    match attribute(fd, DEFAULT_ACL) {
        Attribute::Absent => Writers::default(),
        Attribute::Found(bytes) => parse(&bytes).map_or_else(unverified, |entries| {
            from_entries(&entries, gid, me, Writers::default())
        }),
        Attribute::Unknown => unverified(),
    }
}

/// Writes an ACL for a test: the entries as `(tag, permissions, id)`.
#[cfg(test)]
pub(crate) fn set_acl_for_test(path: &std::path::Path, entries: &[(u16, u16, u32)]) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let mut bytes = 2_u32.to_le_bytes().to_vec();
    for (tag, permissions, id) in entries {
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(&permissions.to_le_bytes());
        bytes.extend_from_slice(&id.to_le_bytes());
    }
    let name = b"system.posix_acl_access\0";
    let mut path = path.as_os_str().as_bytes().to_vec();
    path.push(0);
    // SAFETY: both names are NUL-terminated and the value is as long as the length passed.
    unsafe {
        libc::setxattr(
            path.as_ptr().cast(),
            name.as_ptr().cast(),
            bytes.as_ptr().cast(),
            bytes.len(),
            0,
        ) == 0
    }
}

#[cfg(test)]
pub(crate) const ACL_USER_OBJ: u16 = TAG_USER_OBJ;
#[cfg(test)]
pub(crate) const ACL_USER: u16 = TAG_USER;
#[cfg(test)]
pub(crate) const ACL_GROUP_OBJ: u16 = TAG_GROUP_OBJ;
#[cfg(test)]
pub(crate) const ACL_MASK: u16 = TAG_MASK;
#[cfg(test)]
pub(crate) const ACL_OTHER: u16 = TAG_OTHER;
#[cfg(test)]
pub(crate) const ACL_GROUP: u16 = TAG_GROUP;

/// Makes `path` writable by a group that certainly has others in it (root's), the way a
/// shared directory is, whatever groups this machine has. Tests only.
#[cfg(test)]
pub(crate) fn share_with_a_group_for_test(path: &std::path::Path) {
    assert!(
        set_acl_for_test(
            path,
            &[
                (TAG_USER_OBJ, 7, 0),
                (TAG_GROUP_OBJ, 5, 0),
                (TAG_GROUP, 7, 0),
                (TAG_MASK, 7, 0),
                (TAG_OTHER, 5, 0),
            ]
        ),
        "this test needs POSIX ACLs on the temporary directory"
    );
}
