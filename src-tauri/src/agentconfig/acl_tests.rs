//! Who can write a directory, from its ACL and its mode, and the file the owner names
//! co-writers in. Real extended attributes on a real directory; skipped where the file system
//! has none.
use super::acl::{
    ACL_GROUP, ACL_GROUP_OBJ, ACL_MASK, ACL_OTHER, ACL_USER, ACL_USER_OBJ, Writers,
    set_acl_for_test, writers,
};
use super::declared;
use nix::unistd::{getegid, geteuid};
use std::{
    fs,
    os::fd::OwnedFd,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::PathBuf,
};

fn dir(mode: u32) -> (PathBuf, OwnedFd) {
    let base = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("wyss-acl-{}", uuid::Uuid::new_v4().simple()));
    fs::DirBuilder::new().mode(0o700).create(&base).unwrap();
    fs::set_permissions(&base, fs::Permissions::from_mode(mode)).unwrap();
    let fd = OwnedFd::from(fs::File::open(&base).unwrap());
    (base, fd)
}

fn who(base: &PathBuf, fd: &OwnedFd) -> Writers {
    let meta = fs::metadata(base).unwrap();
    use std::os::unix::fs::MetadataExt;
    writers(fd, meta.mode(), meta.gid(), geteuid().as_raw())
}

#[test]
fn a_plain_private_directory_has_no_other_writer() {
    let (base, fd) = dir(0o700);
    assert_eq!(who(&base, &fd), Writers::default());
    fs::remove_dir(base).unwrap();
}

#[test]
fn world_write_is_everyone_and_group_write_depends_on_who_is_in_the_group() {
    let (base, fd) = dir(0o777);
    assert!(who(&base, &fd).everyone);
    fs::remove_dir(&base).unwrap();
    let (base, fd) = dir(0o770);
    let private = super::acl::group_is_private(getegid().as_raw(), geteuid().as_raw());
    assert_eq!(who(&base, &fd).group_shared, !private);
    fs::remove_dir(base).unwrap();
}

#[test]
fn a_named_user_counts_only_through_the_mask() {
    let (base, fd) = dir(0o700);
    let entries = |mask: u16| {
        [
            (ACL_USER_OBJ, 7, 0),
            (ACL_USER, 7, 65534),
            (ACL_GROUP_OBJ, 0, 0),
            (ACL_MASK, mask, 0),
            (ACL_OTHER, 0, 0),
        ]
    };
    if !set_acl_for_test(&base, &entries(7)) {
        fs::remove_dir(base).unwrap();
        return;
    }
    assert_eq!(who(&base, &fd).named, vec![65534]);
    // The same entry under a mask without write grants nothing.
    assert!(set_acl_for_test(&base, &entries(5)));
    assert!(who(&base, &fd).named.is_empty());
    // The owner is never listed as another writer of his own directory.
    assert!(set_acl_for_test(
        &base,
        &[
            (ACL_USER_OBJ, 7, 0),
            (ACL_USER, 7, geteuid().as_raw()),
            (ACL_GROUP_OBJ, 0, 0),
            (ACL_MASK, 7, 0),
            (ACL_OTHER, 0, 0),
        ]
    ));
    assert!(who(&base, &fd).named.is_empty());
    fs::remove_dir(base).unwrap();
}

#[test]
fn a_named_group_with_others_in_it_is_a_shared_writer() {
    let (base, fd) = dir(0o700);
    if !set_acl_for_test(
        &base,
        &[
            (ACL_USER_OBJ, 7, 0),
            (ACL_GROUP_OBJ, 0, 0),
            (ACL_GROUP, 7, 0), // root's group
            (ACL_MASK, 7, 0),
            (ACL_OTHER, 0, 0),
        ],
    ) {
        fs::remove_dir(base).unwrap();
        return;
    }
    assert!(who(&base, &fd).group_shared);
    fs::remove_dir(base).unwrap();
}

