//! Tests for the synthetic capture fixture. Every filesystem object lives in a [`SyntheticRoot`]
//! created under the system temporary directory, and every "external canary" is a synthetic file
//! in a second such root. No home directory, project, credential or agent runtime is read.
use super::{
    CONFINED, Identity, NodeKind, Phase, Stage, SyntheticCapture, SyntheticRoot, directory_error,
    is_classified_leaf, leaf_error, node_kind, openat2, read_prefix, same_places,
};
use crate::contracts::b1::{
    B1ErrorCode, B1Result, GitOid, MAX_INPUT_BYTES,
    admission::{
        AdmittedSnapshot, BaseTree, SnapshotContent, SuppliedContent, SuppliedRecord, TaskContext,
        TrustedCapture, admit,
    },
    digest,
    path::{B1RelPath, base64url},
    source::{B1SourceSelectionV1, ChangeOrigin, EXECUTABLE_MODE, REGULAR_MODE},
};
use nix::{
    errno::Errno,
    fcntl::OFlag,
    libc,
    sys::stat::{Mode, SFlag, mknod},
    unistd::mkfifo,
};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{FileExt, PermissionsExt, symlink},
        },
    },
    sync::mpsc,
    thread,
    time::Duration,
};

const TASK: &str = "11111111-1111-4111-8111-111111111111";
const SELECTION: &str = "22222222-2222-4222-8222-222222222222";
const REPO: &str = "33333333-3333-4333-8333-333333333333";
const POLICY: &str = "44444444-4444-4444-8444-444444444444";
const BASE: &str = "0123456789abcdef0123456789abcdef01234567";
const SHA_POLICY: &str = "abababababababababababababababababababababababababababababababab";
const SHA_REVIEW: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";
const CANARY: &[u8] = b"SYNTHETIC-CANARY: this must never be captured";

/// Aborts the process if a capture hangs, so a regression cannot stall the whole test run.
struct Watchdog(#[allow(dead_code)] mpsc::Sender<()>);

fn watchdog() -> Watchdog {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        if receiver.recv_timeout(Duration::from_secs(30)) == Err(mpsc::RecvTimeoutError::Timeout) {
            eprintln!("synthetic capture hung; aborting");
            std::process::abort();
        }
    });
    Watchdog(sender)
}

fn oid(text: &str) -> GitOid {
    serde_json::from_value(json!(text)).unwrap()
}

/// What the owner's selection names. Everything is an add from the untracked set, except deletes.
enum Want<'a> {
    Regular(&'a [u8], &'a [u8], u32),
    Symlink(&'a [u8], &'a [u8]),
    Delete(&'a [u8]),
}

fn want_path<'a>(want: &Want<'a>) -> &'a [u8] {
    match want {
        Want::Regular(path, ..) | Want::Symlink(path, _) | Want::Delete(path) => path,
    }
}

fn regular<'a>(path: &'a str, bytes: &'a [u8]) -> Want<'a> {
    Want::Regular(path.as_bytes(), bytes, REGULAR_MODE)
}

fn symlink_want<'a>(path: &'a str, target: &'a str) -> Want<'a> {
    Want::Symlink(path.as_bytes(), target.as_bytes())
}

fn selection_for(mut wants: Vec<Want<'_>>) -> (B1SourceSelectionV1, BaseTree) {
    wants.sort_by_key(|want| want_path(want).to_vec());
    let mut base_entries = Vec::new();
    let overlay: Vec<Value> = wants
        .iter()
        .map(|want| {
            let encoded = base64url::encode(want_path(want));
            match want {
                Want::Regular(_, bytes, mode) => json!({
                    "path": encoded, "operation": "add", "origin": "untracked",
                    "expected_base_oid": null, "rename_from": null,
                    "content": {
                        "kind": "regular", "content_sha256": digest::raw_sha256(bytes).as_str(),
                        "size": bytes.len(), "mode": mode
                    },
                    "reviewed_sha256": SHA_REVIEW
                }),
                Want::Symlink(_, target) => json!({
                    "path": encoded, "operation": "add", "origin": "untracked",
                    "expected_base_oid": null, "rename_from": null,
                    "content": {"kind": "symlink", "target": base64url::encode(target)},
                    "reviewed_sha256": SHA_REVIEW
                }),
                Want::Delete(raw) => {
                    base_entries.push((B1RelPath::from_bytes(raw).unwrap(), oid(BASE)));
                    json!({
                        "path": encoded, "operation": "delete", "origin": "unstaged",
                        "expected_base_oid": BASE, "rename_from": null, "content": null,
                        "reviewed_sha256": SHA_REVIEW
                    })
                }
            }
        })
        .collect();
    let selection = serde_json::from_value(json!({
        "schema_version": 1, "task_id": TASK, "selection_id": SELECTION,
        "repo_id": REPO, "base_commit_oid": BASE, "repo_object_format": "sha1",
        "admitted_overlay": overlay, "policy_id": POLICY,
        "policy_sha256": SHA_POLICY, "issued_at": "2026-10-16T10:00:00Z"
    }))
    .unwrap();
    (selection, BaseTree::new(oid(BASE), base_entries).unwrap())
}

fn context() -> TaskContext {
    TaskContext::new(serde_json::from_value(json!(TASK)).unwrap(), 1).unwrap()
}

fn enabled(root: &SyntheticRoot) -> SyntheticCapture<'_> {
    SyntheticCapture::disabled(root).enabled_for_synthetic_test()
}

