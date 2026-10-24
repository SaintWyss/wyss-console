//! The exact JSON the window receives, checked in as golden files that the TypeScript decoder
//! parses (`tests/m3-contract.test.ts`). The Rust projection and the TypeScript decoder cannot
//! drift apart without one of the two suites failing.
//!
//! Regenerate with `WYSS_UPDATE_GOLDEN=1 cargo test --lib m3::golden`; a normal run compares.
use super::{
    fixture::FixtureModel,
    gui::{FixtureSource, FixtureView, M3Service},
    model::ModelRequest,
};
use crate::{
    adapters::claude::Profile,
    f8::{
        claude::{
            doubles::{Bridge, Exec},
            exec::Outcome,
            worker::{ClaudeConfig, ClaudeWorker, Identity},
        },
        engine::Worker,
    },
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

const CONV: &str = "5f0c1f3e-8a42-4b0e-9a6f-0b6c1d2e3f40";
const INFERENCE: &str = include_str!("../../claude-bridge/fixtures/inference-turn.jsonl");
const TOOLS: &str = include_str!("../../claude-bridge/fixtures/local-tools-approved.jsonl");

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/m3")
}
fn event(body: &str, tag: &str) -> Value {
    body.lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|v| v["e"] == tag)
        .unwrap()
}
/// Wall-clock values are replaced so the files are reproducible.
fn normalize(v: &mut Value) {
    match v {
        Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                if k == "observed_at" {
                    *x = json!(1_800_000_000u64);
                } else {
                    normalize(x);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(normalize),
        _ => (),
    }
}
fn check(name: &str, mut actual: Value) {
    normalize(&mut actual);
    let path = golden_dir().join(name);
    let pretty = serde_json::to_string_pretty(&actual).unwrap() + "\n";
    if std::env::var("WYSS_UPDATE_GOLDEN").is_ok() {
        std::fs::create_dir_all(golden_dir()).unwrap();
        std::fs::write(&path, &pretty).unwrap();
    }
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "{} is missing: regenerate with WYSS_UPDATE_GOLDEN=1",
            path.display()
        )
    });
    // Compared as data: the file may be reformatted by the repository's formatter.
    let on_disk: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        on_disk, actual,
        "{name} differs from the projection: regenerate with WYSS_UPDATE_GOLDEN=1 and review the diff"
    );
}

