//! M1-01 (contract §5): valid empty Scratch, project, Native, Protocol 1 and Protocol 2
//! records round-trip; unknown or duplicate keys, schema 2, dangling IDs, mixed Native and
//! Managed profile choices, forbidden secret/prompt/argv fields and u64 overflow are refused
//! before anything could be saved; byte, count and depth ceilings are exact at the bound and
//! one past it. These are the only M1 tests that exist; M1-02..13 belong to later increments.
use super::fixtures::*;
use super::records::{ManagedSafetyProfile, Node, StoreV1, Target};
use super::scalar::{AbsolutePath, Counter, EpochText, NativeId, TerminalId, Text};
use super::{
    M1Error, M1ErrorCode as Code, MAX_ASSOCIATIONS, MAX_BINDINGS_PER_PROJECT,
    MAX_CLOSED_OPERATIONS, MAX_LEAVES, MAX_MIGRATION_ROWS, MAX_OPERATIONS, MAX_PROJECTS,
    MAX_STORE_BYTES, MAX_WORKSPACES_PER_PROJECT, decode, encode,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

fn load(value: &Value) -> Result<StoreV1, M1Error> {
    decode(&bytes(value))
}

fn edited(change: impl FnOnce(&mut Value)) -> Value {
    let mut value = full();
    change(&mut value);
    value
}

fn refusal(value: &Value) -> M1Error {
    match load(value) {
        Ok(_) => panic!("the store must be refused"),
        Err(error) => error,
    }
}

#[track_caller]
fn refused_as(value: &Value, code: Code) {
    let error = refusal(value);
    assert_eq!(error.code, code, "{error}");
}

#[track_caller]
fn invalid(value: &Value) {
    refused_as(value, Code::InvalidRecord);
}

#[track_caller]
fn invalid_at(value: &Value, field: &str) {
    let error = refusal(value);
    assert_eq!(
        (error.code, error.field),
        (Code::InvalidRecord, field),
        "{error}"
    );
}

#[track_caller]
fn capacity_at(value: &Value, field: &str) {
    let error = refusal(value);
    assert_eq!(
        (error.code, error.field),
        (Code::CapacityExceeded, field),
        "{error}"
    );
}

#[track_caller]
fn accepted(value: &Value) -> StoreV1 {
    match load(value) {
        Ok(store) => store,
        Err(error) => panic!("the store must be accepted: {error}"),
    }
}

// ---- positive fixtures and determinism ----

#[test]
fn empty_scratch_and_the_full_store_round_trip_to_identical_bytes() {
    for fixture in [scratch_only(), full()] {
        let store = accepted(&fixture);
        let first = encode(&store).unwrap();
        let again = encode(&decode(&first).unwrap()).unwrap();
        assert_eq!(first, again, "encoding must be a fixed point");
        assert_eq!(decode(&first).unwrap(), store);
    }
}

#[test]
fn the_full_store_covers_every_session_and_receipt_kind() {
    let store = accepted(&full());
    assert_eq!(store.projects.len(), 2);
    let kinds: Vec<&str> = store
        .sessions
        .iter()
        .map(|s| match &s.target {
            Target::Native { .. } => "native",
            Target::Shell { .. } => "shell",
            Target::Managed { protocol, .. } => match protocol {
                super::records::ManagedProtocol::P1 => "p1",
                super::records::ManagedProtocol::P2 => "p2",
            },
        })
        .collect();
    assert_eq!(kinds, ["shell", "native", "p1", "p2"]);
    let states: Vec<_> = store.operations.iter().map(|o| o.state).collect();
    assert_eq!(states.len(), 5);
}

#[test]
fn the_canonical_bytes_are_deterministic_across_runs() {
    let canonical = encode(&accepted(&full())).unwrap();
    // Recorded from this contract's field order; a change here is a change of the format.
    let digest: String = Sha256::digest(&canonical)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(digest, GOLDEN_FULL_SHA256);
    let small: String = Sha256::digest(encode(&accepted(&scratch_only())).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(small, GOLDEN_SCRATCH_SHA256);
}

const GOLDEN_FULL_SHA256: &str = "c91c31656546117c0a1e75208c495df7af444c2e9ff3b3bba3846a05f69b5e7f";
const GOLDEN_SCRATCH_SHA256: &str =
    "55e9a41782a807b8c34bcdd657c9b539f525ba89ec158a528d21d43cb9074c09";

#[test]
fn encoded_form_uses_the_contract_field_names_and_string_counters() {
    let text = String::from_utf8(encode(&accepted(&full())).unwrap()).unwrap();
    for needle in [
        "\"schema\":1",
        "\"storeId\"",
        "\"closedOperations\"",
        "\"revision\":\"7\"",
        "\"managedSafetyProfile\":\"local_tools\"",
        "\"nativeProfile\":{\"id\"",
        "\"protocol\":2",
        "\"device\":\"64768\"",
    ] {
        assert!(text.contains(needle), "{needle}");
    }
}

// ---- unknown, missing and duplicate members ----

const RECORD_PATHS: &[&str] = &[
    "",
    "/projects/0",
    "/projects/0/profileDefaults",
    "/projects/0/profileDefaults/claude",
    "/bindings/1",
    "/bindings/1/observation",
    "/workspaces/0",
    "/workspaces/0/layout",
    "/workspaces/1/layout/b",
    "/workspaces/1/layout/a",
    "/workspaces/1/preferences",
    "/workspaces/0/leaves/0",
    "/sessions/0",
    "/sessions/0/target",
    "/sessions/1/target",
    "/sessions/3/target",
    "/operations/0",
    "/migration",
    "/migration/sources/0",
    "/migration/map/0",
];

#[test]
fn an_unknown_member_is_refused_in_every_record_including_forbidden_names() {
    for path in RECORD_PATHS {
        for name in [
            "extra",
            "prompt",
            "argv",
            "secret",
            "token",
            "credential",
            "password",
        ] {
            let value = edited(|v| {
                let target = if path.is_empty() {
                    &mut *v
                } else {
                    v.pointer_mut(path).unwrap()
                };
                target[name] = json!("x");
            });
            invalid(&value);
        }
    }
}

#[test]
fn every_member_is_required_and_null_is_written_not_omitted() {
    for path in RECORD_PATHS {
        let keys: Vec<String> = {
            let base = full();
            let target = if path.is_empty() {
                &base
            } else {
                base.pointer(path).unwrap()
            };
            target.as_object().unwrap().keys().cloned().collect()
        };
        for key in keys {
            let value = edited(|v| {
                let target = if path.is_empty() {
                    &mut *v
                } else {
                    v.pointer_mut(path).unwrap()
                };
                target.as_object_mut().unwrap().remove(&key);
            });
            assert!(
                load(&value).is_err(),
                "{path}/{key} omitted must be refused"
            );
        }
    }
}

#[test]
fn duplicate_members_are_refused_at_every_depth() {
    let canonical = String::from_utf8(encode(&accepted(&full())).unwrap()).unwrap();
    for (needle, duplicate) in [
        (
            "\"revision\":\"7\"",
            "\"revision\":\"7\",\"revision\":\"8\"",
        ),
        ("\"archived\":false", "\"archived\":false,\"archived\":true"),
        ("\"ratio\":0.5", "\"ratio\":0.5,\"ratio\":0.4"),
        ("\"protocol\":2", "\"protocol\":2,\"protocol\":1"),
    ] {
        let tampered = canonical.replacen(needle, duplicate, 1);
        assert_ne!(tampered, canonical, "{needle}");
        let error = decode(tampered.as_bytes()).unwrap_err();
        assert_eq!(error.code, Code::InvalidRecord, "{needle}");
    }
}

// ---- schema ----

#[test]
fn a_newer_schema_is_refused_as_newer_without_reading_its_records() {
    let mut newer = json!({"schema": 2, "somethingNew": [1, 2, 3]});
    refused_as(&newer, Code::UnsupportedSchema);
    newer["schema"] = json!(18446744073709551615u64);
    refused_as(&newer, Code::UnsupportedSchema);
    refused_as(&edited(|v| v["schema"] = json!(2)), Code::UnsupportedSchema);
}

#[test]
fn a_schema_that_is_not_exactly_one_is_invalid() {
    for schema in [
        json!(0),
        json!(-1),
        json!(1.5),
        json!("1"),
        json!(null),
        json!(true),
    ] {
        invalid(&edited(|v| v["schema"] = schema.clone()));
    }
    let mut value = full();
    value.as_object_mut().unwrap().remove("schema");
    invalid(&value);
    invalid(&json!([1]));
    invalid(&json!("store"));
}

// ---- corrupt bytes ----

#[test]
fn malformed_bytes_are_refused_without_a_panic_or_a_fallback() {
    let good = encode(&accepted(&full())).unwrap();
    let cut = good[..good.len() / 2].to_vec();
    let mut trailing = good.clone();
    trailing.extend_from_slice(b" {}");
    let mut non_utf8 = good.clone();
    non_utf8[10] = 0xff;
    let cases: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"null".to_vec(),
        b"{".to_vec(),
        b"{\"schema\":1,}".to_vec(),
        b"{\"schema\":NaN}".to_vec(),
        b"{\"schema\":Infinity}".to_vec(),
        b"{\"schema\":1e999}".to_vec(),
        cut,
        trailing,
        non_utf8,
        b"\xef\xbb\xbf{}".to_vec(),
    ];
    for case in cases {
        assert_eq!(decode(&case).unwrap_err().code, Code::InvalidRecord);
    }
}

#[test]
fn a_diagnostic_never_carries_the_rejected_content() {
    let value = edited(|v| v["secret-sk-live-0123456789"] = json!("hunter2-prompt-argv"));
    let text = refusal(&value).to_string();
    for leaked in ["secret-sk", "hunter2", "prompt", "argv"] {
        assert!(!text.contains(leaked), "{text}");
    }
}

// ---- identities and scalars ----

#[test]
fn native_ids_and_uuids_are_different_and_neither_is_coerced() {
    assert!(NativeId::parse(NATIVE_A).is_ok());
    for bad in [
        "0123456789ABCDEF0123456789ABCDEF",
        "0123456789abcdef0123456789abcde",
        "0123456789abcdef0123456789abcdef0",
        "01234567-89ab-cdef-0123-456789abcdef",
        "0123456789abcdef0123456789abcdeg",
        "",
    ] {
        assert!(NativeId::parse(bad).is_err(), "{bad}");
    }
    assert!(TerminalId::parse(&id(0x60, 1)).is_ok());
    for bad in [
        NATIVE_A,
        "11111111-1111-4111-8111-11111111111G",
        "11111111-1111-4111-8111-1111111111111",
        "{11111111-1111-4111-8111-111111111111}",
        "11111111111141118111111111111111",
    ] {
        assert!(TerminalId::parse(bad).is_err(), "{bad}");
    }
    assert!(TerminalId::parse("abcdef01-0000-4000-8000-00000000000a").is_ok());
    assert!(TerminalId::parse("ABCDEF01-0000-4000-8000-00000000000A").is_err());
    // A UUID where a Native ID belongs, and the reverse, inside a record.
    invalid(&edited(|v| {
        v["sessions"][1]["target"]["sessionId"] = json!(id(0x60, 9))
    }));
    invalid(&edited(|v| {
        v["sessions"][1]["target"]["terminalId"] = json!(NATIVE_A)
    }));
    invalid(&edited(|v| {
        v["projects"][0]["profileDefaults"]["claude"]["id"] = json!(id(0x60, 9))
    }));
    invalid(&edited(|v| {
        v["projects"][0]["id"] = json!("ABCDEF01-0000-4000-8000-00000000000A")
    }));
}

#[test]
fn counters_are_canonical_positive_decimal_strings() {
    for good in ["1", "7", "18446744073709551615"] {
        assert_eq!(
            Counter::try_from(good.to_owned())
                .unwrap()
                .get()
                .to_string(),
            good
        );
    }
    for bad in [
        "",
        "0",
        "01",
        "+1",
        "-1",
        " 1",
        "1 ",
        "1.0",
        "1e3",
        "٣",
        "18446744073709551616",
    ] {
        assert!(Counter::try_from(bad.to_owned()).is_err(), "{bad:?}");
    }
    assert!(Counter::new(0).is_err());
    assert_eq!(Counter::new(u64::MAX).unwrap().next(), None);
    for number in [json!(1), json!(1.0), json!(null), json!([]), json!({})] {
        invalid(&edited(|v| v["revision"] = number.clone()));
    }
}

#[test]
fn u64_overflow_and_a_revision_that_cannot_advance_are_refused() {
    invalid(&edited(|v| v["revision"] = json!("18446744073709551616")));
    invalid(&edited(|v| {
        v["revision"] = json!("99999999999999999999999")
    }));
    invalid_at(
        &edited(|v| v["revision"] = json!("18446744073709551615")),
        "revision",
    );
    invalid_at(
        &edited(|v| v["projects"][0]["revision"] = json!("18446744073709551615")),
        "projects.revision",
    );
    invalid_at(
        &edited(|v| v["bindings"][1]["revision"] = json!("18446744073709551615")),
        "bindings.revision",
    );
    invalid_at(
        &edited(|v| v["workspaces"][1]["revision"] = json!("18446744073709551615")),
        "workspaces.revision",
    );
    // Observations and binding revisions are values read from elsewhere, not revisions to advance.
    accepted(&edited(|v| {
        v["bindings"][1]["observation"]["inode"] = json!("18446744073709551615");
    }));
    accepted(&edited(|v| {
        v["bindings"][1]["observation"]["device"] = json!("18446744073709551615");
    }));
}

#[test]
fn profile_versions_fit_the_owners_u32() {
    accepted(&edited(|v| {
        v["projects"][0]["profileDefaults"]["claude"]["version"] = json!("4294967295")
    }));
    for bad in ["0", "4294967296", "01", "-1", "x"] {
        invalid(&edited(|v| {
            v["projects"][0]["profileDefaults"]["claude"]["version"] = json!(bad)
        }));
    }
    invalid(&edited(|v| {
        v["projects"][0]["profileDefaults"]["claude"]["version"] = json!(2)
    }));
}

#[test]
fn text_is_bounded_in_scalars_and_free_of_controls() {
    let at_bound = "é".repeat(64);
    assert!(Text::parse(&at_bound).is_ok(), "64 scalars of 2 bytes each");
    assert!(Text::parse(&"é".repeat(65)).is_err());
    assert!(Text::parse(&"a".repeat(64)).is_ok());
    assert!(Text::parse(&"a".repeat(65)).is_err());
    assert!(Text::parse("").is_err());
    for control in ["a\nb", "a\u{0}b", "\ttab", "a\u{7f}", "a\u{85}b"] {
        assert!(Text::parse(control).is_err(), "{control:?}");
    }
    invalid(&edited(|v| v["projects"][0]["label"] = json!("")));
    invalid(&edited(|v| {
        v["workspaces"][1]["label"] = json!("x".repeat(65))
    }));
    invalid(&edited(|v| {
        v["workspaces"][1]["leaves"][0]["title"] = json!("a\nb")
    }));
}

#[test]
fn paths_are_absolute_canonical_local_text_within_4096_bytes() {
    let exact = format!("/{}", "a".repeat(4095));
    assert_eq!(exact.len(), 4096);
    assert!(AbsolutePath::parse(&exact).is_ok());
    assert!(AbsolutePath::parse(&format!("{exact}a")).is_err());
    assert!(AbsolutePath::parse("/").is_ok());
    assert!(AbsolutePath::parse("/home/fixture/projeto-ñ").is_ok());
    for bad in [
        "",
        "relative/path",
        "./x",
        "/a//b",
        "/a/./b",
        "/a/../b",
        "/a/",
        "/a\u{0}b",
        "/a\nb",
        "~/x",
        "file:///x",
    ] {
        assert!(AbsolutePath::parse(bad).is_err(), "{bad:?}");
    }
    invalid(&edited(|v| {
        v["bindings"][1]["path"] = json!("not/absolute")
    }));
}

#[test]
fn epochs_are_opaque_bounded_text() {
    assert!(EpochText::parse(&"e".repeat(256)).is_ok());
    assert!(EpochText::parse(&"e".repeat(257)).is_err());
    assert!(EpochText::parse(&"é".repeat(128)).is_ok());
    assert!(EpochText::parse(&"é".repeat(129)).is_err());
    assert!(EpochText::parse("").is_err());
    assert!(EpochText::parse("a\nb").is_err());
    invalid(&edited(|v| {
        v["sessions"][0]["target"]["hostEpoch"] = json!("")
    }));
    invalid(&edited(|v| {
        v["operations"][2]["claim"] = json!("c".repeat(257))
    }));
}

#[test]
fn timestamps_and_digests_keep_their_strict_forms() {
    for bad in [
        "2026-10-22T12:00:00",
        "2026-10-22 12:00:00Z",
        "yesterday",
        "",
        "2026-10-22T12:00:00+02:00",
    ] {
        invalid(&edited(|v| v["projects"][0]["createdAt"] = json!(bad)));
    }
    for bad in [
        "ABCDEF".repeat(10) + "ABCD",
        "abc".to_owned(),
        "0".repeat(65),
        String::new(),
    ] {
        invalid(&edited(|v| {
            v["migration"]["sources"][0]["digest"] = json!(bad.clone())
        }));
    }
}

// ---- closed enums ----

#[test]
fn unknown_enum_values_are_refused() {
    for (path, value) in [
        ("/bindings/1/state", "gone"),
        ("/bindings/1/checkoutKind", "svn"),
        ("/sessions/1/state", "running"),
        ("/operations/0/state", "done"),
        ("/operations/0/kind", "stop"),
        ("/operations/0/provider", "gemini"),
        ("/operations/0/mode", "hybrid"),
        ("/operations/1/managedSafetyProfile", "full_access"),
        ("/workspaces/1/preferences/mode", "hybrid"),
        ("/workspaces/1/preferences/provider", "shell"),
        ("/workspaces/1/layout/dir", "diagonal"),
        ("/workspaces/1/layout/kind", "tab"),
        ("/sessions/1/target/kind", "remote"),
        ("/migration/state", "pending"),
        ("/migration/sources/0/kind", "sqlite"),
        ("/migration/map/0/sourceKind", "path"),
    ] {
        invalid(&edited(|v| *v.pointer_mut(path).unwrap() = json!(value)));
    }
    for protocol in [json!(0), json!(3), json!("1"), json!(1.5), json!(null)] {
        invalid(&edited(|v| {
            v["sessions"][2]["target"]["protocol"] = protocol.clone()
        }));
    }
}

// ---- Native versus Managed profile separation ----

fn launch(provider: &str, mode: &str, native: Value, safety: Value) -> Value {
    edited(|v| {
        v["operations"][4]["provider"] = json!(provider);
        v["operations"][4]["mode"] = json!(mode);
        v["operations"][4]["nativeProfile"] = native;
        v["operations"][4]["managedSafetyProfile"] = safety;
        v["operations"][4]["managedAuthority"] = if mode == "managed" {
            authority(1, 1)
        } else {
            Value::Null
        };
    })
}

#[test]
fn a_launch_keeps_the_native_profile_and_the_managed_safety_profile_apart() {
    let native = profile(NATIVE_A, "1");
    for (provider, mode, native_profile, safety) in [
        ("claude", "native", Value::Null, Value::Null),
        ("claude", "native", native.clone(), Value::Null),
        ("codex", "native", native.clone(), Value::Null),
        ("shell", "native", Value::Null, Value::Null),
        ("codex", "managed", Value::Null, json!("inference")),
        ("codex", "managed", Value::Null, json!("local_tools")),
        ("claude", "managed", Value::Null, json!("inference")),
    ] {
        accepted(&launch(provider, mode, native_profile, safety));
    }
    for (provider, mode, native_profile, safety) in [
        // Native launches never carry a Managed profile.
        ("claude", "native", Value::Null, json!("inference")),
        ("codex", "native", native.clone(), json!("local_tools")),
        // A Managed launch names a Managed profile and no Native one.
        ("codex", "managed", Value::Null, Value::Null),
        ("codex", "managed", native.clone(), json!("inference")),
        ("codex", "managed", native.clone(), Value::Null),
        // A shell is Native with neither.
        ("shell", "native", native.clone(), Value::Null),
        ("shell", "native", Value::Null, json!("inference")),
        ("shell", "managed", Value::Null, json!("inference")),
        ("shell", "managed", Value::Null, Value::Null),
    ] {
        invalid_at(
            &launch(provider, mode, native_profile, safety),
            "operations.profile",
        );
    }
}

#[test]
fn a_native_profile_reference_is_not_a_managed_profile_and_vice_versa() {
    invalid(&launch(
        "codex",
        "managed",
        Value::Null,
        profile(NATIVE_A, "1"),
    ));
    invalid(&launch("claude", "native", json!("inference"), Value::Null));
    invalid(&launch("claude", "native", json!(NATIVE_A), Value::Null));
}

#[test]
fn workspace_preferences_obey_the_same_separation_and_never_store_local_tools() {
    let preferences = |provider: Value, mode: &str, native: Value, safety: Value| {
        edited(|v| {
            v["workspaces"][1]["preferences"] = json!({
                "provider": provider, "mode": mode,
                "nativeProfile": native, "managedSafetyProfile": safety
            });
        })
    };
    let native = profile(NATIVE_A, "1");
    accepted(&preferences(
        json!("claude"),
        "native",
        native.clone(),
        Value::Null,
    ));
    accepted(&preferences(
        Value::Null,
        "native",
        Value::Null,
        Value::Null,
    ));
    accepted(&preferences(
        json!("codex"),
        "managed",
        Value::Null,
        json!("inference"),
    ));
    accepted(&preferences(
        json!("codex"),
        "managed",
        Value::Null,
        Value::Null,
    ));
    for (mode, native_profile, safety) in [
        ("native", Value::Null, json!("inference")),
        ("native", native.clone(), json!("local_tools")),
        ("managed", native.clone(), json!("inference")),
        ("managed", native.clone(), Value::Null),
        ("managed", Value::Null, json!("local_tools")),
    ] {
        invalid(&preferences(json!("codex"), mode, native_profile, safety));
    }
}

#[test]
fn local_tools_is_never_inferred_from_a_stored_request() {
    // An imported Managed association may carry no requested profile at all.
    let store = accepted(&full());
    let requested: Vec<_> = store
        .sessions
        .iter()
        .filter_map(|s| match &s.target {
            Target::Managed {
                requested_safety_profile,
                ..
            } => Some(*requested_safety_profile),
            _ => None,
        })
        .collect();
    assert_eq!(requested, [None, Some(ManagedSafetyProfile::LocalTools)]);
    invalid(&edited(|v| {
        v["sessions"][2]["target"]["requestedSafetyProfile"] = json!(profile(NATIVE_A, "1"))
    }));
    invalid(&edited(|v| {
        v["sessions"][2]["target"]["requestedSafetyProfile"] = json!("LOCAL_TOOLS")
    }));
    // The field is a request: a Native or shell target has no such field to carry.
    invalid(&edited(|v| {
        v["sessions"][1]["target"]["requestedSafetyProfile"] = json!("inference")
    }));
    invalid(&edited(|v| {
        v["sessions"][0]["target"]["nativeProfile"] = json!(profile(NATIVE_A, "1"))
    }));
}

// ---- references ----

#[test]
fn every_reference_must_exist_on_this_host() {
    let missing = id(0xee, 1);
    for (path, field) in [
        (
            "/projects/0/defaultWorkspaceId",
            "projects.defaultWorkspaceId",
        ),
        ("/projects/0/defaultBindingId", "projects.defaultBindingId"),
        ("/bindings/1/projectId", "projects.defaultBindingId"),
        ("/bindings/0/projectId", "bindings.projectId"),
        (
            "/bindings/1/previousBindingId",
            "bindings.previousBindingId",
        ),
        ("/workspaces/2/projectId", "workspaces.projectId"),
        (
            "/workspaces/1/leaves/0/associationId",
            "workspaces.leaves.associationId",
        ),
        ("/sessions/1/originProjectId", "sessions.originProjectId"),
        ("/sessions/1/bindingId", "sessions.bindingId"),
        ("/sessions/1/operationId", "sessions.operationId"),
        ("/operations/0/associationId", "sessions.operationId"),
        ("/operations/2/workspaceId", "operations.leafId"),
        ("/operations/2/leafId", "operations.leafId"),
    ] {
        invalid_at(
            &edited(|v| *v.pointer_mut(path).unwrap() = json!(missing)),
            field,
        );
    }
    invalid_at(
        &edited(|v| v["workspaces"][1]["focus"] = json!(missing)),
        "workspaces.focus",
    );
    invalid_at(
        &edited(|v| v["workspaces"][1]["layout"]["b"]["a"]["id"] = json!(missing)),
        "workspaces.leaves",
    );
}

#[test]
fn records_of_another_host_are_refused() {
    let other = id(1, 2);
    invalid_at(
        &edited(|v| v["bindings"][1]["hostId"] = json!(other)),
        "bindings.hostId",
    );
    invalid_at(
        &edited(|v| v["workspaces"][1]["hostId"] = json!(other)),
        "workspaces.hostId",
    );
    invalid_at(
        &edited(|v| v["sessions"][1]["hostId"] = json!(other)),
        "sessions.hostId",
    );
}

#[test]
fn ids_are_unique_within_each_kind() {
    for (list, field) in [
        ("projects", "projects"),
        ("bindings", "bindings"),
        ("workspaces", "workspaces"),
        ("sessions", "sessions"),
        ("operations", "operations"),
    ] {
        let value = edited(|v| {
            let first = v[list][0].clone();
            v[list].as_array_mut().unwrap().push(first);
        });
        invalid_at(&value, field);
    }
    invalid_at(
        &edited(|v| v["closedOperations"] = json!([id(0x70, 99), id(0x70, 99)])),
        "closedOperations",
    );
    // A compacted ID may never be live again.
    invalid_at(
        &edited(|v| v["closedOperations"] = json!([id(0x70, 1)])),
        "closedOperations",
    );
}

#[test]
fn an_owner_target_has_one_association() {
    let mut twin = full();
    let mut copy = twin["sessions"][1].clone();
    copy["id"] = json!(id(0x50, 77));
    copy["operationId"] = Value::Null;
    twin["sessions"].as_array_mut().unwrap().push(copy);
    invalid_at(&twin, "sessions.target");
    // The same Managed session ID under another protocol or stream is another session.
    let mut stream = full();
    let mut other = stream["sessions"][3].clone();
    other["id"] = json!(id(0x50, 78));
    other["operationId"] = Value::Null;
    other["target"]["storeStreamId"] = json!(id(0x80, 9));
    stream["sessions"].as_array_mut().unwrap().push(other);
    accepted(&stream);
    let mut protocol = full();
    let mut other = protocol["sessions"][3].clone();
    other["id"] = json!(id(0x50, 79));
    other["operationId"] = Value::Null;
    other["target"]["protocol"] = json!(1);
    protocol["sessions"].as_array_mut().unwrap().push(other);
    accepted(&protocol);
}

#[test]
fn binding_references_are_paired_and_stay_inside_their_project() {
    invalid_at(
        &edited(|v| v["sessions"][1]["bindingRevision"] = Value::Null),
        "sessions.bindingRevision",
    );
    invalid_at(
        &edited(|v| v["sessions"][1]["bindingId"] = Value::Null),
        "sessions.bindingRevision",
    );
    invalid_at(
        &edited(|v| v["operations"][0]["bindingRevision"] = Value::Null),
        "operations.bindingRevision",
    );
    // A bound association belongs to its binding's project.
    invalid_at(
        &edited(|v| v["sessions"][1]["originProjectId"] = Value::Null),
        "sessions.bindingId",
    );
    invalid_at(
        &edited(|v| v["sessions"][1]["originProjectId"] = json!(id(0x10, 2))),
        "sessions.bindingId",
    );
}

#[test]
fn bindings_chain_inside_one_project_without_a_cycle() {
    invalid_at(
        &edited(|v| v["bindings"][1]["previousBindingId"] = json!(id(0x20, 2))),
        "bindings.previousBindingId",
    );
    invalid_at(
        &edited(|v| v["bindings"][0]["previousBindingId"] = json!(id(0x20, 2))),
        "bindings.previousBindingId",
    );
    invalid_at(
        &edited(|v| v["projects"][0]["defaultBindingId"] = json!(id(0x20, 3))),
        "projects.defaultBindingId",
    );
}

// ---- Scratch, tombstones, layouts ----

#[test]
fn exactly_one_projectless_scratch_workspace_with_native_preferences() {
    let mut none = scratch_only();
    none["workspaces"] = json!([]);
    invalid_at(&none, "workspaces.scratch");
    let mut two = scratch_only();
    let mut second = two["workspaces"][0].clone();
    second["id"] = json!(id(0x30, 9));
    two["workspaces"].as_array_mut().unwrap().push(second);
    invalid_at(&two, "workspaces.scratch");
    invalid_at(
        &edited(|v| v["workspaces"][0]["label"] = json!("Elsewhere")),
        "workspaces.scratch",
    );
    invalid_at(
        &edited(|v| v["workspaces"][0]["preferences"]["mode"] = json!("managed")),
        "workspaces.scratch",
    );
}

#[test]
fn live_projects_have_a_default_workspace_of_their_own() {
    invalid_at(
        &edited(|v| v["projects"][0]["defaultWorkspaceId"] = Value::Null),
        "projects.defaultWorkspaceId",
    );
    invalid_at(
        &edited(|v| v["projects"][0]["defaultWorkspaceId"] = json!(id(0x30, 1))),
        "projects.defaultWorkspaceId",
    );
    let mut orphan = full();
    orphan["workspaces"].as_array_mut().unwrap().push(workspace(
        8,
        json!(id(0x10, 9)),
        "Orphan",
        Value::Null,
        json!([]),
    ));
    invalid_at(&orphan, "workspaces.projectId");
}

#[test]
fn a_tombstone_keeps_history_and_nothing_that_could_act() {
    accepted(&full());
    invalid_at(
        &edited(|v| v["projects"][1]["defaultWorkspaceId"] = json!(id(0x30, 2))),
        "projects.tombstone",
    );
    invalid_at(
        &edited(|v| v["projects"][1]["defaultBindingId"] = json!(id(0x20, 2))),
        "projects.tombstone",
    );
    let mut with_binding = full();
    with_binding["bindings"]
        .as_array_mut()
        .unwrap()
        .push(binding(9, &id(0x10, 2), Value::Null, "valid"));
    invalid_at(&with_binding, "projects.tombstone");
    let mut with_workspace = full();
    with_workspace["workspaces"]
        .as_array_mut()
        .unwrap()
        .push(workspace(
            9,
            json!(id(0x10, 2)),
            "Dead",
            Value::Null,
            json!([]),
        ));
    invalid_at(&with_workspace, "projects.tombstone");
    // History may still name the tombstoned project.
    let mut history = full();
    let mut retained = history["sessions"][0].clone();
    retained["id"] = json!(id(0x50, 61));
    retained["target"]["terminalId"] = json!(id(0x60, 61));
    retained["originProjectId"] = json!(id(0x10, 2));
    retained["state"] = json!("unavailable");
    history["sessions"].as_array_mut().unwrap().push(retained);
    accepted(&history);
}

#[test]
fn layouts_are_trees_of_the_workspaces_own_leaves() {
    invalid_at(
        &edited(|v| drop(v["workspaces"][1]["leaves"].as_array_mut().unwrap().pop())),
        "workspaces.leaves",
    );
    invalid_at(
        &edited(|v| v["workspaces"][1]["layout"] = Value::Null),
        "workspaces.leaves",
    );
    invalid_at(
        &edited(|v| v["workspaces"][1]["layout"]["b"]["b"] = leaf_node(3)),
        "workspaces.leaves",
    );
    invalid_at(
        &edited(|v| {
            let first = v["workspaces"][1]["leaves"][0].clone();
            v["workspaces"][1]["leaves"]
                .as_array_mut()
                .unwrap()
                .push(first);
        }),
        "workspaces.leaves",
    );
    invalid_at(
        &edited(|v| v["workspaces"][1]["leaves"][1]["associationId"] = json!(id(0x50, 2))),
        "workspaces.leaves.associationId",
    );
    // Two views of one session in different workspaces are mirrors, allowed.
    accepted(&edited(|v| {
        v["workspaces"][2]["leaves"][0]["associationId"] = json!(id(0x50, 2))
    }));
    accepted(&edited(|v| {
        v["workspaces"][1]["focus"] = json!(id(0x40, 4))
    }));
    invalid_at(
        &edited(|v| v["workspaces"][1]["focus"] = json!(id(0x40, 1))),
        "workspaces.focus",
    );
}

#[test]
fn split_ratios_stay_inside_their_bounds() {
    for good in [0.1, 0.5, 0.9] {
        accepted(&edited(|v| {
            v["workspaces"][1]["layout"]["ratio"] = json!(good)
        }));
    }
    for bad in [0.0, 0.09, 0.0999, 0.9001, 0.91, 1.0, -0.5, 7.0] {
        invalid_at(
            &edited(|v| v["workspaces"][1]["layout"]["ratio"] = json!(bad)),
            "layout.ratio",
        );
    }
    invalid(&edited(|v| {
        v["workspaces"][1]["layout"]["ratio"] = json!("0.5")
    }));
    invalid(&edited(|v| {
        v["workspaces"][1]["layout"]["ratio"] = json!(null)
    }));
}

#[test]
fn a_non_finite_ratio_is_refused_when_a_store_is_built_by_hand() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut store = accepted(&full());
        let Some(Node::Split { ratio, .. }) = store.workspaces[1].layout.as_mut() else {
            panic!("the fixture layout is a split");
        };
        *ratio = bad;
        assert_eq!(store.validate().unwrap_err().field, "layout.ratio");
        assert!(encode(&store).is_err());
    }
}

