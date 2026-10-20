use super::{
    Sequence,
    event_envelope::{EventEnvelopeV1, Persistence},
};
use serde_json::Value;

fn corpus() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/event-envelope-v1.json"
    ))
    .unwrap()
}

#[test]
fn shared_valid_fixtures_validate_and_round_trip() {
    for case in corpus()["valid"].as_array().unwrap() {
        let original = case["event"].clone();
        let event = EventEnvelopeV1::from_value(original.clone())
            .unwrap_or_else(|error| panic!("{}: {error}", case["name"]));
        event.validate().unwrap();
        let serialized = serde_json::to_value(&event).unwrap();
        assert_eq!(serialized, original, "{}", case["name"]);
        let restored: EventEnvelopeV1 = serde_json::from_value(serialized.clone()).unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap(), serialized);
        assert!(serialized["sequence"].is_string());
    }
}

#[test]
fn shared_invalid_fixtures_reject_with_matching_diagnostics() {
    for case in corpus()["invalid"].as_array().unwrap() {
        let error = EventEnvelopeV1::from_value(case["event"].clone()).unwrap_err();
        assert_eq!(
            error.field,
            case["error"]["field"].as_str().unwrap(),
            "{}",
            case["name"]
        );
        assert_eq!(
            error.code.as_str(),
            case["error"]["code"].as_str().unwrap(),
            "{}",
            case["name"]
        );
        assert!(
            serde_json::from_value::<EventEnvelopeV1>(case["event"].clone()).is_err(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn maximum_sequence_and_ephemeral_default() {
    let maximum: Sequence = "18446744073709551615".parse().unwrap();
    assert_eq!(maximum.get(), u64::MAX);
    assert_eq!(
        serde_json::to_value(maximum).unwrap(),
        "18446744073709551615"
    );
    assert!(serde_json::from_value::<Sequence>(serde_json::json!(42)).is_err());
    assert_eq!(Persistence::default(), Persistence::Ephemeral);
    let event = EventEnvelopeV1::from_value(corpus()["valid"][0]["event"].clone()).unwrap();
    assert_eq!(event.sequence().get(), 1);
    assert_eq!(event.persistence(), Persistence::Ephemeral);
}

fn approval(change: impl FnOnce(&mut Value)) -> Result<(), ()> {
    use super::control::ManagedApprovalRecord;
    let mut v = serde_json::json!({
        "managed_approval_id": "11111111-1111-4111-8111-111111111111",
        "managed_turn_id": "22222222-2222-4222-8222-222222222222",
        "managed_session_id": "33333333-3333-4333-8333-333333333333",
        "runtime_instance_id": "44444444-4444-4444-8444-444444444444",
        "kind": "command_execution",
        "provider_item_id": "call_1",
        "admitted_lease_id": "55555555-5555-4555-8555-555555555555",
        "admitted_lease_revision": 1,
        "state": "pending",
        "revision": 1,
        "action_fingerprint": "a".repeat(64),
        "structure": {"command_bytes": 4, "cwd_bytes": 2, "parsed_actions": 0, "files": 0, "diff_bytes": 0},
        "decision": null,
        "effect": "none",
        "created_at": "2026-10-07T00:00:00Z",
        "updated_at": "2026-10-07T00:00:00Z",
        "expires_at": "2026-10-07T00:05:00Z",
        "terminal_at": null,
        "terminal_reason": null,
        "created_sequence": "7"
    });
    change(&mut v);
    serde_json::from_value::<ManagedApprovalRecord>(v)
        .map_err(|_| ())?
        .validate()
        .map_err(|_| ())
}
fn decided(v: &mut Value, state: &str, decision: Value, effect: &str) {
    v["state"] = state.into();
    v["decision"] = decision;
    v["effect"] = effect.into();
    v["revision"] = 2.into();
    v["terminal_at"] = "2026-10-07T00:00:00Z".into();
    v["terminal_reason"] = "client_decision".into();
}

#[test]
fn approval_record_admits_only_coherent_state_decision_effect_and_bounds() {
    approval(|_| ()).unwrap();
    let ok: [(&str, Value, &str); 8] = [
        ("accepted", "accept".into(), "dispatched"),
        ("accepted", "accept".into(), "confirmed"),
        ("indeterminate", "accept".into(), "unknown"),
        ("declined", "decline".into(), "none"),
        ("cancelled", "cancel".into(), "none"),
        ("expired", Value::Null, "none"),
        ("invalidated", Value::Null, "none"),
        ("accepted", "accept".into(), "dispatched"),
    ];
    for (state, decision, effect) in ok {
        approval(|v| decided(v, state, decision.clone(), effect))
            .unwrap_or_else(|()| panic!("{state}/{decision}/{effect}"));
    }
    let bad: [(&str, Value, &str); 10] = [
        ("accepted", "accept".into(), "none"),
        ("accepted", "accept".into(), "unknown"),
        ("accepted", "decline".into(), "dispatched"),
        ("accepted", Value::Null, "dispatched"),
        ("indeterminate", Value::Null, "unknown"),
        ("indeterminate", "accept".into(), "dispatched"),
        ("declined", "decline".into(), "dispatched"),
        ("cancelled", "decline".into(), "none"),
        ("expired", "accept".into(), "none"),
        ("invalidated", Value::Null, "confirmed"),
    ];
    for (state, decision, effect) in bad {
        assert!(
            approval(|v| decided(v, state, decision.clone(), effect)).is_err(),
            "{state}/{decision}/{effect}"
        );
    }
    let changes: [&dyn Fn(&mut Value); 22] = [
        &|v| v["command"] = "printf".into(),
        &|v| v["state"] = "approved".into(),
        &|v| v["kind"] = "network_access".into(),
        &|v| v["decision"] = "accept".into(),
        &|v| v["decision"] = "accept_for_session".into(),
        &|v| v["effect"] = "dispatched".into(),
        &|v| v["terminal_at"] = "2026-10-07T00:00:00Z".into(),
        &|v| v["terminal_reason"] = "approval_timeout".into(),
        &|v| v["revision"] = 0.into(),
        &|v| v["admitted_lease_revision"] = 0.into(),
        &|v| v["action_fingerprint"] = "A".repeat(64).into(),
        &|v| v["action_fingerprint"] = "a".repeat(63).into(),
        &|v| v["provider_item_id"] = "".into(),
        &|v| v["provider_item_id"] = "call 1".into(),
        &|v| v["provider_item_id"] = "c".repeat(129).into(),
        &|v| v["expires_at"] = "2026-10-07T00:05:01Z".into(),
        &|v| v["expires_at"] = "2026-10-07T00:00:00Z".into(),
        &|v| v["updated_at"] = "2026-10-06T23:59:59Z".into(),
        &|v| v["managed_turn_id"] = v["managed_approval_id"].clone(),
        &|v| v["structure"]["files"] = 1.into(),
        &|v| v["structure"]["command_bytes"] = 0.into(),
        &|v| v["structure"]["command_bytes"] = (16 * 1024 + 1).into(),
    ];
    for (n, change) in changes.iter().enumerate() {
        assert!(approval(change).is_err(), "change {n}");
    }
    // A file approval exists only for a complete, bounded proposal.
    let file = |files: u64, diff: u64| {
        approval(|v| {
            v["kind"] = "file_change".into();
            v["structure"] = serde_json::json!({"command_bytes": 0, "cwd_bytes": 0, "parsed_actions": 0, "files": files, "diff_bytes": diff});
        })
    };
    file(1, 0).unwrap();
    file(64, 256 * 1024).unwrap();
    assert!(file(0, 0).is_err() && file(65, 0).is_err() && file(1, 256 * 1024 + 1).is_err());
    // The terminal instant never lies after the last update.
    assert!(
        approval(|v| {
            decided(v, "declined", "decline".into(), "none");
            v["terminal_at"] = "2026-10-07T00:00:01Z".into();
        })
        .is_err()
    );
}

#[test]
fn approval_decisions_are_exactly_three_and_presentation_types_hide_their_content() {
    use super::control::{
        ApprovalDecision, ApprovalRespond, ApprovalState, ApprovalStatus, CommandPresentation,
        ManagedApprovalRecord,
    };
    for (text, state) in [
        ("accept", ApprovalState::Accepted),
        ("decline", ApprovalState::Declined),
        ("cancel", ApprovalState::Cancelled),
    ] {
        let d: ApprovalDecision = serde_json::from_value(text.into()).unwrap();
        assert_eq!(d.state(), state);
    }
    for forbidden in [
        "accept_for_session",
        "acceptForSession",
        "accept_with_execpolicy_amendment",
        "acceptWithExecpolicyAmendment",
        "apply_network_policy_amendment",
        "Accept",
    ] {
        assert!(serde_json::from_value::<ApprovalDecision>(forbidden.into()).is_err());
        assert!(
            serde_json::from_value::<ApprovalRespond>(serde_json::json!({
                "intent_id": "11111111-1111-4111-8111-111111111111",
                "managed_approval_id": "22222222-2222-4222-8222-222222222222",
                "expected_revision": 1,
                "decision": forbidden
            }))
            .is_err()
        );
    }
    assert!(
        serde_json::from_value::<ApprovalRespond>(serde_json::json!({
            "intent_id": "11111111-1111-4111-8111-111111111111",
            "managed_approval_id": "22222222-2222-4222-8222-222222222222",
            "expected_revision": 1,
            "decision": "accept",
            "amendment": ["rm"]
        }))
        .is_err()
    );
    let record: ManagedApprovalRecord = serde_json::from_value(serde_json::json!({
        "managed_approval_id": "11111111-1111-4111-8111-111111111111",
        "managed_turn_id": "22222222-2222-4222-8222-222222222222",
        "managed_session_id": "33333333-3333-4333-8333-333333333333",
        "runtime_instance_id": "44444444-4444-4444-8444-444444444444",
        "kind": "command_execution",
        "provider_item_id": "call_1",
        "admitted_lease_id": "55555555-5555-4555-8555-555555555555",
        "admitted_lease_revision": 1,
        "state": "pending",
        "revision": 1,
        "action_fingerprint": "a".repeat(64),
        "structure": {"command_bytes": 4, "cwd_bytes": 2, "parsed_actions": 0, "files": 0, "diff_bytes": 0},
        "decision": null,
        "effect": "none",
        "created_at": "2026-10-07T00:00:00Z",
        "updated_at": "2026-10-07T00:00:00Z",
        "expires_at": "2026-10-07T00:05:00Z",
        "terminal_at": null,
        "terminal_reason": null,
        "created_sequence": "7"
    }))
    .unwrap();
    let status = ApprovalStatus {
        approval: record,
        acceptable: true,
        command: Some(CommandPresentation {
            command: "printf SECRET-COMMAND".into(),
            cwd: "/w".into(),
            parsed_actions: vec![],
        }),
        file_count: None,
        file: None,
    };
    status.validate().unwrap();
    assert_eq!(format!("{status:?}"), "ApprovalStatus(<sensitive>)");
    let mut wrong_kind = status.clone();
    wrong_kind.file_count = Some(1);
    assert!(wrong_kind.validate().is_err());
    let mut lost = status.clone();
    lost.command = None;
    assert!(
        lost.validate().is_err(),
        "acceptable requires the transient action"
    );
    lost.acceptable = false;
    lost.validate().unwrap();
}

#[test]
fn runtime_start_selects_a_profile_without_changing_the_stored_encoding_of_older_intents() {
    use super::control::{Operation, RuntimeMutation, RuntimeSafetyProfile, RuntimeStatus};
    let base = serde_json::json!({
        "intent_id": "11111111-1111-4111-8111-111111111111",
        "adapter_id": "codex",
        "runtime_kind": "codex",
        "expected_revision": null
    });
    // A receipt journaled before profiles existed still parses and re-encodes byte for byte.
    let plain: RuntimeMutation = serde_json::from_value(base.clone()).unwrap();
    assert_eq!(
        (plain.safety_profile, plain.workspace_root.clone()),
        (None, None)
    );
    assert_eq!(serde_json::to_value(&plain).unwrap(), base);
    let with = |profile: Value, workspace: Value| -> RuntimeMutation {
        let mut v = base.clone();
        if !profile.is_null() {
            v["safety_profile"] = profile;
        }
        if !workspace.is_null() {
            v["workspace_root"] = workspace;
        }
        serde_json::from_value(v).unwrap()
    };
    let local = with("codex_local_tools_v1".into(), "/work/repo".into());
    assert_eq!(
        local.safety_profile,
        Some(RuntimeSafetyProfile::CodexLocalToolsV1)
    );
    Operation::RuntimeStart(local.clone()).validate().unwrap();
    Operation::RuntimeStart(plain.clone()).validate().unwrap();
    Operation::RuntimeStart(with("codex_inference_v1".into(), Value::Null))
        .validate()
        .unwrap();
    Operation::RuntimeStop(plain).validate().unwrap();
    for refused in [
        Operation::RuntimeStart(with("codex_local_tools_v1".into(), Value::Null)),
        Operation::RuntimeStart(with(Value::Null, "/work/repo".into())),
        Operation::RuntimeStart(with("codex_inference_v1".into(), "/work/repo".into())),
        Operation::RuntimeStart(with("codex_local_tools_v1".into(), "relative".into())),
        Operation::RuntimeStart(with("codex_local_tools_v1".into(), "/w\n".into())),
        Operation::RuntimeStop(local.clone()),
        Operation::RuntimeStop(with("codex_inference_v1".into(), Value::Null)),
    ] {
        assert!(refused.validate().is_err(), "{refused:?}");
    }
    let mut unknown = base.clone();
    unknown["safety_profile"] = "codex_full_access".into();
    assert!(serde_json::from_value::<RuntimeMutation>(unknown).is_err());
    let mut extra = base.clone();
    extra["argv"] = serde_json::json!(["--dangerously-bypass"]);
    assert!(serde_json::from_value::<RuntimeMutation>(extra).is_err());
    // A status names a workspace exactly when it reports the isolated profile.
    let status = |profile: Value, workspace: Value| -> RuntimeStatus {
        let mut v = serde_json::json!({
            "adapter_id": "codex", "runtime_kind": "codex", "desired_state": "running",
            "revision": 1, "actual_state": "ready", "runtime_instance_id": null,
            "runtime_version": "0.159.0", "compatibility": "compatible", "model_count": 1,
            "diagnostic": null,
            "outbound_methods": {"initialize": 1, "initialized": 1, "model_list": 1},
            "runtime_safety_profile": profile
        });
        if !workspace.is_null() {
            v["workspace_root"] = workspace;
        }
        serde_json::from_value(v).unwrap()
    };
    status("codex_inference_v1".into(), Value::Null)
        .validate()
        .unwrap();
    status("codex_local_tools_v1".into(), "/work/repo".into())
        .validate()
        .unwrap();
    status(Value::Null, Value::Null).validate().unwrap();
    assert!(
        status("codex_inference_v1".into(), "/work/repo".into())
            .validate()
            .is_err()
    );
    assert!(
        status("codex_local_tools_v1".into(), Value::Null)
            .validate()
            .is_err()
    );
    assert_eq!(
        status("codex_inference_v1".into(), Value::Null)
            .outbound_methods
            .config_requirements_read,
        0
    );
}

#[test]
fn a_model_offer_is_bounded_unique_and_never_prose_and_its_list_matches_the_models() {
    use super::control::ModelOffer;
    let ok = || ModelOffer {
        is_default: true,
        default_effort: Some("medium".into()),
        efforts: vec!["low".into(), "medium".into()],
        replaced_by: Some("newer".into()),
        retires_at: Some("2027-01-31T00:00:00Z".into()),
    };
    assert!(ok().valid());
    let mut many = ok();
    many.efforts = (0..=ModelOffer::MOST_EFFORTS)
        .map(|n| format!("e{n}"))
        .collect();
    for bad in [
        many,
        ModelOffer {
            efforts: vec!["low".into(), "low".into()],
            ..ok()
        },
        ModelOffer {
            efforts: vec!["".into()],
            ..ok()
        },
        ModelOffer {
            efforts: vec!["a".repeat(65)],
            ..ok()
        },
        ModelOffer {
            efforts: vec!["a\nb".into()],
            ..ok()
        },
        ModelOffer {
            default_effort: Some("x".repeat(65)),
            ..ok()
        },
        ModelOffer {
            replaced_by: Some("m".repeat(257)),
            ..ok()
        },
        ModelOffer {
            retires_at: Some(String::new()),
            ..ok()
        },
    ] {
        assert!(!bad.valid(), "{bad:?}");
    }
}
