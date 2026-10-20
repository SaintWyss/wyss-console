use super::*;
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt, symlink},
};

fn instance(n: u32) -> AgentRuntimeInstanceId {
    AgentRuntimeInstanceId::parse(&format!("00000000-0000-4000-8000-{:012x}", 0xa000 + n)).unwrap()
}
fn root() -> (std::path::PathBuf, StagingRoot) {
    let p = std::env::temp_dir().join(format!(
        "wyss-f8-stage-{}",
        crate::control::store::uuid().as_str()
    ));
    fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
    let r = StagingRoot::open(&p).unwrap();
    (p, r)
}

#[test]
fn a_launch_directory_is_private_exclusive_and_files_are_read_only_and_single_link() {
    let (path, root) = root();
    let mut a = root.create(&instance(1)).unwrap();
    a.write_file("catalogue.json", b"{\"models\":[\"a\"]}")
        .unwrap();
    let dir = path.join(instance(1).as_str());
    assert_eq!(
        fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let file = dir.join("catalogue.json");
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o400
    );
    assert_eq!(fs::metadata(&file).unwrap().nlink(), 1);
    // The same launch identity can never be staged twice; an existing file is never overwritten.
    assert!(root.create(&instance(1)).is_err());
    assert_eq!(
        a.write_file("catalogue.json", b"other"),
        Err(E::AlreadyRunning)
    );
    assert_eq!(fs::read(&file).unwrap(), b"{\"models\":[\"a\"]}");
    a.cleanup().unwrap();
    assert!(!dir.exists());
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn concurrent_launches_cannot_collide_and_b_is_unchanged_by_a_failed_a() {
    let (path, root) = root();
    let mut b = root.create(&instance(2)).unwrap();
    b.write_file("catalogue.json", b"B-bytes").unwrap();
    let before = b.identity("catalogue.json").unwrap();
    // A tries B's identity (collision), a symlink planted at its own name, and a hardlink to B.
    assert!(root.create(&instance(2)).is_err());
    let target = path.join(instance(2).as_str());
    symlink(&target, path.join(instance(3).as_str())).unwrap();
    assert!(
        root.create(&instance(3)).is_err(),
        "symlink at the launch name"
    );
    let mut a = root.create(&instance(4)).unwrap();
    fs::hard_link(
        target.join("catalogue.json"),
        path.join(instance(4).as_str()).join("catalogue.json"),
    )
    .unwrap();
    assert_eq!(
        a.write_file("catalogue.json", b"A-bytes"),
        Err(E::AlreadyRunning)
    );
    // B's live catalogue bytes and inode are untouched.
    assert_eq!(fs::read(target.join("catalogue.json")).unwrap(), b"B-bytes");
    assert_eq!(
        fs::metadata(target.join("catalogue.json")).unwrap().ino(),
        before.1
    );
    // B notices the extra link (a tampering signal) and will not vouch for or delete the file.
    assert_eq!(b.identity("catalogue.json"), Err(E::UnsafePath));
    // Cleaning A refuses to unlink what it did not create and leaves B alone.
    assert!(a.cleanup().is_err());
    assert_eq!(fs::read(target.join("catalogue.json")).unwrap(), b"B-bytes");
    assert!(b.cleanup().is_err());
    assert_eq!(fs::read(target.join("catalogue.json")).unwrap(), b"B-bytes");
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn cleanup_never_removes_a_replaced_entry_or_directory() {
    let (path, root) = root();
    let mut a = root.create(&instance(5)).unwrap();
    a.write_file("catalogue.json", b"mine").unwrap();
    let dir = path.join(instance(5).as_str());
    // Someone swaps the file for a symlink to a victim file outside the staging tree.
    let victim = path.join("victim");
    fs::write(&victim, b"precious").unwrap();
    fs::remove_file(dir.join("catalogue.json")).unwrap_or_else(|_| {
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    });
    symlink(&victim, dir.join("catalogue.json")).unwrap();
    assert_eq!(a.identity("catalogue.json"), Err(E::UnsafePath));
    assert!(a.cleanup().is_err());
    assert_eq!(fs::read(&victim).unwrap(), b"precious");
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn roots_and_names_are_validated() {
    let (path, root) = root();
    // A group/world accessible root is refused, as is a symlinked or missing one.
    let open = path.join("open");
    fs::DirBuilder::new().mode(0o755).create(&open).unwrap();
    assert!(StagingRoot::open(&open).is_err());
    symlink(&path, path.join("link")).unwrap();
    assert!(StagingRoot::open(&path.join("link")).is_err());
    assert!(StagingRoot::open(&path.join("missing")).is_err());
    let mut a = root.create(&instance(6)).unwrap();
    for bad in ["", ".", "..", "a/b", "A", "x y", &"a".repeat(65)] {
        assert_eq!(a.write_file(bad, b"x"), Err(E::InvalidRequest), "{bad:?}");
    }
    assert_eq!(
        a.write_file("big", &vec![0; MAX_FILE_BYTES + 1]),
        Err(E::InvalidRequest)
    );
    a.cleanup().unwrap();
    fs::remove_dir_all(path).unwrap();
}
