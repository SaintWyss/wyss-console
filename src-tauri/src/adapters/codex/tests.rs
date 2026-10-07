use super::*;
use crate::contracts::{
    agent_adapter::{AgentAdapterDescriptorV1, CapabilityAccess, CapabilityId, CapabilitySupport},
    identities::{AgentRuntimeInstanceId, ManagedSessionId},
};
use serde_json::{Value, json};
fn corpus() -> Value {
    serde_json::from_str(include_str!(
        "../../../../tests/fixtures/codex-app-server-v1.json"
    ))
    .unwrap()
}
fn binding() -> SessionBinding {
    SessionBinding {
        managed_session_id: ManagedSessionId::parse("11111111-1111-4111-8111-111111111111")
            .unwrap(),
        runtime_instance_id: AgentRuntimeInstanceId::parse("22222222-2222-4222-8222-222222222222")
            .unwrap(),
        thread_id: CodexThreadId::parse("thread-fixture").unwrap(),
    }
}
fn direction(c: &Value) -> Direction {
    if c["direction"] == "client" {
        Direction::ClientToServer
    } else {
        Direction::ServerToClient
    }
}
fn case(name: &str) -> Value {
    corpus()["valid"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap()
        .clone()
}
fn parse_case(c: &Value) -> Result<WireMessage> {
    WireMessage::parse(c["wire"].as_str().unwrap().as_bytes(), direction(c))
}
fn message(name: &str) -> WireMessage {
    parse_case(&case(name)).unwrap()
}
fn operation(c: &Value) -> Option<Operation> {
    c.get("operation")
        .map(|v| serde_json::from_value(v.clone()).unwrap())
}
fn request_id(c: &Value) -> CodexRequestId {
    if c["request_id_kind"] == "integer" {
        CodexRequestId::Integer(c["request_id"].as_str().unwrap().parse().unwrap())
    } else {
        CodexRequestId::String(c["request_id"].as_str().unwrap().to_owned())
    }
}
fn observation(c: &Value, m: &WireMessage) -> Result<CodexObservation> {
    if let Some(op) = operation(c) {
        normalize_response(m, &request_id(c), op, Some(&binding()))
    } else {
        normalize_inbound(m, Some(&binding()))
    }
}
fn snapshot(o: CodexObservation) -> Value {
    match o {
        CodexObservation::Acknowledged => json!({"kind":"acknowledged"}),
        CodexObservation::Session { state, common, .. } => {
            json!({"kind":"session","state":state,"common":common})
        }
        CodexObservation::Turn {
            outcome,
            common,
            error,
            ..
        } => {
            json!({"kind":"turn","outcome":match outcome {TurnOutcome::InProgress=>"in_progress",TurnOutcome::Completed=>"completed",TurnOutcome::Failed=>"failed",TurnOutcome::Interrupted=>"interrupted"},"common":common,"error":error})
        }
        CodexObservation::Item(i) => {
            json!({"kind":"item","item_kind":match i.kind {ItemKind::UserMessage=>"user_message",ItemKind::AgentMessage=>"agent_message",ItemKind::CommandExecution=>"command_execution",ItemKind::FileChange=>"file_change",ItemKind::McpToolCall=>"mcp_tool_call",ItemKind::ReasoningSummary=>"reasoning_summary",ItemKind::Plan=>"plan",ItemKind::Unknown=>"unknown"},"stage":if i.stage==ItemStage::Started {"started"}else{"completed"},"surface":i.surface,"status":i.status,"detail":i.detail})
        }
        CodexObservation::Content {
            kind, text, detail, ..
        } => {
            json!({"kind":"content","content_kind":match kind {ContentKind::AgentMessage=>"agent_message",ContentKind::CommandOutput=>"command_output",ContentKind::ReasoningSummary=>"reasoning_summary",ContentKind::ToolProgress=>"tool_progress",ContentKind::Plan=>"plan"},"text":text,"detail":detail})
        }
        CodexObservation::Approval(a) => {
            json!({"kind":"approval","approval_kind":a.kind,"surface":a.surface})
        }
        CodexObservation::ApprovalResolved { request_id, .. } => {
            json!({"kind":"approval_resolved","request_id_decimal":match request_id {CodexRequestId::String(s)=>s,CodexRequestId::Integer(n)=>n.to_string()}})
        }
        CodexObservation::ModelCatalog {
            models,
            next_cursor,
        } => {
            json!({"kind":"model_catalog","models":models.into_iter().map(|m|json!({"model":m.model,"codex":m.codex})).collect::<Vec<_>>(),"next_cursor":next_cursor})
        }
        CodexObservation::UnknownNotification { surface, .. } => {
            json!({"kind":"unknown_notification","surface":surface})
        }
        CodexObservation::UnsupportedServerRequest { surface, .. } => {
            json!({"kind":"unsupported_server_request","surface":surface})
        }
        CodexObservation::SuppressedPrivateReasoning => {
            json!({"kind":"suppressed_private_reasoning"})
        }
        CodexObservation::Error(error) => json!({"kind":"error","error":error}),
    }
}
#[test]
fn valid_schema_derived_fixture_corpus_and_semantic_round_trip() {
    let c = corpus();
    let cases = c["valid"].as_array().unwrap();
    assert_eq!(cases.len(), 75);
    for c in cases {
        let m = parse_case(c).unwrap_or_else(|e| panic!("{}: {e}", c["name"]));
        let reparsed = WireMessage::parse(&m.encode().unwrap(), direction(c)).unwrap();
        assert_eq!(m.vendor_value(), reparsed.vendor_value(), "{}", c["name"]);
        assert_eq!(m.id(), reparsed.id());
        if let Some(expected) = c.get("expected") {
            assert_eq!(
                &snapshot(observation(c, &m).unwrap_or_else(|e| panic!("{}: {e}", c["name"]))),
                expected,
                "{}",
                c["name"]
            );
        }
        assert_eq!(c["provenance"]["retrieved"], "2026-10-06");
        assert!(
            c["provenance"]["source"]
                .as_str()
                .unwrap()
                .starts_with("https://github.com/openai/codex/blob/")
        );
    }
}
#[test]
fn invalid_fixture_corpus_rejects_before_normalization() {
    let c = corpus();
    let cases = c["invalid"].as_array().unwrap();
    assert_eq!(cases.len(), 36);
    for c in cases {
        let result = parse_case(c).and_then(|m| {
            if let Some(op) = operation(c) {
                validate_response(&m, &request_id(c), op)
            } else {
                Ok(())
            }
        });
        assert!(result.is_err(), "accepted {}", c["name"]);
        // Errors contain only stable codes, never provider payload text.
        assert!(result.unwrap_err().code.len() < 64);
    }
}
#[test]
fn request_ids_preserve_type_and_full_signed_range() {
    assert_eq!(
        message("request-max-i64").id(),
        Some(&CodexRequestId::Integer(i64::MAX))
    );
    assert_eq!(
        message("request-min-i64").id(),
        Some(&CodexRequestId::Integer(i64::MIN))
    );
    assert_ne!(
        message("request-string-id").id(),
        Some(&CodexRequestId::Integer(42))
    );
    let m =
        WireMessage::from_value(json!({"id":"42","result":{}}), Direction::ServerToClient).unwrap();
    assert_eq!(
        validate_response(&m, &CodexRequestId::Integer(42), Operation::TurnInterrupt)
            .unwrap_err()
            .code,
        "response_correlation"
    );
}
#[test]
fn envelope_direction_distinguishes_requests_without_jsonrpc_header() {
    let raw = br#"{"id":1,"method":"future/request"}"#;
    assert_eq!(
        WireMessage::parse(raw, Direction::ClientToServer)
            .unwrap()
            .kind(),
        WireKind::ClientRequest
    );
    assert_eq!(
        WireMessage::parse(raw, Direction::ServerToClient)
            .unwrap()
            .kind(),
        WireKind::ServerRequest
    );
    assert!(
        !String::from_utf8(message("initialize").encode().unwrap())
            .unwrap()
            .contains("jsonrpc")
    );
    assert_eq!(
        message("unknown-response").kind(),
        WireKind::SuccessResponse
    );
}
#[test]
fn initialization_requires_response_then_initialized_and_preserves_failure_order() {
    let mut state = ProtocolState::default();
    assert!(state.outbound(&message("model-list")).is_err());
    assert!(state.outbound(&message("initialized")).is_err());
    state.outbound(&message("initialize")).unwrap();
    assert!(!state.experimental_enabled());
    assert!(state.outbound(&message("initialize")).is_err());
    assert!(state.outbound(&message("initialized")).is_err());
    let mut wrong = message("initialize-response").vendor_value().clone();
    wrong["id"] = json!(1);
    assert!(
        state
            .initialize_response(
                &WireMessage::from_value(wrong, Direction::ServerToClient).unwrap()
            )
            .is_err()
    );
    state
        .initialize_response(&message("initialize-response"))
        .unwrap();
    assert_eq!(state.state(), &InitializationState::AwaitingInitialized);
    assert!(state.outbound(&message("model-list")).is_err());
    state
        .outbound(&initialized_notification().unwrap())
        .unwrap();
    state.outbound(&message("model-list")).unwrap();
    assert!(state.outbound(&message("initialize")).is_err());
    assert!(state.outbound(&message("initialized")).is_err());
    state.close();
    assert!(state.outbound(&message("model-list")).is_err());
    let mut failed = ProtocolState::default();
    failed.outbound(&message("initialize")).unwrap();
    failed
        .initialize_response(&message("error-response"))
        .unwrap();
    assert_eq!(failed.state(), &InitializationState::Failed);
    assert!(failed.outbound(&message("initialized")).is_err());
}
#[test]
fn outbound_small_stable_subset_encodes_deterministically() {
    let thread = CodexThreadId::parse("thread-fixture").unwrap();
    let turn = CodexTurnId::parse("turn-fixture").unwrap();
    let operations = [
        ClientOperation::Initialize {
            client_info: ClientInfo {
                name: "wyss_console".into(),
                title: Some("Wyss Console".into()),
                version: "fixture-version".into(),
            },
            experimental_api: false,
        },
        ClientOperation::ThreadStart {
            cwd: Some("/tmp".into()),
            model: None,
            model_provider: None,
        },
        ClientOperation::ThreadResume {
            thread_id: thread.clone(),
        },
        ClientOperation::TurnStart {
            thread_id: thread.clone(),
            text: "fixture prompt".into(),
        },
        ClientOperation::TurnInterrupt {
            thread_id: thread,
            turn_id: turn,
        },
        ClientOperation::ModelList {
            limit: None,
            cursor: None,
            include_hidden: true,
        },
    ];
    for op in operations {
        let m = op.encode(CodexRequestId::Integer(i64::MAX)).unwrap();
        assert_eq!(
            m.encode().unwrap(),
            op.encode(CodexRequestId::Integer(i64::MAX))
                .unwrap()
                .encode()
                .unwrap()
        );
        assert_eq!(m.kind(), WireKind::ClientRequest);
    }
    let init = ClientOperation::Initialize {
        client_info: ClientInfo {
            name: "wyss_console".into(),
            title: None,
            version: "fixture".into(),
        },
        experimental_api: true,
    }
    .encode(CodexRequestId::Integer(1))
    .unwrap();
    let mut state = ProtocolState::default();
    state.outbound(&init).unwrap();
    assert!(state.experimental_enabled());
}
#[test]
fn vendor_thread_identity_never_allocates_or_replaces_managed_identity() {
    let m = message("thread-started-idle");
    let CodexObservation::Session {
        thread_id, common, ..
    } = normalize_inbound(&m, Some(&binding())).unwrap()
    else {
        panic!()
    };
    assert_eq!(thread_id.as_str(), "thread-fixture");
    assert_eq!(
        common.unwrap().managed_session_id(),
        &binding().managed_session_id
    );
    let CodexObservation::Session { common, .. } = normalize_inbound(&m, None).unwrap() else {
        panic!()
    };
    assert!(common.is_none());
    let mut b = binding();
    b.thread_id = CodexThreadId::parse("different-thread").unwrap();
    assert_eq!(
        normalize_inbound(&m, Some(&b)).unwrap_err().code,
        "session_mapping_mismatch"
    );
}
#[test]
fn failed_and_interrupted_turns_are_not_successful_workflow_completion() {
    for (name, wanted) in [
        ("turn-completed-completed", TurnOutcome::Completed),
        ("turn-completed-failed", TurnOutcome::Failed),
        ("turn-completed-interrupted", TurnOutcome::Interrupted),
    ] {
        let CodexObservation::Turn {
            outcome, common, ..
        } = normalize_inbound(&message(name), Some(&binding())).unwrap()
        else {
            panic!()
        };
        assert_eq!(outcome, wanted);
        assert_ne!(
            common.unwrap().state(),
            crate::contracts::agent_adapter::ManagedAgentSessionState::Completed
        );
    }
}
#[test]
fn private_reasoning_is_not_projected_but_declared_summaries_are() {
    let snapshot = snapshot(normalize_inbound(&message("item-reasoning-completed"), None).unwrap())
        .to_string();
    assert!(!snapshot.contains("PRIVATE_REASONING"));
    assert!(snapshot.contains("user-visible summary"));
    assert!(matches!(
        normalize_inbound(&message("private-reasoning"), None).unwrap(),
        CodexObservation::SuppressedPrivateReasoning
    ));
    assert!(matches!(
        normalize_inbound(&message("item/reasoning/summaryTextDelta"), None).unwrap(),
        CodexObservation::Content {
            kind: ContentKind::ReasoningSummary,
            ..
        }
    ));
}
#[test]
fn unknown_notifications_and_items_keep_only_vendor_semantics() {
    let m = message("unknown-notification");
    let CodexObservation::UnknownNotification {
        vendor_params,
        surface,
        ..
    } = normalize_inbound(&m, None).unwrap()
    else {
        panic!()
    };
    assert_eq!(vendor_params, m.params().cloned());
    assert_eq!(surface, Surface::Unknown);
    let m = message("item-futureItem-completed");
    let CodexObservation::Item(i) = normalize_inbound(&m, None).unwrap() else {
        panic!()
    };
    assert_eq!(i.kind, ItemKind::Unknown);
    assert_eq!(i.surface, Surface::Unknown);
    assert_eq!(
        m.vendor_value()["params"]["item"]["vendorDetail"],
        json!({"bounded":true})
    );
}
#[test]
fn unknown_server_requests_cannot_use_known_approval_encoders() {
    let m = message("unknown-server-request");
    assert!(matches!(
        normalize_inbound(&m, None).unwrap(),
        CodexObservation::UnsupportedServerRequest { .. }
    ));
    assert_eq!(
        encode_approval_response(&m, ApprovalDecision::Accept)
            .unwrap_err()
            .code,
        "unsupported_server_request"
    );
    assert!(encode_empty_permission_response(&m).is_err());
}
#[test]
fn command_and_file_decisions_encode_only_their_verified_subtypes() {
    for (name, op) in [
        ("command-approval-request", Operation::CommandApproval),
        ("file_change-approval-request", Operation::FileApproval),
    ] {
        let request = message(name);
        for decision in [
            ApprovalDecision::Accept,
            ApprovalDecision::AcceptForSession,
            ApprovalDecision::Decline,
            ApprovalDecision::Cancel,
        ] {
            let m = encode_approval_response(&request, decision).unwrap();
            validate_response(&m, request.id().unwrap(), op).unwrap();
        }
    }
    let command = message("command-approval-request");
    let file = message("file_change-approval-request");
    for d in [
        ApprovalDecision::AcceptWithExecpolicyAmendment(vec!["printf".into()]),
        ApprovalDecision::ApplyNetworkPolicyAmendment {
            action: NetworkAction::Deny,
            host: "example.invalid".into(),
        },
    ] {
        assert!(encode_approval_response(&command, d.clone()).is_ok());
        assert!(encode_approval_response(&file, d).is_err());
    }
}
#[test]
fn permission_requests_are_distinct_and_only_empty_grants_encode() {
    let p = message("permissions-approval-request");
    let CodexObservation::Approval(a) = normalize_inbound(&p, None).unwrap() else {
        panic!()
    };
    assert_eq!(a.kind, ApprovalKind::Permissions);
    assert!(encode_approval_response(&p, ApprovalDecision::Accept).is_err());
    let response = encode_empty_permission_response(&p).unwrap();
    validate_response(&response, p.id().unwrap(), Operation::PermissionApproval).unwrap();
    assert_eq!(response.result().unwrap()["permissions"], json!({}));
}
#[test]
fn resolved_request_retains_full_integer_correlation_not_item_guessing() {
    let CodexObservation::ApprovalResolved {
        request_id,
        thread_id,
    } = normalize_inbound(&message("request-resolved"), None).unwrap()
    else {
        panic!()
    };
    assert_eq!(request_id, CodexRequestId::Integer(i64::MAX));
    assert_eq!(thread_id.as_str(), "thread-fixture");
}
#[test]
fn catalog_has_no_entitlement_or_invented_openai_provider() {
    let c = case("model-list-response");
    let m = parse_case(&c).unwrap();
    let CodexObservation::ModelCatalog { models, .. } = observation(&c, &m).unwrap() else {
        panic!()
    };
    let normalized = serde_json::to_value(&models[0].model).unwrap();
    assert!(normalized["provider_id"].is_null());
    assert_eq!(normalized["availability"], "unknown");
    assert_eq!(models[0].codex["hidden"], true);
    assert_eq!(models[0].codex["isDefault"], false);
    let c = case("thread-start-response");
    let o = observation(&c, &parse_case(&c).unwrap()).unwrap();
    assert_eq!(
        snapshot(o)["common"]["observed_model"]["provider_id"],
        "custom-provider"
    );
}
#[test]
fn raw_provider_error_never_becomes_a_safe_diagnostic() {
    let m = message("error-response");
    let normalized = snapshot(normalize_inbound(&m, None).unwrap()).to_string();
    assert!(!normalized.contains("PRIVATE_PROVIDER_ERROR"));
    assert!(!normalized.contains("sensitive"));
    assert!(
        m.vendor_value()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("PRIVATE_PROVIDER_ERROR")
    );
    assert!(!format!("{m:?}").contains("PRIVATE_PROVIDER_ERROR"));
}
#[test]
fn bounds_apply_to_unknown_vendor_content_before_preservation() {
    let raw = vec![b' '; MAX_FRAME_BYTES + 1];
    assert_eq!(
        WireMessage::parse(&raw, Direction::ServerToClient)
            .unwrap_err()
            .code,
        "frame_too_large"
    );
    let mut deep = json!(true);
    for _ in 0..18 {
        deep = json!({"x":deep});
    }
    assert_eq!(
        WireMessage::from_value(
            json!({"method":"future","params":deep}),
            Direction::ServerToClient
        )
        .unwrap_err()
        .code,
        "payload_bounds"
    );
    assert!(
        WireMessage::from_value(
            json!({"method":"future","params":"x".repeat(MAX_CONTENT_BYTES+1)}),
            Direction::ServerToClient
        )
        .is_err()
    );
    assert!(
        WireMessage::from_value(
            json!({"method":"future","params":vec![0;257]}),
            Direction::ServerToClient
        )
        .is_err()
    );
}
#[test]
fn experimental_observations_never_become_stable_claims() {
    assert!(matches!(
        normalize_inbound(&message("experimental-item/plan/delta"), None).unwrap(),
        CodexObservation::UnknownNotification {
            surface: Surface::Experimental,
            ..
        }
    ));
    let CodexObservation::Item(i) = normalize_inbound(&message("item-plan-started"), None).unwrap()
    else {
        panic!()
    };
    assert_eq!(i.surface, Surface::Experimental);
    let CodexObservation::Approval(a) =
        normalize_inbound(&message("approval-request-experimental-field"), None).unwrap()
    else {
        panic!()
    };
    assert_eq!(a.surface, Surface::Experimental);
}
#[test]
fn static_codec_descriptor_claims_translation_only_not_local_compatibility() {
    let v: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/codex-offline-descriptor-v1.json"
    ))
    .unwrap();
    let d = AgentAdapterDescriptorV1::from_value(v.clone()).unwrap();
    assert_eq!(v["compatibility"]["state"], "unknown");
    assert!(v["runtime_version"]["value"].is_null());
    assert!(!v.to_string().contains("confirmed"));
    assert_eq!(
        d.support(CapabilityId::ModelList, CapabilityAccess::Observe),
        CapabilitySupport::Conditional
    );
    assert_eq!(
        d.support(CapabilityId::Subagents, CapabilityAccess::Request),
        CapabilitySupport::Unknown
    );
}