struct Host {
    w: ClaudeWorker<Bridge>,
    bridge: Bridge,
    exec: Exec,
    model: FixtureModel,
    ws: String,
}
impl Host {
    fn new(profile: Profile) -> Self {
        let dir = std::env::temp_dir().join("wyss-m3-golden-ws");
        std::fs::create_dir_all(&dir).unwrap();
        let ws = std::fs::canonicalize(dir)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let (bridge, exec) = (Bridge::default(), Exec::default());
        let cfg = ClaudeConfig {
            identity: Identity {
                managed_session: "0d0d0d0d-1111-4222-8333-444444444444".into(),
                runtime_instance: "1e1e1e1e-1111-4222-8333-555555555555".into(),
                generation: 1,
            },
            profile,
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
            attachment_profile: None,
        };
        let w = ClaudeWorker::new(bridge.clone(), cfg, Box::new(exec.clone()))
            .ok()
            .unwrap();
        let mut model = FixtureModel::new();
        model.starting(
            profile
                .attachment_name()
                .strip_suffix("_v1")
                .map_or("claude_inference_v1", |_| profile.attachment_name()),
        );
        Self {
            w,
            bridge,
            exec,
            model,
            ws,
        }
    }
    fn pump(&mut self) {
        loop {
            let frames = self.w.inbound(16).unwrap_or_else(|e| {
                panic!(
                    "the worker failed: {e:?} {:?}",
                    self.w.provider_report().map(|r| r.failure)
                )
            });
            if frames.is_empty() {
                break;
            }
            for f in frames {
                let v: Value = serde_json::from_slice(&f).unwrap();
                if self.model.apply(&v) == super::fixture::Reaction::Bind {
                    let frame = json!({"method":"thread/start","operation_id":"11111111-2222-4333-8444-555555555555","workspace":self.ws});
                    self.w.send(&serde_json::to_vec(&frame).unwrap()).unwrap();
                }
            }
        }
    }
    fn ready(&mut self, fixture: &str) {
        self.bridge
            .push(json!({"v":1,"e":"hello","proto":1,"python":"3.14.8","sdk_version":"0.2.173"}));
        self.bridge.push(event(fixture, "ready"));
        self.pump();
    }
    fn prompt(&mut self, turn: &str, text: &str, fixture: &str) {
        self.w
            .send(
                &serde_json::to_vec(&json!({"method":"turn/start","turn":turn,"input":text}))
                    .unwrap(),
            )
            .unwrap();
        self.model.prompt(turn, text);
        let mut ev = event(fixture, "turn_started");
        ev["turn"] = json!(turn);
        ev["provider_session"] = json!(CONV);
        self.bridge.push(ev);
        self.pump();
    }
    fn finish(&mut self, turn: &str, fixture: &str) {
        let mut r = event(fixture, "turn_result");
        r["turn"] = json!(turn);
        self.bridge.push(r);
        self.bridge.push(json!({"v":1,"e":"turn_idle","turn":turn}));
        self.pump();
    }
    fn view(&self) -> FixtureView {
        self.model.view(self.w.provider_report())
    }
}
struct Src(FixtureView);
impl FixtureSource for Src {
    fn view(&self) -> FixtureView {
        self.0.clone()
    }
}
fn state_with(view: FixtureView) -> Value {
    serde_json::to_value(M3Service::with(None, Some(Arc::new(Src(view)))).state(true)).unwrap()
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[test]
fn golden_inactive() {
    check(
        "state-inactive.json",
        serde_json::to_value(M3Service::inactive().state(false)).unwrap(),
    );
}

#[test]
fn golden_inference_with_usage_limits_model_facts_and_a_reroute() {
    let mut h = Host::new(Profile::Inference);
    h.ready(INFERENCE);
    // A first turn the provider never reported usage for.
    let t1 = "t0000001-0000-4000-8000-000000000001";
    h.prompt(t1, "and the tests?", INFERENCE);
    h.bridge
        .push(json!({"v":1,"e":"error","turn":t1,"code":"sdk_error:CLIConnectionError"}));
    h.bridge.push(json!({"v":1,"e":"turn_idle","turn":t1}));
    h.pump();
    // A second turn with a model asked for, usage, a limit and an answer from another model.
    let t2 = "t0000002-0000-4000-8000-000000000002";
    h.w.request_model_with_gate(ModelRequest::new("sonnet", None).unwrap(), now())
        .unwrap();
    h.prompt(t2, "please explain this repository", INFERENCE);
    h.bridge.push(json!({"v":1,"e":"tap_request","path":"/v1/messages","tools":[],"model":"claude-sonnet-5-5","effort":"medium","thinking":"adaptive"}));
    h.bridge.push(
        json!({"v":1,"e":"text","turn":t2,"text":"It is a desktop console for managed agents."}),
    );
    h.bridge.push(json!({"v":1,"e":"usage","turn":t2,"message_id":"msg_1","model":"claude-haiku-5-5","input":11,"output":1,"cache_creation":3,"cache_read":5}));
    h.bridge.push(json!({"v":1,"e":"rate_limit","status":"allowed_warning","type":"five_hour","resets_at":1_900_000_000u64,"utilization":0.72}));
    h.finish(t2, INFERENCE);
    check("state-fixture-inference.json", state_with(h.view()));
}

#[test]
fn golden_local_tools_with_a_held_command() {
    let mut h = Host::new(Profile::LocalTools);
    h.ready(TOOLS);
    h.prompt(
        "t0000003-0000-4000-8000-000000000003",
        "run the build",
        TOOLS,
    );
    h.bridge.push(json!({"v":1,"e":"action_proposal","turn":"t0000003-0000-4000-8000-000000000003","action":"act-1","tool":"run_command","args":{"command":"cargo build --release"}}));
    h.pump();
    check("state-fixture-tools-held.json", state_with(h.view()));
}

#[test]
fn golden_local_tools_with_every_way_a_call_can_end() {
    let mut h = Host::new(Profile::LocalTools);
    h.ready(TOOLS);
    let turn = "t0000004-0000-4000-8000-000000000004";
    h.prompt(turn, "do several things", TOOLS);
    let call = |h: &mut Host, id: &str, args: Value, tool: &str| {
        h.bridge.push(
            json!({"v":1,"e":"action_proposal","turn":turn,"action":id,"tool":tool,"args":args}),
        );
        h.pump();
    };
    // Completed.
    call(
        &mut h,
        "act-1",
        json!({"command":"echo one"}),
        "run_command",
    );
    h.w.send_critical(b"approval claude-act-1 accept").unwrap();
    h.exec.0.lock().unwrap().outcome = Some(Outcome::Completed("exit status 0\none\n".into()));
    h.pump();
    // Declined.
    call(
        &mut h,
        "act-2",
        json!({"command":"echo two"}),
        "run_command",
    );
    h.w.send_critical(b"approval claude-act-2 decline").unwrap();
    h.pump();
    // Failed.
    call(
        &mut h,
        "act-3",
        json!({"command":"echo three"}),
        "run_command",
    );
    h.w.send_critical(b"approval claude-act-3 accept").unwrap();
    h.exec.0.lock().unwrap().outcome = Some(Outcome::Failed("nothing was written".into()));
    h.pump();
    // Indeterminate.
    call(
        &mut h,
        "act-4",
        json!({"command":"echo four"}),
        "run_command",
    );
    h.w.send_critical(b"approval claude-act-4 accept").unwrap();
    h.exec.0.lock().unwrap().outcome = Some(Outcome::Indeterminate);
    h.pump();
    // Refused before the user was asked.
    call(
        &mut h,
        "act-5",
        json!({"path":"../escape","content":"x"}),
        "write_file",
    );
    // A file change held for a decision.
    call(
        &mut h,
        "act-6",
        json!({"path":"notes.txt","content":"line one\nline two\n"}),
        "write_file",
    );
    let view = h.view();
    check("state-fixture-tools-ended.json", state_with(view));
}

#[test]
fn golden_failed_and_refused_runtimes() {
    let mut refused = FixtureModel::new();
    refused.starting("claude_inference_v1");
    refused.refused("managed_policy_present");
    check("state-fixture-refused.json", state_with(refused.view(None)));
    let mut failed = FixtureModel::new();
    failed.starting("claude_local_tools_v1");
    failed.failed("the runtime stopped responding");
    check("state-fixture-failed.json", state_with(failed.view(None)));
}
