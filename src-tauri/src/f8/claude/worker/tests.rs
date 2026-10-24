use super::*;
use crate::{
    adapters::claude::frames::uuid_ok,
    f8::claude::exec::{ApprovedCommand, ApprovedWrite, SandboxExecutor},
    f8::claude::worker::Identity,
};
use std::os::unix::fs::DirBuilderExt;

const SESSION: &str = "5f0c1f3e-8a42-4b0e-9a6f-0b6c1d2e3f40";
const OTHER: &str = "7a1d2c3b-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
const INFERENCE: &str = include_str!("../../../../claude-bridge/fixtures/inference-turn.jsonl");
const TOOLS: &str = include_str!("../../../../claude-bridge/fixtures/local-tools-approved.jsonl");

use crate::f8::claude::doubles::{Bridge, Exec};

// ---- helpers ---------------------------------------------------------------------------
fn workspace() -> String {
    let p = std::env::temp_dir().join(format!(
        "wyss-claude-worker-{}",
        crate::control::store::uuid().as_str()
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
    std::fs::canonicalize(&p)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned()
}
fn identity() -> Identity {
    Identity {
        managed_session: "0d0d0d0d-1111-4222-8333-444444444444".into(),
        runtime_instance: "1e1e1e1e-1111-4222-8333-555555555555".into(),
        generation: 1,
    }
}
fn config(profile: Profile, workspace: &str) -> ClaudeConfig {
    ClaudeConfig {
        identity: identity(),
        profile,
        workspace: workspace.to_owned(),
        revision: 1,
        cwd: "/run/wyss/cwd".into(),
        session: SESSION.into(),
        pinned_model: None,
        effort: None,
        upstream: None,
        api_key: None,
        turn_timeout_s: 600,
        attachment_profile: None,
    }
}
struct Rig {
    w: ClaudeWorker<Bridge>,
    bridge: Bridge,
    exec: Exec,
    ws: String,
}
fn rig(profile: Profile) -> Rig {
    let ws = workspace();
    let bridge = Bridge::default();
    let exec = Exec::default();
    let w = ClaudeWorker::new(bridge.clone(), config(profile, &ws), Box::new(exec.clone()))
        .ok()
        .unwrap();
    Rig {
        w,
        bridge,
        exec,
        ws,
    }
}
fn fixture_event(body: &str, tag: &str) -> Value {
    body.lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|v| v["e"] == tag)
        .unwrap()
}
fn hello() -> Value {
    json!({"v":1,"e":"hello","proto":1,"python":"3.14.8","sdk_version":"0.2.173"})
}
impl Rig {
    fn notices(&mut self) -> Vec<Value> {
        let mut out = Vec::new();
        loop {
            let frames = self.w.inbound(16).unwrap();
            if frames.is_empty() {
                return out;
            }
            out.extend(
                frames
                    .iter()
                    .map(|f| serde_json::from_slice::<Value>(f).unwrap()),
            );
        }
    }
    fn kinds(&mut self) -> Vec<String> {
        self.notices()
            .iter()
            .map(|n| n["kind"].as_str().unwrap().to_owned())
            .collect()
    }
    fn ready(&mut self, fixture: &str) {
        self.bridge.push(hello());
        self.bridge.push(fixture_event(fixture, "ready"));
        assert_eq!(self.kinds(), ["runtime_ready", "models"]);
    }
    fn bind(&mut self) {
        self.w
            .send(
                &serde_json::to_vec(
                    &json!({"method":"thread/start","operation_id":OTHER,"workspace":self.ws}),
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(self.kinds(), ["binding_confirmed"]);
    }
    fn start_turn(&mut self, turn: &str) {
        self.w
            .send(
                &serde_json::to_vec(&json!({"method":"turn/start","turn":turn,"input":"hello"}))
                    .unwrap(),
            )
            .unwrap();
    }
    fn attest_turn(&mut self, fixture: &str, turn: &str) {
        let mut ev = fixture_event(fixture, "turn_started");
        ev["turn"] = json!(turn);
        ev["provider_session"] = json!(SESSION);
        self.bridge.push(ev);
        assert_eq!(self.kinds(), ["turn_running"]);
    }
    fn local_turn(&mut self) {
        self.ready(TOOLS);
        self.bind();
        self.start_turn("t1");
        self.attest_turn(TOOLS, "t1");
    }
    fn propose(&mut self, action: &str, tool: &str, args: Value) {
        self.bridge.push(json!({"v":1,"e":"action_proposal","turn":"t1","action":action,"tool":tool,"args":args}));
    }
    fn failing(&mut self) -> E {
        self.w.inbound(16).expect_err("expected a failure")
    }
    fn crit(&mut self, item: &str, d: &str) -> ControlResult<()> {
        self.w
            .send_critical(format!("approval {item} {d}").as_bytes())
    }
}

// ---- construction and readiness -----------------------------------------------------------
#[test]
fn configure_is_sent_once_with_the_frozen_profile_and_the_minted_session() {
    let r = rig(Profile::Inference);
    let sent = r.bridge.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["c"], "configure");
    assert_eq!(sent[0]["profile"], "inference");
    assert_eq!(sent[0]["session"], SESSION);
    assert!(uuid_ok(SESSION));
    assert_eq!(sent[0]["model"], Value::Null);
    assert!(sent[0].get("api_key").is_none());
}

#[test]
fn a_bad_configuration_hands_the_owned_worker_back() {
    let bridge = Bridge::default();
    let mut c = config(Profile::Inference, "/w");
    c.revision = 0;
    let (e, _w) = ClaudeWorker::new(bridge.clone(), c, Box::new(Exec::default()))
        .err()
        .unwrap();
    assert_eq!(e, E::InvalidRequest);
    assert!(bridge.sent().is_empty());
    let mut c = config(Profile::Inference, "/w");
    c.pinned_model = Some("has space".into());
    assert!(ClaudeWorker::new(bridge.clone(), c, Box::new(Exec::default())).is_err());
    let mut c = config(Profile::Inference, "/w");
    c.session = "x".into();
    assert!(ClaudeWorker::new(bridge, c, Box::new(Exec::default())).is_err());
}

#[test]
fn nothing_works_before_the_attestation_passes() {
    let mut r = rig(Profile::Inference);
    let att = r.w.attachment.clone();
    assert_eq!(r.w.preflight(&att), Err(E::InvalidTransition));
    assert_eq!(
        r.w.send(br#"{"method":"model/list"}"#),
        Err(E::InvalidTransition)
    );
    assert_eq!(r.w.provider_report().unwrap().phase, Phase::Starting);
}

#[test]
fn the_real_ready_frame_makes_the_runtime_ready_and_offers_the_providers_models() {
    let mut r = rig(Profile::Inference);
    r.bridge.push(hello());
    r.bridge.push(fixture_event(INFERENCE, "ready"));
    let n = r.notices();
    assert_eq!(n[0]["kind"], "runtime_ready");
    assert_eq!(n[0]["observed_version"], "2.1.309");
    assert_eq!(n[0]["profile"], "claude_inference_v1");
    assert_eq!(n[0]["workspace"], r.ws);
    assert_eq!(n[1]["kind"], "models");
    assert_eq!(n[1]["complete"], true);
    let ids: Vec<_> = n[1]["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["model_id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "claude-opus-5-5",
            "claude-fable-5-1",
            "claude-sonnet-5-5",
            "claude-haiku-5-5"
        ]
    );
    // Offered is not entitlement.
    assert!(
        n[1]["models"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["availability"] == "unknown")
    );
    let defaults: Vec<_> = n[1]["offers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| o["is_default"] == true)
        .collect();
    assert_eq!(defaults.len(), 1);
    let att = r.w.attachment.clone();
    assert_eq!(r.w.preflight(&att), Ok(()));
    let rep = r.w.provider_report().unwrap();
    assert_eq!((rep.phase, rep.refusal), (Phase::Ready, None));
    assert_eq!(rep.build.sdk_version.as_deref(), Some("0.2.173"));
    assert_eq!(rep.offered.len(), 4);
}

#[test]
fn preflight_refuses_another_context() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    let mut other = r.w.attachment.clone();
    other.revision += 1;
    assert_eq!(r.w.preflight(&other), Err(E::GateDenied));
    other = r.w.attachment.clone();
    other.workspace.push('x');
    assert_eq!(r.w.preflight(&other), Err(E::GateDenied));
}

#[test]
fn a_refused_attestation_fails_the_worker_for_good_and_says_why() {
    let mut r = rig(Profile::Inference);
    r.bridge.push(hello());
    let mut ready = fixture_event(INFERENCE, "ready");
    ready["attestation"]["mcp_servers"] =
        json!([{"name":"managedmcp","status":"failed","source":"enterprise"}]);
    r.bridge.push(ready);
    assert_eq!(r.failing(), E::GateDenied);
    assert_eq!(r.w.refusal(), Some(Refusal::ManagedPolicyPresent));
    // Sticky: it is never ready, accepts nothing and keeps failing.
    assert_eq!(r.w.inbound(1).err(), Some(E::GateDenied));
    assert_eq!(r.w.send(br#"{"method":"model/list"}"#), Err(E::GateDenied));
    let att = r.w.attachment.clone();
    assert_eq!(r.w.preflight(&att), Err(E::GateDenied));
    let rep = r.w.provider_report().unwrap();
    assert_eq!(
        (rep.phase, rep.refusal),
        (Phase::Refused, Some("managed_policy_present"))
    );
}

#[test]
fn the_profiles_are_not_interchangeable_at_the_attestation() {
    let mut r = rig(Profile::LocalTools);
    r.bridge.push(hello());
    r.bridge.push(fixture_event(INFERENCE, "ready"));
    assert_eq!(r.failing(), E::GateDenied);
    let mut r = rig(Profile::Inference);
    r.bridge.push(hello());
    r.bridge.push(fixture_event(TOOLS, "ready"));
    assert_eq!(r.failing(), E::GateDenied);
}

#[test]
fn handshake_order_is_enforced() {
    let mut r = rig(Profile::Inference);
    r.bridge.push(fixture_event(INFERENCE, "ready"));
    assert_eq!(r.failing(), E::InvalidTransition);
    let mut r = rig(Profile::Inference);
    r.bridge.push(hello());
    r.bridge.push(hello());
    assert_eq!(r.failing(), E::InvalidTransition);
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bridge.push(fixture_event(INFERENCE, "ready"));
    assert_eq!(r.failing(), E::InvalidTransition);
}

// ---- bindings ------------------------------------------------------------------------------
#[test]
fn a_new_binding_confirms_the_authority_minted_conversation_immediately() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.w.send(
        &serde_json::to_vec(
            &json!({"method":"thread/start","operation_id":OTHER,"workspace":r.ws}),
        )
        .unwrap(),
    )
    .unwrap();
    let n = r.notices();
    assert_eq!(n[0]["provider_thread_id"], SESSION);
    assert_eq!(n[0]["operation_id"], OTHER);
    // One binding per runtime.
    let again = r.w.send(
        &serde_json::to_vec(
            &json!({"method":"thread/start","operation_id":OTHER,"workspace":r.ws}),
        )
        .unwrap(),
    );
    assert_eq!(again, Err(E::BindingAlreadyExists));
}

#[test]
fn resuming_the_minted_conversation_needs_no_restart_and_another_one_asks_the_bridge() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    let frame =
        json!({"method":"thread/resume","operation_id":OTHER,"workspace":r.ws,"thread_id":OTHER});
    r.w.send(&serde_json::to_vec(&frame).unwrap()).unwrap();
    assert_eq!(r.bridge.sent_of("bind").len(), 1);
    assert_eq!(r.bridge.sent_of("bind")[0]["resume"], OTHER);
    // Not confirmed until the bridge says it re-attested and bound.
    assert!(r.notices().is_empty());
    r.bridge.push(fixture_event(INFERENCE, "ready"));
    r.bridge.push(json!({"v":1,"e":"bound","session":OTHER}));
    let n = r.notices();
    assert_eq!(n.len(), 1);
    assert_eq!(
        (n[0]["kind"].as_str(), n[0]["provider_thread_id"].as_str()),
        (Some("binding_confirmed"), Some(OTHER))
    );

    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    let same =
        json!({"method":"thread/resume","operation_id":OTHER,"workspace":r.ws,"thread_id":SESSION});
    r.w.send(&serde_json::to_vec(&same).unwrap()).unwrap();
    assert!(r.bridge.sent_of("bind").is_empty());
    assert_eq!(r.kinds(), ["binding_confirmed"]);
}

#[test]
fn a_bound_event_for_another_conversation_or_unasked_fails() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    let f =
        json!({"method":"thread/resume","operation_id":OTHER,"workspace":r.ws,"thread_id":OTHER});
    r.w.send(&serde_json::to_vec(&f).unwrap()).unwrap();
    r.bridge.push(json!({"v":1,"e":"bound","session":SESSION}));
    assert_eq!(r.failing(), E::CorrelationError);
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bridge.push(json!({"v":1,"e":"bound","session":OTHER}));
    assert_eq!(r.failing(), E::InvalidTransition);
}

#[test]
fn binding_refuses_another_workspace_and_a_resume_after_a_turn() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    let f = json!({"method":"thread/start","operation_id":OTHER,"workspace":"/somewhere/else"});
    assert_eq!(
        r.w.send(&serde_json::to_vec(&f).unwrap()),
        Err(E::GateDenied)
    );
    r.w.send(
        &serde_json::to_vec(
            &json!({"method":"thread/start","operation_id":OTHER,"workspace":r.ws}),
        )
        .unwrap(),
    )
    .unwrap();
    r.kinds();
    r.start_turn("t1");
    r.attest_turn(INFERENCE, "t1");
    let f =
        json!({"method":"thread/resume","operation_id":OTHER,"workspace":r.ws,"thread_id":OTHER});
    assert_eq!(
        r.w.send(&serde_json::to_vec(&f).unwrap()),
        Err(E::BindingAlreadyExists)
    );
}

