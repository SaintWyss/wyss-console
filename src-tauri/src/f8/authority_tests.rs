//! A whole protocol-2 / schema-8 conversation against the real engine and durable writers.
//! Ordinary in-memory SQLite and in-process workers only; no provider, credential or socket.
use super::*;
use crate::{
    control::f8_fixture,
    f8::{
        engine::{Spawned, Worker},
        migration,
        registry::Registry,
        v8::validate_v8,
    },
};
use std::sync::{Arc, Mutex};

const AUTH: &str = "00000000-0000-4000-8000-0000000000a1";
const FP: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn uuid() -> String {
    crate::control::store::uuid().as_str().to_owned()
}
fn wire(s: &str) -> WireUuid {
    WireUuid::try_from(s.to_owned()).unwrap()
}

type Sent = Arc<Mutex<Vec<Vec<u8>>>>;
struct Recorder {
    sent: Sent,
    exited: Arc<Mutex<bool>>,
    refuse_critical: Arc<Mutex<bool>>,
}
impl Worker for Recorder {
    fn send(&mut self, frame: &[u8]) -> ControlResult<()> {
        self.sent.lock().unwrap().push(frame.to_vec());
        Ok(())
    }
    fn send_critical(&mut self, frame: &[u8]) -> ControlResult<()> {
        if *self.refuse_critical.lock().unwrap() {
            return Err(E::CapacityExceeded);
        }
        self.send(frame)
    }
    fn has_exited(&mut self) -> bool {
        *self.exited.lock().unwrap()
    }
}
struct Spawn {
    sent: Sent,
    exited: Arc<Mutex<bool>>,
    refuse_critical: Arc<Mutex<bool>>,
}
impl Spawner for Spawn {
    fn spawn(
        &mut self,
        _launch: &RuntimeRef,
        _staging: Option<&mut super::super::staging::LaunchStaging>,
    ) -> ControlResult<Spawned> {
        Ok(Spawned {
            worker: Box::new(Recorder {
                sent: self.sent.clone(),
                exited: self.exited.clone(),
                refuse_critical: self.refuse_critical.clone(),
            }),
        })
    }
}

struct Rig {
    authority: Authority,
    client: Client,
    spawn: Spawn,
    sessions: Vec<String>,
}
impl Rig {
    fn new() -> Self {
        Self::with_capacity(1)
    }
    fn with_capacity(capacity: usize) -> Self {
        let mut source = f8_fixture::v7_with_sessions(1);
        let conn = migration::prepare(&mut source).unwrap().into_connection();
        let sessions: Vec<String> = conn
            .prepare("SELECT session_id FROM managed_sessions ORDER BY created_sequence")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let registry = Registry::new(wire(AUTH)).unwrap().with_test_limit(capacity);
        let engine = Engine::new(conn, wire(AUTH), registry, None).unwrap();
        Self {
            authority: Authority::new(engine),
            client: Client::default(),
            spawn: Spawn {
                sent: Arc::default(),
                exited: Arc::default(),
                refuse_critical: Arc::default(),
            },
            sessions,
        }
    }
    fn raw(&mut self, v: Value) -> Value {
        let bytes = serde_json::to_vec(&v).unwrap();
        let out = self
            .authority
            .handle(&mut self.client, &bytes, &mut self.spawn, Instant::now());
        serde_json::from_slice(&out).unwrap()
    }
    fn call(&mut self, method: &str, params: Value) -> Value {
        self.raw(
            json!({"schema_version":2,"request_id":uuid(),"authority_instance_id":AUTH,
            "method":method,"params":params}),
        )
    }
    fn hello(&mut self) -> Value {
        self.raw(
            json!({"schema_version":2,"request_id":uuid(),"method":"authority/hello",
            "params":{"protocol_version":2,"client_instance_id":uuid()}}),
        )
    }
    fn sent(&self) -> Vec<Vec<u8>> {
        self.spawn.sent.lock().unwrap().clone()
    }
    fn expect(&self, session: &str) -> (u64, u64) {
        self.authority
            .engine()
            .connection()
            .query_row(
                "SELECT generation,revision FROM managed_runtimes WHERE managed_session_id=?1",
                [session],
                |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64)),
            )
            .unwrap_or((0, 0))
    }
    /// Starts the first session's runtime over the wire and completes the provider handshake.
    fn ready(&mut self) -> (RuntimeRef, Value) {
        self.ready_for(0)
    }
    fn ready_for(&mut self, index: usize) -> (RuntimeRef, Value) {
        let s = self.sessions[index].clone();
        let (g, r) = self.expect(&s);
        let reply = self.call(
            "runtime/start",
            json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex",
            "managed_session_id":s,"expected_generation":g,"expected_revision":r}),
        );
        let value = &reply["result"]["value"];
        assert_eq!(reply["result"]["kind"], "runtime_control", "{reply}");
        let reference = self.authority.engine().reference(&s).unwrap();
        assert_eq!(value["generation"], reference.generation);
        self.authority
            .engine_mut()
            .complete_handshake(
                &reference,
                "0.159.0",
                Attachment {
                    workspace: "/work/space".into(),
                    profile: "codex_local_tools_v1".into(),
                    revision: 1,
                },
            )
            .unwrap();
        (
            reference.clone(),
            json!({"managed_session_id":s,"generation":reference.generation,
            "runtime_instance_id":reference.runtime_instance_id}),
        )
    }
}
fn code(v: &Value) -> &str {
    v["error"]["code"].as_str().unwrap_or("<ok>")
}
fn all_scopes() -> Value {
    json!([
        "session_create",
        "session_resume",
        "turn_start",
        "command_execute",
        "file_change"
    ])
}