#[test]
fn experimental_fields_on_stable_methods_require_explicit_opt_in() {
    let mut state = ProtocolState::default();
    state.outbound(&message("initialize")).unwrap();
    state
        .initialize_response(&message("initialize-response"))
        .unwrap();
    state.outbound(&message("initialized")).unwrap();
    for (method, field) in [
        ("thread/start", "dynamicTools"),
        ("thread/resume", "history"),
        ("turn/start", "additionalContext"),
    ] {
        let name = match method {
            "thread/start" => "thread-start",
            "thread/resume" => "thread-resume",
            _ => "turn-start",
        };
        let mut v = message(name).vendor_value().clone();
        v["params"][field] = json!([]);
        let m = WireMessage::from_value(v, Direction::ClientToServer).unwrap();
        assert_eq!(m.surface(), Surface::Experimental);
        assert_eq!(
            state.outbound(&m).unwrap_err().code,
            "experimental_not_enabled"
        );
    }
}
#[test]
fn approval_encoders_preserve_exact_decisions_and_request_identity() {
    let request = message("command-approval-request");
    for (decision, expected) in [
        (ApprovalDecision::Accept, "accept"),
        (ApprovalDecision::AcceptForSession, "acceptForSession"),
        (ApprovalDecision::Decline, "decline"),
        (ApprovalDecision::Cancel, "cancel"),
    ] {
        let response = encode_approval_response(&request, decision).unwrap();
        assert_eq!(response.id(), request.id());
        assert_eq!(response.result().unwrap()["decision"], expected);
    }
}
#[test]
fn request_with_notification_method_is_never_mistaken_for_notification() {
    let request = WireMessage::from_value(
        json!({"id":1,"method":"item/reasoning/textDelta","params":{}}),
        Direction::ServerToClient,
    )
    .unwrap();
    assert!(matches!(
        normalize_inbound(&request, None).unwrap(),
        CodexObservation::UnsupportedServerRequest { .. }
    ));
}

