//! M1-02, the part about who else can use a folder: a project root is looked at through the
//! descriptor that was opened, and anybody else who can write it (its POSIX ACL, its mode bits, or
//! the ACL it gives what is made in it) makes it refuse, except the accounts the owner named in
//! the `trusted-writers` file the configuration engine already believes. What cannot be read is
//! never taken to be private. Real extended attributes on real folders; where the file system
//! keeps no ACL the test says so on stderr and checks what it can.
use super::*;
use crate::projects::owner::inspect_root_with;
use std::{
    ffi::CString,
    fs,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

const USER_OBJ: u16 = 0x01;
const USER: u16 = 0x02;
const GROUP_OBJ: u16 = 0x04;
const GROUP: u16 = 0x08;
const MASK: u16 = 0x10;
const OTHER: u16 = 0x20;
/// An account that exists on any Linux system and is not the user's.
const NOBODY: u32 = 65534;
/// The group of root, which has more than the owner in it.
const ROOTS: u32 = 0;

fn acl(entries: &[(u16, u16, u32)]) -> Vec<u8> {
    let mut blob = 2u32.to_le_bytes().to_vec();
    for (tag, perm, id) in entries {
        blob.extend(tag.to_le_bytes());
        blob.extend(perm.to_le_bytes());
        blob.extend(id.to_le_bytes());
    }
    blob
}

/// The owner alone, plus `named` under `mask`.
fn with(named: &[(u16, u16, u32)], mask: u16) -> Vec<u8> {
    let mut entries = vec![(USER_OBJ, 7, 0), (GROUP_OBJ, 0, 0), (OTHER, 0, 0)];
    entries.extend_from_slice(named);
    entries.push((MASK, mask, 0));
    // The kernel takes the entries in order of tag, then of id.
    entries.sort_by_key(|(tag, _, id)| (*tag, *id));
    acl(&entries)
}

fn folder(dir: &TestDir, name: &str, mode: u32) -> PathBuf {
    let path = fs::canonicalize(&dir.0).unwrap().join(name);
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
    path
}

/// A state directory with the owner's declared accounts, as the configuration engine reads it.
fn state(dir: &TestDir, accounts: &str, mode: u32) -> PathBuf {
    let state = fs::canonicalize(&dir.0).unwrap().join("state");
    fs::create_dir_all(state.join("configuration")).unwrap();
    fs::set_permissions(
        state.join("configuration"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let file = state.join("configuration/trusted-writers");
    fs::write(&file, accounts).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(mode)).unwrap();
    state
}

fn inspect(path: &Path, state: Option<&Path>) -> Result<(), RootError> {
    inspect_root_with(path.to_str().unwrap(), None, state).map(|_| ())
}

/// Writes `blob` as an ACL attribute. `false` when this file system keeps none.
fn set_acl(path: &Path, name: &str, blob: &[u8]) -> bool {
    let (path, name) = (
        CString::new(path.to_str().unwrap()).unwrap(),
        CString::new(name).unwrap(),
    );
    // SAFETY: both strings are NUL-terminated and the value is `blob.len()` bytes.
    let done = unsafe {
        nix::libc::setxattr(
            path.as_ptr(),
            name.as_ptr(),
            blob.as_ptr().cast(),
            blob.len(),
            0,
        )
    };
    if done == 0 {
        return true;
    }
    let error = std::io::Error::last_os_error();
    assert!(
        matches!(
            error.raw_os_error(),
            Some(nix::libc::ENOTSUP | nix::libc::EPERM)
        ),
        "setting an ACL failed unexpectedly: {error}"
    );
    eprintln!("SKIPPED (this filesystem keeps no ACL): {error}");
    false
}

const ACCESS: &str = "system.posix_acl_access";
const DEFAULT: &str = "system.posix_acl_default";

#[test]
fn a_private_folder_is_a_root_and_one_the_world_can_write_is_not() {
    let dir = TestDir::new();
    inspect(&folder(&dir, "private", 0o700), None).unwrap();
    // Others reading is not others writing.
    inspect(&folder(&dir, "readable", 0o755), None).unwrap();
    for mode in [0o777, 0o757, 0o707, 0o702] {
        let open = folder(&dir, &format!("open-{mode:o}"), mode);
        assert_eq!(inspect(&open, None), Err(RootError::Shared), "{mode:o}");
    }
}

#[test]
fn a_folder_a_group_with_others_in_it_can_write_is_not_a_root() {
    let dir = TestDir::new();
    let path = folder(&dir, "group", 0o700);
    if !set_acl(
        &path,
        ACCESS,
        &acl(&[
            (USER_OBJ, 7, 0),
            (GROUP_OBJ, 0, 0),
            (GROUP, 7, ROOTS),
            (MASK, 7, 0),
            (OTHER, 0, 0),
        ]),
    ) {
        return;
    }
    assert_eq!(inspect(&path, None), Err(RootError::Shared));
    // A group is never an account the owner named: naming accounts does not let it in.
    let declared = state(&dir, "nobody\nroot\n", 0o600);
    assert_eq!(inspect(&path, Some(&declared)), Err(RootError::Shared));
    // The group reading only is not a way to write.
    assert!(set_acl(
        &path,
        ACCESS,
        &acl(&[
            (USER_OBJ, 7, 0),
            (GROUP_OBJ, 0, 0),
            (GROUP, 5, ROOTS),
            (MASK, 7, 0),
            (OTHER, 0, 0),
        ]),
    ));
    inspect(&path, None).unwrap();
}

#[test]
fn a_named_account_that_can_write_refuses_the_folder_unless_the_owner_named_it() {
    let dir = TestDir::new();
    let path = folder(&dir, "acl", 0o700);
    let me = nix::unistd::geteuid().as_raw();
    // Only the owner named: nothing foreign.
    if !set_acl(&path, ACCESS, &with(&[(USER, 7, me)], 7)) {
        return;
    }
    inspect(&path, None).unwrap();
    // Another account named, with write.
    assert!(set_acl(&path, ACCESS, &with(&[(USER, 7, NOBODY)], 7)));
    assert_eq!(inspect(&path, None), Err(RootError::Shared));
    // Only the account the owner declared, in a file only the owner can write.
    let declared = state(&dir, "# on purpose\nnobody\n", 0o600);
    inspect(&path, Some(&declared)).unwrap();
    let another = state(&dir, "root\n", 0o600);
    assert_eq!(inspect(&path, Some(&another)), Err(RootError::Shared));
    // A declaration anybody else could have written is no declaration.
    for mode in [0o660, 0o666] {
        let loose = state(&dir, "nobody\n", mode);
        assert_eq!(
            inspect(&path, Some(&loose)),
            Err(RootError::Shared),
            "{mode:o}"
        );
    }
    // Masked down, or granted read only, it writes nothing.
    assert!(set_acl(&path, ACCESS, &with(&[(USER, 7, NOBODY)], 5)));
    inspect(&path, None).unwrap();
    assert!(set_acl(&path, ACCESS, &with(&[(USER, 4, NOBODY)], 7)));
    inspect(&path, None).unwrap();
}

#[test]
fn what_new_files_here_will_be_given_to_another_account_counts_too() {
    let dir = TestDir::new();
    let path = folder(&dir, "default", 0o700);
    if !set_acl(&path, DEFAULT, &with(&[(USER, 7, NOBODY)], 7)) {
        return;
    }
    assert_eq!(inspect(&path, None), Err(RootError::Shared));
    inspect(&path, Some(&state(&dir, "nobody\n", 0o600))).unwrap();
    // Everybody writing what is made here is not an account anyone named.
    assert!(set_acl(
        &path,
        DEFAULT,
        &acl(&[(USER_OBJ, 7, 0), (GROUP_OBJ, 5, 0), (OTHER, 2, 0)]),
    ));
    assert_eq!(
        inspect(&path, Some(&state(&dir, "nobody\n", 0o600))),
        Err(RootError::Shared)
    );
}

#[test]
fn an_acl_that_cannot_be_read_is_not_taken_to_be_private() {
    let dir = TestDir::new();
    let path = folder(&dir, "unreadable", 0o700);
    // A descriptor made only to name the folder cannot be asked for its attributes: that is a
    // failure to know, not the absence of an ACL, and a folder nobody can vouch for is shared.
    let named = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_PATH | nix::libc::O_DIRECTORY | nix::libc::O_CLOEXEC)
        .open(&path)
        .unwrap();
    let meta = named.metadata().unwrap();
    assert!(crate::agentconfig::folder_is_shared(
        &named,
        meta.mode(),
        meta.gid(),
        meta.uid(),
        None
    ));
    // The same folder through a descriptor that can be asked is private.
    let open = fs::File::open(&path).unwrap();
    assert!(!crate::agentconfig::folder_is_shared(
        &open,
        meta.mode(),
        meta.gid(),
        meta.uid(),
        None
    ));
}

#[test]
fn a_link_is_not_followed_to_a_better_folder() {
    let dir = TestDir::new();
    let open = folder(&dir, "open", 0o777);
    let private = folder(&dir, "private", 0o700);
    let link = fs::canonicalize(&dir.0).unwrap().join("link");
    std::os::unix::fs::symlink(&private, &link).unwrap();
    assert_eq!(inspect(&link, None), Err(RootError::Ineligible));
    assert_eq!(inspect(&open, None), Err(RootError::Shared));
}