fn capture_code(capture: &SyntheticCapture<'_>, selection: &B1SourceSelectionV1) -> B1ErrorCode {
    capture.capture(selection).unwrap_err().code
}

fn admit_captured(
    selection: &B1SourceSelectionV1,
    base: &BaseTree,
    records: &[SuppliedRecord],
) -> B1Result<AdmittedSnapshot> {
    admit(&context(), selection, base, records, 1)
}

// ---- Synthetic filesystem setup (path based; setup only, never used by the capture) --------

fn write(root: &SyntheticRoot, relative: &str, bytes: &[u8]) {
    let target = root.at(relative);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, bytes).unwrap();
}

fn canary_root() -> SyntheticRoot {
    let external = SyntheticRoot::create();
    write(&external, "canary.txt", CANARY);
    external
}

fn contains_canary(records: &[SuppliedRecord]) -> bool {
    records.iter().any(|record| match &record.content {
        SuppliedContent::Regular { bytes, .. } => bytes.windows(CANARY.len()).any(|w| w == CANARY),
        SuppliedContent::Symlink { target } => target.windows(CANARY.len()).any(|w| w == CANARY),
        SuppliedContent::Unsupported(_) => false,
    })
}

fn only_regular(records: &[SuppliedRecord]) -> (u32, &[u8]) {
    match &records[0].content {
        SuppliedContent::Regular { mode, bytes } => (*mode, bytes),
        other => panic!("expected a regular record, got {other:?}"),
    }
}

// ---- The fixture is disabled by default ----------------------------------------------------

#[test]
fn a_new_capture_is_disabled_and_touches_nothing() {
    let root = SyntheticRoot::create();
    // A FIFO would fail with INPUT_KIND_UNSUPPORTED if the capture looked at it at all.
    mkfifo(&root.at("pipe"), Mode::S_IRWXU).unwrap();
    let (selection, _) = selection_for(vec![regular("pipe", b"x")]);
    let capture = SyntheticCapture::disabled(&root);
    assert_eq!(
        capture_code(&capture, &selection),
        B1ErrorCode::IsolationUnavailable
    );
    assert_eq!(
        capture_code(&enabled(&root), &selection),
        B1ErrorCode::InputKindUnsupported
    );
}

#[test]
fn roots_are_private_temporary_directories_removed_on_drop() {
    let root = SyntheticRoot::create();
    let location = root.path().to_path_buf();
    assert!(location.starts_with(std::env::temp_dir()));
    assert_eq!(
        fs::metadata(&location).unwrap().permissions().mode() & 0o777,
        0o700
    );
    write(&root, "a/b/c.txt", b"x");
    let outside = canary_root();
    symlink(outside.path(), root.at("escape")).unwrap();
    drop(root);
    assert!(!location.exists());
    // Removal removed the link, and never followed it into the other root.
    assert_eq!(fs::read(outside.at("canary.txt")).unwrap(), CANARY);
}

#[test]
fn the_required_kernel_primitive_is_present() {
    let root = SyntheticRoot::create();
    write(&root, "f", b"x");
    openat2(
        root.fd.as_raw_fd(),
        b"f",
        OFlag::O_RDONLY | OFlag::O_NOFOLLOW,
    )
    .expect("openat2 is required; the fixture has no path-based fallback");
}

// ---- Regular files ----------------------------------------------------------------------------