// ---- operation receipts ----

#[test]
fn receipt_state_decides_its_claim_and_association() {
    // `pending` has a claim and no association; only `committed` has an association.
    invalid_at(
        &edited(|v| v["operations"][2]["claim"] = Value::Null),
        "operations.claim",
    );
    invalid_at(
        &edited(|v| v["operations"][3]["claim"] = json!("claim-2")),
        "operations.claim",
    );
    invalid_at(
        &edited(|v| v["operations"][0]["claim"] = json!("claim-2")),
        "operations.claim",
    );
    invalid_at(
        &edited(|v| v["operations"][0]["associationId"] = Value::Null),
        "sessions.operationId",
    );
    invalid_at(
        &edited(|v| v["operations"][3]["associationId"] = json!(id(0x50, 2))),
        "operations.associationId",
    );
    invalid_at(
        &edited(|v| v["operations"][4]["associationId"] = json!(id(0x50, 2))),
        "operations.associationId",
    );
}

#[test]
fn a_committed_receipt_and_its_association_must_agree() {
    invalid_at(
        &edited(|v| v["sessions"][1]["operationId"] = json!(id(0x70, 2))),
        "sessions.operationId",
    );
    invalid_at(
        &edited(|v| v["sessions"][1]["operationId"] = Value::Null),
        "operations.associationId",
    );
    // A compacted receipt leaves only its ID behind, and the association may still cite it.
    let mut compacted = full();
    compacted["operations"].as_array_mut().unwrap().remove(0);
    compacted["closedOperations"] = json!([id(0x70, 99), id(0x70, 1)]);
    accepted(&compacted);
    compacted["closedOperations"] = json!([id(0x70, 99)]);
    invalid_at(&compacted, "sessions.operationId");
}