fn state_with(file: Option<(&str, u32)>) -> PathBuf {
    let base = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("wyss-declared-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir_all(base.join("configuration")).unwrap();
    if let Some((text, mode)) = file {
        let path = base.join("configuration/trusted-writers");
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
    }
    base
}

#[test]
fn the_file_of_named_accounts_is_believed_only_when_only_the_owner_can_write_it() {
    let text = "# the sandboxed account\nnobody   # ok\n\nBad Name\nroot\n";
    let good = state_with(Some((text, 0o600)));
    assert_eq!(declared::load(&good), ["nobody", "root"]);
    // Writable by the group or by anyone: ignored whole.
    for mode in [0o660, 0o666, 0o620] {
        let bad = state_with(Some((text, mode)));
        assert!(declared::load(&bad).is_empty(), "{mode:o}");
    }
    // Absent, or a link, or too large: nothing.
    assert!(declared::load(&state_with(None)).is_empty());
    let linked = state_with(None);
    std::os::unix::fs::symlink(
        good.join("configuration/trusted-writers"),
        linked.join("configuration/trusted-writers"),
    )
    .unwrap();
    assert!(declared::load(&linked).is_empty());
    let big = state_with(Some((&"nobody\n".repeat(1000), 0o600)));
    assert!(declared::load(&big).is_empty());
    // A folder that others can write, or that is itself a link, is not believed either.
    let open_folder = state_with(Some((text, 0o600)));
    fs::set_permissions(
        open_folder.join("configuration"),
        fs::Permissions::from_mode(0o777),
    )
    .unwrap();
    assert!(declared::load(&open_folder).is_empty());
    let behind_a_link = state_with(None);
    fs::remove_dir(behind_a_link.join("configuration")).unwrap();
    std::os::unix::fs::symlink(
        good.join("configuration"),
        behind_a_link.join("configuration"),
    )
    .unwrap();
    assert!(declared::load(&behind_a_link).is_empty());
}

#[test]
fn a_name_that_is_not_an_account_here_declares_nobody() {
    let uids = declared::uids(&["nobody".to_owned(), "no-such-account-xyz".to_owned()]);
    assert_eq!(uids.len(), 1);
}

fn image(entries: &[(u16, u16, u32)]) -> Vec<u8> {
    let mut bytes = 2_u32.to_le_bytes().to_vec();
    for (tag, permissions, id) in entries {
        bytes.extend(tag.to_le_bytes());
        bytes.extend(permissions.to_le_bytes());
        bytes.extend(id.to_le_bytes());
    }
    bytes
}

#[test]
fn an_acl_the_kernel_does_not_write_is_not_understood_and_so_is_not_private() {
    use super::acl::parse;
    let base = [
        (ACL_USER_OBJ, 7, 0),
        (ACL_GROUP_OBJ, 5, 0),
        (ACL_OTHER, 0, 0),
    ];
    assert!(parse(&image(&base)).is_some());
    // A tag or a permission bit that does not exist, another version, a torn image.
    assert!(parse(&image(&[(0x40, 7, 0)])).is_none());
    assert!(parse(&image(&[(ACL_USER_OBJ, 8, 0)])).is_none());
    let mut other_version = image(&base);
    other_version[0] = 1;
    assert!(parse(&other_version).is_none());
    assert!(parse(&image(&base)[..image(&base).len() - 3]).is_none());
    assert!(parse(&[]).is_none());
}

#[test]
fn the_acl_a_directory_gives_what_is_made_in_it_is_read_apart() {
    use super::acl::inherited;
    let (base, fd) = dir(0o700);
    // Nothing inherited: nobody.
    assert_eq!(
        inherited(&fd, getegid().as_raw(), geteuid().as_raw()),
        Writers::default()
    );
    fs::remove_dir(base).unwrap();
}

#[test]
fn an_attribute_that_cannot_be_asked_for_is_somebody_who_cannot_be_named() {
    use super::acl::inherited;
    use std::os::unix::fs::OpenOptionsExt;
    let (base, _) = dir(0o700);
    // A descriptor that only names the directory cannot be asked for its attributes. That is a
    // failure to know, not an absence: both the ACL it has and the one it gives are unverified.
    let named = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_PATH | nix::libc::O_DIRECTORY | nix::libc::O_CLOEXEC)
        .open(&base)
        .unwrap();
    let (me, gid) = (geteuid().as_raw(), getegid().as_raw());
    assert!(writers(&named, 0o700, gid, me).everyone);
    assert!(inherited(&named, gid, me).everyone);
    // The same directory through one that can be asked has neither.
    let open = fs::File::open(&base).unwrap();
    assert_eq!(writers(&open, 0o700, gid, me), Writers::default());
    assert_eq!(inherited(&open, gid, me), Writers::default());
    fs::remove_dir(base).unwrap();
}