// ---- turns ---------------------------------------------------------------------------------
#[test]
fn a_whole_real_inference_turn_replays_through_the_normalized_vocabulary() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    r.start_turn("t1");
    assert_eq!(r.bridge.sent_of("prompt")[0]["text"], "hello");
    let mut kinds = Vec::new();
    for line in INFERENCE.lines() {
        let mut v: Value = serde_json::from_str(line).unwrap();
        if matches!(v["e"].as_str(), Some("hello" | "ready")) {
            continue;
        }
        if v.get("turn").is_some() {
            v["turn"] = json!("t1");
        }
        if v["e"] == "turn_started" {
            v["provider_session"] = json!(SESSION);
        }
        r.bridge.push(v);
        kinds.extend(r.kinds());
    }
    assert_eq!(kinds.first().map(String::as_str), Some("turn_running"));
    assert_eq!(kinds.last().map(String::as_str), Some("turn_completed"));
    assert!(kinds.iter().filter(|k| *k == "content").count() >= 3);
    let rep = r.w.provider_report().unwrap();
    assert_eq!(rep.usage.turns.len(), 1);
    assert_eq!(rep.usage.turns[0].total.output, Some(7));
    assert_eq!(rep.usage.turns[0].completeness, Completeness::Complete);
    assert!(rep.tap.requests >= 1 && rep.tap.violations == 0);
    assert!(rep.tap.tools_seen.is_empty());
    assert_eq!(rep.tap.efforts_seen, ["medium"]);
}

#[test]
fn a_turn_needs_a_binding_and_only_one_runs_at_a_time() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    let f = serde_json::to_vec(&json!({"method":"turn/start","turn":"t1","input":"x"})).unwrap();
    assert_eq!(r.w.send(&f), Err(E::BindingNotFound));
    r.bind();
    r.w.send(&f).unwrap();
    assert_eq!(r.w.send(&f), Err(E::ActiveTurnExists));
    assert_eq!(r.bridge.sent_of("prompt").len(), 1);
}

#[test]
fn prompts_are_bounded_and_the_frame_must_be_well_formed() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    let big = serde_json::to_vec(
        &json!({"method":"turn/start","turn":"t1","input":"x".repeat(MAX_PROMPT_BYTES + 1)}),
    )
    .unwrap();
    assert_eq!(r.w.send(&big), Err(E::FrameTooLarge));
    for bad in [
        br#"{"method":"turn/start","input":"x"}"#.as_slice(),
        br#"{"method":"turn/start","turn":"t1"}"#,
        br#"{"method":"exec"}"#,
        br#"{"method":"turn/start","turn":"t","turn":"u","input":"x"}"#,
        b"not json",
    ] {
        assert!(r.w.send(bad).is_err());
    }
    assert!(r.bridge.sent_of("prompt").is_empty());
}