#[test]
fn a_managed_receipt_names_the_authority_it_was_asked_of_and_only_a_managed_one_does() {
    // Operation 0 is Native, 1 and 2 are Managed (1 committed with its association).
    invalid_at(
        &edited(|v| v["operations"][0]["managedAuthority"] = json!(authority(1, 1))),
        "operations.managedAuthority",
    );
    invalid_at(
        &edited(|v| v["operations"][2]["managedAuthority"] = Value::Null),
        "operations.managedAuthority",
    );
    // The omitted field is a malformed record, as for every optional value.
    let error = refusal(&edited(|v| {
        v["operations"][1]
            .as_object_mut()
            .unwrap()
            .remove("managedAuthority");
    }));
    assert_eq!(error.code, Code::InvalidRecord, "{error}");
    // A committed receipt and its association are of the same authority: protocol and stream.
    invalid_at(
        &edited(|v| v["operations"][1]["managedAuthority"] = authority(1, 2)),
        "operations.target",
    );
    invalid_at(
        &edited(|v| v["operations"][1]["managedAuthority"] = authority(2, 1)),
        "operations.target",
    );
    // Only protocols 1 and 2 exist, and the stream is a UUID.
    for bad in [
        json!({"protocol": 3, "storeStreamId": id(0x80, 2)}),
        json!({"protocol": 2, "storeStreamId": "not-a-uuid"}),
        json!({"protocol": 2, "storeStreamId": id(0x80, 2), "path": "/x"}),
    ] {
        let error = refusal(&edited(|v| {
            v["operations"][1]["managedAuthority"] = bad.clone()
        }));
        assert_eq!(error.code, Code::InvalidRecord, "{bad}: {error}");
    }
}

