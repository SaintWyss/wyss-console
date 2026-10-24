//! The Claude worker against the real engine, authority and durable writers (schema 8, in-memory
//! SQLite): the same protocol-2 conversation the Codex tests drive, with a Claude worker standing
//! where Codex's does. The provider side is the scripted bridge double fed with frames the real SDK
//! produced; the executor is a recording double (the real one has its own tests).
//!
//! **What this does and does not show.** The accepted wire and the V8 store name only Codex, so
//! these workers *present themselves as Codex*: the wire request says `codex`/`codex_local_tools_v1`
//! and the worker's attachment profile is overridden (a test-only field) to match, and the store
//! records `codex/codex`. That demonstrates that the engine, the fences and the durable writers
//! behave the same with a worker of this shape standing in the seam. It is **not** Claude admission
//! through the production authority: the second half of this file proves the production authority
//! refuses every real Claude identity, at the wire, at the handshake and in the store (ADR0055).
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
    install_presenting(rig, Some("codex_local_tools_v1"))
}
/// `presenting`: the attachment profile the worker shows the engine (`None` is its own, real one).
fn install_presenting(rig: &mut Rig, presenting: Option<&'static str>) -> Registry {
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
            broker_token: crate::adapters::claude::frames::BrokerToken::new("ab".repeat(32))
                .unwrap(),
            admission: crate::f8::claude::admission::Admission::fixture(),
            turn_timeout_s: 600,
            attachment_profile: presenting.map(str::to_owned),
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
fn a_claude_worker_presenting_as_codex_completes_the_handshake_and_publishes_its_models() {
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
fn a_claude_worker_presenting_as_codex_that_fails_attestation_never_becomes_ready() {
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

// ---- what the production authority refuses ----------------------------------------------------

/// The reply is a refusal and no worker was ever built for it.
fn assert_refused_without_a_worker(reply: &Value, workers: &Registry) {
    assert!(
        reply.get("error").is_some() || reply["result"]["kind"] != "runtime_control",
        "{reply}"
    );
    assert!(
        workers.lock().unwrap().is_empty(),
        "a worker was built: {reply}"
    );
}

#[test]
fn the_wire_cannot_name_a_claude_runtime_or_a_claude_profile() {
    let mut rig = Rig::new();
    rig.hello();
    let workers = install_presenting(&mut rig, None);
    let s = rig.sessions[1].clone();
    let (g, r) = rig.expect(&s);
    for (adapter, kind, profile) in [
        ("claude", "claude", "claude_local_tools_v1"),
        ("claude", "claude", "claude_inference_v1"),
        ("codex", "codex", "claude_local_tools_v1"),
        ("codex", "codex", "claude_inference_v1"),
        ("claude", "claude", "codex_local_tools_v1"),
        ("claude_code", "claude_code", "codex_local_tools_v1"),
    ] {
        let reply = rig.call(
            "runtime/start",
            json!({"intent_id":uuid(),"adapter_id":adapter,"runtime_kind":kind,"managed_session_id":s,
                "expected_generation":g,"expected_revision":r,"safety_profile":profile,
                "workspace_root":rig.workspace.to_str().unwrap()}),
        );
        assert_refused_without_a_worker(&reply, &workers);
    }
    assert_eq!(runtime_state(&rig, &s), None);
}

/// The only way left to get a worker in front of the handshake is to ask for Codex; a worker that
/// then shows its own, real profile is refused, which is why the aliasing above is needed at all.
#[test]
fn a_claude_worker_showing_its_real_profile_is_refused_at_the_handshake() {
    let mut rig = Rig::new();
    rig.hello();
    let workers = install_presenting(&mut rig, None);
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
    bridge.push(fixture_event("ready"));
    let sc = s.clone();
    tick_until(&mut rig, "the runtime is refused and stopped", |rig| {
        rig.authority.engine().reference(&sc).is_none()
    });
    assert_ne!(runtime_state(&rig, &s), Some("ready"));
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

/// The same fact one layer down: the accepted V8 schema rejects a Claude row itself, in each of
/// the three tables ADR0055 has to rebuild. Each Codex control fails for a *different* reason
/// (it is the CHECK that speaks), so the assertion is about the constraint and not about luck.
#[test]
fn the_v8_store_rejects_a_claude_runtime_launch_and_binding_by_constraint() {
    let rig = Rig::new();
    let conn = rig.authority.engine().connection();
    let attempt = |table: &str, adapter: &str| -> String {
        let columns: Vec<(String, String)> = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap()
            .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
            .unwrap()
            .flatten()
            .collect();
        let names: Vec<_> = columns.iter().map(|c| c.0.as_str()).collect();
        let values: Vec<String> = columns
            .iter()
            .map(|(n, t)| match n.as_str() {
                "adapter_id" | "runtime_kind" => format!("'{adapter}'"),
                _ if t.eq_ignore_ascii_case("INTEGER") => "1".to_owned(),
                _ => format!("'{n}'"),
            })
            .collect();
        match conn.execute(
            &format!(
                "INSERT INTO {table}({}) VALUES({})",
                names.join(","),
                values.join(",")
            ),
            [],
        ) {
            Ok(_) => String::new(),
            Err(e) => e.to_string(),
        }
    };
    for table in [
        "managed_runtimes",
        "managed_runtime_launches",
        "managed_session_bindings",
    ] {
        // Every one of the three declares the Codex-only CHECKs in its own definition.
        let sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            sql.contains("CHECK(adapter_id='codex')")
                && sql.contains("CHECK(runtime_kind='codex')"),
            "{table}: {sql}"
        );
        let claude = attempt(table, "claude");
        assert!(!claude.is_empty(), "{table}: a Claude row was accepted");
        // A reservation trigger fires before the CHECK on the launches table (a launch must
        // be reserved first), so only the other two can be asked to say it in words.
        if table != "managed_runtime_launches" {
            assert!(
                claude.contains("CHECK constraint failed") && claude.contains("'codex'"),
                "{table}: {claude}"
            );
            let codex = attempt(table, "codex");
            assert!(
                !codex.contains("CHECK constraint failed: adapter_id")
                    && !codex.contains("CHECK constraint failed: runtime_kind"),
                "{table}: the control failed for the same reason: {codex}"
            );
        }
    }
}

#[path = "v9_rehearsal_tests.rs"]
mod v9_rehearsal;