#[test]
fn text_outside_an_attested_turn_is_a_failure_not_output() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    r.bridge
        .push(json!({"v":1,"e":"text","turn":"t1","text":"surprise"}));
    assert_eq!(r.failing(), E::CorrelationError);

    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    r.start_turn("t1");
    r.bridge
        .push(json!({"v":1,"e":"text","turn":"t1","text":"before the inventory was checked"}));
    assert_eq!(r.failing(), E::GateDenied);

    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    r.start_turn("t1");
    r.attest_turn(INFERENCE, "t1");
    r.bridge
        .push(json!({"v":1,"e":"text","turn":"another","text":"x"}));
    assert_eq!(r.failing(), E::CorrelationError);
}

#[test]
fn total_output_per_turn_is_bounded() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    r.start_turn("t1");
    r.attest_turn(INFERENCE, "t1");
    let chunk = "a".repeat(16 * 1024);
    let mut failed = None;
    for _ in 0..80 {
        r.bridge
            .push(json!({"v":1,"e":"text","turn":"t1","text":chunk}));
        match r.w.inbound(16) {
            Ok(_) => (),
            Err(e) => {
                failed = Some(e);
                break;
            }
        }
    }
    assert_eq!(failed, Some(E::CapacityExceeded));
}

#[test]
fn the_conversation_the_provider_reports_must_be_the_bound_one() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    r.start_turn("t1");
    let mut ev = fixture_event(INFERENCE, "turn_started");
    ev["turn"] = json!("t1");
    ev["provider_session"] = json!(OTHER);
    r.bridge.push(ev);
    assert_eq!(r.failing(), E::CorrelationError);
}

#[test]
fn a_turn_inventory_that_fails_the_policy_refuses_before_any_text_is_forwarded() {
    type Mutate = fn(&mut Value);
    let cases: [(Mutate, Refusal); 4] = [
        (
            |e: &mut Value| e["init"]["tools"] = json!(["Bash"]),
            Refusal::ExecutableToolSurface,
        ),
        (
            |e: &mut Value| {
                e["init"]["mcp_servers"] =
                    json!([{"name":"x","status":"connected","source":"project"}])
            },
            Refusal::McpServerPresent,
        ),
        (
            |e: &mut Value| e["init"]["plugins"] = json!([{"name":"p","source":"p@local"}]),
            Refusal::PluginPresent,
        ),
        (
            |e: &mut Value| e["init"]["permission_mode"] = json!("bypassPermissions"),
            Refusal::PermissionModeUnexpected,
        ),
    ];
    for (mutate, expected) in cases {
        let mut r = rig(Profile::Inference);
        r.ready(INFERENCE);
        r.bind();
        r.start_turn("t1");
        let mut ev = fixture_event(INFERENCE, "turn_started");
        ev["turn"] = json!("t1");
        ev["provider_session"] = json!(SESSION);
        mutate(&mut ev);
        r.bridge.push(ev);
        r.bridge
            .push(json!({"v":1,"e":"text","turn":"t1","text":"never shown"}));
        assert_eq!(r.failing(), E::GateDenied);
        assert_eq!(r.w.refusal(), Some(expected));
        assert!(r.w.inbound(16).is_err());
    }
}

#[test]
fn the_api_tap_is_an_independent_leg() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bridge.push(json!({"v":1,"e":"tap_request","path":"/v1/messages","tools":["Bash"],"model":"m","effort":null,"thinking":null}));
    assert_eq!(r.failing(), E::GateDenied);
    assert_eq!(r.w.refusal(), Some(Refusal::ProviderRequestOffersTool));
    assert_eq!(r.w.provider_report().unwrap().tap.violations, 1);

    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bridge
        .push(json!({"v":1,"e":"tap_violation","why":"tool_not_allowed:Bash"}));
    assert_eq!(r.failing(), E::GateDenied);

    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bridge.push(json!({"v":1,"e":"tap_request","path":"/v1/files","tools":[],"model":null,"effort":null,"thinking":null}));
    assert_eq!(r.failing(), E::GateDenied);
    assert_eq!(r.w.refusal(), Some(Refusal::ProviderRequestUnexpectedPath));
}

#[test]
fn a_pinned_model_must_be_the_one_the_provider_receives() {
    let ws = workspace();
    let bridge = Bridge::default();
    let mut c = config(Profile::Inference, &ws);
    c.pinned_model = Some("claude-sonnet-5-5".into());
    let mut w = ClaudeWorker::new(bridge.clone(), c, Box::new(Exec::default()))
        .ok()
        .unwrap();
    assert_eq!(bridge.sent()[0]["model"], "claude-sonnet-5-5");
    bridge.push(hello());
    bridge.push(fixture_event(INFERENCE, "ready"));
    w.inbound(16).unwrap();
    bridge.push(json!({"v":1,"e":"tap_request","path":"/v1/messages","tools":[],"model":"claude-opus-5-5","effort":null,"thinking":null}));
    assert_eq!(w.inbound(16).err(), Some(E::GateDenied));
    assert_eq!(w.refusal(), Some(Refusal::ModelMismatch));
}

#[test]
fn a_tool_call_the_model_was_never_offered_is_a_refusal() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    r.start_turn("t1");
    r.attest_turn(INFERENCE, "t1");
    r.bridge
        .push(json!({"v":1,"e":"tool_use","turn":"t1","name":"Bash"}));
    assert_eq!(r.failing(), E::GateDenied);
    assert_eq!(r.w.refusal(), Some(Refusal::ExecutableToolSurface));

    let mut r = rig(Profile::LocalTools);
    r.local_turn();
    r.bridge
        .push(json!({"v":1,"e":"tool_use","turn":"t1","name":"Bash"}));
    assert_eq!(r.failing(), E::GateDenied);

    let mut r = rig(Profile::LocalTools);
    r.local_turn();
    r.bridge
        .push(json!({"v":1,"e":"tool_use","turn":"t1","name":"mcp__wyss__run_command"}));
    assert!(r.w.inbound(16).is_ok());
}

#[test]
fn unknown_notices_events_and_fatal_reports_fail_the_worker() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    r.start_turn("t1");
    r.bridge
        .push(json!({"v":1,"e":"notice","turn":"t1","subtype":"ui_invalidate"}));
    r.bridge
        .push(json!({"v":1,"e":"notice","turn":"t1","subtype":"status"}));
    assert!(r.w.inbound(16).is_ok());
    r.bridge
        .push(json!({"v":1,"e":"notice","turn":"t1","subtype":"api_retry"}));
    r.bridge
        .push(json!({"v":1,"e":"notice","turn":"t1","subtype":"permission_denied"}));
    assert!(r.w.inbound(16).is_ok());
    assert_eq!(r.w.provider_report().unwrap().denied_attempts, 1);
    for unqualified in [
        "stop_hook_summary",
        "memory_saved",
        "local_command",
        "task_started",
        "plugin_install",
        "mystery",
    ] {
        let mut r = rig(Profile::Inference);
        r.ready(INFERENCE);
        r.bind();
        r.start_turn("t1");
        r.bridge
            .push(json!({"v":1,"e":"notice","turn":"t1","subtype":unqualified}));
        assert_eq!(r.failing(), E::InvalidRequest, "{unqualified}");
    }
    r.bridge
        .push(json!({"v":1,"e":"notice","turn":"t1","subtype":"compact_boundary_x"}));
    assert_eq!(r.failing(), E::InvalidRequest);

    for raw in [
        br#"{"v":1,"e":"mystery"}"#.as_slice(),
        br#"{"v":1,"e":"text","turn":"t","text":"x","extra":1}"#,
        b"garbage",
        &[0xff, 0xfe],
        &vec![b'a'; 70_000],
    ] {
        let mut r = rig(Profile::Inference);
        r.ready(INFERENCE);
        r.bridge.push_raw(raw);
        assert!(r.w.inbound(16).is_err());
        assert!(r.w.inbound(16).is_err());
    }
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bridge
        .push(json!({"v":1,"e":"fatal","code":"bridge_error:KeyError"}));
    assert_eq!(r.failing(), E::ProviderRejected);
}

#[test]
fn results_map_to_completed_failed_and_interrupted() {
    for (result, expected) in [
        (
            json!({"subtype":"success","is_error":false,"terminal_reason":"completed"}),
            "turn_completed",
        ),
        (
            json!({"subtype":"error_during_execution","is_error":true,"terminal_reason":null}),
            "turn_failed",
        ),
        (
            json!({"subtype":"success","is_error":true,"terminal_reason":null,"api_error_status":529}),
            "turn_failed",
        ),
        (
            json!({"subtype":"success","is_error":false,"terminal_reason":"aborted_streaming"}),
            "turn_failed",
        ),
    ] {
        let mut r = rig(Profile::Inference);
        r.ready(INFERENCE);
        r.bind();
        r.start_turn("t1");
        r.attest_turn(INFERENCE, "t1");
        let mut ev = fixture_event(INFERENCE, "turn_result");
        ev["turn"] = json!("t1");
        for (k, v) in result.as_object().unwrap() {
            ev[k] = v.clone();
        }
        r.bridge.push(ev);
        r.bridge.push(json!({"v":1,"e":"turn_idle","turn":"t1"}));
        assert_eq!(r.kinds(), [expected], "{result}");
        // The turn is over: the next one can start.
        r.start_turn("t2");
    }
}