#[test]
fn a_leaf_has_at_most_one_unfinished_receipt() {
    let mut busy = full();
    let mut second = busy["operations"][2].clone();
    second["id"] = json!(id(0x70, 50));
    busy["operations"].as_array_mut().unwrap().push(second);
    invalid_at(&busy, "operations.leafId");
    let mut other_leaf = full();
    let mut second = other_leaf["operations"][2].clone();
    second["id"] = json!(id(0x70, 50));
    second["leafId"] = json!(id(0x40, 3));
    second["workspaceId"] = json!(id(0x30, 2));
    other_leaf["operations"]
        .as_array_mut()
        .unwrap()
        .push(second);
    accepted(&other_leaf);
    // Finished receipts do not hold their leaf.
    let mut finished = full();
    let mut second = finished["operations"][4].clone();
    second["id"] = json!(id(0x70, 51));
    finished["operations"].as_array_mut().unwrap().push(second);
    accepted(&finished);
}

// ---- migration ----

#[test]
fn migration_records_are_unique_by_source_and_may_coalesce_targets() {
    // The fixture already maps two project sources to one project.
    accepted(&full());
    invalid_at(
        &edited(|v| v["migration"]["map"][1]["sourceKey"] = json!(digest(3))),
        "migration.map",
    );
    accepted(&edited(|v| {
        v["migration"]["map"][1]["sourceKind"] = json!("registry_session")
    }));
    invalid_at(
        &edited(|v| v["migration"]["sources"][1]["kind"] = json!("registry")),
        "migration.sources",
    );
    invalid_at(
        &edited(|v| v["migration"]["sources"] = json!([])),
        "migration.sources",
    );
    invalid(&edited(|v| v["migration"]["version"] = json!(2)));
    invalid(&edited(|v| {
        v["migration"]["map"][0]["targetId"] = json!(NATIVE_A)
    }));
}

