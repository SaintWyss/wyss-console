use super::*;

fn change(path: &str, kind: FileChangeKind, to: Option<&str>, diff: &str) -> FileChange {
    FileChange {
        path: path.into(),
        kind,
        move_path: to.map(str::to_owned),
        diff: diff.into(),
    }
}
fn command(item: &str, text: &str, cwd: &str) -> String {
    CommandAction::new(item, text, cwd, vec![])
        .unwrap()
        .fingerprint()
}
fn files(item: &str, changes: Vec<FileChange>) -> String {
    FileAction::new(item, changes).unwrap().fingerprint()
}
fn pair() -> Vec<FileChange> {
    vec![
        change("/work/a.txt", FileChangeKind::Add, None, "+a\n"),
        change("/work/b.txt", FileChangeKind::Move, Some("/work/c.txt"), ""),
    ]
}
// Vectors computed outside this crate (Python hashlib) from the documented encoding.
#[test]
fn command_fingerprint_matches_an_independent_vector_and_binds_every_field() {
    let base = command("call_1", "printf hello", "/work/space");
    assert_eq!(
        base,
        "31ead4097ca564f72a66b99c83f4025fce9ccafb54cc78abef9da3db23ca39e9"
    );
    assert!(valid_fingerprint(&base));
    assert_eq!(base, command("call_1", "printf hello", "/work//space/./"));
    for other in [
        command("call_2", "printf hello", "/work/space"),
        command("call_1", "printf hello ", "/work/space"),
        command("call_1", "printf hello", "/work/space2"),
    ] {
        assert_ne!(base, other);
    }
    // Presentation hints are derived by the provider and are not part of the action.
    assert_eq!(
        base,
        CommandAction::new("call_1", "printf hello", "/work/space", vec!["read".into()])
            .unwrap()
            .fingerprint()
    );
}
#[test]
fn file_fingerprint_matches_an_independent_vector_is_order_free_and_binds_every_field() {
    let base = files("call_2", pair());
    assert_eq!(
        base,
        "3e19a6cb0bf21eef05ab2c3802ca975370a96721dc574e8d39b206398d17f46b"
    );
    let mut reversed = pair();
    reversed.reverse();
    assert_eq!(base, files("call_2", reversed));
    let mutate = |f: &dyn Fn(&mut Vec<FileChange>)| {
        let mut c = pair();
        f(&mut c);
        files("call_2", c)
    };
    for other in [
        files("call_3", pair()),
        mutate(&|c| c[0].path = "/work/a2.txt".into()),
        mutate(&|c| c[0].kind = FileChangeKind::Update),
        mutate(&|c| c[0].diff = "+b\n".into()),
        mutate(&|c| c[1].move_path = Some("/work/d.txt".into())),
        mutate(&|c| c[1].diff = "-x\n".into()),
        mutate(&|c| {
            c.pop();
        }),
        mutate(&|c| c.push(change("/work/z", FileChangeKind::Delete, None, ""))),
    ] {
        assert_ne!(base, other);
    }
    // A command and a file change never collide, whatever their content.
    assert_ne!(base, command("call_2", "+a\n", "/work/a.txt"));
}
#[test]
fn field_boundaries_are_unambiguous() {
    assert_ne!(command("call_1", "bc", "/a"), command("call_1", "c", "/ab"));
    assert_ne!(
        files(
            "call_1",
            vec![change("/a", FileChangeKind::Add, None, "bc")]
        ),
        files(
            "call_1",
            vec![change("/ab", FileChangeKind::Add, None, "c")]
        )
    );
}
#[test]
fn paths_normalize_lexically_and_refuse_relative_parent_and_control_forms() {
    for (raw, normal) in [
        ("/", "/"),
        ("//", "/"),
        ("/a/./b//c/", "/a/b/c"),
        ("/a/...", "/a/..."),
    ] {
        assert_eq!(normal_path(raw, 64).unwrap(), normal);
    }
    for raw in ["", "a/b", "./a", "/a/../b", "/..", "/a\n", "/a\0b"] {
        assert!(normal_path(raw, 64).is_err(), "{raw:?}");
    }
    assert!(normal_path(&format!("/{}", "a".repeat(63)), 64).is_ok());
    assert!(normal_path(&format!("/{}", "a".repeat(64)), 64).is_err());
}
#[test]
fn command_bounds_are_exact() {
    let ok = |text: &str, cwd: &str, parsed: Vec<String>| {
        CommandAction::new("call_1", text, cwd, parsed).is_ok()
    };
    assert!(ok(&"x".repeat(MAX_COMMAND_BYTES), "/w", vec![]));
    assert!(!ok(&"x".repeat(MAX_COMMAND_BYTES + 1), "/w", vec![]));
    assert!(!ok("", "/w", vec![]));
    assert!(!ok("a\0b", "/w", vec![]));
    assert!(ok(
        "x",
        &format!("/{}", "c".repeat(MAX_CWD_BYTES - 1)),
        vec![]
    ));
    assert!(!ok("x", &format!("/{}", "c".repeat(MAX_CWD_BYTES)), vec![]));
    assert!(!ok("x", "relative", vec![]));
    assert!(ok(
        "x",
        "/w",
        vec!["a".repeat(MAX_PARSED_ACTION_BYTES); MAX_PARSED_ACTIONS]
    ));
    assert!(!ok("x", "/w", vec![String::new(); MAX_PARSED_ACTIONS + 1]));
    assert!(!ok(
        "x",
        "/w",
        vec!["a".repeat(MAX_PARSED_ACTION_BYTES + 1)]
    ));
    assert!(CommandAction::new("", "x", "/w", vec![]).is_err());
    assert!(CommandAction::new("bad item", "x", "/w", vec![]).is_err());
    let a = CommandAction::new("call_1", "printf hi", "/w/", vec!["p".into()]).unwrap();
    assert_eq!(
        a.structure(),
        ApprovalStructure {
            command_bytes: 9,
            cwd_bytes: 2,
            parsed_actions: 1,
            files: 0,
            diff_bytes: 0
        }
    );
    a.structure()
        .validate(ApprovalKind::CommandExecution)
        .unwrap();
    assert!(a.structure().validate(ApprovalKind::FileChange).is_err());
    assert_eq!(a.cwd(), "/w");
    assert_eq!(a.provider_item_id(), "call_1");
    assert_eq!(a.presentation().command, "printf hi");
}
#[test]
fn file_bounds_are_exact_and_inconsistent_proposals_are_refused() {
    let many = |n: usize| -> Vec<FileChange> {
        (0..n)
            .map(|i| change(&format!("/w/{i}"), FileChangeKind::Add, None, ""))
            .collect()
    };
    assert!(FileAction::new("call_1", many(MAX_FILES)).is_ok());
    assert!(FileAction::new("call_1", many(MAX_FILES + 1)).is_err());
    assert!(FileAction::new("call_1", vec![]).is_err());
    let one = |diff: String| vec![change("/w/a", FileChangeKind::Update, None, &diff)];
    assert!(FileAction::new("call_1", one("d".repeat(MAX_FILE_DIFF_BYTES))).is_ok());
    assert!(FileAction::new("call_1", one("d".repeat(MAX_FILE_DIFF_BYTES + 1))).is_err());
    let total = |last: usize| {
        vec![
            change(
                "/w/a",
                FileChangeKind::Add,
                None,
                &"d".repeat(MAX_FILE_DIFF_BYTES),
            ),
            change("/w/b", FileChangeKind::Add, None, &"d".repeat(last)),
        ]
    };
    assert!(FileAction::new("call_1", total(MAX_TOTAL_DIFF_BYTES - MAX_FILE_DIFF_BYTES)).is_ok());
    let mut over = total(MAX_TOTAL_DIFF_BYTES - MAX_FILE_DIFF_BYTES);
    over.push(change("/w/c", FileChangeKind::Add, None, "d"));
    assert!(FileAction::new("call_1", over).is_err());
    for bad in [
        vec![change("w/a", FileChangeKind::Add, None, "")],
        vec![change("/w/../etc/passwd", FileChangeKind::Add, None, "")],
        vec![change("/w/a", FileChangeKind::Add, Some("/w/b"), "")],
        vec![change("/w/a", FileChangeKind::Move, None, "")],
        vec![change("/w/a", FileChangeKind::Move, Some("/w/./a"), "")],
        vec![change("/w/a", FileChangeKind::Move, Some("../b"), "")],
        vec![change("/w/a", FileChangeKind::Add, None, "x\0y")],
        vec![
            change("/w/a", FileChangeKind::Add, None, ""),
            change("/w//a", FileChangeKind::Delete, None, ""),
        ],
        vec![
            change("/w/a", FileChangeKind::Add, None, ""),
            change("/w/b", FileChangeKind::Move, Some("/w/a"), ""),
        ],
    ] {
        assert!(FileAction::new("call_1", bad).is_err());
    }
    let a = FileAction::new("call_2", pair()).unwrap();
    assert_eq!(
        a.structure(),
        ApprovalStructure {
            command_bytes: 0,
            cwd_bytes: 0,
            parsed_actions: 0,
            files: 2,
            diff_bytes: 3
        }
    );
    a.structure().validate(ApprovalKind::FileChange).unwrap();
    assert!(
        a.structure()
            .validate(ApprovalKind::CommandExecution)
            .is_err()
    );
    assert_eq!(a.changes()[0].path, "/work/a.txt");
    assert_eq!(a.provider_item_id(), "call_2");
}