#[test]
fn an_unauthorized_model_substitution_is_a_refusal() {
    for subtype in [
        "model_fallback",
        "model_refusal_fallback",
        "model_consent_fallback",
    ] {
        let mut r = rig(Profile::Inference);
        r.ready(INFERENCE);
        r.bind();
        r.start_turn("t1");
        r.attest_turn(INFERENCE, "t1");
        r.bridge
            .push(json!({"v":1,"e":"notice","turn":"t1","subtype":subtype}));
        assert_eq!(r.failing(), E::GateDenied, "{subtype}");
        assert_eq!(r.w.refusal(), Some(Refusal::ModelSubstituted));
    }
}

#[test]
fn a_retry_storm_is_cut_off_after_a_few_provider_requests_and_is_a_failure_not_a_cancel() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    r.start_turn("t1");
    r.attest_turn(INFERENCE, "t1");
    for _ in 0..3 {
        r.bridge
            .push(json!({"v":1,"e":"notice","turn":"t1","subtype":"api_retry"}));
    }
    r.w.inbound(16).unwrap();
    assert!(
        r.bridge.sent_of("interrupt").is_empty(),
        "three retries are tolerated"
    );
    r.bridge
        .push(json!({"v":1,"e":"notice","turn":"t1","subtype":"api_retry"}));
    r.w.inbound(16).unwrap();
    assert_eq!(r.bridge.sent_of("interrupt").len(), 1);
    r.bridge
        .push(json!({"v":1,"e":"notice","turn":"t1","subtype":"api_retry"}));
    r.w.inbound(16).unwrap();
    assert_eq!(r.bridge.sent_of("interrupt").len(), 1, "told once");
    // The CLI reports the abort as an error result; the authority cut it off, so it failed.
    let mut ev = fixture_event(INFERENCE, "turn_result");
    ev["turn"] = json!("t1");
    ev["subtype"] = json!("error_during_execution");
    ev["is_error"] = json!(true);
    ev["terminal_reason"] = json!("aborted_streaming");
    r.bridge.push(ev);
    r.bridge.push(json!({"v":1,"e":"turn_idle","turn":"t1"}));
    assert_eq!(r.kinds(), ["turn_failed"]);
}

#[test]
fn a_provider_error_class_is_recorded_and_an_unknown_one_fails_the_worker() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    r.start_turn("t1");
    r.attest_turn(INFERENCE, "t1");
    for kind in ["authentication_failed", "rate_limit", "server_error"] {
        r.bridge
            .push(json!({"v":1,"e":"provider_error","turn":"t1","kind":kind}));
        r.w.inbound(16).unwrap();
        assert_eq!(
            r.w.provider_report().unwrap().provider_error.as_deref(),
            Some(kind)
        );
    }
    r.bridge
        .push(json!({"v":1,"e":"provider_error","turn":"t1","kind":"mystery_error"}));
    assert_eq!(r.failing(), E::InvalidRequest);
    assert_eq!(
        r.w.provider_report().unwrap().failure.as_deref(),
        Some("provider_error:mystery_error")
    );
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bridge
        .push(json!({"v":1,"e":"provider_error","turn":"nope","kind":"server_error"}));
    assert_eq!(r.failing(), E::CorrelationError);
}

#[test]
fn a_user_stop_that_the_cli_reports_as_an_error_is_still_an_interruption() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    r.start_turn("t1");
    r.attest_turn(INFERENCE, "t1");
    r.w.send(br#"{"method":"turn/interrupt"}"#).unwrap();
    let mut ev = fixture_event(INFERENCE, "turn_result");
    ev["turn"] = json!("t1");
    ev["subtype"] = json!("error_during_execution");
    ev["is_error"] = json!(true);
    ev["terminal_reason"] = json!("aborted_streaming");
    r.bridge.push(ev);
    r.bridge.push(json!({"v":1,"e":"turn_idle","turn":"t1"}));
    assert_eq!(r.kinds(), ["turn_interrupted"]);
    let rep = r.w.provider_report().unwrap();
    assert_eq!(rep.usage.turns[0].completeness, Completeness::ProviderError);
}

#[test]
fn an_abort_nobody_asked_for_is_a_failure() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    r.start_turn("t1");
    r.attest_turn(INFERENCE, "t1");
    let mut ev = fixture_event(INFERENCE, "turn_result");
    ev["turn"] = json!("t1");
    ev["terminal_reason"] = json!("aborted_streaming");
    r.bridge.push(ev);
    r.bridge.push(json!({"v":1,"e":"turn_idle","turn":"t1"}));
    assert_eq!(r.kinds(), ["turn_failed"]);
}

#[test]
fn an_interrupt_is_forwarded_and_marks_the_turn_interrupted() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    assert_eq!(
        r.w.send(br#"{"method":"turn/interrupt"}"#),
        Err(E::InvalidTransition)
    );
    r.start_turn("t1");
    r.attest_turn(INFERENCE, "t1");
    r.w.send(br#"{"method":"turn/interrupt"}"#).unwrap();
    assert_eq!(r.bridge.sent_of("interrupt")[0]["turn"], "t1");
    let mut ev = fixture_event(INFERENCE, "turn_result");
    ev["turn"] = json!("t1");
    ev["terminal_reason"] = json!("aborted_streaming");
    r.bridge.push(ev);
    // The turn is not over for the authority until the bridge can take another prompt.
    assert!(r.kinds().is_empty());
    r.bridge.push(json!({"v":1,"e":"turn_idle","turn":"t1"}));
    assert_eq!(r.kinds(), ["turn_interrupted"]);
}

#[test]
fn the_authority_sees_a_turn_end_only_when_the_bridge_is_idle_again() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    r.start_turn("t1");
    r.attest_turn(INFERENCE, "t1");
    let mut ev = fixture_event(INFERENCE, "turn_result");
    ev["turn"] = json!("t1");
    r.bridge.push(ev);
    assert!(r.kinds().is_empty(), "a result alone is not the end");
    // Nothing else may start in the meantime: the bridge has not said it can take a prompt.
    let f = serde_json::to_vec(&json!({"method":"turn/start","turn":"t2","input":"x"})).unwrap();
    assert_eq!(r.w.send(&f), Err(E::ActiveTurnExists));
    r.bridge.push(json!({"v":1,"e":"turn_idle","turn":"t1"}));
    assert_eq!(r.kinds(), ["turn_completed"]);
    assert_eq!(r.w.send(&f), Ok(()));
}

#[test]
fn a_turn_that_goes_idle_without_a_result_fails_and_its_usage_is_not_invented() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    r.start_turn("t1");
    r.attest_turn(INFERENCE, "t1");
    r.bridge.push(json!({"v":1,"e":"usage","turn":"t1","message_id":"m1","model":"claude-opus-5-5","input":11,"output":1,"cache_creation":null,"cache_read":null}));
    r.bridge
        .push(json!({"v":1,"e":"error","turn":"t1","code":"sdk_error:CLIConnectionError"}));
    r.bridge.push(json!({"v":1,"e":"turn_idle","turn":"t1"}));
    assert_eq!(r.kinds(), ["turn_failed"]);
    let rep = r.w.provider_report().unwrap();
    let t = &rep.usage.turns[0];
    assert_eq!(t.completeness, Completeness::Unreported);
    assert_eq!((t.total.input, t.total.cache_read), (Some(11), None));
}

#[test]
fn rate_limit_reports_are_kept_and_unknown_names_are_ignored() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bridge.push(json!({"v":1,"e":"rate_limit","status":"allowed_warning","type":"five_hour","resets_at":1900000000,"utilization":0.72}));
    r.bridge.push(json!({"v":1,"e":"rate_limit","status":"allowed","type":"monthly","resets_at":1,"utilization":0.1}));
    r.w.inbound(16).unwrap();
    let limits = r.w.provider_report().unwrap().usage.limits;
    assert_eq!(limits.len(), 1);
    assert_eq!(limits[0].utilization, Some(0.72));
}