#[test]
fn regular_files_are_captured_with_bytes_mode_and_selected_origin() {
    let root = SyntheticRoot::create();
    write(&root, "README.md", b"# synthetic\n");
    write(&root, "src/deep/er/tool.sh", b"#!/bin/sh\n");
    fs::set_permissions(
        root.at("src/deep/er/tool.sh"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    write(&root, "empty", b"");
    write(&root, "binary.bin", &[0, 255, 254, 0, 1]);
    let (selection, base) = selection_for(vec![
        regular("README.md", b"# synthetic\n"),
        Want::Regular(b"src/deep/er/tool.sh", b"#!/bin/sh\n", EXECUTABLE_MODE),
        regular("empty", b""),
        regular("binary.bin", &[0, 255, 254, 0, 1]),
    ]);
    let records = enabled(&root).capture(&selection).unwrap();
    assert_eq!(records.len(), 4);
    assert!(records.iter().all(|r| r.origin == ChangeOrigin::Untracked));
    let tool = records
        .iter()
        .find(|r| r.path.as_bytes() == b"src/deep/er/tool.sh")
        .unwrap();
    assert!(matches!(
        &tool.content,
        SuppliedContent::Regular { mode, bytes } if *mode == EXECUTABLE_MODE && bytes == b"#!/bin/sh\n"
    ));
    let snapshot = admit_captured(&selection, &base, &records).unwrap();
    snapshot.verify(&context()).unwrap();
    snapshot.verify_against(&context(), &selection).unwrap();
}

#[test]
fn non_utf8_and_maximum_length_names_are_preserved_exactly() {
    let root = SyntheticRoot::create();
    let odd: &[u8] = &[0xff, 0xfe, b'.', b't'];
    let long = "n".repeat(255);
    fs::write(root.path().join(std::ffi::OsStr::from_bytes(odd)), b"odd").unwrap();
    write(&root, &long, b"long");
    let (selection, _) = selection_for(vec![
        Want::Regular(odd, b"odd", REGULAR_MODE),
        regular(&long, b"long"),
    ]);
    let records = enabled(&root).capture(&selection).unwrap();
    assert_eq!(records.len(), 2);
}

#[test]
fn a_file_longer_than_the_selection_is_read_only_one_byte_past_it() {
    let root = SyntheticRoot::create();
    let big = File::create(root.at("big")).unwrap();
    big.set_len(64 * 1024 * 1024).unwrap(); // sparse, so it costs no disk
    let (selection, base) = selection_for(vec![regular("big", &[0u8; 10])]);
    let records = enabled(&root).capture(&selection).unwrap();
    assert_eq!(only_regular(&records).1.len(), 11);
    // Admission is what refuses it, as a size mismatch against the selection.
    assert_eq!(
        admit_captured(&selection, &base, &records)
            .unwrap_err()
            .code,
        B1ErrorCode::ContentSizeMismatch
    );
}

#[test]
fn selections_beyond_the_declared_limits_are_refused_before_any_read() {
    let root = SyntheticRoot::create();
    // The named file does not exist. A refusal on the limit proves nothing was opened.
    let mut value = serde_json::to_value(selection_for(vec![regular("f", b"x")]).0).unwrap();
    value["admitted_overlay"][0]["content"]["size"] = json!(MAX_INPUT_BYTES + 1);
    let selection: B1SourceSelectionV1 = serde_json::from_value(value).unwrap();
    assert_eq!(
        capture_code(&enabled(&root), &selection),
        B1ErrorCode::LimitExceeded
    );
}

// ---- Symlinks are inert --------------------------------------------------------------------

#[test]
fn symlinks_are_captured_as_target_bytes_and_never_followed() {
    let root = SyntheticRoot::create();
    write(&root, "real.txt", b"unselected target content");
    symlink("real.txt", root.at("link")).unwrap();
    symlink("does/not/exist", root.at("dangling")).unwrap();
    let (selection, base) = selection_for(vec![
        symlink_want("link", "real.txt"),
        symlink_want("dangling", "does/not/exist"),
    ]);
    let records = enabled(&root).capture(&selection).unwrap();
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .all(|r| matches!(r.content, SuppliedContent::Symlink { .. }))
    );
    let snapshot = admit_captured(&selection, &base, &records).unwrap();
    assert!(
        snapshot
            .entries()
            .iter()
            .all(|e| matches!(e.content(), SnapshotContent::Symlink { .. }))
    );
}

#[test]
fn a_symlink_to_a_canary_is_reported_as_a_symlink_and_the_canary_stays_unread() {
    let root = SyntheticRoot::create();
    let external = canary_root();
    let absolute = external.at("canary.txt");
    symlink(&absolute, root.at("looks_regular")).unwrap();
    // The owner believes the path is a regular file with the canary's bytes.
    let (selection, base) = selection_for(vec![regular("looks_regular", CANARY)]);
    let records = enabled(&root).capture(&selection).unwrap();
    assert!(!contains_canary(&records));
    assert!(matches!(
        &records[0].content,
        SuppliedContent::Symlink { target } if target.as_slice() == absolute.as_os_str().as_bytes()
    ));
    assert_eq!(
        admit_captured(&selection, &base, &records)
            .unwrap_err()
            .code,
        B1ErrorCode::SnapshotMismatch
    );
}

#[test]
fn a_symlink_whose_target_differs_from_the_selection_is_refused_by_admission() {
    let root = SyntheticRoot::create();
    symlink("elsewhere", root.at("link")).unwrap();
    let (selection, base) = selection_for(vec![symlink_want("link", "real.txt")]);
    let records = enabled(&root).capture(&selection).unwrap();
    assert_eq!(
        admit_captured(&selection, &base, &records)
            .unwrap_err()
            .code,
        B1ErrorCode::SnapshotMismatch
    );
}

// ---- Hardlinks to synthetic external canaries ------------------------------------------------

#[test]
fn a_selected_hardlink_to_an_external_canary_is_refused() {
    let root = SyntheticRoot::create();
    let external = canary_root();
    fs::hard_link(external.at("canary.txt"), root.at("alias.txt")).unwrap();
    let (selection, _) = selection_for(vec![regular("alias.txt", CANARY)]);
    assert_eq!(
        capture_code(&enabled(&root), &selection),
        B1ErrorCode::HardlinkOrAliasUnsafe
    );
}

#[test]
fn an_unselected_hardlink_never_enters_and_never_blocks_other_files() {
    let root = SyntheticRoot::create();
    let external = canary_root();
    fs::hard_link(external.at("canary.txt"), root.at("alias.txt")).unwrap();
    write(&root, "ok.txt", b"fine");
    let (selection, _) = selection_for(vec![regular("ok.txt", b"fine")]);
    let records = enabled(&root).capture(&selection).unwrap();
    assert_eq!(records.len(), 1);
    assert!(!contains_canary(&records));
}

#[test]
fn hardlinks_between_two_selected_paths_are_refused_too() {
    let root = SyntheticRoot::create();
    write(&root, "a.txt", b"same");
    fs::hard_link(root.at("a.txt"), root.at("b.txt")).unwrap();
    let (selection, _) = selection_for(vec![regular("a.txt", b"same"), regular("b.txt", b"same")]);
    assert_eq!(
        capture_code(&enabled(&root), &selection),
        B1ErrorCode::HardlinkOrAliasUnsafe
    );
}

#[test]
fn the_hardlink_rule_follows_the_link_count_and_not_the_name() {
    let root = SyntheticRoot::create();
    let external = canary_root();
    fs::hard_link(external.at("canary.txt"), root.at("alias.txt")).unwrap();
    let (selection, _) = selection_for(vec![regular("alias.txt", CANARY)]);
    assert!(enabled(&root).capture(&selection).is_err());
    fs::remove_file(external.at("canary.txt")).unwrap();
    // Only one link remains, so the inode is no longer aliased.
    let records = enabled(&root).capture(&selection).unwrap();
    assert_eq!(only_regular(&records).1, CANARY);
}

#[test]
fn a_hardlink_created_during_capture_is_detected() {
    let root = SyntheticRoot::create();
    write(&root, "f.txt", b"data");
    let external = canary_root();
    let (selection, _) = selection_for(vec![regular("f.txt", b"data")]);
    let capture = enabled(&root).with_hook(|stage, _| {
        if stage == Stage::AfterRead {
            fs::hard_link(root.at("f.txt"), external.at("alias")).unwrap();
        }
    });
    assert_eq!(
        capture_code(&capture, &selection),
        B1ErrorCode::CaptureChanged
    );
}

// ---- Directory and symlink replacement -----------------------------------------------------

#[test]
fn an_ancestor_directory_that_is_a_symlink_is_refused() {
    let root = SyntheticRoot::create();
    let external = canary_root();
    symlink(external.path(), root.at("dirlink")).unwrap();
    symlink(".", root.at("self")).unwrap();
    symlink("loop", root.at("loop")).unwrap();
    symlink("/", root.at("slash")).unwrap();
    for name in ["dirlink/canary.txt", "self/f", "loop/f", "slash/etc"] {
        let (selection, _) = selection_for(vec![regular(name, CANARY)]);
        assert_eq!(
            capture_code(&enabled(&root), &selection),
            B1ErrorCode::HardlinkOrAliasUnsafe,
            "{name}"
        );
    }
}

#[test]
fn a_directory_replaced_by_a_symlink_before_capture_cannot_redirect_it() {
    let root = SyntheticRoot::create();
    let external = canary_root();
    write(&root, "d/canary.txt", b"original");
    let (selection, _) = selection_for(vec![regular("d/canary.txt", b"original")]);
    fs::rename(root.at("d"), root.at("d.old")).unwrap();
    symlink(external.path(), root.at("d")).unwrap();
    assert_eq!(
        capture_code(&enabled(&root), &selection),
        B1ErrorCode::HardlinkOrAliasUnsafe
    );
}

#[test]
fn a_directory_replaced_by_a_symlink_during_capture_is_detected() {
    let root = SyntheticRoot::create();
    let external = canary_root();
    write(&root, "d/canary.txt", b"original");
    let (selection, _) = selection_for(vec![regular("d/canary.txt", b"original")]);
    let capture = enabled(&root).with_hook(|stage, _| {
        if stage == Stage::AfterClassify {
            fs::rename(root.at("d"), root.at("d.old")).unwrap();
            symlink(external.path(), root.at("d")).unwrap();
        }
    });
    assert_eq!(
        capture_code(&capture, &selection),
        B1ErrorCode::CaptureChanged
    );
}

#[test]
fn a_directory_replaced_by_a_lookalike_directory_during_capture_is_detected() {
    let root = SyntheticRoot::create();
    write(&root, "d/f.txt", b"same");
    let (selection, _) = selection_for(vec![regular("d/f.txt", b"same")]);
    let capture = enabled(&root).with_hook(|stage, _| {
        if stage == Stage::AfterRead {
            fs::rename(root.at("d"), root.at("d.old")).unwrap();
            write(&root, "d/f.txt", b"same");
        }
    });
    // Identical bytes at an identical path are still a different directory inode.
    assert_eq!(
        capture_code(&capture, &selection),
        B1ErrorCode::CaptureChanged
    );
}

#[test]
fn a_file_replaced_by_a_symlink_before_capture_is_reported_as_one() {
    let root = SyntheticRoot::create();
    let external = canary_root();
    write(&root, "f.txt", b"original");
    let (selection, base) = selection_for(vec![regular("f.txt", b"original")]);
    fs::remove_file(root.at("f.txt")).unwrap();
    symlink(external.at("canary.txt"), root.at("f.txt")).unwrap();
    let records = enabled(&root).capture(&selection).unwrap();
    assert!(!contains_canary(&records));
    assert_eq!(
        admit_captured(&selection, &base, &records)
            .unwrap_err()
            .code,
        B1ErrorCode::SnapshotMismatch
    );
}

#[test]
fn a_file_replaced_by_a_symlink_during_capture_is_detected() {
    for stage in [Stage::AfterClassify, Stage::AfterRead] {
        let root = SyntheticRoot::create();
        let external = canary_root();
        write(&root, "f.txt", b"original");
        let (selection, _) = selection_for(vec![regular("f.txt", b"original")]);
        let capture = enabled(&root).with_hook(|at, _| {
            if at == stage {
                fs::remove_file(root.at("f.txt")).unwrap();
                symlink(external.at("canary.txt"), root.at("f.txt")).unwrap();
            }
        });
        assert_eq!(
            capture_code(&capture, &selection),
            B1ErrorCode::CaptureChanged,
            "{stage:?}"
        );
    }
}

#[test]
fn a_symlink_replaced_during_capture_is_detected() {
    // A replacement with identical target bytes is deliberately absent. Inode numbers are reused
    // by delete and re-create, so it cannot be told apart by identity, and the bytes returned are
    // exactly what the path holds, so nothing observable is wrong.
    type Swap = fn(&SyntheticRoot, &SyntheticRoot);
    let swaps: [(&str, Swap); 2] = [
        ("new target", |root, _| {
            fs::remove_file(root.at("l")).unwrap();
            symlink("other", root.at("l")).unwrap();
        }),
        ("regular file", |root, _| {
            fs::remove_file(root.at("l")).unwrap();
            fs::write(root.at("l"), b"now a file").unwrap();
        }),
    ];
    for (label, swap) in swaps {
        let root = SyntheticRoot::create();
        let external = canary_root();
        symlink("t", root.at("l")).unwrap();
        let (selection, _) = selection_for(vec![symlink_want("l", "t")]);
        let capture = enabled(&root).with_hook(|stage, _| {
            if stage == Stage::AfterRead {
                swap(&root, &external);
            }
        });
        let outcome = capture.capture(&selection);
        eprintln!("symlink swap {label}: {outcome:?}");
        assert_eq!(
            outcome.unwrap_err().code,
            B1ErrorCode::CaptureChanged,
            "{label}"
        );
    }
}

// ---- FIFO, socket, device and other special files ------------------------------------------

#[test]
fn fifos_sockets_and_directories_are_rejected_without_being_opened() {
    let _guard = watchdog();
    let root = SyntheticRoot::create();
    mkfifo(&root.at("pipe"), Mode::S_IRWXU).unwrap();
    mknod(&root.at("sock"), SFlag::S_IFSOCK, Mode::S_IRWXU, 0).unwrap();
    fs::create_dir(root.at("dir")).unwrap();
    for name in ["pipe", "sock", "dir"] {
        let (selection, _) = selection_for(vec![regular(name, b"x")]);
        assert_eq!(
            capture_code(&enabled(&root), &selection),
            B1ErrorCode::InputKindUnsupported,
            "{name}"
        );
    }
    // The selection of one bad entry fails the whole capture, so nothing partial is returned.
    write(&root, "good", b"x");
    let (mixed, _) = selection_for(vec![regular("good", b"x"), regular("pipe", b"x")]);
    assert!(enabled(&root).capture(&mixed).is_err());
}

#[test]
fn device_and_unknown_node_types_are_classified_as_unsupported() {
    // Creating a device node needs privilege, which this fixture never uses. The classification
    // that the capture relies on is therefore checked against the mode bits directly.
    assert_eq!(node_kind(libc::S_IFCHR | 0o666), NodeKind::CharDevice);
    assert_eq!(node_kind(libc::S_IFBLK | 0o660), NodeKind::BlockDevice);
    assert_eq!(node_kind(libc::S_IFIFO | 0o600), NodeKind::Fifo);
    assert_eq!(node_kind(libc::S_IFSOCK | 0o600), NodeKind::Socket);
    assert_eq!(node_kind(libc::S_IFDIR | 0o700), NodeKind::Directory);
    assert_eq!(node_kind(libc::S_IFLNK | 0o777), NodeKind::Symlink);
    assert_eq!(node_kind(libc::S_IFREG | 0o644), NodeKind::Regular);
    assert_eq!(node_kind(0), NodeKind::Unknown);
}

#[test]
fn a_regular_file_swapped_for_a_fifo_after_classification_cannot_hang_the_capture() {
    let _guard = watchdog();
    let root = SyntheticRoot::create();
    write(&root, "f", b"data");
    let (selection, _) = selection_for(vec![regular("f", b"data")]);
    let capture = enabled(&root).with_hook(|stage, _| {
        if stage == Stage::AfterClassify {
            fs::remove_file(root.at("f")).unwrap();
            mkfifo(&root.at("f"), Mode::S_IRWXU).unwrap();
        }
    });
    assert_eq!(
        capture_code(&capture, &selection),
        B1ErrorCode::CaptureChanged
    );
}

#[test]
fn a_regular_file_swapped_for_a_directory_or_socket_is_detected() {
    for kind in ["directory", "socket"] {
        let root = SyntheticRoot::create();
        write(&root, "f", b"data");
        let (selection, _) = selection_for(vec![regular("f", b"data")]);
        let capture = enabled(&root).with_hook(|stage, _| {
            if stage == Stage::AfterClassify {
                fs::remove_file(root.at("f")).unwrap();
                match kind {
                    "directory" => fs::create_dir(root.at("f")).unwrap(),
                    _ => mknod(&root.at("f"), SFlag::S_IFSOCK, Mode::S_IRWXU, 0).unwrap(),
                }
            }
        });
        assert_eq!(
            capture_code(&capture, &selection),
            B1ErrorCode::CaptureChanged,
            "{kind}"
        );
    }
}

// ---- Files that change during capture --------------------------------------------------------

fn changes_during_capture(label: &str, change: fn(&SyntheticRoot)) {
    let root = SyntheticRoot::create();
    write(&root, "f.txt", b"0123456789");
    let (selection, _) = selection_for(vec![regular("f.txt", b"0123456789")]);
    let capture = enabled(&root).with_hook(|stage, _| {
        if stage == Stage::AfterRead {
            change(&root);
        }
    });
    assert_eq!(
        capture_code(&capture, &selection),
        B1ErrorCode::CaptureChanged,
        "{label}"
    );
}

#[test]
fn every_kind_of_mutation_during_capture_is_detected() {
    changes_during_capture("append", |root| {
        let file = fs::OpenOptions::new()
            .append(true)
            .open(root.at("f.txt"))
            .unwrap();
        file.write_at(b"more", 10).unwrap();
    });
    changes_during_capture("same-size overwrite", |root| {
        let file = fs::OpenOptions::new()
            .write(true)
            .open(root.at("f.txt"))
            .unwrap();
        file.write_at(b"ABCDEFGHIJ", 0).unwrap();
    });
    changes_during_capture("one-byte overwrite", |root| {
        let file = fs::OpenOptions::new()
            .write(true)
            .open(root.at("f.txt"))
            .unwrap();
        file.write_at(b"X", 9).unwrap();
    });
    changes_during_capture("truncate", |root| {
        File::options()
            .write(true)
            .open(root.at("f.txt"))
            .unwrap()
            .set_len(0)
            .unwrap();
    });
    changes_during_capture("replacement by rename", |root| {
        fs::write(root.at("new"), b"0123456789").unwrap();
        fs::rename(root.at("new"), root.at("f.txt")).unwrap();
    });
    changes_during_capture("deletion", |root| {
        fs::remove_file(root.at("f.txt")).unwrap();
    });
    changes_during_capture("mode change", |root| {
        fs::set_permissions(root.at("f.txt"), fs::Permissions::from_mode(0o755)).unwrap();
    });
}

#[test]
fn an_unchanging_capture_is_stable_and_repeatable() {
    let root = SyntheticRoot::create();
    write(&root, "f.txt", b"0123456789");
    let (selection, _) = selection_for(vec![regular("f.txt", b"0123456789")]);
    let seen = std::cell::Cell::new(0);
    let capture = enabled(&root).with_hook(|_, _| seen.set(seen.get() + 1));
    let first = capture.capture(&selection).unwrap();
    assert_eq!(first, capture.capture(&selection).unwrap());
    assert_eq!(seen.get(), 4, "two hooks per capture");
}

#[test]
fn a_file_that_changed_before_capture_is_caught_by_admission() {
    let root = SyntheticRoot::create();
    write(&root, "f.txt", b"owner reviewed this");
    let (selection, base) = selection_for(vec![regular("f.txt", b"owner reviewed this")]);
    write(&root, "f.txt", b"edited after the review!");
    let records = enabled(&root).capture(&selection).unwrap();
    assert!(admit_captured(&selection, &base, &records).is_err());
    // Same length, different bytes.
    write(&root, "f.txt", b"owner reviewed thiz");
    let records = enabled(&root).capture(&selection).unwrap();
    assert_eq!(
        admit_captured(&selection, &base, &records)
            .unwrap_err()
            .code,
        B1ErrorCode::ContentDigestMismatch
    );
}

// ---- Explicit selection ---------------------------------------------------------------------

#[test]
fn only_the_named_paths_are_read_and_the_rest_of_the_tree_is_never_walked() {
    let _guard = watchdog();
    let root = SyntheticRoot::create();
    let external = canary_root();
    write(&root, "selected.txt", b"yes");
    write(&root, "unselected.txt", b"no");
    write(&root, "dir/unselected.txt", b"no");
    mkfifo(&root.at("pipe"), Mode::S_IRWXU).unwrap();
    mknod(&root.at("sock"), SFlag::S_IFSOCK, Mode::S_IRWXU, 0).unwrap();
    symlink(external.path(), root.at("escape")).unwrap();
    fs::hard_link(external.at("canary.txt"), root.at("alias")).unwrap();
    let (selection, _) = selection_for(vec![regular("selected.txt", b"yes")]);
    let records = enabled(&root).capture(&selection).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].path.as_bytes(), b"selected.txt");
    assert!(!contains_canary(&records));
}