#[test]
fn confinement_refuses_another_filesystem_and_a_workspace_reached_through_a_link() {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    // A mount point inside the workspace is another filesystem, wherever it leads. The
    // root directory with the process filesystem mounted in it is one on every Linux host.
    let root = fs::metadata("/").unwrap();
    let mounted = fs::metadata("/proc").unwrap();
    assert!(mounted.is_dir() && mounted.dev() != root.dev());
    assert!(confine("/", "/", Leaf::Directory));
    assert!(!confine("/", "/proc", Leaf::Directory));
    assert!(!confine("/", "/proc/self", Leaf::Directory));
    assert!(!confine("/", "/proc/version", Leaf::ExistingFile));
    assert!(!confine("/", "/proc/wyss-new-file", Leaf::FileOrMissing));

    let base = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("wyss-confine-{}", uuid::Uuid::new_v4()));
    fs::DirBuilder::new().mode(0o700).create(&base).unwrap();
    let text = |p: &std::path::Path| p.to_str().unwrap().to_owned();
    let workspace = base.join("real/ws");
    fs::create_dir_all(workspace.join("src")).unwrap();
    fs::write(workspace.join("src/a.txt"), "a").unwrap();
    let ws = text(&workspace);
    let file = text(&workspace.join("src/a.txt"));
    assert!(confine(&ws, &ws, Leaf::Directory));
    assert!(confine(&ws, &file, Leaf::ExistingFile));
    // The workspace keeps its name, but an ancestor is now a link to another tree that
    // holds a directory of the same name: every component below still looks real.
    let elsewhere = base.join("elsewhere");
    fs::create_dir_all(elsewhere.join("ws/src")).unwrap();
    fs::write(elsewhere.join("ws/src/a.txt"), "other").unwrap();
    fs::rename(base.join("real"), base.join("moved")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, base.join("real")).unwrap();
    assert!(fs::symlink_metadata(&workspace).unwrap().is_dir());
    assert!(!confine(&ws, &ws, Leaf::Directory));
    assert!(!confine(&ws, &file, Leaf::ExistingFile));
    assert!(!confine(
        &ws,
        &text(&workspace.join("new.txt")),
        Leaf::FileOrMissing
    ));
    // The workspace itself replaced by a link, and a workspace that is gone.
    fs::remove_file(base.join("real")).unwrap();
    fs::rename(base.join("moved"), base.join("real")).unwrap();
    assert!(confine(&ws, &file, Leaf::ExistingFile));
    fs::rename(&workspace, base.join("real/ws-moved")).unwrap();
    assert!(!confine(&ws, &file, Leaf::ExistingFile));
    std::os::unix::fs::symlink(base.join("real/ws-moved"), &workspace).unwrap();
    assert!(!confine(&ws, &ws, Leaf::Directory));
    assert!(!confine(&ws, &file, Leaf::ExistingFile));
    fs::remove_dir_all(&base).unwrap();
}