#[test]
fn malformed_consumed_and_core_fields_reject_before_projection() {
    for (key, value) in [
        ("approvalPolicy", json!(null)),
        ("sandbox", json!(null)),
        ("approvalsReviewer", json!(12)),
    ] {
        let mut c = case("thread-start-response");
        let mut v: Value = serde_json::from_str(c["wire"].as_str().unwrap()).unwrap();
        v["result"][key] = value;
        c["wire"] = json!(v.to_string());
        let m = parse_case(&c).unwrap();
        assert!(validate_response(&m, &request_id(&c), Operation::ThreadStart).is_err());
    }
    let mut v = message("item-agentMessage-started").vendor_value().clone();
    v["params"]["item"]["phase"] = json!({"not":"text"});
    assert!(WireMessage::from_value(v, Direction::ServerToClient).is_err());
    let mut v = message("item-userMessage-started").vendor_value().clone();
    v["params"]["item"]["content"] = json!([42]);
    assert!(WireMessage::from_value(v, Direction::ServerToClient).is_err());
    let mut v = message("thread-started-idle").vendor_value().clone();
    v["params"]["thread"]["source"] = json!(false);
    assert!(WireMessage::from_value(v, Direction::ServerToClient).is_err());
}
#[test]
fn null_experimental_permission_field_does_not_disagree_with_wire_classification() {
    let mut v = message("command-approval-request").vendor_value().clone();
    v["params"]["additionalPermissions"] = Value::Null;
    let m = WireMessage::from_value(v, Direction::ServerToClient).unwrap();
    let CodexObservation::Approval(a) = normalize_inbound(&m, None).unwrap() else {
        panic!()
    };
    assert_eq!(m.surface(), Surface::Stable);
    assert_eq!(a.surface, m.surface());
}