// ---- approvals (local tools) -----------------------------------------------------------------
#[test]
fn a_command_proposal_is_held_with_the_exact_command_and_nothing_runs() {
    let mut r = rig(Profile::LocalTools);
    r.local_turn();
    r.propose(
        "act-1",
        "run_command",
        json!({"command":"rm -rf build && echo done"}),
    );
    let n = r.notices();
    assert_eq!(n.len(), 1);
    assert_eq!(n[0]["kind"], "approval_required");
    assert_eq!(n[0]["approval_kind"], "command_execution");
    assert_eq!(n[0]["provider_item_id"], "claude-act-1");
    assert_eq!(n[0]["action"]["command"], "rm -rf build && echo done");
    assert_eq!(n[0]["action"]["cwd"], r.ws);
    assert_eq!(n[0]["action_fingerprint"].as_str().unwrap().len(), 64);
    assert!(r.exec.0.lock().unwrap().dispatched.is_empty());
    assert_eq!(r.w.approval_decisions("claude-act-1").len(), 3);
    assert!(r.w.approval_decisions("claude-act-9").is_empty());
    assert_eq!(r.w.provider_report().unwrap().pending_approvals, 1);
}

#[test]
fn accepting_dispatches_exactly_once_and_reports_the_outcome_to_the_model() {
    let mut r = rig(Profile::LocalTools);
    r.local_turn();
    r.propose("act-1", "run_command", json!({"command":"echo hi"}));
    r.notices();
    r.crit("claude-act-1", "accept").unwrap();
    assert_eq!(r.kinds(), ["approval_item_consumed"]);
    {
        let s = r.exec.0.lock().unwrap();
        assert_eq!(s.dispatched.len(), 1);
        let Approved::Command(c) = &s.dispatched[0] else {
            panic!()
        };
        assert_eq!(
            (c.command.as_str(), c.cwd.as_str()),
            ("echo hi", r.ws.as_str())
        );
    }
    // Still running: the model has not been told anything yet.
    assert!(r.bridge.sent_of("action_result").is_empty());
    assert!(r.w.approval_decisions("claude-act-1").is_empty());
    r.exec.0.lock().unwrap().outcome = Some(Outcome::Completed("exit status 0\nhi\n".into()));
    assert_eq!(r.kinds(), ["approval_item_confirmed"]);
    let res = r.bridge.sent_of("action_result");
    assert_eq!(res.len(), 1);
    assert_eq!(
        (
            res[0]["outcome"].as_str(),
            res[0]["output"].as_str(),
            res[0]["action"].as_str()
        ),
        (
            Some("completed"),
            Some("exit status 0\nhi\n"),
            Some("act-1")
        )
    );
}

/// Found by the isolated desktop run: the executor's namespace is only reaped when somebody
/// polls its stop, and nobody did once the outcome was in, so the next turn was refused for good.
#[test]
fn a_finished_action_is_reaped_and_the_next_turn_is_admitted() {
    let mut r = rig(Profile::LocalTools);
    r.local_turn();
    r.propose("act-1", "run_command", json!({"command":"echo hi"}));
    r.notices();
    r.crit("claude-act-1", "accept").unwrap();
    r.kinds();
    r.exec.0.lock().unwrap().outcome = Some(Outcome::Completed("exit status 0\nhi\n".into()));
    assert_eq!(r.kinds(), ["approval_item_confirmed"]);
    let mut ev = fixture_event(TOOLS, "turn_result");
    ev["turn"] = json!("t1");
    r.bridge.push(ev);
    r.bridge.push(json!({"v":1,"e":"turn_idle","turn":"t1"}));
    assert_eq!(r.kinds(), ["turn_completed"]);
    assert!(
        r.w.send(br#"{"method":"turn/start","turn":"t2","input":"again"}"#)
            .is_ok(),
        "the executor must not stay busy after its outcome was delivered"
    );
}

#[test]
fn a_second_answer_for_the_same_approval_dispatches_nothing() {
    let mut r = rig(Profile::LocalTools);
    r.local_turn();
    r.propose("act-1", "run_command", json!({"command":"echo hi"}));
    r.notices();
    r.crit("claude-act-1", "accept").unwrap();
    for d in ["accept", "decline", "cancel"] {
        assert_eq!(r.crit("claude-act-1", d), Err(E::CorrelationError), "{d}");
    }
    assert_eq!(r.exec.0.lock().unwrap().dispatched.len(), 1);
}

#[test]
fn declining_and_cancelling_tell_the_model_and_never_reach_the_executor() {
    for (d, outcome) in [("decline", "denied"), ("cancel", "cancelled")] {
        let mut r = rig(Profile::LocalTools);
        r.local_turn();
        r.propose("act-1", "run_command", json!({"command":"echo hi"}));
        r.notices();
        r.crit("claude-act-1", d).unwrap();
        assert_eq!(r.kinds(), ["approval_item_consumed"]);
        assert_eq!(r.bridge.sent_of("action_result")[0]["outcome"], outcome);
        assert!(r.exec.0.lock().unwrap().dispatched.is_empty());
        assert_eq!(r.crit("claude-act-1", "accept"), Err(E::CorrelationError));
        assert!(r.exec.0.lock().unwrap().dispatched.is_empty());
    }
}

#[test]
fn an_unknown_item_or_malformed_decision_is_refused() {
    let mut r = rig(Profile::LocalTools);
    r.local_turn();
    r.propose("act-1", "run_command", json!({"command":"echo hi"}));
    r.notices();
    assert_eq!(r.crit("claude-act-9", "accept"), Err(E::ApprovalNotFound));
    for bad in [
        b"approval claude-act-1".as_slice(),
        b"approval claude-act-1 accept now",
        b"grant claude-act-1 accept",
        b"approval claude-act-1 yes",
        &[0xff],
    ] {
        assert!(r.w.send_critical(bad).is_err());
    }
    assert!(r.exec.0.lock().unwrap().dispatched.is_empty());
    // The refusals above did not consume the decision.
    assert!(r.crit("claude-act-1", "accept").is_ok());
}

#[test]
fn acceptance_is_refused_when_the_scope_is_gone_at_the_moment_of_the_effect() {
    let mut r = rig(Profile::LocalTools);
    r.local_turn();
    r.propose("act-1", "run_command", json!({"command":"echo hi"}));
    r.notices();
    std::fs::remove_dir_all(&r.ws).unwrap();
    assert_eq!(r.crit("claude-act-1", "accept"), Err(E::GateDenied));
    assert!(r.exec.0.lock().unwrap().dispatched.is_empty());
    let res = r.bridge.sent_of("action_result");
    assert_eq!(res[0]["outcome"], "failed");
    assert_eq!(r.crit("claude-act-1", "accept"), Err(E::CorrelationError));
}

#[test]
fn a_dispatch_that_could_not_start_is_a_failure_that_is_not_confirmed_or_retried() {
    let mut r = rig(Profile::LocalTools);
    r.local_turn();
    r.propose("act-1", "run_command", json!({"command":"echo hi"}));
    r.notices();
    r.exec.0.lock().unwrap().dispatch_error = Some(E::TransportError);
    assert_eq!(r.crit("claude-act-1", "accept"), Err(E::TransportError));
    assert_eq!(r.bridge.sent_of("action_result")[0]["outcome"], "failed");
    assert!(r.kinds().is_empty());
    r.exec.0.lock().unwrap().dispatch_error = None;
    assert_eq!(r.crit("claude-act-1", "accept"), Err(E::CorrelationError));
    assert!(r.exec.0.lock().unwrap().dispatched.is_empty());
}

#[test]
fn an_outcome_nobody_can_vouch_for_is_indeterminate_and_the_model_is_told_not_to_retry() {
    let mut r = rig(Profile::LocalTools);
    r.local_turn();
    r.propose("act-1", "run_command", json!({"command":"echo hi"}));
    r.notices();
    r.crit("claude-act-1", "accept").unwrap();
    r.kinds();
    r.exec.0.lock().unwrap().outcome = Some(Outcome::Indeterminate);
    assert!(r.kinds().is_empty(), "no confirmation");
    assert_eq!(
        r.bridge.sent_of("action_result")[0]["outcome"],
        "indeterminate"
    );
    assert_eq!(r.w.provider_report().unwrap().indeterminate_actions, 1);
}

#[test]
fn a_failed_effect_is_not_a_confirmation() {
    let mut r = rig(Profile::LocalTools);
    r.local_turn();
    r.propose("act-1", "write_file", json!({"path":"a.txt","content":"x"}));
    r.notices();
    r.crit("claude-act-1", "accept").unwrap();
    r.kinds();
    r.exec.0.lock().unwrap().outcome = Some(Outcome::Failed(
        "the file changed after the preview was made; nothing was written".into(),
    ));
    assert!(r.kinds().is_empty());
    assert_eq!(r.bridge.sent_of("action_result")[0]["outcome"], "failed");
}

#[test]
fn proposals_the_user_should_never_be_asked_about_are_refused_to_the_model() {
    for (tool, args) in [
        ("write_file", json!({"path":"../escape","content":"x"})),
        (
            "write_file",
            json!({"path":".claude/settings.json","content":"{}"}),
        ),
        (
            "write_file",
            json!({"path":".git/hooks/pre-commit","content":"#!/bin/sh"}),
        ),
        ("run_command", json!({"command":""})),
        ("run_command", json!({"command":"ls","cwd":"/"})),
        ("shell", json!({"command":"ls"})),
    ] {
        let mut r = rig(Profile::LocalTools);
        r.local_turn();
        r.propose("act-1", tool, args.clone());
        assert!(r.notices().is_empty(), "{tool} {args}");
        let res = r.bridge.sent_of("action_result");
        assert_eq!(res.len(), 1, "{tool} {args}");
        assert_eq!(res[0]["outcome"], "failed");
        assert!(res[0]["output"].as_str().unwrap().starts_with("refused:"));
        assert_eq!(r.w.provider_report().unwrap().refused_proposals, 1);
        // The runtime is still healthy: a refusal is about the proposal, not the provider.
        r.propose("act-2", "run_command", json!({"command":"echo ok"}));
        assert_eq!(r.kinds(), ["approval_required"]);
    }
}

#[test]
fn provider_misbehaviour_around_proposals_fails_the_worker() {
    // A second proposal while one is held.
    let mut r = rig(Profile::LocalTools);
    r.local_turn();
    r.propose("act-1", "run_command", json!({"command":"echo 1"}));
    r.notices();
    r.propose("act-2", "run_command", json!({"command":"echo 2"}));
    assert_eq!(r.failing(), E::IntentConflict);
    // A repeated action id.
    let mut r = rig(Profile::LocalTools);
    r.local_turn();
    r.propose("act-1", "run_command", json!({"command":"echo 1"}));
    r.notices();
    r.crit("claude-act-1", "decline").unwrap();
    r.kinds();
    r.propose("act-1", "run_command", json!({"command":"echo 1"}));
    assert_eq!(r.failing(), E::IntentConflict);
    // A proposal on the profile that offers no tools, and before the turn was attested.
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    r.start_turn("t1");
    r.attest_turn(INFERENCE, "t1");
    r.propose("act-1", "run_command", json!({"command":"id"}));
    assert_eq!(r.failing(), E::GateDenied);
    let mut r = rig(Profile::LocalTools);
    r.ready(TOOLS);
    r.bind();
    r.start_turn("t1");
    r.propose("act-1", "run_command", json!({"command":"id"}));
    assert_eq!(r.failing(), E::GateDenied);
}

#[test]
fn approvals_need_the_local_tools_profile() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    assert_eq!(r.w.send_critical(b"approval x accept"), Err(E::Unsupported));
}