#[test]
fn deleted_paths_are_never_opened_even_when_something_hostile_sits_there() {
    let root = SyntheticRoot::create();
    mkfifo(&root.at("old.txt"), Mode::S_IRWXU).unwrap();
    write(&root, "new.txt", b"new");
    let (selection, base) =
        selection_for(vec![Want::Delete(b"old.txt"), regular("new.txt", b"new")]);
    let records = enabled(&root).capture(&selection).unwrap();
    assert_eq!(records.len(), 1);
    let snapshot = admit_captured(&selection, &base, &records).unwrap();
    assert_eq!(snapshot.entries().len(), 2);
}

#[test]
fn a_selected_path_that_does_not_exist_is_a_change_and_not_an_empty_file() {
    let root = SyntheticRoot::create();
    for name in ["missing.txt", "no/such/dir/f"] {
        let (selection, _) = selection_for(vec![regular(name, b"x")]);
        assert_eq!(
            capture_code(&enabled(&root), &selection),
            B1ErrorCode::CaptureChanged,
            "{name}"
        );
    }
    write(&root, "plain", b"x");
    let (selection, _) = selection_for(vec![regular("plain/child", b"x")]);
    assert_eq!(
        capture_code(&enabled(&root), &selection),
        B1ErrorCode::CaptureChanged
    );
}