// ---- ceilings: exact at the bound, refused one past it ----

#[test]
fn the_byte_ceiling_is_checked_before_any_parsing() {
    let canonical = encode(&accepted(&scratch_only())).unwrap();
    let mut at_bound = canonical.clone();
    at_bound.resize(MAX_STORE_BYTES, b' ');
    accepted_bytes(&at_bound);
    let mut over = canonical;
    over.resize(MAX_STORE_BYTES + 1, b' ');
    assert_eq!(decode(&over).unwrap_err().code, Code::CapacityExceeded);
    // Not JSON at all: the size is what is reported, so nothing was parsed.
    let garbage = vec![0xffu8; MAX_STORE_BYTES + 1];
    let error = decode(&garbage).unwrap_err();
    assert_eq!((error.code, error.field), (Code::CapacityExceeded, "store"));
    assert_eq!(
        decode(&vec![0xffu8; MAX_STORE_BYTES]).unwrap_err().code,
        Code::InvalidRecord
    );
}

#[track_caller]
fn accepted_bytes(input: &[u8]) {
    decode(input).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn project_ceiling() {
    accepted(&with_projects(MAX_PROJECTS as u32));
    capacity_at(&with_projects(MAX_PROJECTS as u32 + 1), "projects");
}

#[test]
fn workspaces_per_project_ceiling() {
    let build = |count: u32| {
        let mut store = with_projects(1);
        for n in 1..count {
            store["workspaces"].as_array_mut().unwrap().push(workspace(
                200 + n,
                json!(id(0x10, 1)),
                "More",
                Value::Null,
                json!([]),
            ));
        }
        store
    };
    accepted(&build(MAX_WORKSPACES_PER_PROJECT as u32));
    capacity_at(&build(MAX_WORKSPACES_PER_PROJECT as u32 + 1), "workspaces");
}

#[test]
fn bindings_per_project_ceiling_counts_relink_history() {
    let build = |count: u32| {
        let mut store = with_projects(1);
        let mut previous = Value::Null;
        for n in 1..=count {
            let state = if n == count { "valid" } else { "retired" };
            store["bindings"].as_array_mut().unwrap().push(binding(
                n,
                &id(0x10, 1),
                previous,
                state,
            ));
            previous = json!(id(0x20, n));
        }
        store
    };
    accepted(&build(MAX_BINDINGS_PER_PROJECT as u32));
    capacity_at(&build(MAX_BINDINGS_PER_PROJECT as u32 + 1), "bindings");
}

#[test]
fn association_ceiling() {
    let build = |count: u32| {
        let mut store = scratch_only();
        for n in 1..=count {
            store["sessions"]
                .as_array_mut()
                .unwrap()
                .push(shell_association(n));
        }
        store
    };
    let at_bound = build(MAX_ASSOCIATIONS as u32);
    accepted(&at_bound);
    assert!(bytes(&at_bound).len() < MAX_STORE_BYTES);
    capacity_at(&build(MAX_ASSOCIATIONS as u32 + 1), "sessions");
}

#[test]
fn operation_receipt_ceilings() {
    let build = |count: u32| {
        let mut store = scratch_only();
        for n in 1..=count {
            store["operations"].as_array_mut().unwrap().push(operation(
                n,
                1,
                1,
                "claude",
                "native",
                Value::Null,
                Value::Null,
                "refused",
                Value::Null,
                Value::Null,
            ));
        }
        store
    };
    accepted(&build(MAX_OPERATIONS as u32));
    capacity_at(&build(MAX_OPERATIONS as u32 + 1), "operations");

    let closed = |count: u32| {
        let mut store = scratch_only();
        store["closedOperations"] = (1..=count).map(|n| json!(id(0x71, n))).collect();
        store
    };
    accepted(&closed(MAX_CLOSED_OPERATIONS as u32));
    capacity_at(
        &closed(MAX_CLOSED_OPERATIONS as u32 + 1),
        "closedOperations",
    );
}

#[test]
fn migration_mapping_ceiling() {
    let build = |count: u32| {
        let mut store = scratch_only();
        let rows: Vec<Value> = (1..=count)
            .map(|n| json!({"sourceKind": "layout_leaf", "sourceKey": digest(n), "targetId": id(0x40, n)}))
            .collect();
        store["migration"] = json!({
            "version": 1, "id": id(0xa0, 1),
            "sources": [{"kind": "layout", "digest": digest(1)}],
            "map": rows, "state": "committed"
        });
        store
    };
    accepted(&build(MAX_MIGRATION_ROWS as u32));
    capacity_at(&build(MAX_MIGRATION_ROWS as u32 + 1), "migration");
}

#[test]
fn leaf_ceiling() {
    let build = |count: u32| {
        let mut store = scratch_only();
        let (layout, leaves) = balanced(1, count);
        store["workspaces"][0]["layout"] = layout;
        store["workspaces"][0]["leaves"] = json!(leaves);
        store
    };
    accepted(&build(MAX_LEAVES as u32));
    capacity_at(&build(MAX_LEAVES as u32 + 1), "leaves");
}

#[test]
fn layout_depth_ceiling_with_the_root_at_level_zero() {
    let build = |depth: u32| {
        let mut store = scratch_only();
        let (layout, leaves) = chain(depth);
        store["workspaces"][0]["layout"] = layout;
        store["workspaces"][0]["leaves"] = json!(leaves);
        store
    };
    accepted(&build(6));
    invalid_at(&build(7), "layout.depth");
    // Far deeper than serde will follow: refused, not a stack overflow.
    invalid(&build(300));
}

#[test]
fn a_hand_built_store_over_a_ceiling_is_refused_on_encode() {
    let mut store = accepted(&with_projects(1));
    let template = store.projects[0].clone();
    store.projects = vec![template; MAX_PROJECTS + 1];
    assert!(encode(&store).is_err());
}

// ---- review findings: placement, references, round trip, redaction ----

#[test]
fn a_leaf_is_placed_exactly_once() {
    let mut twice = scratch_only();
    twice["workspaces"][0]["layout"] = split(leaf_node(1), leaf_node(1));
    twice["workspaces"][0]["leaves"] = json!([leaf(1, Value::Null)]);
    invalid_at(&twice, "workspaces.leaves");
    // And one leaf ID never names two placements anywhere in the store.
    invalid_at(
        &edited(|v| {
            v["workspaces"][2]["layout"] = leaf_node(3);
            v["workspaces"][2]["leaves"] = json!([leaf(3, Value::Null)]);
        }),
        "workspaces.leaves",
    );
}

#[test]
fn an_unfinished_receipt_names_a_context_that_exists_and_agrees() {
    invalid_at(
        &edited(|v| v["operations"][2]["projectId"] = json!(id(0x10, 9))),
        "operations.leafId",
    );
    invalid_at(
        &edited(|v| v["operations"][2]["projectId"] = Value::Null),
        "operations.leafId",
    );
    invalid_at(
        &edited(|v| v["operations"][2]["bindingId"] = json!(id(0x20, 9))),
        "operations.bindingId",
    );
    // Finished and ambiguous receipts are history: they may outlive the leaf or binding they cite.
    accepted(&edited(|v| {
        v["operations"][3]["leafId"] = json!(id(0x40, 99))
    }));
    accepted(&edited(|v| {
        v["operations"][4]["workspaceId"] = json!(id(0x30, 99))
    }));
    accepted(&edited(|v| {
        v["operations"][4]["bindingId"] = json!(id(0x20, 99))
    }));
}

#[test]
fn an_ambiguous_receipt_may_keep_the_association_a_lost_acknowledgement_left() {
    accepted(&edited(|v| {
        v["operations"][3]["provider"] = json!("shell");
        v["operations"][3]["associationId"] = json!(id(0x50, 1));
        v["sessions"][0]["operationId"] = json!(id(0x70, 4));
    }));
    invalid_at(
        &edited(|v| v["operations"][2]["associationId"] = json!(id(0x50, 1))),
        "operations.associationId",
    );
}

#[test]
fn a_receipt_and_the_session_it_produced_agree_on_kind_and_requested_profile() {
    invalid_at(
        &edited(|v| {
            v["sessions"][1]["target"] = json!({"kind": "managed", "protocol": 2,
                "storeStreamId": id(0x80, 5), "sessionId": id(0x90, 5), "requestedSafetyProfile": "local_tools"});
        }),
        "operations.target",
    );
    invalid_at(
        &edited(|v| v["sessions"][3]["target"]["requestedSafetyProfile"] = json!("inference")),
        "operations.target",
    );
    invalid_at(
        &edited(|v| v["sessions"][3]["target"]["requestedSafetyProfile"] = Value::Null),
        "operations.target",
    );
}

#[test]
fn an_owner_target_is_one_session_even_across_kinds() {
    let mut same_native = full();
    let mut copy = same_native["sessions"][1].clone();
    copy["id"] = json!(id(0x50, 70));
    copy["operationId"] = Value::Null;
    copy["target"]["terminalId"] = json!(id(0x60, 70));
    same_native["sessions"].as_array_mut().unwrap().push(copy);
    invalid_at(&same_native, "sessions.target");
    let mut shell_over_native = full();
    let mut copy = shell_over_native["sessions"][0].clone();
    copy["id"] = json!(id(0x50, 71));
    copy["target"]["terminalId"] = json!(id(0x60, 2));
    shell_over_native["sessions"]
        .as_array_mut()
        .unwrap()
        .push(copy);
    invalid_at(&shell_over_native, "sessions.target");
}

#[test]
fn every_representable_split_ratio_survives_a_round_trip() {
    // The store is built from a typed f64, as a layout the application produced would be.
    let base = accepted(&full());
    let round_trip = |ratio: f64| {
        let mut store = base.clone();
        let Some(Node::Split { ratio: slot, .. }) = store.workspaces[1].layout.as_mut() else {
            panic!("the fixture layout is a split");
        };
        *slot = ratio;
        let bytes = encode(&store).unwrap();
        assert_eq!(decode(&bytes).unwrap(), store, "{ratio:e}");
        assert_eq!(
            encode(&decode(&bytes).unwrap()).unwrap(),
            bytes,
            "{ratio:e}"
        );
    };
    // Found by the independent review: serde_json's default float parser is off by one ulp.
    for ratio in [
        0.24036974322645302,
        0.36792573792456285,
        0.48368605141586085,
    ] {
        round_trip(ratio);
    }
    let mut state = 0x2545_f491_4f6c_dd1du64;
    for _ in 0..2_000 {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        round_trip(0.1 + 0.8 * ((state >> 11) as f64 / (1u64 << 53) as f64));
    }
}

#[test]
fn a_revision_that_cannot_advance_is_not_encoded() {
    let mut store = accepted(&full());
    store.revision = Counter::new(u64::MAX).unwrap();
    assert_eq!(encode(&store).unwrap_err().field, "revision");
}

#[test]
fn debug_output_never_prints_a_private_path() {
    let store = accepted(&full());
    let printed = format!("{store:?}");
    assert!(
        !printed.contains("/home/fixture"),
        "a path leaked into Debug output"
    );
    assert!(printed.contains("AbsolutePath(<private>)"));
}

#[test]
fn a_workspace_shows_only_its_own_projects_sessions() {
    // Scratch never shows a project's session, and a project never shows an unassigned one.
    invalid_at(
        &edited(|v| v["workspaces"][0]["leaves"][1]["associationId"] = json!(id(0x50, 2))),
        "workspaces.leaves.associationId",
    );
    invalid_at(
        &edited(|v| v["workspaces"][2]["leaves"][0]["associationId"] = json!(id(0x50, 1))),
        "workspaces.leaves.associationId",
    );
    // Nor another project's session, live or tombstoned.
    invalid_at(
        &edited(|v| {
            v["sessions"][1]["originProjectId"] = json!(id(0x10, 2));
            v["sessions"][1]["bindingId"] = Value::Null;
            v["sessions"][1]["bindingRevision"] = Value::Null;
        }),
        "workspaces.leaves.associationId",
    );
    // The same session in two workspaces of its own project is a mirror, and an unassigned or
    // unavailable session no workspace shows is simply discoverable, with no invented owner.
    accepted(&edited(|v| {
        v["workspaces"][2]["leaves"][0]["associationId"] = json!(id(0x50, 2))
    }));
    accepted(&edited(|v| {
        v["sessions"][0]["state"] = json!("unavailable");
        v["workspaces"][0]["leaves"][0]["associationId"] = Value::Null;
    }));
    let mut orphan = full();
    let mut history = orphan["sessions"][0].clone();
    history["id"] = json!(id(0x50, 60));
    history["target"]["terminalId"] = json!(id(0x60, 60));
    history["state"] = json!("unavailable");
    orphan["sessions"].as_array_mut().unwrap().push(history);
    accepted(&orphan);
}