#[test]
fn a_turn_ending_drops_what_was_held_and_stopping_clears_everything() {
    let mut r = rig(Profile::LocalTools);
    r.local_turn();
    r.propose("act-1", "run_command", json!({"command":"echo 1"}));
    r.notices();
    r.bridge.push(json!({"v":1,"e":"turn_idle","turn":"t1"}));
    r.w.inbound(16).unwrap();
    assert!(r.w.approval_decisions("claude-act-1").is_empty());
    assert_eq!(r.crit("claude-act-1", "accept"), Err(E::ApprovalNotFound));
    assert!(r.exec.0.lock().unwrap().dispatched.is_empty());

    let mut r = rig(Profile::LocalTools);
    r.local_turn();
    r.propose("act-1", "run_command", json!({"command":"echo 1"}));
    r.notices();
    r.w.begin_stop(Instant::now());
    assert!(r.exec.0.lock().unwrap().stopped);
    assert!(r.bridge.0.lock().unwrap().stopping);
    assert!(r.w.approval_decisions("claude-act-1").is_empty());
    assert!(r.crit("claude-act-1", "accept").is_err());
    assert!(r.w.send(br#"{"method":"model/list"}"#).is_err());
    assert_eq!(r.w.poll_stop(Instant::now()), StopState::Reaped);
}

#[test]
fn the_slot_stays_consumed_until_both_the_bridge_and_the_executor_are_reaped() {
    let mut r = rig(Profile::LocalTools);
    r.local_turn();
    r.bridge.0.lock().unwrap().stop = Some(StopState::Pending);
    assert_eq!(r.w.poll_stop(Instant::now()), StopState::Pending);
    r.bridge.0.lock().unwrap().stop = Some(StopState::Stuck);
    assert_eq!(r.w.poll_stop(Instant::now()), StopState::Stuck);
    r.bridge.0.lock().unwrap().stop = None;
    assert_eq!(r.w.poll_stop(Instant::now()), StopState::Reaped);
}

#[test]
fn exits_and_pids_come_from_the_owned_bridge() {
    let mut r = rig(Profile::Inference);
    assert!(!r.w.has_exited());
    r.bridge.0.lock().unwrap().exited = true;
    assert!(r.w.has_exited());
    assert_eq!(r.w.containment(), Containment::PidNamespace);
}

#[test]
fn local_tools_requires_the_proved_namespace_at_preflight() {
    struct Loose(Bridge);
    impl Worker for Loose {
        fn send(&mut self, f: &[u8]) -> ControlResult<()> {
            self.0.send(f)
        }
        fn inbound(&mut self, n: usize) -> ControlResult<Vec<Vec<u8>>> {
            self.0.inbound(n)
        }
    }
    let ws = workspace();
    let bridge = Bridge::default();
    let mut w = ClaudeWorker::new(
        Loose(bridge.clone()),
        config(Profile::LocalTools, &ws),
        Box::new(Exec::default()),
    )
    .ok()
    .unwrap();
    bridge.push(hello());
    bridge.push(fixture_event(TOOLS, "ready"));
    w.inbound(16).unwrap();
    let att = w.attachment.clone();
    assert_eq!(w.preflight(&att), Err(E::GateDenied));
}

#[test]
fn the_bridge_backing_up_is_reported_not_swallowed() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    r.bridge.0.lock().unwrap().fail_send = true;
    let f = serde_json::to_vec(&json!({"method":"turn/start","turn":"t1","input":"x"})).unwrap();
    assert_eq!(r.w.send(&f), Err(E::CapacityExceeded));
}

#[test]
fn the_report_holds_no_prompt_output_or_credential() {
    let ws = workspace();
    let bridge = Bridge::default();
    let mut c = config(Profile::Inference, &ws);
    c.api_key = Some(ApiKey::new("sk-ant-SECRETKEYVALUE".into()).unwrap());
    let mut w = ClaudeWorker::new(bridge.clone(), c, Box::new(Exec::default()))
        .ok()
        .unwrap();
    bridge.push(hello());
    bridge.push(fixture_event(INFERENCE, "ready"));
    w.inbound(16).unwrap();
    w.send(
        &serde_json::to_vec(&json!({"method":"thread/start","operation_id":OTHER,"workspace":ws}))
            .unwrap(),
    )
    .unwrap();
    w.inbound(16).unwrap();
    w.send(
        &serde_json::to_vec(
            &json!({"method":"turn/start","turn":"t1","input":"PRIVATE PROMPT TEXT"}),
        )
        .unwrap(),
    )
    .unwrap();
    let report = serde_json::to_string(&w.provider_report().unwrap()).unwrap();
    for secret in ["PRIVATE PROMPT TEXT", "SECRETKEYVALUE", "sk-ant"] {
        assert!(!report.contains(secret), "{secret}");
    }
}

// ---- the real executor, end to end through the worker ----------------------------------------
fn real_rig(ws: &str) -> (ClaudeWorker<Bridge>, Bridge) {
    let bridge = Bridge::default();
    let mut w = ClaudeWorker::new(
        bridge.clone(),
        config(Profile::LocalTools, ws),
        Box::new(SandboxExecutor::new()),
    )
    .ok()
    .unwrap();
    bridge.push(hello());
    bridge.push(fixture_event(TOOLS, "ready"));
    w.inbound(16).unwrap();
    w.send(
        &serde_json::to_vec(&json!({"method":"thread/start","operation_id":OTHER,"workspace":ws}))
            .unwrap(),
    )
    .unwrap();
    w.inbound(16).unwrap();
    w.send(&serde_json::to_vec(&json!({"method":"turn/start","turn":"t1","input":"go"})).unwrap())
        .unwrap();
    let mut ev = fixture_event(TOOLS, "turn_started");
    ev["turn"] = json!("t1");
    ev["provider_session"] = json!(SESSION);
    bridge.push(ev);
    w.inbound(16).unwrap();
    (w, bridge)
}
fn drive(w: &mut ClaudeWorker<Bridge>) -> Vec<Value> {
    let end = Instant::now() + std::time::Duration::from_secs(30);
    let mut out = Vec::new();
    while Instant::now() < end {
        for f in w.inbound(16).unwrap() {
            out.push(serde_json::from_slice::<Value>(&f).unwrap());
        }
        if out.iter().any(|n| n["kind"] == "approval_item_confirmed") || !w.exec_busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    out
}
impl<W: Worker> ClaudeWorker<W> {
    fn exec_busy(&self) -> bool {
        self.exec.busy()
    }
}

#[test]
fn a_real_approved_file_write_lands_once_and_a_real_command_runs_in_the_namespace() {
    let ws = workspace();
    let (mut w, bridge) = real_rig(&ws);
    bridge.push(json!({"v":1,"e":"action_proposal","turn":"t1","action":"act-1","tool":"write_file","args":{"path":"notes.txt","content":"one\ntwo\n"}}));
    let n: Vec<Value> = w
        .inbound(16)
        .unwrap()
        .iter()
        .map(|f| serde_json::from_slice(f).unwrap())
        .collect();
    assert_eq!(n[0]["approval_kind"], "file_change");
    assert!(
        n[0]["action"]["changes"][0]["diff"]
            .as_str()
            .unwrap()
            .contains("+one\n+two\n")
    );
    assert!(
        !std::path::Path::new(&format!("{ws}/notes.txt")).exists(),
        "nothing before the decision"
    );
    w.send_critical(b"approval claude-act-1 accept").unwrap();
    let out = drive(&mut w);
    assert!(
        out.iter().any(|n| n["kind"] == "approval_item_confirmed"),
        "{out:?}"
    );
    assert_eq!(
        std::fs::read_to_string(format!("{ws}/notes.txt")).unwrap(),
        "one\ntwo\n"
    );
    assert_eq!(bridge.sent_of("action_result")[0]["outcome"], "completed");

    bridge.push(json!({"v":1,"e":"action_proposal","turn":"t1","action":"act-2","tool":"run_command","args":{"command":"cat notes.txt | wc -l > count.txt; cat count.txt"}}));
    w.inbound(16).unwrap();
    w.send_critical(b"approval claude-act-2 accept").unwrap();
    let out = drive(&mut w);
    assert!(
        out.iter().any(|n| n["kind"] == "approval_item_confirmed"),
        "{out:?}"
    );
    assert_eq!(
        std::fs::read_to_string(format!("{ws}/count.txt"))
            .unwrap()
            .trim(),
        "2"
    );
    let res = bridge.sent_of("action_result");
    assert_eq!(res[1]["output"], "exit status 0\n2\n");
}

#[test]
fn a_file_that_changes_between_the_preview_and_the_accept_is_not_overwritten() {
    let ws = workspace();
    let (mut w, bridge) = real_rig(&ws);
    std::fs::write(format!("{ws}/a.txt"), "original\n").unwrap();
    bridge.push(json!({"v":1,"e":"action_proposal","turn":"t1","action":"act-1","tool":"write_file","args":{"path":"a.txt","content":"model's version\n"}}));
    w.inbound(16).unwrap();
    // Someone edits the file while the question is open.
    std::fs::write(format!("{ws}/a.txt"), "edited meanwhile\n").unwrap();
    w.send_critical(b"approval claude-act-1 accept").unwrap();
    let out = drive(&mut w);
    assert!(!out.iter().any(|n| n["kind"] == "approval_item_confirmed"));
    assert_eq!(
        std::fs::read_to_string(format!("{ws}/a.txt")).unwrap(),
        "edited meanwhile\n"
    );
    let res = bridge.sent_of("action_result");
    assert_eq!(
        res[0]["outcome"], "failed",
        "the executor refused: the effect did not happen"
    );
    assert!(
        res[0]["output"]
            .as_str()
            .unwrap()
            .contains("nothing was written")
    );
}

#[test]
fn the_unused_command_types_still_compile_into_the_worker_tests() {
    // Keeps the imported approval payload types exercised by this module.
    let c = ApprovedCommand {
        command: "x".into(),
        cwd: "/".into(),
        workspace: "/".into(),
    };
    let w = ApprovedWrite {
        path: "/a".into(),
        workspace: "/".into(),
        content: String::new(),
        expected_old: None,
    };
    assert!(
        matches!(Approved::Command(c), Approved::Command(_))
            && matches!(Approved::Write(w), Approved::Write(_))
    );
}

// ---- model selection (ADR0054 engineering, gate closed in production) ------------------------
use crate::m3::model::{EffortEvidence, ModelRequest, Refusal as ModelRefusal, Reroute};

fn model_req(m: &str) -> ModelRequest {
    ModelRequest::new(m, None).unwrap()
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
fn model_prompt(r: &Rig) -> Option<String> {
    r.bridge
        .sent_of("prompt")
        .last()
        .and_then(|p| p["model"].as_str().map(str::to_owned))
}

#[test]
fn the_public_request_entry_refuses_while_adr0054_is_not_accepted() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    assert_eq!(
        r.w.request_model(model_req("sonnet"), now()),
        Err(ModelRefusal::GateClosed)
    );
    r.bind();
    r.start_turn("t1");
    assert_eq!(
        model_prompt(&r),
        None,
        "nothing was requested, nothing was sent"
    );
}

#[test]
fn a_request_before_the_provider_listed_its_models_is_refused() {
    let mut r = rig(Profile::Inference);
    assert_eq!(
        r.w.request_model_with_gate(model_req("sonnet"), now()),
        Err(ModelRefusal::CatalogueIncomplete)
    );
}

#[test]
fn a_validated_request_travels_with_exactly_one_turn_as_the_resolved_model() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    r.w.request_model_with_gate(model_req("sonnet"), now())
        .unwrap();
    r.start_turn("t1");
    assert_eq!(
        model_prompt(&r).as_deref(),
        Some("claude-sonnet-5-5"),
        "the alias is resolved by the catalogue"
    );
    r.attest_turn(INFERENCE, "t1");
    // The provider request carries that model; the tap accepts it and refuses any other.
    r.bridge.push(json!({"v":1,"e":"tap_request","path":"/v1/messages","tools":[],"model":"claude-sonnet-5-5","effort":null,"thinking":null}));
    assert!(r.w.inbound(16).is_ok());
    let mut ev = fixture_event(INFERENCE, "turn_result");
    ev["turn"] = json!("t1");
    r.bridge.push(ev);
    r.bridge.push(json!({"v":1,"e":"turn_idle","turn":"t1"}));
    r.kinds();
    // The next turn asks for nothing: the request was one-shot.
    r.start_turn("t2");
    assert_eq!(model_prompt(&r), None);
}

#[test]
fn the_tap_refuses_a_different_model_than_the_one_requested_for_the_turn() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    r.w.request_model_with_gate(model_req("sonnet"), now())
        .unwrap();
    r.start_turn("t1");
    r.attest_turn(INFERENCE, "t1");
    r.bridge.push(json!({"v":1,"e":"tap_request","path":"/v1/messages","tools":[],"model":"claude-opus-5-5","effort":null,"thinking":null}));
    assert_eq!(r.failing(), E::GateDenied);
    assert_eq!(r.w.refusal(), Some(Refusal::ModelMismatch));
}