#[test]
fn paths_that_could_leave_the_root_cannot_be_expressed_in_a_selection() {
    for bad in ["../x", "/abs", "a/../../x", "a//b", "./x", "a/./b", ""] {
        assert!(B1RelPath::from_bytes(bad.as_bytes()).is_err(), "{bad:?}");
    }
}

// ---- Independent owned content --------------------------------------------------------------

#[test]
fn captured_content_is_independent_of_the_filesystem_after_capture() {
    let root = SyntheticRoot::create();
    write(&root, "a.txt", b"alpha");
    write(&root, "b.txt", b"bravo");
    symlink("a.txt", root.at("l")).unwrap();
    let (selection, base) = selection_for(vec![
        regular("a.txt", b"alpha"),
        regular("b.txt", b"bravo"),
        symlink_want("l", "a.txt"),
    ]);
    let records = enabled(&root).capture(&selection).unwrap();
    let before = records.clone();
    let snapshot = admit_captured(&selection, &base, &records).unwrap();

    fs::write(root.at("a.txt"), b"ALPHA CHANGED").unwrap();
    fs::remove_file(root.at("b.txt")).unwrap();
    fs::remove_file(root.at("l")).unwrap();
    symlink("elsewhere", root.at("l")).unwrap();

    assert_eq!(records, before);
    snapshot.verify(&context()).unwrap();
    snapshot.verify_against(&context(), &selection).unwrap();
    drop(root);
    snapshot.verify_against(&context(), &selection).unwrap();
}