#[test]
fn official_notification_envelope_fixtures_round_trip() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/codex-notification-envelope-v1.json"
    ))
    .unwrap();
    assert_eq!(corpus["valid"].as_array().unwrap().len(), 9);
    assert_eq!(corpus["invalid"].as_array().unwrap().len(), 16);
    for c in corpus["valid"].as_array().unwrap() {
        let m = parse_case(c).unwrap_or_else(|e| panic!("{}: {e}", c["name"]));
        assert_eq!(m.kind(), WireKind::Notification);
        let reparsed = WireMessage::parse(&m.encode().unwrap(), direction(c)).unwrap();
        assert_eq!(m.vendor_value(), reparsed.vendor_value());
        assert_eq!(
            m.emitted_at_ms(),
            m.vendor_value().get("emittedAtMs").and_then(Value::as_i64)
        );
    }
    for c in corpus["invalid"].as_array().unwrap() {
        assert!(parse_case(c).is_err(), "accepted {}", c["name"]);
    }
}
#[test]
fn emission_metadata_does_not_change_normalized_observations() {
    for c in corpus()["valid"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["direction"] == "server" && c.get("operation").is_none())
    {
        let original = parse_case(c).unwrap();
        if original.kind() != WireKind::Notification {
            continue;
        }
        let mut value = original.vendor_value().clone();
        value["emittedAtMs"] = json!(i64::MAX);
        let stamped = WireMessage::from_value(value, Direction::ServerToClient).unwrap();
        assert_eq!(stamped.emitted_at_ms(), Some(i64::MAX));
        assert_eq!(stamped.surface(), original.surface());
        assert_eq!(
            snapshot(observation(c, &stamped).unwrap()),
            snapshot(observation(c, &original).unwrap()),
            "{}",
            c["name"]
        );
    }
}
#[test]
fn rejection_structure_contains_only_bounded_keys_and_safe_method() {
    let mut value = json!({"method":"future/notice","params":{"secret":"never retained"},"unverified":"never retained"});
    for i in 0..40 {
        value[format!("field{i}")] = json!("never retained");
    }
    value["x".repeat(65)] = json!("never retained");
    let failure = WireMessage::parse_diagnosed(
        &serde_json::to_vec(&value).unwrap(),
        Direction::ServerToClient,
    )
    .unwrap_err();
    assert_eq!(failure.error.code, "unknown_field");
    let structure = failure.structure.unwrap();
    assert_eq!(structure.category_candidate, "notification");
    assert!(structure.keys_omitted);
    assert!(structure.top_level_keys.len() <= 16);
    assert!(structure.top_level_keys.iter().all(|k| k.len() <= 64));
    assert!(
        !serde_json::to_string(&structure)
            .unwrap()
            .contains("never retained")
    );
}

