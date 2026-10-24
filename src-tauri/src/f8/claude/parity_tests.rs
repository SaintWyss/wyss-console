//! The Claude worker against the real engine, authority and durable writers (schema 8, in-memory
//! SQLite): the same protocol-2 conversation the Codex tests drive, with a Claude worker standing
//! where Codex's does. The provider side is the scripted bridge double fed with frames the real SDK
//! produced; the executor is a recording double (the real one has its own tests).
//!
//! The accepted wire still names the Codex profile strings, so these workers present
//! `codex_local_tools_v1` as their attachment profile. That aliasing exists only in this test
//! module: it demonstrates that nothing above the `Worker` seam depends on the provider.
use super::*;
use crate::{
    adapters::claude::Profile,
    f8::claude::{
        doubles::{Bridge, Exec},
        exec::{Approved, Outcome},
        worker::{ClaudeConfig, ClaudeWorker},
    },
};
use std::collections::BTreeMap;

const CONV: &str = "5f0c1f3e-8a42-4b0e-9a6f-0b6c1d2e3f40";
const TOOLS: &str = include_str!("../../../claude-bridge/fixtures/local-tools-approved.jsonl");

type Registry = Arc<Mutex<BTreeMap<String, (Bridge, Exec)>>>;

fn fixture_event(tag: &str) -> Value {
    TOOLS
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|v| v["e"] == tag)
        .unwrap()
}
fn install(rig: &mut Rig) -> Registry {
    let workers: Registry = Arc::default();
    let registry = workers.clone();
    let ws = rig.workspace.to_str().unwrap().to_owned();
    rig.spawn.factory = Some(Arc::new(move |launch: &RuntimeRef| {
        let (bridge, exec) = (Bridge::default(), Exec::default());
        registry.lock().unwrap().insert(
            launch.managed_session_id.as_str().to_owned(),
            (bridge.clone(), exec.clone()),
        );
        let cfg = ClaudeConfig {
            identity: crate::f8::claude::worker::Identity {
                managed_session: launch.managed_session_id.as_str().to_owned(),
                runtime_instance: launch.runtime_instance_id.as_str().to_owned(),
                generation: launch.generation,
            },
            profile: Profile::LocalTools,
            workspace: ws.clone(),
            revision: 1,
            cwd: "/run/wyss/cwd".into(),
            session: CONV.into(),
            pinned_model: None,
            effort: None,
            upstream: None,
            api_key: None,
            turn_timeout_s: 600,
            attachment_profile: Some("codex_local_tools_v1".into()),
        };
        Box::new(
            ClaudeWorker::new(bridge, cfg, Box::new(exec))
                .ok()
                .expect("a valid configuration"),
        )
    }));
    workers
}
fn tick_until(rig: &mut Rig, what: &str, mut done: impl FnMut(&mut Rig) -> bool) {
    for _ in 0..200 {
        rig.authority.tick(Instant::now()).unwrap();
        if done(rig) {
            return;
        }
    }
    panic!("never reached: {what}");
}
fn runtime_state(rig: &Rig, session: &str) -> Option<&'static str> {
    rig.authority.engine().runtime_view(session).map(|r| r.0)
}
struct Live {
    reference: RuntimeRef,
    fence: Value,
    bridge: Bridge,
    exec: Exec,
    session: String,
}
fn start(rig: &mut Rig, workers: &Registry, index: usize) -> Live {
    let s = rig.sessions[index].clone();
    let (g, r) = rig.expect(&s);
    let reply = rig.call(
        "runtime/start",
        json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex","managed_session_id":s,
            "expected_generation":g,"expected_revision":r,"safety_profile":"codex_local_tools_v1",
            "workspace_root":rig.workspace.to_str().unwrap()}),
    );
    assert_eq!(reply["result"]["kind"], "runtime_control", "{reply}");
    let (bridge, exec) = workers
        .lock()
        .unwrap()
        .get(&s)
        .cloned()
        .expect("the factory built the worker");
    bridge.push(json!({"v":1,"e":"hello","proto":1,"python":"3.14.8","sdk_version":"0.2.173"}));
    bridge.push(fixture_event("ready"));
    let sc = s.clone();
    tick_until(rig, "runtime ready", |rig| {
        runtime_state(rig, &sc) == Some("ready")
    });
    let reference = rig.authority.engine().reference(&s).unwrap();
    let fence = json!({"managed_session_id":s,"generation":reference.generation,"runtime_instance_id":reference.runtime_instance_id});
    Live {
        reference,
        fence,
        bridge,
        exec,
        session: s,
    }
}
fn grant(rig: &mut Rig, l: &Live) -> String {
    let lease = rig.call(
        "executionLease/grant",
        json!({"intent_id":uuid(),"session_id":l.session,"expected_session_revision":1,
            "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":l.fence}),
    );
    assert_eq!(lease["result"]["kind"], "lease", "{lease}");
    lease["result"]["value"]["lease_id"]
        .as_str()
        .unwrap()
        .to_owned()
}
fn bind_new(rig: &mut Rig, l: &Live) {
    let reply = rig.call(
        "managedSession/runtime/create",
        json!({"intent_id":uuid(),"managed_session_id":l.session,"expected_session_revision":1,
            "workspace_root":rig.workspace.to_str().unwrap(),"runtime_fence":l.fence}),
    );
    assert_eq!(code(&reply), "runtime_operation_in_progress", "{reply}");
    let s = l.session.clone();
    tick_until(rig, "binding confirmed", |rig| {
        rig.authority
            .engine()
            .connection()
            .query_row::<i64, _, _>(
                "SELECT count(*) FROM managed_session_bindings WHERE managed_session_id=?1",
                [&s],
                |r| r.get(0),
            )
            .unwrap()
            == 1
    });
}
fn begin_turn(rig: &mut Rig, l: &Live, prompt: &str) -> String {
    let turn = rig.call(
        "managedSession/turn/start",
        json!({"intent_id":uuid(),"managed_session_id":l.session,"expected_session_revision":1,
            "prompt":prompt,"runtime_fence":l.fence}),
    );
    assert_eq!(turn["result"]["kind"], "turn", "{turn}");
    turn["result"]["value"]["managed_turn_id"]
        .as_str()
        .unwrap()
        .to_owned()
}
fn turn_state(rig: &Rig, id: &str) -> String {
    rig.authority
        .engine()
        .connection()
        .query_row(
            "SELECT state FROM managed_turns WHERE managed_turn_id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
}
fn attest(l: &Live, turn: &str) {
    let mut ev = fixture_event("turn_started");
    ev["turn"] = json!(turn);
    ev["provider_session"] = json!(CONV);
    l.bridge.push(ev);
}
fn proposal(l: &Live, turn: &str, action: &str, command: &str) {
    l.bridge.push(json!({"v":1,"e":"action_proposal","turn":turn,"action":action,"tool":"run_command","args":{"command":command}}));
}

#[test]
fn a_claude_runtime_completes_the_authority_handshake_and_publishes_its_models() {
    let mut rig = Rig::new();
    rig.hello();
    let workers = install(&mut rig);
    let l = start(&mut rig, &workers, 1);
    let status = rig.call(
        "runtime/status",
        json!({"adapter_id":"codex","runtime_kind":"codex","managed_session_id":l.session}),
    );
    assert_eq!(
        status["result"]["value"]["actual_state"], "ready",
        "{status}"
    );
    assert_eq!(
        status["result"]["value"]["runtime_identity"]["observed_version"],
        "2.1.309"
    );
    let models = rig.call(
        "runtime/model/list",
        json!({"adapter_id":"codex","runtime_kind":"codex","managed_session_id":l.session,
            "expected_generation":l.reference.generation,"expected_runtime_instance_id":l.reference.runtime_instance_id,"cursor":null,"limit":20}),
    );
    assert_eq!(models["result"]["kind"], "runtime_models", "{models}");
    let ids: Vec<_> = models["result"]["value"]["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["model_id"].as_str().unwrap().to_owned())
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
    // The first frame the bridge received is its configuration; nothing else was sent.
    assert_eq!(l.bridge.sent().len(), 1);
    assert_eq!(l.bridge.sent()[0]["c"], "configure");
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn a_claude_runtime_that_fails_attestation_never_becomes_ready_and_is_stopped() {
    let mut rig = Rig::new();
    rig.hello();
    let workers = install(&mut rig);
    let s = rig.sessions[1].clone();
    let (g, r) = rig.expect(&s);
    rig.call(
        "runtime/start",
        json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex","managed_session_id":s,
            "expected_generation":g,"expected_revision":r,"safety_profile":"codex_local_tools_v1",
            "workspace_root":rig.workspace.to_str().unwrap()}),
    );
    let (bridge, _) = workers.lock().unwrap().get(&s).cloned().unwrap();
    bridge.push(json!({"v":1,"e":"hello","proto":1,"python":"3.14.8","sdk_version":"0.2.173"}));
    let mut ready = fixture_event("ready");
    ready["attestation"]["mcp_servers"] =
        json!([{"name":"managedmcp","status":"failed","source":"enterprise"}]);
    bridge.push(ready);
    let sc = s.clone();
    tick_until(&mut rig, "the runtime is stopped", |rig| {
        runtime_state(rig, &sc) != Some("ready") && rig.authority.engine().reference(&sc).is_none()
    });
    let status = rig.call(
        "runtime/status",
        json!({"adapter_id":"codex","runtime_kind":"codex","managed_session_id":s}),
    );
    assert_ne!(
        status["result"]["value"]["actual_state"], "ready",
        "{status}"
    );
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn the_conversation_binds_runs_a_turn_and_streams_through_the_durable_writers() {
    let mut rig = Rig::new();
    rig.hello();
    let workers = install(&mut rig);
    let l = start(&mut rig, &workers, 1);
    grant(&mut rig, &l);
    bind_new(&mut rig, &l);
    let thread: String = rig
        .authority
        .engine()
        .connection()
        .query_row(
            "SELECT runtime_session_id FROM managed_session_bindings WHERE managed_session_id=?1",
            [&l.session],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        thread, CONV,
        "the binding carries the conversation id the authority minted"
    );
    let turn = begin_turn(&mut rig, &l, "please explain this repository");
    assert_eq!(
        l.bridge.sent_of("prompt")[0]["text"],
        "please explain this repository"
    );
    assert_eq!(l.bridge.sent_of("prompt")[0]["turn"], turn.as_str());
    attest(&l, &turn);
    l.bridge
        .push(json!({"v":1,"e":"text","turn":turn,"text":"It is a desktop console "}));
    l.bridge
        .push(json!({"v":1,"e":"text","turn":turn,"text":"for managed agents."}));
    let mut result = fixture_event("turn_result");
    result["turn"] = json!(turn);
    l.bridge.push(result);
    l.bridge.push(json!({"v":1,"e":"turn_idle","turn":turn}));
    tick_until(&mut rig, "turn completed", |rig| {
        turn_state(rig, &turn) == "completed"
    });
    // The streamed text is durable and readable through the protocol, in order.
    let events = rig.call(
        "managedSession/turn/events",
        json!({"managed_turn_id":turn,"after_sequence":null,"limit":50}),
    );
    let text = events.to_string();
    assert!(
        text.contains("It is a desktop console ") && text.contains("for managed agents."),
        "{text}"
    );
    assert_eq!(rig.authority.engine().active_turn(&l.session), None);
    // Another turn can follow: the worker is idle again.
    let second = begin_turn(&mut rig, &l, "and the tests?");
    assert_ne!(second, turn);
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn a_held_command_is_visible_exactly_and_nothing_runs_until_the_decision_is_accepted() {
    let mut rig = Rig::new();
    rig.hello();
    let workers = install(&mut rig);
    let l = start(&mut rig, &workers, 1);
    grant(&mut rig, &l);
    bind_new(&mut rig, &l);
    let turn = begin_turn(&mut rig, &l, "run the build");
    attest(&l, &turn);
    proposal(&l, &turn, "act-1", "cargo build --release");
    tick_until(&mut rig, "approval pending", |rig| {
        rig.authority.engine().pending_approvals(&l.session) == 1
    });
    assert!(
        l.exec.0.lock().unwrap().dispatched.is_empty(),
        "held before any decision"
    );
    let pending = rig.call(
        "managedTurn/approval/pending",
        json!({"managed_turn_id":turn}),
    );
    let shown = pending.to_string();
    assert!(shown.contains("command_execution"), "{shown}");
    let approval_id = pending["result"]["value"]["approval"]["managed_approval_id"]
        .as_str()
        .expect(&shown)
        .to_owned();
    // The complete exact command is what the client can page through.
    let view = rig.call(
        "managedTurn/approval/status",
        json!({"managed_approval_id":approval_id}),
    );
    assert!(view.to_string().contains("cargo build --release"), "{view}");
}

#[test]
fn accept_dispatches_once_and_the_durable_record_confirms_the_effect_only_when_it_completed() {
    let mut rig = Rig::new();
    rig.hello();
    let workers = install(&mut rig);
    let l = start(&mut rig, &workers, 1);
    grant(&mut rig, &l);
    bind_new(&mut rig, &l);
    let turn = begin_turn(&mut rig, &l, "run it");
    attest(&l, &turn);
    proposal(&l, &turn, "act-1", "echo hi");
    tick_until(&mut rig, "approval pending", |rig| {
        rig.authority.engine().pending_approvals(&l.session) == 1
    });
    let pending = rig.call(
        "managedTurn/approval/pending",
        json!({"managed_turn_id":turn}),
    );
    let approval_id = pending["result"]["value"]["approval"]["managed_approval_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let decide = uuid();
    let respond = |rig: &mut Rig| {
        rig.call(
            "managedTurn/approval/respond",
            json!({"intent_id":decide,"managed_approval_id":approval_id,"expected_revision":1,"decision":"accept","runtime_fence":l.fence}),
        )
    };
    let ok = respond(&mut rig);
    assert_eq!(ok["result"]["kind"], "approval_receipt", "{ok}");
    // Exactly one dispatch, of exactly the held command; a replayed decision dispatches nothing.
    {
        let s = l.exec.0.lock().unwrap();
        assert_eq!(s.dispatched.len(), 1);
        let Approved::Command(c) = &s.dispatched[0] else {
            panic!()
        };
        assert_eq!(c.command, "echo hi");
    }
    let again = respond(&mut rig);
    assert_eq!(again["result"]["value"], ok["result"]["value"]);
    assert_eq!(l.exec.0.lock().unwrap().dispatched.len(), 1);
    let state = |rig: &Rig| -> String {
        rig.authority
            .engine()
            .connection()
            .query_row(
                "SELECT state FROM managed_approval_delivery WHERE managed_approval_id=?1",
                [&approval_id],
                |r| r.get(0),
            )
            .unwrap()
    };
    // Dispatched but not finished: delivered, not confirmed.
    tick_until(&mut rig, "consumed", |rig| {
        state(rig) == "provider_consumed"
    });
    assert_ne!(state(&rig), "effect_confirmed");
    // The executor finishes; only then is the effect confirmed, and the model gets its result.
    l.exec.0.lock().unwrap().outcome = Some(Outcome::Completed("exit status 0\nhi\n".into()));
    tick_until(&mut rig, "confirmed", |rig| {
        state(rig) == "effect_confirmed"
    });
    assert_eq!(l.bridge.sent_of("action_result")[0]["outcome"], "completed");
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn a_declined_command_is_never_dispatched_and_the_model_is_told() {
    let mut rig = Rig::new();
    rig.hello();
    let workers = install(&mut rig);
    let l = start(&mut rig, &workers, 1);
    grant(&mut rig, &l);
    bind_new(&mut rig, &l);
    let turn = begin_turn(&mut rig, &l, "run it");
    attest(&l, &turn);
    proposal(&l, &turn, "act-1", "rm -rf /");
    tick_until(&mut rig, "approval pending", |rig| {
        rig.authority.engine().pending_approvals(&l.session) == 1
    });
    let pending = rig.call(
        "managedTurn/approval/pending",
        json!({"managed_turn_id":turn}),
    );
    let approval_id = pending["result"]["value"]["approval"]["managed_approval_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = rig.call(
        "managedTurn/approval/respond",
        json!({"intent_id":uuid(),"managed_approval_id":approval_id,"expected_revision":1,"decision":"decline","runtime_fence":l.fence}),
    );
    assert_eq!(r["result"]["kind"], "approval_receipt", "{r}");
    assert!(l.exec.0.lock().unwrap().dispatched.is_empty());
    assert_eq!(l.bridge.sent_of("action_result")[0]["outcome"], "denied");
}

#[test]
fn losing_the_runtime_after_an_accepted_dispatch_leaves_the_effect_indeterminate_and_never_replays_it()
 {
    let mut rig = Rig::new();
    rig.hello();
    let workers = install(&mut rig);
    let l = start(&mut rig, &workers, 1);
    grant(&mut rig, &l);
    bind_new(&mut rig, &l);
    let turn = begin_turn(&mut rig, &l, "run it");
    attest(&l, &turn);
    proposal(&l, &turn, "act-1", "touch marker");
    tick_until(&mut rig, "approval pending", |rig| {
        rig.authority.engine().pending_approvals(&l.session) == 1
    });
    let pending = rig.call(
        "managedTurn/approval/pending",
        json!({"managed_turn_id":turn}),
    );
    let approval_id = pending["result"]["value"]["approval"]["managed_approval_id"]
        .as_str()
        .unwrap()
        .to_owned();
    rig.call(
        "managedTurn/approval/respond",
        json!({"intent_id":uuid(),"managed_approval_id":approval_id,"expected_revision":1,"decision":"accept","runtime_fence":l.fence}),
    );
    // The bridge dies while the dispatch is in flight and nothing can say whether it finished.
    l.bridge.0.lock().unwrap().exited = true;
    let session = l.session.clone();
    tick_until(&mut rig, "runtime lost", |rig| {
        rig.authority.engine().reference(&session).is_none()
    });
    let delivery: String = rig
        .authority
        .engine()
        .connection()
        .query_row(
            "SELECT state FROM managed_approval_delivery WHERE managed_approval_id=?1",
            [&approval_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(delivery, "effect_confirmed");
    assert_eq!(turn_state(&rig, &turn), "indeterminate");
    assert_eq!(
        l.exec.0.lock().unwrap().dispatched.len(),
        1,
        "never dispatched again"
    );
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn a_provider_event_from_another_sessions_bridge_cannot_touch_this_turn() {
    let mut rig = Rig::new();
    rig.hello();
    let workers = install(&mut rig);
    let l = start(&mut rig, &workers, 1);
    grant(&mut rig, &l);
    bind_new(&mut rig, &l);
    let turn = begin_turn(&mut rig, &l, "x");
    attest(&l, &turn);
    // A frame naming a turn this worker does not own fails the worker, not the other session.
    l.bridge
        .push(json!({"v":1,"e":"text","turn":"some-other-turn","text":"smuggled"}));
    let session = l.session.clone();
    tick_until(&mut rig, "worker failed and was reaped", |rig| {
        rig.authority.engine().reference(&session).is_none()
    });
    assert_ne!(turn_state(&rig, &turn), "completed");
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn the_global_capacity_of_one_runtime_holds_for_claude_and_a_stop_frees_the_slot() {
    let mut rig = Rig::new();
    rig.hello();
    let workers = install(&mut rig);
    let first = start(&mut rig, &workers, 1);
    let (g, r) = rig.expect(&rig.sessions[0].clone());
    let second = rig.call(
        "runtime/start",
        json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex","managed_session_id":rig.sessions[0],
            "expected_generation":g,"expected_revision":r,"safety_profile":"codex_local_tools_v1",
            "workspace_root":rig.workspace.to_str().unwrap()}),
    );
    assert!(
        second.get("error").is_some(),
        "a second concurrent runtime is refused: {second}"
    );
    let stop = rig.call(
        "runtime/stop",
        json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex","managed_session_id":first.session,
            "expected_generation":first.reference.generation,"expected_revision":1,
            "expected_runtime_instance_id":first.reference.runtime_instance_id}),
    );
    assert_eq!(stop["result"]["kind"], "runtime_control", "{stop}");
    let session = first.session.clone();
    tick_until(&mut rig, "slot released", |rig| {
        rig.authority.engine().reference(&session).is_none()
    });
    assert!(
        first.bridge.0.lock().unwrap().stopping,
        "the owned bridge was told to stop"
    );
    assert!(
        first.exec.0.lock().unwrap().stopped,
        "and so was the executor"
    );
}