#[test]
fn handshake_versions_and_unsupported_methods_are_strict() {
    let mut rig = Rig::new();
    let s = rig.sessions[0].clone();
    // Before hello nothing runs; a protocol-1 frame is a version error, never an implicit session.
    let r = rig.call("authority/status", json!({}));
    assert_eq!(code(&r), "handshake_required");
    let v1 = rig.raw(
        json!({"schema_version":1,"request_id":uuid(),"method":"authority/hello",
        "params":{"protocol_version":1,"client_instance_id":uuid()}}),
    );
    assert_eq!(code(&v1), "protocol_version_unsupported");
    let hello = rig.hello();
    assert_eq!(hello["result"]["kind"], "hello");
    assert_eq!(hello["result"]["value"]["protocol_version"], 2);
    assert_eq!(hello["result"]["value"]["storage_schema_version"], 8);
    // A stale authority instance and unported methods refuse.
    let stale = rig.raw(json!({"schema_version":2,"request_id":uuid(),
        "authority_instance_id":"00000000-0000-4000-8000-0000000000ff","method":"authority/status","params":{}}));
    assert_eq!(code(&stale), "correlation_error");
    let r = rig.call("managedSession/get", json!({"managed_session_id":s}));
    assert_eq!(code(&r), "unsupported");
    // Runtime status of a never-started runtime does not invent one.
    let r = rig.call(
        "runtime/status",
        json!({"adapter_id":"codex","runtime_kind":"codex","managed_session_id":s}),
    );
    assert_eq!(r["result"]["value"]["actual_state"], "stopped", "{r}");
    assert_eq!(rig.authority.engine().reference(&s), None);
}