#[test]
fn captured_inodes_are_never_shared_with_the_snapshot() {
    let root = SyntheticRoot::create();
    write(&root, "a.txt", b"alpha");
    let (selection, base) = selection_for(vec![regular("a.txt", b"alpha")]);
    let records = enabled(&root).capture(&selection).unwrap();
    let snapshot = admit_captured(&selection, &base, &records).unwrap();
    // Writing through the original inode in place cannot reach the owned copy.
    let file = fs::OpenOptions::new()
        .write(true)
        .open(root.at("a.txt"))
        .unwrap();
    file.write_at(b"OMEGA", 0).unwrap();
    snapshot.verify_against(&context(), &selection).unwrap();
    match snapshot.entries()[0].content() {
        SnapshotContent::Regular { bytes, .. } => assert_eq!(bytes.as_slice(), b"alpha"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_tampered_snapshot_taken_from_a_capture_is_refused() {
    let root = SyntheticRoot::create();
    write(&root, "a.txt", b"alpha");
    let (selection, base) = selection_for(vec![regular("a.txt", b"alpha")]);
    let records = enabled(&root).capture(&selection).unwrap();
    let snapshot = admit_captured(&selection, &base, &records).unwrap();
    let mut forged = records.clone();
    forged[0].content = SuppliedContent::Regular {
        mode: REGULAR_MODE,
        bytes: b"alpHa".to_vec(),
    };
    assert_eq!(
        admit_captured(&selection, &base, &forged).unwrap_err().code,
        B1ErrorCode::ContentDigestMismatch
    );
    snapshot.verify_against(&context(), &selection).unwrap();
}

// ---- Descriptor-based path confinement ---------------------------------------------------------

#[test]
fn a_held_directory_descriptor_stays_bound_when_its_path_is_replaced() {
    let root = SyntheticRoot::create();
    let external = canary_root();
    write(&root, "d/f.txt", b"original");
    write(&external, "f.txt", b"decoy");
    let held = enabled(&root).open_directory(b"d", Phase::Initial).unwrap();

    fs::rename(root.at("d"), root.at("d.old")).unwrap();
    symlink(external.path(), root.at("d")).unwrap();

    // Path-based access now follows the symlink to the external root...
    assert_eq!(fs::read(root.at("d/f.txt")).unwrap(), b"decoy");
    // ...while the descriptor still names the original directory.
    let fd = openat2(
        held.as_raw_fd(),
        b"f.txt",
        OFlag::O_RDONLY | OFlag::O_NOFOLLOW,
    )
    .unwrap();
    assert_eq!(
        read_prefix(&File::from(fd), 100).unwrap(),
        b"original".to_vec()
    );
}

#[test]
fn confined_resolution_refuses_every_escape_spelling() {
    let root = SyntheticRoot::create();
    let external = canary_root();
    write(&root, "d/f", b"x");
    symlink(external.path(), root.at("d/out")).unwrap();
    symlink("f", root.at("d/inner")).unwrap();
    let absolute = external.at("canary.txt");
    let read = OFlag::O_RDONLY;
    let open = |name: &[u8], flags: OFlag| openat2(root.fd.as_raw_fd(), name, flags);

    // Lexical escapes are refused by RESOLVE_BENEATH.
    assert_eq!(open(b"../x", read).unwrap_err(), Errno::EXDEV);
    assert_eq!(open(b"d/../../x", read).unwrap_err(), Errno::EXDEV);
    assert_eq!(
        open(absolute.as_os_str().as_bytes(), read).unwrap_err(),
        Errno::EXDEV
    );
    // Symlinks are refused in any position, even when they stay inside the root.
    assert_eq!(open(b"d/out/canary.txt", read).unwrap_err(), Errno::ELOOP);
    assert_eq!(open(b"d/inner", read).unwrap_err(), Errno::ELOOP);
    assert_eq!(
        open(b"d/inner", read | OFlag::O_NOFOLLOW).unwrap_err(),
        Errno::ELOOP
    );
    // The same file, reached without a link, opens.
    assert!(open(b"d/f", read).is_ok());
}

// ---- Boundary logic pinned directly (found by mutation testing) ---------------------------------

#[test]
fn every_openat2_restriction_is_requested() {
    // Written as literals so that a change to the composition of the mask is visible. Mount
    // crossings (NO_XDEV) cannot be provoked without privilege, so this is the only check on it.
    assert_eq!(libc::RESOLVE_NO_XDEV, 0x01);
    assert_eq!(libc::RESOLVE_NO_MAGICLINKS, 0x02);
    assert_eq!(libc::RESOLVE_NO_SYMLINKS, 0x04);
    assert_eq!(libc::RESOLVE_BENEATH, 0x08);
    assert_eq!(CONFINED, 0x0f);
}

#[test]
fn open_errors_fail_closed_and_depend_on_the_phase() {
    for errno in [Errno::ENOSYS, Errno::EPERM] {
        assert_eq!(leaf_error(errno).code, B1ErrorCode::IsolationUnavailable);
        for phase in [Phase::Initial, Phase::Recheck] {
            assert_eq!(
                directory_error(errno, phase).code,
                B1ErrorCode::IsolationUnavailable
            );
        }
    }
    for errno in [Errno::ELOOP, Errno::EXDEV] {
        assert_eq!(
            directory_error(errno, Phase::Initial).code,
            B1ErrorCode::HardlinkOrAliasUnsafe
        );
        assert_eq!(
            directory_error(errno, Phase::Recheck).code,
            B1ErrorCode::CaptureChanged
        );
        assert_eq!(leaf_error(errno).code, B1ErrorCode::CaptureChanged);
    }
    for errno in [Errno::ENOENT, Errno::ENOTDIR, Errno::EACCES, Errno::ENXIO] {
        assert_eq!(leaf_error(errno).code, B1ErrorCode::CaptureChanged);
        for phase in [Phase::Initial, Phase::Recheck] {
            assert_eq!(
                directory_error(errno, phase).code,
                B1ErrorCode::CaptureChanged
            );
        }
    }
}

#[test]
fn identity_predicates_require_every_part_to_match() {
    let a = Identity {
        device: 1,
        inode: 1,
    };
    let b = Identity {
        device: 1,
        inode: 2,
    };
    let c = Identity {
        device: 2,
        inode: 1,
    };
    let regular = libc::S_IFREG | 0o644;
    assert!(is_classified_leaf(regular, a, a));
    assert!(!is_classified_leaf(regular, a, b));
    assert!(!is_classified_leaf(regular, a, c));
    for other in [libc::S_IFIFO, libc::S_IFLNK, libc::S_IFDIR, libc::S_IFSOCK] {
        assert!(!is_classified_leaf(other | 0o644, a, a));
    }
    assert!(same_places((a, a), (b, b)));
    assert!(!same_places((a, b), (b, b)));
    assert!(!same_places((a, a), (b, c)));
    assert!(!same_places((a, b), (b, c)));
}

#[test]
fn special_files_in_an_ancestor_position_are_never_opened() {
    // Opening a FIFO for reading without O_DIRECTORY would block forever.
    let _guard = watchdog();
    let root = SyntheticRoot::create();
    mkfifo(&root.at("pipe"), Mode::S_IRWXU).unwrap();
    mknod(&root.at("sock"), SFlag::S_IFSOCK, Mode::S_IRWXU, 0).unwrap();
    write(&root, "file", b"x");
    for name in ["pipe/child", "pipe/a/b", "sock/child", "file/child"] {
        let (selection, _) = selection_for(vec![regular(name, b"x")]);
        assert_eq!(
            capture_code(&enabled(&root), &selection),
            B1ErrorCode::CaptureChanged,
            "{name}"
        );
    }
}

fn sparse(root: &SyntheticRoot, name: &str, length: u64) {
    File::create(root.at(name))
        .unwrap()
        .set_len(length)
        .unwrap();
}

#[test]
fn reads_stop_exactly_one_byte_past_the_declared_size() {
    let root = SyntheticRoot::create();
    sparse(&root, "big", 1024 * 1024);
    // Sizes on both sides of the internal 8 KiB chunk, so an off-by-one in the loop shows.
    for declared in [0usize, 1, 8191, 8192, 8193, 20_000] {
        let expected = vec![0u8; declared];
        let (selection, base) = selection_for(vec![regular("big", &expected)]);
        let records = enabled(&root).capture(&selection).unwrap();
        assert_eq!(only_regular(&records).1.len(), declared + 1, "{declared}");
        assert_eq!(
            admit_captured(&selection, &base, &records)
                .unwrap_err()
                .code,
            B1ErrorCode::ContentSizeMismatch
        );
    }
}

#[test]
fn the_maximum_declared_size_is_captured_and_one_more_byte_is_refused_by_admission() {
    let root = SyntheticRoot::create();
    let full = vec![0u8; MAX_INPUT_BYTES as usize];
    sparse(&root, "exact", MAX_INPUT_BYTES);
    sparse(&root, "over", MAX_INPUT_BYTES + 100);
    let (exact, base) = selection_for(vec![regular("exact", &full)]);
    let records = enabled(&root).capture(&exact).unwrap();
    admit_captured(&exact, &base, &records).unwrap();
    let (over, base) = selection_for(vec![regular("over", &full)]);
    let records = enabled(&root).capture(&over).unwrap();
    assert_eq!(only_regular(&records).1.len() as u64, MAX_INPUT_BYTES + 1);
    // Admission refuses the per-input limit before it compares with the selection.
    assert_eq!(
        admit_captured(&over, &base, &records).unwrap_err().code,
        B1ErrorCode::LimitExceeded
    );
}

#[test]
fn a_regular_file_where_the_selection_declares_a_symlink_is_bounded_by_the_input_limit() {
    let root = SyntheticRoot::create();
    sparse(&root, "exact", MAX_INPUT_BYTES);
    sparse(&root, "over", MAX_INPUT_BYTES + 1);
    // The owner declared a symlink, so no declared size bounds the read. The global limit does.
    let (declared_exact, _) = selection_for(vec![symlink_want("exact", "t")]);
    let records = enabled(&root).capture(&declared_exact).unwrap();
    assert_eq!(only_regular(&records).1.len() as u64, MAX_INPUT_BYTES);
    let (declared_over, _) = selection_for(vec![symlink_want("over", "t")]);
    assert_eq!(
        capture_code(&enabled(&root), &declared_over),
        B1ErrorCode::LimitExceeded
    );
}