#[test]
fn stable_materialization_codec_is_minimal_strict_and_correlated() {
    let id = CodexRequestId::String("materialize-exact-id".into());
    let thread = CodexThreadId::parse("opaque-provider-thread").unwrap();
    for unarchive in [false, true] {
        let request = materialization_request(id.clone(), &thread, unarchive).unwrap();
        assert_eq!(request.params().unwrap(), &json!({"threadId":thread}));
        assert_eq!(message_surface(&request), Surface::Stable);
        let op = if unarchive {
            Operation::ThreadUnarchive
        } else {
            Operation::ThreadArchive
        };
        let result = if unarchive {
            let c = case("thread-start-response");
            let v: Value = serde_json::from_str(c["wire"].as_str().unwrap()).unwrap();
            json!({"thread":v["result"]["thread"]})
        } else {
            json!({})
        };
        let response =
            WireMessage::from_value(json!({"id":id,"result":result}), Direction::ServerToClient)
                .unwrap();
        validate_response(&response, &id, op).unwrap();
        assert!(validate_response(&response, &CodexRequestId::Integer(1), op).is_err());
        let extra = WireMessage::from_value(
            json!({"id":id,"result":{"unexpected":true}}),
            Direction::ServerToClient,
        )
        .unwrap();
        assert!(validate_response(&extra, &id, op).is_err());
        for params in [
            json!({}),
            json!({"threadId":""}),
            json!({"threadId":3}),
            json!({"threadId":"ok","config":{}}),
        ] {
            assert!(WireMessage::request(id.clone(), request.method().unwrap(), params).is_err());
        }
    }
}

#[test]
fn materialization_notifications_are_validated_and_consumed_without_attachment() {
    for method in ["thread/archived", "thread/unarchived"] {
        let m = WireMessage::from_value(
            json!({"method":method,"params":{"threadId":"thread-valid"}}),
            Direction::ServerToClient,
        )
        .unwrap();
        assert!(normalize_inbound(&m, None).is_ok());
        assert!(
            WireMessage::from_value(
                json!({"method":method,"params":{"threadId":3}}),
                Direction::ServerToClient
            )
            .is_err()
        );
    }
}