#[test]
fn unknown_models_and_per_turn_effort_are_refused_for_claude() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    for name in ["gpt-6", "default", "claude-opus-4"] {
        assert_eq!(
            r.w.request_model_with_gate(model_req(name), now()),
            Err(ModelRefusal::UnknownModel),
            "{name}"
        );
    }
    let with_effort = ModelRequest::new("opus", Some("high")).unwrap();
    assert_eq!(
        r.w.request_model_with_gate(with_effort, now()),
        Err(ModelRefusal::EffortNotPerTurn)
    );
}

#[test]
fn a_catalogue_that_aged_out_admits_nothing() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    let later = now() + crate::m3::model::CATALOGUE_MAX_AGE_SECS + 60;
    assert_eq!(
        r.w.request_model_with_gate(model_req("sonnet"), later),
        Err(ModelRefusal::CatalogueStale)
    );
}

#[test]
fn a_request_is_revalidated_at_dispatch_and_a_changed_catalogue_refuses_the_turn() {
    let mut r = rig(Profile::Inference);
    r.ready(INFERENCE);
    r.bind();
    r.w.request_model_with_gate(model_req("sonnet"), now())
        .unwrap();
    // The provider's listing is observed again (a new revision) before the turn is sent.
    let att = r.w.attestation.clone().unwrap();
    r.w.observe_catalogue(&att);
    let f = serde_json::to_vec(&json!({"method":"turn/start","turn":"t1","input":"x"})).unwrap();
    assert_eq!(r.w.send(&f), Err(E::GateDenied));
    assert!(
        r.bridge.sent_of("prompt").is_empty(),
        "nothing reached the bridge"
    );
    // The stale request is gone: a plain turn may follow, with no model.
    r.w.send(&f).unwrap();
    assert_eq!(model_prompt(&r), None);
}