#[test]
fn full_conversation_start_lease_turn_approval_cancel_and_audit() {
    let mut rig = Rig::new();
    rig.hello();
    let (reference, fence) = rig.ready();
    let s = rig.sessions[0].clone();
    let status = rig.call(
        "runtime/status",
        json!({"adapter_id":"codex","runtime_kind":"codex","managed_session_id":s}),
    );
    assert_eq!(
        status["result"]["value"]["actual_state"], "ready",
        "{status}"
    );
    assert_eq!(
        status["result"]["value"]["runtime_identity"]["generation"],
        reference.generation
    );

    // Leases need the live fence; a stale or foreign fence changes nothing.
    let grant = |rig: &mut Rig, fence: Value| {
        rig.call(
            "executionLease/grant",
            json!({"intent_id":uuid(),"session_id":s,"expected_session_revision":1,
            "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence}),
        )
    };
    let mut stale = fence.clone();
    stale["generation"] = json!(reference.generation + 1);
    assert_eq!(code(&grant(&mut rig, stale)), "stale_revision");
    let lease = grant(&mut rig, fence.clone());
    assert_eq!(lease["result"]["kind"], "lease", "{lease}");
    assert_eq!(
        code(&grant(&mut rig, fence.clone())),
        "lease_already_active"
    );

    // Turn start: replay returns the record without a second frame; a changed intent conflicts.
    let intent = uuid();
    let start = |rig: &mut Rig, prompt: &str| {
        rig.call(
            "managedSession/turn/start",
            json!({"intent_id":intent,"managed_session_id":s,
            "expected_session_revision":1,"prompt":prompt,"runtime_fence":fence}),
        )
    };
    let turn = start(&mut rig, "hello world");
    assert_eq!(turn["result"]["kind"], "turn", "{turn}");
    let turn_id = turn["result"]["value"]["managed_turn_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(rig.sent().len(), 1);
    assert!(String::from_utf8_lossy(&rig.sent()[0]).contains("\"turn/start\""));
    assert_eq!(
        start(&mut rig, "hello world")["result"]["value"],
        turn["result"]["value"]
    );
    assert_eq!(rig.sent().len(), 1, "a replay never reaches the provider");
    assert_eq!(code(&start(&mut rig, "other prompt")), "intent_conflict");
    let second = rig.call(
        "managedSession/turn/start",
        json!({"intent_id":uuid(),"managed_session_id":s,
        "expected_session_revision":1,"prompt":"again","runtime_fence":fence}),
    );
    assert_eq!(code(&second), "active_turn_exists");

    // The provider runs the turn and asks for one approval.
    let turn_ref = ManagedTurnId::parse(&turn_id).unwrap();
    let alien = RuntimeRef {
        managed_session_id: crate::contracts::identities::ManagedSessionId::parse(&rig.sessions[1]).unwrap(),
        ..reference.clone()
    };
    let before_foreign: i64 = rig.authority.engine().connection().query_row(
        "SELECT count(*) FROM control_events", [], |r| r.get(0)
    ).unwrap();
    // The original writer checks that the turn belongs to this authority, not that
    // this inbound worker is the launch that owns the turn.
    assert!(rig.authority.provider_turn_event(
        &alien, &turn_ref, Some("foreign-turn"), TurnState::Running, None
    ).is_err());
    let after_foreign: i64 = rig.authority.engine().connection().query_row(
        "SELECT count(*) FROM control_events", [], |r| r.get(0)
    ).unwrap();
    assert_eq!(before_foreign, after_foreign, "a foreign provider event must not persist");
    rig.authority
        .provider_turn_event(
            &reference,
            &turn_ref,
            Some("provider-turn"),
            TurnState::Running,
            None,
        )
        .unwrap();
    let approval = rig
        .authority
        .provider_approval_request(
            &reference,
            ApprovalKind::CommandExecution,
            "item-1",
            FP,
            ApprovalStructure {
                command_bytes: 12,
                cwd_bytes: 6,
                parsed_actions: 1,
                files: 0,
                diff_bytes: 0,
            },
        )
        .unwrap();
    let respond = |rig: &mut Rig, intent: &str, rev: u64, fence: Value| {
        rig.call(
            "managedTurn/approval/respond",
            json!({"intent_id":intent,"managed_approval_id":approval.managed_approval_id,
            "expected_revision":rev,"decision":"accept","runtime_fence":fence}),
        )
    };
    let decide = uuid();
    assert_eq!(
        code(&respond(&mut rig, &uuid(), 9, fence.clone())),
        "stale_revision"
    );
    let before = rig.sent().len();
    let ok = respond(&mut rig, &decide, 1, fence.clone());
    assert_eq!(ok["result"]["kind"], "approval_receipt", "{ok}");
    assert_eq!(rig.sent().len(), before + 1);
    assert_eq!(rig.sent().last().unwrap(), b"approval item-1 accept");
    // The same decision replays its receipt and never answers the provider again.
    let again = respond(&mut rig, &decide, 1, fence.clone());
    assert_eq!(again["result"]["value"], ok["result"]["value"]);
    assert_eq!(rig.sent().len(), before + 1);
    let before_confirm: i64 = rig.authority.engine().connection().query_row(
        "SELECT count(*) FROM control_events", [], |r| r.get(0)
    ).unwrap();
    assert!(rig.authority.provider_approval_confirmed(
        &alien, &approval.managed_approval_id, "item-1"
    ).is_err());
    assert!(rig.authority.provider_approval_confirmed(
        &reference, &approval.managed_approval_id, "wrong-item"
    ).is_err());
    let after_confirm: i64 = rig.authority.engine().connection().query_row(
        "SELECT count(*) FROM control_events", [], |r| r.get(0)
    ).unwrap();
    assert_eq!(before_confirm, after_confirm, "foreign acknowledgements cannot confirm effects");
    rig.authority
        .provider_approval_confirmed(&reference, &approval.managed_approval_id, "item-1")
        .unwrap();

    // Cancel an active turn once; cancelling the ended turn returns it with no dispatch.
    let cancel = |rig: &mut Rig, rev: u64| {
        rig.call(
            "managedSession/turn/cancel",
            json!({"intent_id":uuid(),"managed_turn_id":turn_id,
            "expected_revision":rev,"runtime_fence":fence}),
        )
    };
    let sent = rig.sent().len();
    let r = cancel(&mut rig, 2);
    assert_eq!(r["result"]["kind"], "turn", "{r}");
    assert_eq!(rig.sent().len(), sent + 1);
    rig.authority
        .provider_turn_event(
            &reference,
            &turn_ref,
            None,
            TurnState::Interrupted,
            Some(TurnReason::UserCancel),
        )
        .unwrap();
    let r = cancel(&mut rig, 3);
    assert_eq!(r["result"]["value"]["state"], "interrupted");
    assert_eq!(rig.sent().len(), sent + 1, "no dispatch for an ended turn");

    // The whole stream pages through the protocol and the image is valid schema-8.
    let stream: String = rig
        .authority
        .engine()
        .connection()
        .query_row("SELECT stream_id FROM authority_metadata", [], |r| r.get(0))
        .unwrap();
    let page = rig.call(
        "audit/read",
        json!({"stream_id":stream,"after_sequence":null,"limit":50}),
    );
    assert_eq!(page["result"]["kind"], "audit", "{page}");
    assert!(page["result"]["value"]["events"].as_array().unwrap().len() >= 10);
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn a_foreign_or_swapped_fence_and_a_lost_lease_change_nothing() {
    let mut rig = Rig::new();
    rig.hello();
    let (reference, fence) = rig.ready();
    let s = rig.sessions[0].clone();
    let other = rig.sessions[1].clone();
    let lease = rig.call(
        "executionLease/grant",
        json!({"intent_id":uuid(),"session_id":s,"expected_session_revision":1,
        "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence}),
    );
    let lease_id = lease["result"]["value"]["lease_id"]
        .as_str()
        .unwrap()
        .to_owned();
    // A fence naming another session cannot be combined with this session's identifiers.
    let swapped = json!({"managed_session_id":other,"generation":reference.generation,
        "runtime_instance_id":reference.runtime_instance_id});
    let r = rig.call(
        "managedSession/turn/start",
        json!({"intent_id":uuid(),"managed_session_id":s,
        "expected_session_revision":1,"prompt":"x","runtime_fence":swapped}),
    );
    assert_eq!(code(&r), "invalid_request");
    // A renewal under another session's fence is refused; revocation takes no fence at all.
    let r = rig.call(
        "executionLease/renew",
        json!({"intent_id":uuid(),"lease_id":lease_id,
        "expected_revision":1,"ttl_seconds":60,"runtime_fence":swapped}),
    );
    assert_eq!(code(&r), "stale_revision", "{r}");
    let r = rig.call(
        "executionLease/revoke",
        json!({"intent_id":uuid(),"lease_id":lease_id,
        "expected_revision":1,"runtime_fence":fence}),
    );
    assert_eq!(code(&r), "invalid_request", "revoke has no fence parameter");
    assert!(rig.sent().is_empty());
    // A turn then a revoked lease: the turn is interrupted, its ticket gone, nothing more is written.
    let turn = rig.call(
        "managedSession/turn/start",
        json!({"intent_id":uuid(),"managed_session_id":s,
        "expected_session_revision":1,"prompt":"work","runtime_fence":fence}),
    );
    assert_eq!(turn["result"]["kind"], "turn", "{turn}");
    let revoked = rig.call(
        "executionLease/revoke",
        json!({"intent_id":uuid(),"lease_id":lease_id,
        "expected_revision":1}),
    );
    assert_eq!(revoked["result"]["value"]["state"], "revoked");
    assert_eq!(rig.authority.engine().active_turn(&s), None);
    let state: String = rig
        .authority
        .engine()
        .connection()
        .query_row(
            "SELECT state FROM managed_turns ORDER BY created_at DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, "interrupted");
    let before = rig.sent().len();
    let retry = rig.call(
        "managedSession/turn/start",
        json!({"intent_id":uuid(),"managed_session_id":s,
        "expected_session_revision":1,"prompt":"again","runtime_fence":fence}),
    );
    assert_eq!(code(&retry), "gate_denied");
    assert_eq!(rig.sent().len(), before);
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn a_dead_runtime_is_noticed_recovered_and_never_inherited() {
    let mut rig = Rig::new();
    rig.hello();
    let (reference, fence) = rig.ready();
    let s = rig.sessions[0].clone();
    rig.call(
        "executionLease/grant",
        json!({"intent_id":uuid(),"session_id":s,"expected_session_revision":1,
        "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence}),
    );
    let turn = rig.call(
        "managedSession/turn/start",
        json!({"intent_id":uuid(),"managed_session_id":s,
        "expected_session_revision":1,"prompt":"work","runtime_fence":fence}),
    );
    let turn_id = turn["result"]["value"]["managed_turn_id"]
        .as_str()
        .unwrap()
        .to_owned();
    rig.authority
        .provider_turn_event(
            &reference,
            &ManagedTurnId::parse(&turn_id).unwrap(),
            Some("p"),
            TurnState::Running,
            None,
        )
        .unwrap();
    // The provider process dies on its own.
    *rig.spawn.exited.lock().unwrap() = true;
    rig.authority.tick(Instant::now()).unwrap();
    assert!(!rig.authority.engine().is_live(&s));
    let state: String = rig
        .authority
        .engine()
        .connection()
        .query_row(
            "SELECT state FROM managed_turns WHERE managed_turn_id=?1",
            [&turn_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, "indeterminate");
    validate_v8(rig.authority.engine().connection()).unwrap();
    // The old fence is dead; the next explicit start is a new generation with a clean slate.
    let r = rig.call(
        "managedSession/turn/start",
        json!({"intent_id":uuid(),"managed_session_id":s,
        "expected_session_revision":1,"prompt":"late","runtime_fence":fence}),
    );
    assert_eq!(code(&r), "stale_revision");
    *rig.spawn.exited.lock().unwrap() = false;
    let (again, _) = rig.ready();
    assert_eq!(again.generation, reference.generation + 1);
    assert_eq!(rig.authority.engine().active_turn(&s), None);
}
