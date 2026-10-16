//! Adversarial contract tests. Each rejection asserts its stable code, so a test cannot pass
//! by failing for an unrelated reason.
use super::{
    B1ErrorCode, MAX_ENTRIES, MAX_INPUT_BYTES, MAX_PATH_BYTES,
    lifecycle::{B1PendingEvidence, B1TaskLifecycle, B1TaskState},
    path::{B1LinkTarget, B1RelPath, base64url, validate_destination_set},
    records::{
        B1AdmissionRecordV1, B1ExportRecordV1, B1FailureV1, B1QuarantineReceiptV1,
        B1WorkspaceReceiptV1, RiskFlag, classify_entry, quarantine_ref,
    },
    source::{B1SourceSelectionV1, EXECUTABLE_MODE, REGULAR_MODE},
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const TASK: &str = "11111111-1111-4111-8111-111111111111";
const SELECTION: &str = "22222222-2222-4222-8222-222222222222";
const REPO: &str = "33333333-3333-4333-8333-333333333333";
const POLICY: &str = "44444444-4444-4444-8444-444444444444";
const FS: &str = "55555555-5555-4555-8555-555555555555";
const CORRELATION: &str = "66666666-6666-4666-8666-666666666666";
const BASE: &str = "0123456789abcdef0123456789abcdef01234567";
const BASE_B: &str = "fedcba9876543210fedcba9876543210fedcba98";
const SHA: &str = "abababababababababababababababababababababababababababababababab";
const SHA_B: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";
const NOW: &str = "2026-10-16T10:00:00Z";
const SHA256_OID: &str = "abababababababababababababababababababababababababababababababab";

fn wire(text: &str) -> String {
    base64url::encode(text.as_bytes())
}

fn wire_bytes(bytes: &[u8]) -> String {
    base64url::encode(bytes)
}

fn path(text: &str) -> B1RelPath {
    B1RelPath::from_bytes(text.as_bytes()).unwrap()
}

fn typed<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

fn regular_content(size: u64, mode: u32) -> Value {
    json!({"kind": "regular", "content_sha256": SHA, "size": size, "mode": mode})
}

fn add(text: &str) -> Value {
    json!({
        "path": wire(text), "operation": "add", "expected_base_oid": null,
        "rename_from": null, "content": regular_content(5, REGULAR_MODE),
        "reviewed_sha256": SHA_B
    })
}

fn modify(text: &str) -> Value {
    json!({
        "path": wire(text), "operation": "modify", "expected_base_oid": BASE,
        "rename_from": null, "content": regular_content(12, REGULAR_MODE),
        "reviewed_sha256": SHA_B
    })
}

fn delete(text: &str) -> Value {
    json!({
        "path": wire(text), "operation": "delete", "expected_base_oid": BASE,
        "rename_from": null, "content": null, "reviewed_sha256": SHA_B
    })
}

fn rename(to: &str, from: &str) -> Value {
    json!({
        "path": wire(to), "operation": "rename", "expected_base_oid": BASE,
        "rename_from": wire(from), "content": regular_content(7, REGULAR_MODE),
        "reviewed_sha256": SHA_B
    })
}

fn symlink(text: &str, target: &[u8]) -> Value {
    json!({
        "path": wire(text), "operation": "add", "expected_base_oid": null,
        "rename_from": null,
        "content": {"kind": "symlink", "target": wire_bytes(target)},
        "reviewed_sha256": SHA_B
    })
}

fn selection(overlay: Vec<Value>) -> Value {
    json!({
        "schema_version": 1, "task_id": TASK, "selection_id": SELECTION,
        "repo_id": REPO, "base_commit_oid": BASE, "repo_object_format": "sha1",
        "admitted_overlay": overlay, "policy_id": POLICY, "policy_sha256": SHA,
        "issued_at": NOW
    })
}

fn valid_selection(overlay: Vec<Value>) -> B1SourceSelectionV1 {
    let parsed: B1SourceSelectionV1 = typed(selection(overlay));
    parsed.validate().unwrap();
    parsed
}

/// Parses a well-shaped selection and returns the code its semantic validation raised.
fn selection_code(value: Value) -> B1ErrorCode {
    let parsed: B1SourceSelectionV1 = typed(value);
    parsed.validate().unwrap_err().code
}

fn overlay_code(overlay: Vec<Value>) -> B1ErrorCode {
    selection_code(selection(overlay))
}

fn entry_code(entry: Value) -> B1ErrorCode {
    overlay_code(vec![entry])
}

fn assert_shape_rejected(value: Value) {
    assert!(serde_json::from_value::<B1SourceSelectionV1>(value).is_err());
}

#[test]
fn accepts_exact_bytes_including_non_utf8_and_round_trips_on_the_wire() {
    let cases: &[&[u8]] = &[
        b"src/main.rs",
        b".gitignore",
        b".github/workflows/ci.yml",
        b"dir/a:b",
        b"caf\xc3\xa9/notes.md",
        b"latin\xe9.txt",
    ];
    for bytes in cases {
        let parsed = B1RelPath::from_bytes(bytes).unwrap();
        assert_eq!(parsed.as_bytes(), *bytes);
        let text: String = parsed.clone().into();
        assert_eq!(
            serde_json::from_value::<B1RelPath>(json!(text)).unwrap(),
            parsed
        );
    }
}

#[test]
fn rejects_unsafe_path_bytes() {
    let cases: &[&[u8]] = &[
        b"",
        b"/etc/passwd",
        b"a//b",
        b"a/",
        b".",
        b"..",
        b"a/../b",
        b"a/./b",
        b"./a",
        b"a\\b",
        b"a\0b",
        b"C:/x",
    ];
    for bytes in cases {
        assert_eq!(
            B1RelPath::from_bytes(bytes).unwrap_err().code,
            B1ErrorCode::PathUnsafe,
            "{bytes:?}"
        );
    }
}

#[test]
fn rejects_git_control_paths_and_their_aliases() {
    let cases: &[&[u8]] = &[
        b".git",
        b".GIT",
        b".Git/config",
        b"x/.git",
        b"x/.git/hooks/pre-commit",
        b".git.",
        b".git ",
        b".git. ",
        b"GIT~1",
        b"x/git~1/y",
    ];
    for bytes in cases {
        assert_eq!(
            B1RelPath::from_bytes(bytes).unwrap_err().code,
            B1ErrorCode::PathUnsafe,
            "{bytes:?}"
        );
    }
}

#[test]
fn enforces_length_depth_and_component_limits() {
    assert!(B1RelPath::from_bytes(&vec![b'a'; 255]).is_ok());
    assert_eq!(
        B1RelPath::from_bytes(&vec![b'a'; 256]).unwrap_err().code,
        B1ErrorCode::LimitExceeded
    );
    let depth_32 = vec!["d"; 32].join("/");
    assert!(B1RelPath::from_bytes(depth_32.as_bytes()).is_ok());
    let depth_33 = vec!["d"; 33].join("/");
    assert_eq!(
        B1RelPath::from_bytes(depth_33.as_bytes()).unwrap_err().code,
        B1ErrorCode::LimitExceeded
    );
    let oversized = vec![b'a'; MAX_PATH_BYTES + 1];
    assert_eq!(
        B1RelPath::from_bytes(&oversized).unwrap_err().code,
        B1ErrorCode::LimitExceeded
    );
}

#[test]
fn wire_encoding_accepts_only_the_canonical_spelling() {
    assert_eq!(base64url::decode("AA", "path").unwrap(), vec![0]);
    for byte in 0..=255u8 {
        let bytes = [byte];
        assert_eq!(
            base64url::decode(&base64url::encode(&bytes), "path").unwrap(),
            bytes
        );
    }
    // Non-zero trailing bits are a second spelling of the same byte, so they are refused.
    for text in ["AB", "A", "AA=", "AA+A", "AA/A", "AA A"] {
        assert_eq!(
            base64url::decode(text, "path").unwrap_err().code,
            B1ErrorCode::PathEncodingNoncanonical,
            "{text:?}"
        );
    }
    // Decoded traversal text is still checked by the path rules.
    assert!(serde_json::from_value::<B1RelPath>(json!(wire("../escape"))).is_err());
}

#[test]
fn destination_set_rejects_collisions_and_file_directory_conflicts() {
    let conflicts = [
        ("README", "readme"),
        ("a", "a"),
        ("a", "a/b"),
        ("a/b", "a"),
        ("Docs/Readme.md", "docs/README.md"),
        ("x/Y", "X/y/z"),
    ];
    for (first, second) in conflicts {
        assert_eq!(
            validate_destination_set(&[path(first), path(second)])
                .unwrap_err()
                .code,
            B1ErrorCode::PathCollision,
            "{first} vs {second}"
        );
    }
    validate_destination_set(&[path("a-b"), path("a_b"), path("ab/c")]).unwrap();
}

#[test]
fn link_targets_are_relative_and_contained_from_their_location() {
    let link = |text: &str| path(text);
    let target = |text: &str| B1LinkTarget::from_bytes(text.as_bytes()).unwrap();
    target("x").validate_for(&link("l")).unwrap();
    target("../x").validate_for(&link("dir/l")).unwrap();
    target("sub/../x").validate_for(&link("dir/l")).unwrap();

    let escapes = [
        ("dir/l", "../../x"),
        ("l", "../x"),
        ("l", "/etc/passwd"),
        ("dir/l", "a//b"),
        ("dir/l", "./x"),
        ("dir/l", "x/."),
        ("dir/l", ".git/config"),
        ("dir/l", "../.git/config"),
        ("dir/l", "x\\y"),
        ("dir/l", "x\0y"),
        ("dir/l", ""),
    ];
    for (location, text) in escapes {
        assert_eq!(
            target(text).validate_for(&link(location)).unwrap_err().code,
            B1ErrorCode::LinkTargetUnsafe,
            "{location} -> {text:?}"
        );
    }
    assert_eq!(
        B1LinkTarget::from_bytes(&vec![b'a'; 4097])
            .unwrap_err()
            .code,
        B1ErrorCode::LimitExceeded
    );
}

#[test]
fn valid_selection_round_trips_without_any_repository_path() {
    let value = selection(vec![add("docs/new.md"), delete("old.txt")]);
    let parsed: B1SourceSelectionV1 = typed(value.clone());
    parsed.validate().unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
}

#[test]
fn rejects_unknown_fields_wrong_schema_and_smuggled_repository_paths() {
    let mut with_path = selection(vec![]);
    with_path["repo_path"] = json!("/home/user/project");
    assert_shape_rejected(with_path);

    let mut schema_two = selection(vec![]);
    schema_two["schema_version"] = json!(2);
    assert_shape_rejected(schema_two);

    let mut extra_entry_field = selection(vec![add("a")]);
    extra_entry_field["admitted_overlay"][0]["mode_override"] = json!(120000);
    assert_shape_rejected(extra_entry_field);

    let mut extra_content_field = selection(vec![add("a")]);
    extra_content_field["admitted_overlay"][0]["content"]["archive"] = json!(true);
    assert_shape_rejected(extra_content_field);
}

#[test]
fn base_and_overlay_object_ids_must_be_lowercase_full_length_and_match_the_format() {
    let mut uppercase = selection(vec![]);
    uppercase["base_commit_oid"] = json!(BASE.to_uppercase());
    assert_shape_rejected(uppercase);

    let mut short = selection(vec![]);
    short["base_commit_oid"] = json!(&BASE[..39]);
    assert_shape_rejected(short);

    let mut null_object = selection(vec![]);
    null_object["base_commit_oid"] = json!("0".repeat(40));
    assert_shape_rejected(null_object);

    let mut mismatched = selection(vec![]);
    mismatched["repo_object_format"] = json!("sha256");
    assert_eq!(selection_code(mismatched), B1ErrorCode::BaseOidInvalid);

    let mut sha256_repo = selection(vec![]);
    sha256_repo["repo_object_format"] = json!("sha256");
    sha256_repo["base_commit_oid"] = json!(SHA256_OID);
    assert!(typed::<B1SourceSelectionV1>(sha256_repo).validate().is_ok());

    let mut entry_oid = modify("a");
    entry_oid["expected_base_oid"] = json!(SHA256_OID);
    assert_eq!(entry_code(entry_oid), B1ErrorCode::OidInvalid);
}

#[test]
fn operation_shape_is_enforced_for_every_change_kind() {
    let mut add_with_base = add("a");
    add_with_base["expected_base_oid"] = json!(BASE);
    assert_eq!(entry_code(add_with_base), B1ErrorCode::OverlayInvalid);

    let mut modify_without_base = modify("a");
    modify_without_base["expected_base_oid"] = json!(null);
    assert_eq!(entry_code(modify_without_base), B1ErrorCode::OverlayInvalid);

    let mut modify_without_content = modify("a");
    modify_without_content["content"] = json!(null);
    assert_eq!(
        entry_code(modify_without_content),
        B1ErrorCode::OverlayInvalid
    );

    let mut delete_with_content = delete("a");
    delete_with_content["content"] = regular_content(1, REGULAR_MODE);
    assert_eq!(entry_code(delete_with_content), B1ErrorCode::OverlayInvalid);

    let mut delete_without_base = delete("a");
    delete_without_base["expected_base_oid"] = json!(null);
    assert_eq!(entry_code(delete_without_base), B1ErrorCode::OverlayInvalid);

    let mut add_with_source = add("a");
    add_with_source["rename_from"] = json!(wire("b"));
    assert_eq!(entry_code(add_with_source), B1ErrorCode::OverlayInvalid);

    let mut rename_without_source = rename("b", "a");
    rename_without_source["rename_from"] = json!(null);
    assert_eq!(
        entry_code(rename_without_source),
        B1ErrorCode::OverlayInvalid
    );

    assert_eq!(entry_code(rename("a", "a")), B1ErrorCode::OverlayInvalid);
}

#[test]
fn rename_is_only_a_paired_delete_and_add() {
    assert_eq!(
        overlay_code(vec![rename("b", "a")]),
        B1ErrorCode::OverlayInvalid,
        "a rename without its source delete"
    );

    let mut mismatched_source_base = delete("a");
    mismatched_source_base["expected_base_oid"] = json!(BASE_B);
    assert_eq!(
        overlay_code(vec![mismatched_source_base, rename("b", "a")]),
        B1ErrorCode::OverlayInvalid
    );

    valid_selection(vec![delete("a"), rename("b", "a")]);
    // The source may sort after its destination; pairing is by path, not by position.
    valid_selection(vec![rename("a", "z"), delete("z")]);
}

#[test]
fn overlay_must_be_strictly_ordered_and_free_of_duplicates() {
    assert_eq!(
        overlay_code(vec![modify("b"), modify("a")]),
        B1ErrorCode::NonCanonicalOrder
    );
    assert_eq!(
        overlay_code(vec![modify("a"), modify("a")]),
        B1ErrorCode::NonCanonicalOrder
    );
    // A delete and an add of the same path would be two different intents for one path.
    assert_eq!(
        overlay_code(vec![delete("a"), add("a")]),
        B1ErrorCode::NonCanonicalOrder
    );
}

#[test]
fn destinations_that_collide_or_conflict_are_refused() {
    assert_eq!(
        overlay_code(vec![add("Readme"), add("readme")]),
        B1ErrorCode::PathCollision
    );
    assert_eq!(
        overlay_code(vec![add("a"), add("a/b")]),
        B1ErrorCode::PathCollision
    );
    // Deleting a file to place a directory at the same name is one legitimate change.
    valid_selection(vec![delete("a"), add("a/b")]);
    valid_selection(vec![add("a"), delete("a/b")]);
}

#[test]
fn gitlinks_special_modes_and_unknown_content_kinds_are_unrepresentable() {
    let mut gitlink_kind = add("sub");
    gitlink_kind["content"] = json!({"kind": "gitlink", "commit": BASE});
    assert_shape_rejected(selection(vec![gitlink_kind]));

    let mut gitlink_mode = add("sub");
    gitlink_mode["content"] = regular_content(0, 0o160000);
    assert_eq!(entry_code(gitlink_mode), B1ErrorCode::ModeUnsupported);

    let mut device_mode = add("dev");
    device_mode["content"] = regular_content(0, 0o020000);
    assert_eq!(entry_code(device_mode), B1ErrorCode::ModeUnsupported);

    let mut group_writable = add("a");
    group_writable["content"] = regular_content(1, 0o100664);
    assert_eq!(entry_code(group_writable), B1ErrorCode::ModeUnsupported);

    let mut executable = add("bin/run");
    executable["content"] = regular_content(1, EXECUTABLE_MODE);
    valid_selection(vec![executable]);
}

#[test]
fn symlink_entries_are_validated_against_their_own_location() {
    valid_selection(vec![symlink("dir/l", b"../x")]);
    assert_eq!(
        entry_code(symlink("l", b"../x")),
        B1ErrorCode::LinkTargetUnsafe
    );
    assert_eq!(
        entry_code(symlink("dir/l", b"../../../etc")),
        B1ErrorCode::LinkTargetUnsafe
    );
    assert_eq!(
        entry_code(symlink("dir/l", b"../.git/hooks/x")),
        B1ErrorCode::LinkTargetUnsafe
    );
}

#[test]
fn budgets_bound_entry_size_total_bytes_and_count() {
    let mut oversized = add("big");
    oversized["content"]["size"] = json!(MAX_INPUT_BYTES + 1);
    assert_eq!(entry_code(oversized), B1ErrorCode::LimitExceeded);

    let at_limit = |name: String| {
        let mut entry = add(&name);
        entry["content"]["size"] = json!(MAX_INPUT_BYTES);
        entry
    };
    let eight: Vec<Value> = (0..8).map(|i| at_limit(format!("f{i}"))).collect();
    valid_selection(eight.clone());
    let mut nine = eight;
    nine.push(at_limit("f8".to_owned()));
    assert_eq!(overlay_code(nine), B1ErrorCode::LimitExceeded);

    let many: Vec<Value> = (0..=MAX_ENTRIES)
        .map(|i| add(&format!("f{i:05}")))
        .collect();
    assert_eq!(overlay_code(many), B1ErrorCode::LimitExceeded);
}

fn admission_json() -> Value {
    json!({
        "schema_version": 1, "task_id": TASK, "selection_id": SELECTION,
        "generation": 1, "record_revision": 1, "base_commit_oid": BASE,
        "policy_sha256": SHA, "approved_overlay_sha256": SHA_B,
        "admitted_tree_sha256": SHA_B, "admitted_bytes": 12, "admitted_entries": 1
    })
}

fn workspace_json() -> Value {
    json!({
        "schema_version": 1, "task_id": TASK, "generation": 1,
        "source_manifest_sha256": SHA, "task_fs_id": FS,
        "mount_policy_sha256": SHA, "preexec_profile_sha256": SHA_B,
        "runtime_fingerprint_sha256": SHA_B, "checked_at": NOW
    })
}

fn export_json() -> Value {
    json!({
        "schema_version": 1, "task_id": TASK, "generation": 1,
        "base_admission_sha256": SHA,
        "entries": [{
            "path": wire("docs/new.md"), "operation": "add",
            "content": regular_content(5, REGULAR_MODE)
        }],
        "exporter_sha256": SHA_B, "source_tree_sha256": SHA_B
    })
}

fn quarantine_json() -> Value {
    json!({
        "schema_version": 1, "task_id": TASK, "expected_generation": 1,
        "source_base_oid": BASE, "admission_sha256": SHA, "export_sha256": SHA_B,
        "result_store_ref": format!("refs/b1-quarantine/{TASK}/1"), "cas_revision": 1,
        "result_tree_oid": BASE_B, "risk_flags": ["build_script"], "status": "pending"
    })
}

fn admission_record(value: Value) -> B1AdmissionRecordV1 {
    typed(value)
}

#[test]
fn admission_record_binds_only_to_the_selection_it_came_from() {
    let record = admission_record(admission_json());
    record.validate().unwrap();
    record
        .bind_selection(&valid_selection(vec![add("a")]))
        .unwrap();

    let mut other_selection = selection(vec![add("a")]);
    other_selection["selection_id"] = json!(FS);
    let other: B1SourceSelectionV1 = typed(other_selection);
    assert_eq!(
        record.bind_selection(&other).unwrap_err().code,
        B1ErrorCode::AdmissionUnauthorized
    );

    let mut other_policy = selection(vec![add("a")]);
    other_policy["policy_sha256"] = json!(SHA_B);
    let other: B1SourceSelectionV1 = typed(other_policy);
    assert_eq!(
        record.bind_selection(&other).unwrap_err().code,
        B1ErrorCode::AdmissionUnauthorized
    );

    let mut other_base = selection(vec![add("a")]);
    other_base["base_commit_oid"] = json!(BASE_B);
    let other: B1SourceSelectionV1 = typed(other_base);
    assert_eq!(
        record.bind_selection(&other).unwrap_err().code,
        B1ErrorCode::BaseOidInvalid
    );
}

#[test]
fn generations_and_revisions_are_positive_bounded_counters() {
    for field in ["generation", "record_revision"] {
        for bad in [json!(0), json!(i64::MAX as u64 + 1), json!(u64::MAX)] {
            let mut value = admission_json();
            value[field] = bad;
            assert_eq!(
                admission_record(value).validate().unwrap_err().code,
                B1ErrorCode::CounterInvalid,
                "{field}"
            );
        }
    }
    let mut at_bound = admission_json();
    at_bound["generation"] = json!(i64::MAX as u64);
    admission_record(at_bound).validate().unwrap();

    let mut oversized_bytes = admission_json();
    oversized_bytes["admitted_bytes"] = json!(16 * 1024 * 1024 + 1);
    assert_eq!(
        admission_record(oversized_bytes)
            .validate()
            .unwrap_err()
            .code,
        B1ErrorCode::LimitExceeded
    );
}

#[test]
fn export_records_reject_renames_unsafe_nodes_and_collisions() {
    let valid: B1ExportRecordV1 = typed(export_json());
    valid.validate().unwrap();

    let mut rename_entry = export_json();
    rename_entry["entries"][0]["operation"] = json!("rename");
    assert!(serde_json::from_value::<B1ExportRecordV1>(rename_entry).is_err());

    let check = |entries: Vec<Value>| -> B1ErrorCode {
        let mut value = export_json();
        value["entries"] = json!(entries);
        typed::<B1ExportRecordV1>(value)
            .validate()
            .unwrap_err()
            .code
    };
    let content = regular_content(5, REGULAR_MODE);
    assert_eq!(
        check(vec![
            json!({"path": wire("a"), "operation": "add", "content": null})
        ]),
        B1ErrorCode::ExportNodeUnsafe
    );
    assert_eq!(
        check(vec![
            json!({"path": wire("a"), "operation": "delete", "content": content})
        ]),
        B1ErrorCode::ExportNodeUnsafe
    );
    assert_eq!(
        check(vec![
            json!({"path": wire("b"), "operation": "add", "content": content}),
            json!({"path": wire("a"), "operation": "add", "content": content}),
        ]),
        B1ErrorCode::NonCanonicalOrder
    );
    assert_eq!(
        check(vec![
            json!({"path": wire("a"), "operation": "add", "content": content}),
            json!({"path": wire("a/b"), "operation": "add", "content": content}),
        ]),
        B1ErrorCode::PathCollision
    );
    assert_eq!(
        check(vec![json!({
            "path": wire("dir/l"), "operation": "add",
            "content": {"kind": "symlink", "target": wire("../../etc")}
        })]),
        B1ErrorCode::LinkTargetUnsafe
    );

    let mut generation_zero = export_json();
    generation_zero["generation"] = json!(0);
    assert_eq!(
        typed::<B1ExportRecordV1>(generation_zero)
            .validate()
            .unwrap_err()
            .code,
        B1ErrorCode::CounterInvalid
    );
}

#[test]
fn risk_flags_are_case_insensitive_and_apply_at_any_depth() {
    use RiskFlag::*;
    let flags = |text: &str, executable: bool| classify_entry(&path(text), executable);
    assert_eq!(
        flags(".github/workflows/ci.yml", false),
        vec![GitHubWorkflow]
    );
    assert_eq!(
        flags(".Github/workflows/ci.yml", false),
        vec![GitHubWorkflow]
    );
    assert_eq!(
        flags("pkg/.vscode/settings.json", false),
        vec![EditorConfiguration]
    );
    assert_eq!(flags("sub/AGENTS.md", false), vec![AgentInstruction]);
    assert_eq!(flags("claude.md", false), vec![AgentInstruction]);
    assert_eq!(
        flags(".claude/settings.json", false),
        vec![AgentConfiguration]
    );
    assert_eq!(flags(".codex/config.toml", false), vec![AgentConfiguration]);
    assert_eq!(flags("package.json", false), vec![PackageManifest]);
    assert_eq!(flags(".husky/pre-commit", false), vec![GitHooks]);
    assert_eq!(flags("makefile", false), vec![BuildScript]);
    assert_eq!(flags(".gitmodules", false), vec![GitMetadata]);
    assert_eq!(flags(".gitattributes", false), vec![GitMetadata]);
    assert_eq!(flags(".lfsconfig", false), vec![GitMetadata]);
    assert_eq!(flags(".envrc", false), vec![EnvironmentLoader]);
    assert_eq!(flags("bin/run.sh", true), vec![Executable]);
    assert!(flags("src/main.rs", false).is_empty());
    assert_eq!(
        flags(".github/AGENTS.md", true),
        vec![AgentInstruction, GitHubWorkflow, Executable]
    );
}

fn as_quarantine(value: Value) -> B1QuarantineReceiptV1 {
    typed(value)
}

#[test]
fn quarantine_receipt_is_pending_only_and_pinned_to_its_own_ref() {
    as_quarantine(quarantine_json()).validate().unwrap();
    assert_eq!(
        quarantine_ref(&typed(json!(TASK)), 1),
        format!("refs/b1-quarantine/{TASK}/1")
    );

    let mut published = quarantine_json();
    published["status"] = json!("published");
    assert!(serde_json::from_value::<B1QuarantineReceiptV1>(published).is_err());

    for bad_ref in [
        "refs/heads/main".to_owned(),
        format!("refs/b1-quarantine/{SELECTION}/1"),
        format!("refs/b1-quarantine/{TASK}/2"),
    ] {
        let mut value = quarantine_json();
        value["result_store_ref"] = json!(bad_ref);
        assert_eq!(
            as_quarantine(value).validate().unwrap_err().code,
            B1ErrorCode::ReceiptInvalid,
            "{bad_ref}"
        );
    }

    let mut unsorted = quarantine_json();
    unsorted["risk_flags"] = json!(["package_manifest", "build_script"]);
    assert_eq!(
        as_quarantine(unsorted).validate().unwrap_err().code,
        B1ErrorCode::NonCanonicalOrder
    );
    let mut duplicated = quarantine_json();
    duplicated["risk_flags"] = json!(["build_script", "build_script"]);
    assert_eq!(
        as_quarantine(duplicated).validate().unwrap_err().code,
        B1ErrorCode::NonCanonicalOrder
    );
    let mut unknown_flag = quarantine_json();
    unknown_flag["risk_flags"] = json!(["unclassified"]);
    assert!(serde_json::from_value::<B1QuarantineReceiptV1>(unknown_flag).is_err());

    let mut zero_cas = quarantine_json();
    zero_cas["cas_revision"] = json!(0);
    assert_eq!(
        as_quarantine(zero_cas).validate().unwrap_err().code,
        B1ErrorCode::CounterInvalid
    );
    let mut format_mismatch = quarantine_json();
    format_mismatch["result_tree_oid"] = json!(SHA256_OID);
    assert_eq!(
        as_quarantine(format_mismatch).validate().unwrap_err().code,
        B1ErrorCode::OidInvalid
    );
}

fn failure(stage: &str, cleanup: &str, action: &str) -> B1FailureV1 {
    typed(json!({
        "schema_version": 1, "task_id": TASK, "stage": stage,
        "stable_error_code": "PATH_COLLISION", "action_required": action,
        "cleanup_status": cleanup, "correlation_id": CORRELATION
    }))
}

#[test]
fn failure_records_require_terminal_stages_and_consistent_cleanup() {
    failure("FAILED_CLEAN", "verified", "none")
        .validate()
        .unwrap();
    failure("BLOCKED_UNSAFE", "verified", "owner_review")
        .validate()
        .unwrap();
    failure(
        "FAILED_CLEANUP_UNVERIFIED",
        "unverified",
        "operator_recovery",
    )
    .validate()
    .unwrap();

    for (stage, cleanup, action) in [
        ("FAILED_CLEANUP_UNVERIFIED", "verified", "operator_recovery"),
        ("FAILED_CLEANUP_UNVERIFIED", "unverified", "owner_review"),
        ("FAILED_CLEAN", "unverified", "none"),
        ("REJECTED", "unverified", "none"),
        ("PENDING", "verified", "none"),
        ("RUNNING", "verified", "none"),
    ] {
        assert_eq!(
            failure(stage, cleanup, action).validate().unwrap_err().code,
            B1ErrorCode::ReceiptInvalid,
            "{stage}/{cleanup}/{action}"
        );
    }

    let mut free_text = json!({
        "schema_version": 1, "task_id": TASK, "stage": "REJECTED",
        "stable_error_code": "PATH_COLLISION", "action_required": "none",
        "cleanup_status": "verified", "correlation_id": CORRELATION,
        "detail": "/home/user/.ssh/id_ed25519"
    });
    assert!(serde_json::from_value::<B1FailureV1>(free_text.clone()).is_err());
    free_text["stable_error_code"] = json!("SOME_RAW_TEXT");
    assert!(serde_json::from_value::<B1FailureV1>(free_text).is_err());
}

#[test]
fn stable_codes_serialize_to_the_doc_52_spelling() {
    assert_eq!(
        serde_json::to_value(B1ErrorCode::AdmissionUnauthorized).unwrap(),
        json!("ADMISSION_UNAUTHORIZED")
    );
    assert_eq!(
        serde_json::to_value(B1ErrorCode::QuarantineCasConflict).unwrap(),
        json!("QUARANTINE_CAS_CONFLICT")
    );
    let all = [
        B1ErrorCode::AdmissionUnauthorized,
        B1ErrorCode::ProviderEgressUnproven,
        B1ErrorCode::PathEncodingNoncanonical,
        B1ErrorCode::PendingEvidenceMismatch,
        B1ErrorCode::ReceiptInvalid,
    ];
    for code in all {
        assert_eq!(serde_json::to_value(code).unwrap(), json!(code.as_str()));
    }
    assert_eq!(
        serde_json::to_value(B1TaskState::FailedCleanupUnverified).unwrap(),
        json!("FAILED_CLEANUP_UNVERIFIED")
    );
}

fn as_workspace(value: Value) -> B1WorkspaceReceiptV1 {
    typed(value)
}

fn fixture_evidence() -> (
    B1AdmissionRecordV1,
    B1WorkspaceReceiptV1,
    B1ExportRecordV1,
    B1QuarantineReceiptV1,
) {
    (
        admission_record(admission_json()),
        as_workspace(workspace_json()),
        typed(export_json()),
        as_quarantine(quarantine_json()),
    )
}

fn validated_lifecycle() -> B1TaskLifecycle {
    let mut lifecycle = B1TaskLifecycle::new();
    for state in [
        B1TaskState::Selecting,
        B1TaskState::Captured,
        B1TaskState::Admitted,
        B1TaskState::Materialized,
        B1TaskState::Confined,
        B1TaskState::Running,
        B1TaskState::Quiescing,
        B1TaskState::Frozen,
        B1TaskState::Exporting,
        B1TaskState::Validated,
    ] {
        lifecycle.advance(state).unwrap();
    }
    lifecycle
}

#[test]
fn happy_path_reaches_pending_only_through_cross_checked_evidence() {
    let mut lifecycle = validated_lifecycle();
    let (admission, workspace, export, quarantine) = fixture_evidence();
    let admission_sha: crate::contracts::b1::Sha256Hex = typed(json!(SHA));
    let export_sha: crate::contracts::b1::Sha256Hex = typed(json!(SHA_B));
    let evidence = B1PendingEvidence {
        admission: &admission,
        admission_sha256: &admission_sha,
        workspace: &workspace,
        export: &export,
        export_sha256: &export_sha,
        quarantine: &quarantine,
    };
    lifecycle.publish_pending(&evidence).unwrap();
    assert_eq!(lifecycle.state(), B1TaskState::Pending);
    assert!(lifecycle.state().is_terminal());
    for next in [B1TaskState::Rejected, B1TaskState::FailedClean] {
        assert!(lifecycle.advance(next).is_err());
    }
    assert_eq!(lifecycle.state(), B1TaskState::Pending);
}

#[test]
fn the_legal_transition_set_is_exactly_the_doc_52_graph() {
    use B1TaskState::*;
    let all = [
        Created,
        Selecting,
        Captured,
        Admitted,
        Materialized,
        Confined,
        Running,
        Quiescing,
        Frozen,
        Exporting,
        Validated,
        Pending,
        Rejected,
        FailedClean,
        FailedCleanupUnverified,
        BlockedUnsafe,
    ];
    let legal: Vec<(B1TaskState, B1TaskState)> = all
        .iter()
        .flat_map(|&from| all.iter().map(move |&to| (from, to)))
        .filter(|&(from, to)| from.can_transition_to(to))
        .collect();
    let expected = [
        (Created, Selecting),
        (Selecting, Captured),
        (Selecting, Rejected),
        (Captured, Admitted),
        (Captured, Rejected),
        (Admitted, Materialized),
        (Admitted, Rejected),
        (Materialized, Confined),
        (Materialized, FailedClean),
        (Materialized, BlockedUnsafe),
        (Confined, Running),
        (Confined, FailedClean),
        (Confined, BlockedUnsafe),
        (Running, Quiescing),
        (Quiescing, Frozen),
        (Quiescing, FailedCleanupUnverified),
        (Frozen, Exporting),
        (Frozen, FailedClean),
        (Exporting, Validated),
        (Exporting, Rejected),
        (Validated, Pending),
        (Validated, Rejected),
    ];
    assert_eq!(legal.len(), expected.len());
    for edge in expected {
        assert!(legal.contains(&edge), "{edge:?} missing");
    }
    for state in all.iter().filter(|state| state.is_terminal()) {
        assert!(
            all.iter().all(|&next| !state.can_transition_to(next)),
            "{state:?} must have no exit"
        );
    }
}

#[test]
fn skipped_reversed_and_unsafe_transitions_leave_the_state_unchanged() {
    let mut lifecycle = B1TaskLifecycle::new();
    assert_eq!(
        lifecycle.advance(B1TaskState::Running).unwrap_err().code,
        B1ErrorCode::InvalidTransition
    );
    assert_eq!(lifecycle.state(), B1TaskState::Created);

    // Quiescing is mandatory before freezing, and freezing before exporting.
    let mut running = B1TaskLifecycle::new();
    for state in [
        B1TaskState::Selecting,
        B1TaskState::Captured,
        B1TaskState::Admitted,
        B1TaskState::Materialized,
        B1TaskState::Confined,
        B1TaskState::Running,
    ] {
        running.advance(state).unwrap();
    }
    assert!(running.advance(B1TaskState::Frozen).is_err());
    assert!(running.advance(B1TaskState::Exporting).is_err());
    // A running tree cannot be declared unsafe without first being quiesced.
    assert!(running.advance(B1TaskState::BlockedUnsafe).is_err());
    assert_eq!(running.state(), B1TaskState::Running);

    // A validated export cannot go back into the worker.
    let mut validated = validated_lifecycle();
    assert!(validated.advance(B1TaskState::Running).is_err());
    assert_eq!(validated.state(), B1TaskState::Validated);

    // BLOCKED_UNSAFE needs an isolation decision, so selection and capture cannot reach it.
    let mut captured = B1TaskLifecycle::new();
    for state in [B1TaskState::Selecting, B1TaskState::Captured] {
        captured.advance(state).unwrap();
    }
    assert!(captured.advance(B1TaskState::BlockedUnsafe).is_err());
    assert_eq!(captured.state(), B1TaskState::Captured);

    let mut materialized = B1TaskLifecycle::new();
    for state in [
        B1TaskState::Selecting,
        B1TaskState::Captured,
        B1TaskState::Admitted,
        B1TaskState::Materialized,
    ] {
        materialized.advance(state).unwrap();
    }
    materialized.advance(B1TaskState::BlockedUnsafe).unwrap();
    assert_eq!(materialized.state(), B1TaskState::BlockedUnsafe);
}

#[test]
fn pending_cannot_be_reached_by_a_generic_transition_or_from_the_wrong_stage() {
    let mut validated = validated_lifecycle();
    assert_eq!(
        validated.advance(B1TaskState::Pending).unwrap_err().code,
        B1ErrorCode::InvalidTransition
    );
    assert_eq!(validated.state(), B1TaskState::Validated);

    let (admission, workspace, export, quarantine) = fixture_evidence();
    let admission_sha: crate::contracts::b1::Sha256Hex = typed(json!(SHA));
    let export_sha: crate::contracts::b1::Sha256Hex = typed(json!(SHA_B));
    let evidence = B1PendingEvidence {
        admission: &admission,
        admission_sha256: &admission_sha,
        workspace: &workspace,
        export: &export,
        export_sha256: &export_sha,
        quarantine: &quarantine,
    };
    let mut frozen = B1TaskLifecycle::new();
    for state in [
        B1TaskState::Selecting,
        B1TaskState::Captured,
        B1TaskState::Admitted,
        B1TaskState::Materialized,
        B1TaskState::Confined,
        B1TaskState::Running,
        B1TaskState::Quiescing,
        B1TaskState::Frozen,
    ] {
        frozen.advance(state).unwrap();
    }
    assert_eq!(
        frozen.publish_pending(&evidence).unwrap_err().code,
        B1ErrorCode::InvalidTransition
    );
    assert_eq!(frozen.state(), B1TaskState::Frozen);
}

#[test]
fn mismatched_evidence_never_publishes_and_never_changes_state() {
    let (admission, workspace, export, quarantine) = fixture_evidence();
    let admission_sha: crate::contracts::b1::Sha256Hex = typed(json!(SHA));
    let export_sha: crate::contracts::b1::Sha256Hex = typed(json!(SHA_B));
    let other_sha: crate::contracts::b1::Sha256Hex = typed(json!("ef".repeat(32)));

    let check = |admission: &B1AdmissionRecordV1,
                 admission_sha: &crate::contracts::b1::Sha256Hex,
                 workspace: &B1WorkspaceReceiptV1,
                 export: &B1ExportRecordV1,
                 export_sha: &crate::contracts::b1::Sha256Hex,
                 quarantine: &B1QuarantineReceiptV1|
     -> B1ErrorCode {
        let mut lifecycle = validated_lifecycle();
        let evidence = B1PendingEvidence {
            admission,
            admission_sha256: admission_sha,
            workspace,
            export,
            export_sha256: export_sha,
            quarantine,
        };
        let code = lifecycle.publish_pending(&evidence).unwrap_err().code;
        assert_eq!(lifecycle.state(), B1TaskState::Validated);
        code
    };

    // The exact-match baseline still passes, so every failure below is attributable.
    let mut lifecycle = validated_lifecycle();
    lifecycle
        .publish_pending(&B1PendingEvidence {
            admission: &admission,
            admission_sha256: &admission_sha,
            workspace: &workspace,
            export: &export,
            export_sha256: &export_sha,
            quarantine: &quarantine,
        })
        .unwrap();

    assert_eq!(
        check(
            &admission,
            &other_sha,
            &workspace,
            &export,
            &export_sha,
            &quarantine
        ),
        B1ErrorCode::PendingEvidenceMismatch,
        "admission digest"
    );
    assert_eq!(
        check(
            &admission,
            &admission_sha,
            &workspace,
            &export,
            &other_sha,
            &quarantine
        ),
        B1ErrorCode::PendingEvidenceMismatch,
        "export digest"
    );

    let mut other_generation = workspace_json();
    other_generation["generation"] = json!(2);
    assert_eq!(
        check(
            &admission,
            &admission_sha,
            &as_workspace(other_generation),
            &export,
            &export_sha,
            &quarantine
        ),
        B1ErrorCode::PendingEvidenceMismatch,
        "workspace generation"
    );

    let mut other_task = workspace_json();
    other_task["task_id"] = json!(SELECTION);
    assert_eq!(
        check(
            &admission,
            &admission_sha,
            &as_workspace(other_task),
            &export,
            &export_sha,
            &quarantine
        ),
        B1ErrorCode::PendingEvidenceMismatch,
        "workspace task"
    );

    let mut export_bound_elsewhere = export_json();
    export_bound_elsewhere["base_admission_sha256"] = json!(SHA_B);
    assert_eq!(
        check(
            &admission,
            &admission_sha,
            &workspace,
            &typed(export_bound_elsewhere),
            &export_sha,
            &quarantine
        ),
        B1ErrorCode::PendingEvidenceMismatch,
        "export not bound to admission"
    );

    let mut receipt_other_base = quarantine_json();
    receipt_other_base["source_base_oid"] = json!(BASE_B);
    receipt_other_base["result_tree_oid"] = json!(BASE);
    assert_eq!(
        check(
            &admission,
            &admission_sha,
            &workspace,
            &export,
            &export_sha,
            &as_quarantine(receipt_other_base)
        ),
        B1ErrorCode::PendingEvidenceMismatch,
        "quarantine base"
    );

    let mut receipt_other_export = quarantine_json();
    receipt_other_export["export_sha256"] = json!(SHA);
    assert_eq!(
        check(
            &admission,
            &admission_sha,
            &workspace,
            &export,
            &export_sha,
            &as_quarantine(receipt_other_export)
        ),
        B1ErrorCode::PendingEvidenceMismatch,
        "quarantine export digest"
    );

    let mut invalid_admission = admission_json();
    invalid_admission["generation"] = json!(0);
    assert_eq!(
        check(
            &admission_record(invalid_admission),
            &admission_sha,
            &workspace,
            &export,
            &export_sha,
            &quarantine
        ),
        B1ErrorCode::CounterInvalid,
        "invalid evidence is re-validated"
    );
}

#[test]
fn overlay_validation_is_callable_directly_for_fixture_inputs() {
    let entries: Vec<super::source::B1OverlayEntryV1> =
        [add("a"), modify("b")].into_iter().map(typed).collect();
    super::source::validate_overlay(&entries, super::ObjectFormat::Sha1).unwrap();
}
