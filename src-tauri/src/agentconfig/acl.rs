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
use std::{
    fs,
    os::fd::{AsRawFd, OwnedFd},
};

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

/// The access ACL of the file behind `fd`, or `None` when it has none.
fn access_acl(fd: &OwnedFd) -> Option<Vec<u8>> {
    let name = b"system.posix_acl_access\0";
    let mut buffer = vec![0_u8; 4096];
    // SAFETY: `name` is NUL-terminated, the buffer is as long as the length passed, and the
    // descriptor is open for the whole call.
    let length = unsafe {
        libc::fgetxattr(
            fd.as_raw_fd(),
            name.as_ptr().cast(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
        )
    };
    let length = usize::try_from(length).ok()?;
    buffer.truncate(length);
    Some(buffer)
}

struct Entry {
    tag: u16,
    permissions: u16,
    id: u32,
}

fn parse(bytes: &[u8]) -> Option<Vec<Entry>> {
    // Version 2, then eight bytes an entry: tag, permissions, id, little-endian.
    if bytes.len() < 4
        || bytes.len() % 8 != 4
        || u32::from_le_bytes(bytes[..4].try_into().ok()?) != 2
    {
        return None;
    }
    Some(
        bytes[4..]
            .as_chunks::<8>()
            .0
            .iter()
            .map(|chunk| Entry {
                tag: u16::from_le_bytes([chunk[0], chunk[1]]),
                permissions: u16::from_le_bytes([chunk[2], chunk[3]]),
                id: u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]),
            })
            .collect(),
    )
}

/// Whether `gid` is a group of which `me` is the only member: no other account lists it as a
/// supplementary group and none has it as its primary one. Read from the local account files;
/// an account that exists only in a network directory is not seen, which is why this is the
/// one place that says so.
pub(crate) fn group_is_private(gid: u32, me: u32) -> bool {
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
    !others_have_it_as_primary && !members_other_than_me
}

/// Who can write the directory behind `fd` (whose mode is `mode` and group `gid`).
pub(crate) fn writers(fd: &OwnedFd, mode: u32, gid: u32, me: u32) -> Writers {
    let mut found = Writers {
        everyone: mode & 0o002 != 0,
        ..Writers::default()
    };
    let acl = access_acl(fd).and_then(|bytes| parse(&bytes));
    let Some(entries) = acl else {
        // No ACL: the mode bits say it all.
        found.group_shared = mode & 0o020 != 0 && !group_is_private(gid, me);
        return found;
    };
    let mask = entries
        .iter()
        .find(|e| e.tag == TAG_MASK)
        .map(|e| e.permissions);
    let effective = |permissions: u16| mask.map_or(permissions, |m| permissions & m);
    for entry in &entries {
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
            TAG_USER_OBJ => (),
            _ => (),
        }
    }
    found.named.sort_unstable();
    found.named.dedup();
    found
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