#[test]
fn the_four_facts_and_a_reroute_are_reported_and_never_merged() {
    let ws = workspace();
    let bridge = Bridge::default();
    let mut c = config(Profile::Inference, &ws);
    c.pinned_model = Some("claude-opus-5-5".into());
    let w = ClaudeWorker::new(bridge.clone(), c, Box::new(Exec::default()))
        .ok()
        .unwrap();
    let mut r = Rig {
        w,
        bridge,
        exec: Exec::default(),
        ws,
    };
    r.ready(INFERENCE);
    r.bind();
    r.w.request_model_with_gate(model_req("sonnet"), now())
        .unwrap();
    r.start_turn("t1");
    r.attest_turn(INFERENCE, "t1");
    r.bridge.push(json!({"v":1,"e":"tap_request","path":"/v1/messages","tools":[],"model":"claude-sonnet-5-5","effort":"medium","thinking":"adaptive"}));
    // The provider names a different model as having served the turn.
    r.bridge.push(json!({"v":1,"e":"usage","turn":"t1","message_id":"m1","model":"claude-haiku-5-5","input":5,"output":1,"cache_creation":null,"cache_read":null}));
    r.w.inbound(16).unwrap();
    let facts = r.w.provider_report().unwrap().model.unwrap();
    let offered = facts.offered.unwrap();
    assert_eq!(offered.entries.len(), 4);
    assert_eq!(
        offered.entitlement,
        crate::m3::model::Entitlement::NotChecked
    );
    assert_eq!(facts.configured.unwrap().model, "claude-opus-5-5");
    assert_eq!(facts.requested.unwrap().resolved_model, "claude-sonnet-5-5");
    assert_eq!(facts.reported_in_use, ["claude-haiku-5-5"]);
    assert!(
        matches!(facts.reroute, Reroute::Rerouted { .. }),
        "{:?}",
        facts.reroute
    );
    // The CLI sent its own default effort; nobody asked for one, so it is not presented as chosen.
    assert_eq!(facts.effort, EffortEvidence::NotRequested);
}

#[test]
fn honored_when_the_provider_names_the_requested_model_and_the_request_effort_is_observed_not_proven()
 {
    let ws = workspace();
    let bridge = Bridge::default();
    let mut c = config(Profile::Inference, &ws);
    c.effort = Some("low".into());
    let w = ClaudeWorker::new(bridge.clone(), c, Box::new(Exec::default()))
        .ok()
        .unwrap();
    let mut r = Rig {
        w,
        bridge,
        exec: Exec::default(),
        ws,
    };
    assert_eq!(
        r.bridge.sent()[0]["effort"],
        "low",
        "the runtime-level effort reaches the CLI at start"
    );
    r.ready(INFERENCE);
    r.bind();
    r.w.request_model_with_gate(model_req("sonnet"), now())
        .unwrap();
    r.start_turn("t1");
    r.attest_turn(INFERENCE, "t1");
    r.bridge.push(json!({"v":1,"e":"tap_request","path":"/v1/messages","tools":[],"model":"claude-sonnet-5-5","effort":"low","thinking":"adaptive"}));
    r.bridge.push(json!({"v":1,"e":"usage","turn":"t1","message_id":"m1","model":"claude-sonnet-5-5","input":5,"output":1,"cache_creation":null,"cache_read":null}));
    r.w.inbound(16).unwrap();
    let facts = r.w.provider_report().unwrap().model.unwrap();
    assert_eq!(facts.reroute, Reroute::Honored);
    assert_eq!(
        facts.effort,
        EffortEvidence::ObservedInProviderRequest("low".into())
    );
}

#[test]
fn runtime_pins_are_checked_when_the_worker_is_built() {
    let ws = workspace();
    let mut c = config(Profile::Inference, &ws);
    c.effort = Some("not an effort".into());
    assert!(ClaudeWorker::new(Bridge::default(), c, Box::new(Exec::default())).is_err());
}

// ---- the lifecycle of each tool call, as the report tells it -----------------------------------
use crate::m3::report::ActionState;

fn states(r: &Rig) -> Vec<(String, ActionState)> {
    r.w.provider_report()
        .unwrap()
        .actions
        .into_iter()
        .map(|a| (a.item, a.state))
        .collect()
}

#[test]
fn every_tool_call_leaves_a_record_of_how_it_ended() {
    let mut r = rig(Profile::LocalTools);
    r.local_turn();
    r.propose("act-1", "run_command", json!({"command":"echo 1"}));
    r.notices();
    assert_eq!(states(&r), [("claude-act-1".into(), ActionState::Held)]);
    r.crit("claude-act-1", "accept").unwrap();
    assert_eq!(states(&r)[0].1, ActionState::Running);
    r.kinds();
    r.exec.0.lock().unwrap().outcome = Some(Outcome::Completed("exit status 0\n1\n".into()));
    r.kinds();
    assert_eq!(states(&r)[0].1, ActionState::Completed);
    // A second call: declined.
    r.propose("act-2", "run_command", json!({"command":"echo 2"}));
    r.notices();
    r.crit("claude-act-2", "decline").unwrap();
    // A third: cancelled.
    r.kinds();
    r.propose("act-3", "run_command", json!({"command":"echo 3"}));
    r.notices();
    r.crit("claude-act-3", "cancel").unwrap();
    // A fourth: refused before the user was asked.
    r.kinds();
    r.propose("act-4", "write_file", json!({"path":"../x","content":"y"}));
    r.notices();
    let got = states(&r);
    let by: std::collections::BTreeMap<_, _> = got.iter().cloned().collect();
    assert_eq!(by["claude-act-2"], ActionState::Declined);
    assert_eq!(by["claude-act-3"], ActionState::Cancelled);
    assert_eq!(by["claude-act-4"], ActionState::Refused);
    let refused =
        r.w.provider_report()
            .unwrap()
            .actions
            .into_iter()
            .find(|a| a.item == "claude-act-4")
            .unwrap();
    assert_eq!(refused.refusal, Some("out_of_scope"));
    assert_eq!(refused.tool, "write_file");
}

#[test]
fn failed_and_indeterminate_outcomes_are_distinct_states() {
    for (outcome, expected) in [
        (
            Outcome::Failed("nothing was written".into()),
            ActionState::Failed,
        ),
        (Outcome::Indeterminate, ActionState::Indeterminate),
    ] {
        let mut r = rig(Profile::LocalTools);
        r.local_turn();
        r.propose("act-1", "run_command", json!({"command":"echo 1"}));
        r.notices();
        r.crit("claude-act-1", "accept").unwrap();
        r.kinds();
        r.exec.0.lock().unwrap().outcome = Some(outcome);
        r.kinds();
        assert_eq!(states(&r)[0].1, expected);
    }
}

#[test]
fn the_action_log_is_bounded_and_holds_no_action_content() {
    let mut r = rig(Profile::LocalTools);
    r.local_turn();
    for i in 0..40 {
        r.propose(
            &format!("act-{i}"),
            "run_command",
            json!({"command": format!("echo SECRET-{i}")}),
        );
        r.notices();
        r.crit(&format!("claude-act-{i}"), "decline").unwrap();
        r.kinds();
    }
    let rep = r.w.provider_report().unwrap();
    assert_eq!(rep.actions.len(), 32);
    assert_eq!(rep.actions[0].item, "claude-act-8");
    assert!(!serde_json::to_string(&rep).unwrap().contains("SECRET-"));
}
