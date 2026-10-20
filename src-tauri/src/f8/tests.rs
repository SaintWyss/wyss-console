use super::{protocol::*, registry::*};
use crate::contracts::{
    WireUuid,
    control::ControlError as E,
    identities::{AgentRuntimeInstanceId, ManagedSessionId},
};
use serde_json::{Value, json};
const A: &str = "00000000-0000-4000-8000-00000000000a";
const B: &str = "00000000-0000-4000-8000-00000000000b";
const AUTH: &str = "00000000-0000-4000-8000-00000000000c";
const INSTANCE: &str = "00000000-0000-4000-8000-00000000000d";
const INTENT: &str = "00000000-0000-4000-8000-00000000000e";
fn id(v: &str) -> WireUuid {
    WireUuid::try_from(v.to_owned()).unwrap()
}
fn request(method: &str, params: Value) -> Value {
    json!({"schema_version":2,"request_id":INTENT,"authority_instance_id":AUTH,"method":method,"params":params})
}
fn start() -> Value {
    request(
        "runtime/start",
        json!({"intent_id":INTENT,"adapter_id":"codex","runtime_kind":"codex","managed_session_id":A,"expected_generation":0,"expected_revision":0}),
    )
}
fn parse(v: &Value) -> Result<Request, E> {
    Request::parse(&serde_json::to_vec(v).unwrap())
}
fn reference(g: u64) -> RuntimeRef {
    RuntimeRef {
        schema: 2,
        authority_instance_id: id(AUTH),
        managed_session_id: ManagedSessionId::parse(A).unwrap(),
        runtime_instance_id: AgentRuntimeInstanceId::parse(INSTANCE).unwrap(),
        generation: g,
    }
}
fn fence() -> Value {
    json!({"managed_session_id":A,"generation":1,"runtime_instance_id":INSTANCE})
}
#[test]
fn real_request_codec_roundtrips_and_v1_stays_closed() {
    let v = start();
    let r = parse(&v).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&r.encode().unwrap()).unwrap(),
        v
    );
    assert!(crate::contracts::control::Request::parse(&r.encode().unwrap()).is_err());
    assert_eq!(r.correlate(None), Err(E::HandshakeRequired));
    assert_eq!(r.correlate(Some(&id(B))), Err(E::CorrelationError));
    r.correlate(Some(&id(AUTH))).unwrap();
    for field in ["argv", "env", "pid", "model", "effort", "prompt"] {
        let mut bad = v.clone();
        bad["params"][field] = json!("no");
        assert!(parse(&bad).is_err(), "{field}");
    }
    for value in [
        json!(null),
        json!(-1),
        json!(1.5),
        json!(true),
        json!("1"),
        json!(u64::MAX),
    ] {
        let mut bad = v.clone();
        bad["params"]["expected_generation"] = value;
        assert!(parse(&bad).is_err());
    }
    for field in [
        "managed_session_id",
        "expected_generation",
        "expected_revision",
    ] {
        let mut bad = v.clone();
        bad["params"].as_object_mut().unwrap().remove(field);
        assert!(parse(&bad).is_err());
    }
}
#[test]
fn lease_grant_inline_session_must_equal_fence_and_be_canonical() {
    let mut v = request(
        "executionLease/grant",
        json!({"intent_id":INTENT,"session_id":A,"expected_session_revision":1,"scopes":["turn_start"],"ttl_seconds":60,"runtime_fence":fence()}),
    );
    assert!(parse(&v).is_ok());
    v["params"]["session_id"] = json!(B);
    assert!(parse(&v).is_err());
    v["params"]["session_id"] = json!(A.to_uppercase());
    assert!(parse(&v).is_err());
    v["params"]["session_id"] = json!(A.replace('-', ""));
    assert!(parse(&v).is_err());
    v["params"]["session_id"] = json!(A);
    v["params"]["runtime_fence"]
        .as_object_mut()
        .unwrap()
        .remove("runtime_instance_id");
    assert!(parse(&v).is_err());
    for method in ["executionLease/active", "executionGate/status"] {
        let v = request(method, json!({"session_id":A.to_uppercase()}));
        assert!(parse(&v).is_err());
    }
}
#[test]
fn duplicate_nested_fields_oversize_trailing_json_and_bad_versions_refuse() {
    let text = serde_json::to_string(&start()).unwrap();
    let duplicate = text.replace(
        "\"expected_generation\":0",
        "\"expected_generation\":0,\"expected_generation\":1",
    );
    assert!(Request::parse(duplicate.as_bytes()).is_err());
    assert!(Request::parse(format!("{text} {{}}").as_bytes()).is_err());
    assert_eq!(
        Request::parse(&vec![b' '; 65537]).unwrap_err(),
        E::FrameTooLarge
    );
    for n in [json!(1), json!(3), json!(2.0), json!("2")] {
        let mut v = start();
        v["schema_version"] = n;
        assert!(parse(&v).is_err());
    }
}
#[test]
fn hello_and_fenced_turn_preserve_exact_targeting() {
    let v = json!({"schema_version":2,"request_id":INTENT,"method":"authority/hello","params":{"protocol_version":2,"client_instance_id":B}});
    let r = parse(&v).unwrap();
    r.correlate(None).unwrap();
    assert!(r.correlate(Some(&id(AUTH))).is_err());
    let mut turn = request(
        "managedSession/turn/start",
        json!({"intent_id":INTENT,"managed_session_id":A,"expected_session_revision":1,"prompt":"fixture","runtime_fence":fence()}),
    );
    assert!(matches!(parse(&turn).unwrap().target(), Target::Fenced(_)));
    turn["params"]["managed_session_id"] = json!(B);
    assert!(parse(&turn).is_err());
}
#[test]
fn registry_holds_stopping_slot_refuses_stale_and_cross_session_observations() {
    let mut reg = Registry::new(id(AUTH)).unwrap();
    let r = reference(1);
    reg.reserve(r.clone()).unwrap();
    assert_eq!(
        reg.observe(&id(AUTH), &r.fence()).unwrap_err(),
        E::GateDenied
    );
    reg.initializing(&r).unwrap();
    let identity = RuntimeIdentity {
        schema: 2,
        authority_instance_id: id(AUTH),
        managed_session_id: r.managed_session_id.clone(),
        runtime_instance_id: r.runtime_instance_id.clone(),
        generation: 1,
        observed_version: "0.159.0".into(),
    };
    reg.ready(identity).unwrap();
    reg.observe(&id(AUTH), &r.fence()).unwrap();
    let mut crossed = r.fence();
    crossed.managed_session_id = ManagedSessionId::parse(B).unwrap();
    assert!(reg.observe(&id(AUTH), &crossed).is_err());
    assert!(reg.observe(&id(B), &r.fence()).is_err());
    reg.stopping(&r).unwrap();
    assert_eq!(reg.active_slots(), 1);
    assert!(reg.reserve(reference(2)).is_err());
    assert!(reg.observe(&id(AUTH), &r.fence()).is_err());
    reg.remove_after_reap(&r).unwrap();
    assert_eq!(reg.active_slots(), 0);
    assert!(reg.reserve(r).is_err());
    let mut next = reference(2);
    next.runtime_instance_id = AgentRuntimeInstanceId::parse(INTENT).unwrap();
    reg.reserve(next).unwrap();
    assert!(reg.observe(&id(AUTH), &reference(1).fence()).is_err());
}
#[test]
fn real_replay_cache_ignores_transport_ids_but_not_fences_and_never_reexecutes() {
    let r = parse(&start()).unwrap();
    let mut cache = ReplayCache::default();
    assert_eq!(cache.lookup(&r).unwrap(), None);
    cache.record(&r, 7).unwrap();
    let mut retry = start();
    retry["request_id"] = json!(B);
    assert_eq!(cache.lookup(&parse(&retry).unwrap()).unwrap(), Some(&7));
    retry["params"]["expected_generation"] = json!(1);
    assert_eq!(
        cache.lookup(&parse(&retry).unwrap()),
        Err(E::IntentConflict)
    );
    // Exact replay is available independently of registry saturation; receipt is not an admission.
    let mut registry = Registry::new(id(AUTH)).unwrap();
    registry.reserve(reference(1)).unwrap();
    assert_eq!(cache.lookup(&r).unwrap(), Some(&7));
    assert_eq!(registry.active_slots(), 1);
}
#[test]
fn response_roundtrip_closed_errors_and_authority_correlation() {
    let r = parse(&start()).unwrap();
    let error = Response::error(id(INTENT), id(AUTH), E::GateDenied).unwrap();
    let encoded = error.encode().unwrap();
    let parsed = Response::parse(&encoded).unwrap();
    assert_eq!(parsed.outcome().as_ref().unwrap_err(), &E::GateDenied);
    parsed.correlate(&r, &id(AUTH)).unwrap();
    assert!(parsed.correlate(&r, &id(B)).is_err());
    let mut v: Value = serde_json::from_slice(&encoded).unwrap();
    v["error"]["detail"] = json!("secret");
    assert!(Response::parse(&serde_json::to_vec(&v).unwrap()).is_err());
    v["error"].as_object_mut().unwrap().remove("detail");
    v["error"]["code"] = json!("invented");
    assert!(Response::parse(&serde_json::to_vec(&v).unwrap()).is_err());
}
#[test]
fn response_runtime_identity_crossing_is_rejected() {
    let identity = RuntimeIdentity {
        schema: 2,
        authority_instance_id: id(AUTH),
        managed_session_id: ManagedSessionId::parse(A).unwrap(),
        runtime_instance_id: AgentRuntimeInstanceId::parse(INSTANCE).unwrap(),
        generation: 1,
        observed_version: "0.159.0".into(),
    };
    let mut v = json!({"schema_version":2,"request_id":INTENT,"authority_instance_id":AUTH,"result":{"kind":"runtime_status","value":{"adapter_id":"codex","runtime_kind":"codex","desired_state":"running","revision":1,"actual_state":"ready","runtime_instance_id":INSTANCE,"runtime_version":"0.159.0","compatibility":"compatible","model_count":7,"diagnostic":null,"outbound_methods":{"initialize":1,"initialized":1,"model_list":1},"runtime_safety_profile":"codex_inference_v1","managed_session_id":A,"generation":1,"runtime_identity":identity,"budget_failure":null}}});
    let r = Response::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
    assert!(Response::parse(&r.encode().unwrap()).is_ok());
    v["result"]["value"]["runtime_identity"]["managed_session_id"] = json!(B);
    assert!(Response::parse(&serde_json::to_vec(&v).unwrap()).is_err());
}
#[test]
fn response_cannot_retarget_old_authority_or_stop_generation_revision_state() {
    let error = Response::error(id(INTENT), id(AUTH), E::GateDenied).unwrap();
    let mut old = start();
    old["authority_instance_id"] = json!(B);
    assert_eq!(
        error.correlate(&parse(&old).unwrap(), &id(AUTH)),
        Err(E::CorrelationError)
    );
    let stop = request(
        "runtime/stop",
        json!({"intent_id":INTENT,"adapter_id":"codex","runtime_kind":"codex","managed_session_id":A,"expected_generation":1,"expected_revision":4,"expected_runtime_instance_id":INSTANCE}),
    );
    let r = parse(&stop).unwrap();
    let v = json!({"schema_version":2,"request_id":INTENT,"authority_instance_id":AUTH,"result":{"kind":"runtime_control","value":{"adapter_id":"codex","runtime_kind":"codex","managed_session_id":A,"generation":1,"desired_state":"stopped","revision":5,"created_at":"2026-10-20T00:00:00Z","updated_at":"2026-10-20T00:00:00Z"}}});
    let decode = |v: &Value| Response::parse(&serde_json::to_vec(v).unwrap()).unwrap();
    decode(&v).correlate(&r, &id(AUTH)).unwrap();
    for (field, value) in [
        ("generation", json!(99)),
        ("revision", json!(4)),
        ("desired_state", json!("running")),
    ] {
        let mut bad = v.clone();
        bad["result"]["value"][field] = value;
        assert_eq!(
            decode(&bad).correlate(&r, &id(AUTH)),
            Err(E::CorrelationError)
        );
    }
}
#[test]
fn model_cursor_cannot_cross_identity_in_request_or_response() {
    let identity = RuntimeIdentity {
        schema: 2,
        authority_instance_id: id(AUTH),
        managed_session_id: ManagedSessionId::parse(A).unwrap(),
        runtime_instance_id: AgentRuntimeInstanceId::parse(INSTANCE).unwrap(),
        generation: 1,
        observed_version: "0.159.0".into(),
    };
    let response = json!({"schema_version":2,"request_id":INTENT,"authority_instance_id":AUTH,"result":{"kind":"runtime_models","value":{"runtime_instance_id":INSTANCE,"models":[],"next":{"runtime_instance_id":INSTANCE,"index":20},"runtime_identity":identity}}});
    let req = request(
        "runtime/model/list",
        json!({"adapter_id":"codex","runtime_kind":"codex","managed_session_id":A,"expected_generation":1,"expected_runtime_instance_id":INSTANCE,"limit":20,"cursor":{"runtime_instance_id":INSTANCE,"index":20}}),
    );
    Response::parse(&serde_json::to_vec(&response).unwrap())
        .unwrap()
        .correlate(&parse(&req).unwrap(), &id(AUTH))
        .unwrap();
    for (field, value) in [("runtime_instance_id", json!(B)), ("index", json!(51))] {
        let mut bad = response.clone();
        bad["result"]["value"]["next"][field] = value.clone();
        assert!(Response::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
        let mut bad = req.clone();
        bad["params"]["cursor"][field] = value;
        assert!(parse(&bad).is_err());
    }
}
fn response_value(kind: &str, value: Value) -> Result<Response, E> {
    Response::parse(&serde_json::to_vec(&json!({"schema_version":2,"request_id":INTENT,"authority_instance_id":AUTH,"result":{"kind":kind,"value":value}})).unwrap())
}
fn session_value(session: &str, closed: bool) -> Value {
    json!({"managed_session_id":session,"adapter_id":"codex","runtime_kind":"codex","control_state":if closed {"closed"} else {"prepared"},"revision":if closed {2} else {1},"created_at":"2026-10-20T00:00:00Z","updated_at":"2026-10-20T00:00:00Z","created_sequence":"1"})
}
fn lease_value(session: &str, state: &str, revision: u64) -> Value {
    json!({"lease_id":B,"session_id":session,"state":state,"scopes":["turn_start"],"revision":revision,"issued_at":"2026-10-20T00:00:00Z","updated_at":"2026-10-20T00:00:00Z","expires_at":"2026-10-20T00:01:00Z","revocation_reason":if state=="revoked" {json!("user_request")} else {Value::Null}})
}
#[test]
fn non_runtime_correlator_refuses_wrong_result_session_lease_gate_and_binding() {
    let get = parse(&request(
        "managedSession/get",
        json!({"managed_session_id":A}),
    ))
    .unwrap();
    response_value("session", session_value(A, false))
        .unwrap()
        .correlate(&get, &id(AUTH))
        .unwrap();
    assert_eq!(
        response_value("session", session_value(B, false))
            .unwrap()
            .correlate(&get, &id(AUTH)),
        Err(E::CorrelationError)
    );
    let close=parse(&request("managedSession/close",json!({"intent_id":INTENT,"managed_session_id":A,"expected_revision":1,"runtime_fence":fence()}))).unwrap();
    response_value("session", session_value(A, true))
        .unwrap()
        .correlate(&close, &id(AUTH))
        .unwrap();
    assert!(
        response_value("session", session_value(A, false))
            .unwrap()
            .correlate(&close, &id(AUTH))
            .is_err()
    );
    let active = parse(&request("executionLease/active", json!({"session_id":A}))).unwrap();
    response_value("active_lease", lease_value(A, "active", 1))
        .unwrap()
        .correlate(&active, &id(AUTH))
        .unwrap();
    assert!(
        response_value("active_lease", lease_value(INSTANCE, "active", 1))
            .unwrap()
            .correlate(&active, &id(AUTH))
            .is_err()
    );
    assert!(
        response_value("session", session_value(A, false))
            .unwrap()
            .correlate(&active, &id(AUTH))
            .is_err()
    );
    let gate = parse(&request(
        "executionGate/status",
        json!({"session_id":A,"action":"turn_start"}),
    ))
    .unwrap();
    response_value(
        "gate_status",
        json!({"session_id":A,"action":"turn_start","reason":"lease_missing"}),
    )
    .unwrap()
    .correlate(&gate, &id(AUTH))
    .unwrap();
    assert!(
        response_value(
            "gate_status",
            json!({"session_id":B,"action":"turn_start","reason":"lease_missing"})
        )
        .unwrap()
        .correlate(&gate, &id(AUTH))
        .is_err()
    );
    let binding = parse(&request(
        "managedSession/runtime/status",
        json!({"managed_session_id":A}),
    ))
    .unwrap();
    for (session, ok) in [(A, true), (B, false)] {
        let result=response_value("binding_status",json!({"managed_session_id":session,"state":"unbound","binding":null,"runtime_instance_id":null,"operation_id":null,"runtime_identity":null})).unwrap();
        assert_eq!(result.correlate(&binding, &id(AUTH)).is_ok(), ok);
    }
}
#[test]
fn lease_receipt_scope_ttl_revision_and_fenced_owner_must_match_request() {
    let grant=parse(&request("executionLease/grant",json!({"intent_id":INTENT,"session_id":A,"expected_session_revision":1,"scopes":["turn_start"],"ttl_seconds":60,"runtime_fence":fence()}))).unwrap();
    let valid = lease_value(A, "active", 1);
    response_value("lease", valid.clone())
        .unwrap()
        .correlate(&grant, &id(AUTH))
        .unwrap();
    for (field, value) in [
        ("scopes", json!(["session_create"])),
        ("revision", json!(2)),
        ("expires_at", json!("2026-10-20T00:01:01Z")),
        ("session_id", json!(INSTANCE)),
    ] {
        let mut bad = valid.clone();
        bad[field] = value;
        assert!(
            response_value("lease", bad)
                .unwrap()
                .correlate(&grant, &id(AUTH))
                .is_err()
        );
    }
    let renew=parse(&request("executionLease/renew",json!({"intent_id":INTENT,"lease_id":B,"expected_revision":1,"ttl_seconds":60,"runtime_fence":fence()}))).unwrap();
    response_value("lease", lease_value(A, "active", 2))
        .unwrap()
        .correlate(&renew, &id(AUTH))
        .unwrap();
    assert!(
        response_value("lease", lease_value(INSTANCE, "active", 2))
            .unwrap()
            .correlate(&renew, &id(AUTH))
            .is_err()
    );
    let revoke = parse(&request(
        "executionLease/revoke",
        json!({"intent_id":INTENT,"lease_id":B,"expected_revision":1}),
    ))
    .unwrap();
    response_value("lease", lease_value(A, "revoked", 2))
        .unwrap()
        .correlate(&revoke, &id(AUTH))
        .unwrap();
    assert!(
        response_value("lease", lease_value(A, "active", 2))
            .unwrap()
            .correlate(&revoke, &id(AUTH))
            .is_err()
    );
}
#[test]
fn approval_receipt_id_decision_revision_and_listing_cursor_cannot_cross() {
    let approve=parse(&request("managedTurn/approval/respond",json!({"intent_id":INTENT,"managed_approval_id":B,"expected_revision":1,"decision":"accept","runtime_fence":fence()}))).unwrap();
    let receipt = json!({"managed_approval_id":B,"managed_turn_id":INTENT,"kind":"command_execution","decision":"accept","state":"accepted","revision":2,"action_fingerprint":"a".repeat(64),"decided_at":"2026-10-20T00:00:00Z"});
    response_value("approval_receipt", receipt.clone())
        .unwrap()
        .correlate(&approve, &id(AUTH))
        .unwrap();
    for (field, value) in [
        ("managed_approval_id", json!(INSTANCE)),
        ("revision", json!(3)),
    ] {
        let mut bad = receipt.clone();
        bad[field] = value;
        assert!(
            response_value("approval_receipt", bad)
                .unwrap()
                .correlate(&approve, &id(AUTH))
                .is_err()
        );
    }
    let mut wrong = receipt;
    wrong["decision"] = json!("decline");
    wrong["state"] = json!("declined");
    assert!(
        response_value("approval_receipt", wrong)
            .unwrap()
            .correlate(&approve, &id(AUTH))
            .is_err()
    );
    let list = parse(&request(
        "managedSession/list",
        json!({"after":null,"limit":1}),
    ))
    .unwrap();
    response_value(
        "sessions",
        json!({"sessions":[session_value(A,false)],"next":"1"}),
    )
    .unwrap()
    .correlate(&list, &id(AUTH))
    .unwrap();
    assert!(
        response_value(
            "sessions",
            json!({"sessions":[session_value(A,false)],"next":"2"})
        )
        .unwrap()
        .correlate(&list, &id(AUTH))
        .is_err()
    );
}
#[test]
fn closed_noop_and_canonical_binding_context_preserve_existing_contracts() {
    let close=parse(&request("managedSession/close",json!({"intent_id":INTENT,"managed_session_id":A,"expected_revision":2,"runtime_fence":fence()}))).unwrap();
    response_value("session", session_value(A, true))
        .unwrap()
        .correlate(&close, &id(AUTH))
        .unwrap();
    let create=parse(&request("managedSession/runtime/create",json!({"intent_id":INTENT,"managed_session_id":A,"expected_session_revision":1,"workspace_root":"/project/.","runtime_fence":fence()}))).unwrap();
    let binding=response_value("binding",json!({"managed_session_id":A,"adapter_id":"codex","runtime_kind":"codex","runtime_session_id":B,"workspace_root":"/project","binding_revision":1,"created_at":"2026-10-20T00:00:00Z","updated_at":"2026-10-20T00:00:00Z"})).unwrap();
    assert_eq!(binding.correlate(&create, &id(AUTH)), Err(E::Unsupported));
    binding
        .correlate_binding_create(&create, &id(AUTH), "/project")
        .unwrap();
    assert!(
        binding
            .correlate_binding_create(&create, &id(AUTH), "/other")
            .is_err()
    );
}
#[test]
fn active_lease_projection_and_fractional_ttl_must_refuse() {
    for state in ["revoked", "expired"] {
        assert!(response_value("active_lease", lease_value(A, state, 2)).is_err());
    }
    let grant=parse(&request("executionLease/grant",json!({"intent_id":INTENT,"session_id":A,"expected_session_revision":1,"scopes":["turn_start"],"ttl_seconds":60,"runtime_fence":fence()}))).unwrap();
    let mut fractional = lease_value(A, "active", 1);
    fractional["expires_at"] = json!("2026-10-20T00:01:00.5Z");
    assert_eq!(
        response_value("lease", fractional)
            .unwrap()
            .correlate(&grant, &id(AUTH)),
        Err(E::CorrelationError)
    );
}
#[test]
fn real_stream_gap_pagination_preserves_cursor_and_refuses_stale_ordinary_events() {
    use crate::contracts::control::{ManagedTurnRecord, TurnEvents};
    let turn:ManagedTurnRecord=serde_json::from_value(json!({"managed_turn_id":B,"managed_session_id":A,"codex_thread_id":"thread-1","codex_turn_id":"turn-1","admitted_lease_id":INTENT,"admitted_lease_revision":1,"runtime_instance_id":INSTANCE,"state":"running","revision":1,"created_at":"2026-10-20T00:00:00Z","updated_at":"2026-10-20T00:00:00Z","terminal_at":null,"terminal_reason":null})).unwrap();
    for count in [0, 300] {
        let query: TurnEvents =
            serde_json::from_value(json!({"managed_turn_id":B,"after_sequence":"1","limit":3}))
                .unwrap();
        let page = crate::control::stream_page_fixture(&turn, &query, count);
        assert!(page.gap);
        let req = parse(&request(
            "managedSession/turn/events",
            serde_json::to_value(&query).unwrap(),
        ))
        .unwrap();
        response_value("turn_events", serde_json::to_value(page).unwrap())
            .unwrap()
            .correlate(&req, &id(AUTH))
            .unwrap();
    }
    let query: TurnEvents =
        serde_json::from_value(json!({"managed_turn_id":B,"after_sequence":null,"limit":3}))
            .unwrap();
    let page = crate::control::stream_page_fixture(&turn, &query, 1);
    let value = serde_json::to_value(page).unwrap();
    let mut late = serde_json::to_value(&query).unwrap();
    late["after_sequence"] = json!("100");
    let req = parse(&request("managedSession/turn/events", late)).unwrap();
    assert_eq!(
        response_value("turn_events", value.clone())
            .unwrap()
            .correlate(&req, &id(AUTH)),
        Err(E::CorrelationError)
    );
    let req = parse(&request(
        "managedSession/turn/events",
        serde_json::to_value(&query).unwrap(),
    ))
    .unwrap();
    let mut bad = value;
    bad["next_sequence"] = json!("99");
    assert!(
        response_value("turn_events", bad)
            .unwrap()
            .correlate(&req, &id(AUTH))
            .is_err()
    );
}
