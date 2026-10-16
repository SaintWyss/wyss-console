//! Admission tests over synthetic in-memory fixtures. No filesystem, Git or process is touched.
use super::{
    AdmittedSnapshot, BaseTree, SnapshotContent, SuppliedContent, SuppliedRecord, TaskContext,
    UnsupportedKind, admit, verify_snapshot,
};
use crate::contracts::b1::source::ChangeOrigin;
use crate::contracts::b1::{
    B1ErrorCode, B1Result, GitOid, MAX_INPUT_BYTES, Sha256Hex,
    digest::{self},
    path::{B1RelPath, base64url},
    records::B1AdmissionRecordV1,
    source::{B1SourceSelectionV1, EXECUTABLE_MODE, REGULAR_MODE},
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const TASK: &str = "11111111-1111-4111-8111-111111111111";
const OTHER_TASK: &str = "77777777-7777-4777-8777-777777777777";
const SELECTION: &str = "22222222-2222-4222-8222-222222222222";
const REPO: &str = "33333333-3333-4333-8333-333333333333";
const POLICY: &str = "44444444-4444-4444-8444-444444444444";
const BASE: &str = "0123456789abcdef0123456789abcdef01234567";
const BASE_B: &str = "fedcba9876543210fedcba9876543210fedcba98";
const SHA_POLICY: &str = "abababababababababababababababababababababababababababababababab";
const SHA_REVIEW: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";
const NOW: &str = "2026-10-16T10:00:00Z";

const README_NEW: &[u8] = b"# Wyss Console\n";
const NEW_DOC: &[u8] = b"new\n";
const APP: &[u8] = b"pub fn app() {}\n";
const LINK_TARGET: &[u8] = b"../README.md";

fn typed<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

fn path(text: &str) -> B1RelPath {
    B1RelPath::from_bytes(text.as_bytes()).unwrap()
}

fn oid(text: &str) -> GitOid {
    typed(json!(text))
}

fn sha(bytes: &[u8]) -> Sha256Hex {
    digest::raw_sha256(bytes)
}

fn content_json(bytes: &[u8], mode: u32) -> Value {
    json!({
        "kind": "regular", "content_sha256": sha(bytes).as_str(),
        "size": bytes.len(), "mode": mode
    })
}

fn overlay_entry(
    text: &str,
    operation: &str,
    origin: &str,
    base: Option<&str>,
    from: Option<&str>,
    content: Option<Value>,
) -> Value {
    json!({
        "path": base64url::encode(text.as_bytes()), "operation": operation,
        "origin": origin, "expected_base_oid": base,
        "rename_from": from.map(|text| base64url::encode(text.as_bytes())),
        "content": content, "reviewed_sha256": SHA_REVIEW
    })
}

fn selection_json(overlay: Vec<Value>) -> Value {
    json!({
        "schema_version": 1, "task_id": TASK, "selection_id": SELECTION,
        "repo_id": REPO, "base_commit_oid": BASE, "repo_object_format": "sha1",
        "admitted_overlay": overlay, "policy_id": POLICY,
        "policy_sha256": SHA_POLICY, "issued_at": NOW
    })
}

fn record(
    context: &TaskContext,
    text: &str,
    origin: ChangeOrigin,
    bytes: &[u8],
    mode: u32,
) -> SuppliedRecord {
    let _ = context;
    SuppliedRecord {
        path: path(text),
        origin,
        content: SuppliedContent::Regular {
            mode,
            bytes: bytes.to_vec(),
        },
    }
}

struct Fixture {
    context: TaskContext,
    selection: B1SourceSelectionV1,
    base: BaseTree,
    supplied: Vec<SuppliedRecord>,
}

/// Covers every supported kind: a modify, a symlink, an untracked add, a delete, and a staged
/// rename paired with the delete of its source. Entries are sorted by raw path bytes.
fn fixture() -> Fixture {
    let overlay = vec![
        overlay_entry(
            "README.md",
            "modify",
            "unstaged",
            Some(BASE),
            None,
            Some(content_json(README_NEW, REGULAR_MODE)),
        ),
        overlay_entry(
            "docs/link",
            "add",
            "untracked",
            None,
            None,
            Some(json!({"kind": "symlink", "target": base64url::encode(LINK_TARGET)})),
        ),
        overlay_entry(
            "docs/new.md",
            "add",
            "untracked",
            None,
            None,
            Some(content_json(NEW_DOC, REGULAR_MODE)),
        ),
        overlay_entry("old.txt", "delete", "unstaged", Some(BASE), None, None),
        overlay_entry(
            "src/app.rs",
            "rename",
            "staged",
            Some(BASE_B),
            Some("src/lib.rs"),
            Some(content_json(APP, REGULAR_MODE)),
        ),
        overlay_entry("src/lib.rs", "delete", "staged", Some(BASE_B), None, None),
    ];
    let selection: B1SourceSelectionV1 = typed(selection_json(overlay));
    selection.validate().unwrap();
    let base = BaseTree::new(
        oid(BASE),
        vec![
            (path("README.md"), oid(BASE)),
            (path("old.txt"), oid(BASE)),
            (path("src/lib.rs"), oid(BASE_B)),
        ],
    )
    .unwrap();
    let context = TaskContext::new(typed(json!(TASK)), 1).unwrap();
    let supplied = vec![
        SuppliedRecord {
            path: path("README.md"),
            origin: ChangeOrigin::Unstaged,
            content: SuppliedContent::Regular {
                mode: REGULAR_MODE,
                bytes: README_NEW.to_vec(),
            },
        },
        SuppliedRecord {
            path: path("docs/link"),
            origin: ChangeOrigin::Untracked,
            content: SuppliedContent::Symlink {
                target: LINK_TARGET.to_vec(),
            },
        },
        record(
            &context,
            "docs/new.md",
            ChangeOrigin::Untracked,
            NEW_DOC,
            REGULAR_MODE,
        ),
        record(
            &context,
            "src/app.rs",
            ChangeOrigin::Staged,
            APP,
            REGULAR_MODE,
        ),
    ];
    Fixture {
        context,
        selection,
        base,
        supplied,
    }
}

fn run(fixture: &Fixture) -> B1Result<AdmittedSnapshot> {
    admit(
        &fixture.context,
        &fixture.selection,
        &fixture.base,
        &fixture.supplied,
        1,
    )
}

fn code(fixture: &Fixture) -> B1ErrorCode {
    run(fixture).unwrap_err().code
}

#[test]
fn admits_the_exact_selection_with_computed_evidence() {
    let fixture = fixture();
    let snapshot = run(&fixture).unwrap();
    assert_eq!(snapshot.entries().len(), 6);
    let expected_bytes = README_NEW.len() + LINK_TARGET.len() + NEW_DOC.len() + APP.len();
    assert_eq!(snapshot.record().admitted_bytes, expected_bytes as u64);
    assert_eq!(snapshot.record().admitted_entries, 6);
    verify_snapshot(&fixture.context, snapshot.record(), snapshot.entries()).unwrap();
    // Identical inputs produce identical evidence, so the record is reproducible.
    assert_eq!(run(&fixture).unwrap().record(), snapshot.record());
}

#[test]
fn origin_is_bound_into_the_approved_overlay_digest() {
    let staged_fixture = fixture();
    let mut unstaged = fixture();
    // Owner approves README as staged instead of unstaged. Supplied bytes are unchanged.
    let mut value = serde_json::to_value(&unstaged.selection).unwrap();
    value["admitted_overlay"][0]["origin"] = json!("staged");
    unstaged.selection = typed(value);
    unstaged.supplied[0].origin = ChangeOrigin::Staged;
    let a = run(&staged_fixture).unwrap();
    let b = run(&unstaged).unwrap();
    assert_ne!(
        a.record().approved_overlay_sha256,
        b.record().approved_overlay_sha256
    );
}

#[test]
fn snapshot_owns_its_bytes_and_later_input_changes_cannot_reach_it() {
    let mut fixture = fixture();
    let snapshot = run(&fixture).unwrap();
    let before = snapshot.clone();
    fixture.supplied[0].content = SuppliedContent::Regular {
        mode: REGULAR_MODE,
        bytes: b"tampered after admission".to_vec(),
    };
    assert_eq!(snapshot, before);
    match snapshot.entries()[0].content() {
        SnapshotContent::Regular { bytes, .. } => assert_eq!(bytes.as_slice(), README_NEW),
        other => panic!("README must stay regular, got {other:?}"),
    }
}

#[test]
fn content_changed_after_selection_is_refused_by_size_and_digest() {
    let mut same_length = fixture();
    same_length.supplied[0].content = SuppliedContent::Regular {
        mode: REGULAR_MODE,
        bytes: b"# Wyss Consolx\n".to_vec(),
    };
    assert_eq!(code(&same_length), B1ErrorCode::ContentDigestMismatch);

    let mut longer = fixture();
    longer.supplied[0].content = SuppliedContent::Regular {
        mode: REGULAR_MODE,
        bytes: b"# Wyss Console\nx".to_vec(),
    };
    assert_eq!(code(&longer), B1ErrorCode::ContentSizeMismatch);

    let mut shorter = fixture();
    shorter.supplied[0].content = SuppliedContent::Regular {
        mode: REGULAR_MODE,
        bytes: b"# Wyss".to_vec(),
    };
    assert_eq!(code(&shorter), B1ErrorCode::ContentSizeMismatch);
}

#[test]
fn a_well_formed_digest_that_does_not_match_the_bytes_is_refused() {
    // The selection declares a valid SHA-256, but of other bytes. The digest is never trusted
    // as proof of the bytes it accompanies.
    let overlay = vec![overlay_entry(
        "docs/new.md",
        "add",
        "untracked",
        None,
        None,
        Some(json!({
            "kind": "regular", "content_sha256": sha(b"other bytes").as_str(),
            "size": NEW_DOC.len(), "mode": REGULAR_MODE
        })),
    )];
    let mut fixture = fixture();
    fixture.selection = typed(selection_json(overlay));
    fixture.base = BaseTree::new(oid(BASE), vec![]).unwrap();
    fixture.supplied = vec![record(
        &fixture.context,
        "docs/new.md",
        ChangeOrigin::Untracked,
        NEW_DOC,
        REGULAR_MODE,
    )];
    assert_eq!(code(&fixture), B1ErrorCode::ContentDigestMismatch);
}

#[test]
fn unselected_files_are_never_silently_included() {
    let mut untracked = fixture();
    untracked.supplied.push(record(
        &untracked.context,
        "secret/.env",
        ChangeOrigin::Untracked,
        b"TOKEN=synthetic\n",
        REGULAR_MODE,
    ));
    assert_eq!(code(&untracked), B1ErrorCode::UnselectedContent);

    let mut tracked = fixture();
    tracked.supplied.push(record(
        &tracked.context,
        "src/other.rs",
        ChangeOrigin::Unstaged,
        b"fn other() {}\n",
        REGULAR_MODE,
    ));
    assert_eq!(code(&tracked), B1ErrorCode::UnselectedContent);
}

#[test]
fn staged_and_unstaged_versions_are_never_merged_or_substituted() {
    let mut wrong_origin = fixture();
    wrong_origin.supplied[0].origin = ChangeOrigin::Staged;
    assert_eq!(code(&wrong_origin), B1ErrorCode::SnapshotMismatch);

    let mut both = fixture();
    both.supplied.push(record(
        &both.context,
        "README.md",
        ChangeOrigin::Staged,
        b"# staged\n",
        REGULAR_MODE,
    ));
    assert_eq!(code(&both), B1ErrorCode::SourceCollision);
}

#[test]
fn missing_selected_content_is_refused() {
    let mut fixture = fixture();
    fixture.supplied.remove(0);
    assert_eq!(code(&fixture), B1ErrorCode::SnapshotMismatch);
}

#[test]
fn supplied_content_for_a_deleted_path_is_a_collision() {
    let mut fixture = fixture();
    fixture.supplied.push(record(
        &fixture.context,
        "old.txt",
        ChangeOrigin::Unstaged,
        b"resurrected\n",
        REGULAR_MODE,
    ));
    assert_eq!(code(&fixture), B1ErrorCode::SourceCollision);
}

#[test]
fn special_files_and_gitlinks_are_refused_even_when_unselected() {
    for kind in [
        UnsupportedKind::Fifo,
        UnsupportedKind::Socket,
        UnsupportedKind::Device,
        UnsupportedKind::Gitlink,
    ] {
        let mut fixture = fixture();
        fixture.supplied.push(SuppliedRecord {
            path: path("pipe"),
            origin: ChangeOrigin::Untracked,
            content: SuppliedContent::Unsupported(kind),
        });
        assert_eq!(
            code(&fixture),
            B1ErrorCode::InputKindUnsupported,
            "{kind:?}"
        );
    }
}

#[test]
fn file_mode_must_match_the_selection() {
    let mut fixture = fixture();
    fixture.supplied[0].content = SuppliedContent::Regular {
        mode: EXECUTABLE_MODE,
        bytes: README_NEW.to_vec(),
    };
    assert_eq!(code(&fixture), B1ErrorCode::SnapshotMismatch);
}

#[test]
fn symlink_target_and_kind_must_match_the_selection() {
    let mut other_target = fixture();
    other_target.supplied[1].content = SuppliedContent::Symlink {
        target: b"../OTHER.md".to_vec(),
    };
    assert_eq!(code(&other_target), B1ErrorCode::SnapshotMismatch);

    let mut wrong_kind = fixture();
    wrong_kind.supplied[1].content = SuppliedContent::Regular {
        mode: REGULAR_MODE,
        bytes: LINK_TARGET.to_vec(),
    };
    assert_eq!(code(&wrong_kind), B1ErrorCode::SnapshotMismatch);
}

#[test]
fn base_identity_and_expectations_are_checked() {
    let mut other_base = fixture();
    other_base.base = BaseTree::new(oid(BASE_B), vec![]).unwrap();
    assert_eq!(code(&other_base), B1ErrorCode::BaseOidInvalid);

    let mut moved_path = fixture();
    moved_path.base = BaseTree::new(
        oid(BASE),
        vec![
            (path("README.md"), oid(BASE_B)),
            (path("old.txt"), oid(BASE)),
            (path("src/lib.rs"), oid(BASE_B)),
        ],
    )
    .unwrap();
    assert_eq!(code(&moved_path), B1ErrorCode::SnapshotMismatch);

    let mut add_over_existing = fixture();
    add_over_existing.base = BaseTree::new(
        oid(BASE),
        vec![
            (path("README.md"), oid(BASE)),
            (path("docs/new.md"), oid(BASE)),
            (path("old.txt"), oid(BASE)),
            (path("src/lib.rs"), oid(BASE_B)),
        ],
    )
    .unwrap();
    assert_eq!(code(&add_over_existing), B1ErrorCode::SnapshotMismatch);

    let sha256_oid: GitOid = typed(json!("ab".repeat(32)));
    assert_eq!(
        BaseTree::new(oid(BASE), vec![(path("x"), sha256_oid)])
            .unwrap_err()
            .code,
        B1ErrorCode::OidInvalid
    );
}

#[test]
fn wrong_task_and_stale_or_invalid_generations_are_refused() {
    let fixture_value = fixture();
    let other = TaskContext::new(typed(json!(OTHER_TASK)), 1).unwrap();
    assert_eq!(
        admit(
            &other,
            &fixture_value.selection,
            &fixture_value.base,
            &fixture_value.supplied,
            1
        )
        .unwrap_err()
        .code,
        B1ErrorCode::AdmissionUnauthorized
    );

    let snapshot = run(&fixture_value).unwrap();
    let newer = TaskContext::new(typed(json!(TASK)), 2).unwrap();
    assert_eq!(
        verify_snapshot(&newer, snapshot.record(), snapshot.entries())
            .unwrap_err()
            .code,
        B1ErrorCode::StaleGeneration
    );
    assert_eq!(
        TaskContext::new(typed(json!(TASK)), 0).unwrap_err().code,
        B1ErrorCode::CounterInvalid
    );
    assert_eq!(
        TaskContext::new(typed(json!(TASK)), u64::MAX)
            .unwrap_err()
            .code,
        B1ErrorCode::CounterInvalid
    );
}

#[test]
fn snapshot_identity_mismatches_are_refused() {
    let fixture_a = fixture();
    let snapshot_a = run(&fixture_a).unwrap();

    // A different snapshot of the same size must not verify against A's record.
    let mut fixture_b = fixture();
    fixture_b.supplied[0].content = SuppliedContent::Regular {
        mode: REGULAR_MODE,
        bytes: b"# Wyss Consolx\n".to_vec(),
    };
    fixture_b.selection = typed(selection_json(vec![overlay_entry(
        "README.md",
        "modify",
        "unstaged",
        Some(BASE),
        None,
        Some(json!({
            "kind": "regular", "content_sha256": sha(b"# Wyss Consolx\n").as_str(),
            "size": 15, "mode": REGULAR_MODE
        })),
    )]));
    fixture_b.base = BaseTree::new(oid(BASE), vec![(path("README.md"), oid(BASE))]).unwrap();
    fixture_b.supplied = vec![fixture_b.supplied[0].clone()];
    let snapshot_b = run(&fixture_b).unwrap();
    assert_eq!(
        verify_snapshot(
            &fixture_a.context,
            snapshot_a.record(),
            snapshot_b.entries()
        )
        .unwrap_err()
        .code,
        B1ErrorCode::SnapshotMismatch
    );

    // A fabricated tree digest, with consistent totals, is refused.
    let mut forged: B1AdmissionRecordV1 = snapshot_a.record().clone();
    forged.admitted_tree_sha256 = sha(b"forged");
    assert_eq!(
        verify_snapshot(&fixture_a.context, &forged, snapshot_a.entries())
            .unwrap_err()
            .code,
        B1ErrorCode::SnapshotMismatch
    );

    // A byte total that disagrees with the entries is refused.
    let mut inflated = snapshot_a.record().clone();
    inflated.admitted_bytes += 1;
    assert_eq!(
        verify_snapshot(&fixture_a.context, &inflated, snapshot_a.entries())
            .unwrap_err()
            .code,
        B1ErrorCode::SnapshotMismatch
    );

    // An overflowing or over-budget total is refused as a limit, not wrapped.
    let mut overflow = snapshot_a.record().clone();
    overflow.admitted_bytes = u64::MAX;
    assert_eq!(
        verify_snapshot(&fixture_a.context, &overflow, snapshot_a.entries())
            .unwrap_err()
            .code,
        B1ErrorCode::LimitExceeded
    );
}

#[test]
fn per_input_size_limit_is_enforced_before_hashing() {
    let mut fixture = fixture();
    fixture.supplied[0].content = SuppliedContent::Regular {
        mode: REGULAR_MODE,
        bytes: vec![0u8; MAX_INPUT_BYTES as usize + 1],
    };
    assert_eq!(code(&fixture), B1ErrorCode::LimitExceeded);
}

#[test]
fn admission_record_round_trips_and_rejects_unknown_fields() {
    let snapshot = run(&fixture()).unwrap();
    let text = serde_json::to_value(snapshot.record()).unwrap();
    let parsed: B1AdmissionRecordV1 = typed(text.clone());
    assert_eq!(&parsed, snapshot.record());
    let mut injected = text;
    injected["host_path"] = json!("/home/user/project");
    assert!(serde_json::from_value::<B1AdmissionRecordV1>(injected).is_err());
}
