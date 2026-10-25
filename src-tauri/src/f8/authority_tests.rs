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
type Inbound = Arc<Mutex<Vec<(String, Vec<u8>)>>>;
struct Recorder {
    session: String,
    inbound: Inbound,
    sent: Sent,
    exited: Arc<Mutex<bool>>,
    refuse_critical: Arc<Mutex<bool>>,
    ambiguous_send: Arc<Mutex<bool>>,
}
impl Worker for Recorder {
    fn containment(&mut self) -> super::super::engine::Containment {
        super::super::engine::Containment::PidNamespace
    }
    fn preflight(&mut self, _context: &Attachment) -> ControlResult<()> {
        Ok(())
    }
    fn send(&mut self, frame: &[u8]) -> ControlResult<()> {
        // Simulates a transport that might have delivered bytes before
        // reporting an error. A safe caller cannot infer zero effect.
        self.sent.lock().unwrap().push(frame.to_vec());
        if *self.ambiguous_send.lock().unwrap() {
            return Err(E::TransportError);
        }
        Ok(())
    }
    fn send_critical(&mut self, frame: &[u8]) -> ControlResult<()> {
        if *self.refuse_critical.lock().unwrap() {
            return Err(E::CapacityExceeded);
        }
        self.send(frame)
    }
    fn inbound(&mut self, max_frames: usize) -> ControlResult<Vec<Vec<u8>>> {
        let mut incoming = self.inbound.lock().unwrap();
        let mut frames = Vec::new();
        let mut index = 0;
        while index < incoming.len() && frames.len() < max_frames {
            if incoming[index].0 == self.session {
                frames.push(incoming.remove(index).1);
            } else {
                index += 1;
            }
        }
        Ok(frames)
    }
    fn has_exited(&mut self) -> bool {
        *self.exited.lock().unwrap()
    }
}
type Factory = Arc<dyn Fn(&RuntimeRef) -> Box<dyn Worker> + Send + Sync>;
struct Spawn {
    inbound: Inbound,
    sent: Sent,
    exited: Arc<Mutex<bool>>,
    refuse_critical: Arc<Mutex<bool>>,
    ambiguous_send: Arc<Mutex<bool>>,
    /// When set, the workers come from here (the provider-parity tests put a Claude worker in).
    factory: Option<Factory>,
}
impl Spawner for Spawn {
    fn supports_provider(&self, p: crate::f8::provider::Provider) -> bool {
        p == crate::f8::provider::Provider::Codex || self.factory.is_some()
    }
    fn descendant_containment(&self) -> bool {
        true
    }
    fn spawn(
        &mut self,
        launch: &RuntimeRef,
        _staging: Option<&mut super::super::staging::LaunchStaging>,
    ) -> ControlResult<Spawned> {
        if let Some(factory) = &self.factory {
            return Ok(Spawned {
                worker: factory(launch),
            });
        }
        Ok(Spawned {
            worker: Box::new(Recorder {
                session: launch.managed_session_id.as_str().to_owned(),
                inbound: self.inbound.clone(),
                sent: self.sent.clone(),
                exited: self.exited.clone(),
                refuse_critical: self.refuse_critical.clone(),
                ambiguous_send: self.ambiguous_send.clone(),
            }),
        })
    }
}

struct Rig {
    authority: Authority,
    client: Client,
    spawn: Spawn,
    sessions: Vec<String>,
    workspace: std::path::PathBuf,
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
        let workspace: String = conn
            .query_row(
                "SELECT workspace_root FROM managed_session_bindings LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let workspace = std::path::PathBuf::from(workspace);
        std::fs::create_dir(&workspace).unwrap();
        let registry = Registry::new(wire(AUTH)).unwrap().with_test_limit(capacity);
        let engine = Engine::new(conn, wire(AUTH), registry, None).unwrap();
        Self {
            authority: Authority::new(engine),
            client: Client::default(),
            spawn: Spawn {
                inbound: Arc::default(),
                sent: Arc::default(),
                exited: Arc::default(),
                refuse_critical: Arc::default(),
                ambiguous_send: Arc::default(),
                factory: None,
            },
            sessions,
            workspace,
        }
    }
    fn bind(&mut self, launch: &RuntimeRef) {
        let id = uuid();
        let reply=self.call("managedSession/runtime/resume",json!({"intent_id":id,"managed_session_id":launch.managed_session_id,"expected_session_revision":1,"runtime_fence":launch.fence()}));
        assert_eq!(code(&reply), "runtime_operation_in_progress", "{reply}");
        let thread:String=self.authority.engine().connection().query_row("SELECT runtime_session_id FROM managed_session_bindings WHERE managed_session_id=?1",[launch.managed_session_id.as_str()],|r|r.get(0)).unwrap();
        super::super::operations::confirm_binding(
            self.authority.engine_mut(),
            launch,
            &wire(&id),
            &thread,
        )
        .unwrap();
    }
    fn action_approval(
        &mut self,
        launch: &RuntimeRef,
        item: &str,
    ) -> crate::contracts::approval::ManagedApprovalRecord {
        let action = super::super::operations::ProviderAction::Command {
            command: "true".into(),
            cwd: self.workspace.to_str().unwrap().into(),
            parsed_actions: vec![],
        };
        let (kind, fingerprint, structure) = action.metadata(item).unwrap();
        let record = self
            .authority
            .provider_approval_request(launch, kind, item, &fingerprint, structure)
            .unwrap();
        self.authority
            .engine_mut()
            .hold_approval_action(launch, item, action)
            .unwrap();
        record
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
            "managed_session_id":s,"expected_generation":g,"expected_revision":r,
                "safety_profile":"codex_local_tools_v1","workspace_root":self.workspace.to_str().unwrap()}),
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
                    workspace: self.workspace.to_str().unwrap().into(),
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
impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.workspace);
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
    assert_eq!(r["result"]["kind"], "session", "{r}");
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

    rig.bind(&reference);
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
    assert_eq!(rig.sent().len(), 2);
    assert!(String::from_utf8_lossy(&rig.sent()[1]).contains("\"turn/start\""));
    assert_eq!(
        start(&mut rig, "hello world")["result"]["value"],
        turn["result"]["value"]
    );
    assert_eq!(rig.sent().len(), 2, "a replay never reaches the provider");
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
        managed_session_id: crate::contracts::identities::ManagedSessionId::parse(&rig.sessions[1])
            .unwrap(),
        ..reference.clone()
    };
    let before_foreign: i64 = rig
        .authority
        .engine()
        .connection()
        .query_row("SELECT count(*) FROM control_events", [], |r| r.get(0))
        .unwrap();
    // The original writer checks that the turn belongs to this authority, not that
    // this inbound worker is the launch that owns the turn.
    assert!(
        rig.authority
            .provider_turn_event(
                &alien,
                &turn_ref,
                Some("foreign-turn"),
                TurnState::Running,
                None
            )
            .is_err()
    );
    let after_foreign: i64 = rig
        .authority
        .engine()
        .connection()
        .query_row("SELECT count(*) FROM control_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        before_foreign, after_foreign,
        "a foreign provider event must not persist"
    );
    rig.authority
        .provider_turn_event(
            &reference,
            &turn_ref,
            Some("provider-turn"),
            TurnState::Running,
            None,
        )
        .unwrap();
    let approval = rig.action_approval(&reference, "item-1");
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
    let before_confirm: i64 = rig
        .authority
        .engine()
        .connection()
        .query_row("SELECT count(*) FROM control_events", [], |r| r.get(0))
        .unwrap();
    assert!(
        rig.authority
            .provider_approval_confirmed(&alien, &approval.managed_approval_id, "item-1")
            .is_err()
    );
    assert!(
        rig.authority
            .provider_approval_confirmed(&reference, &approval.managed_approval_id, "wrong-item")
            .is_err()
    );
    let after_confirm: i64 = rig
        .authority
        .engine()
        .connection()
        .query_row("SELECT count(*) FROM control_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        before_confirm, after_confirm,
        "foreign acknowledgements cannot confirm effects"
    );
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
    rig.bind(&reference);
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
    rig.bind(&reference);
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

#[test]
fn reviewed_real_foreign_launch_cannot_mutate_another_sessions_turn_or_approval() {
    let mut rig = Rig::with_capacity(2);
    rig.hello();
    let (a, fence_a) = rig.ready_for(0);
    let (b, _fence_b) = rig.ready_for(1);
    let session = rig.sessions[0].clone();
    let granted = rig.call(
        "executionLease/grant",
        json!({"intent_id":uuid(),"session_id":session,"expected_session_revision":1,
            "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence_a}),
    );
    assert_eq!(granted["result"]["kind"], "lease", "{granted}");
    rig.bind(&a);
    let started = rig.call(
        "managedSession/turn/start",
        json!({"intent_id":uuid(),"managed_session_id":session,"expected_session_revision":1,
            "prompt":"isolated","runtime_fence":fence_a}),
    );
    let turn = ManagedTurnId::parse(
        started["result"]["value"]["managed_turn_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let head = |rig: &Rig| -> i64 {
        rig.authority
            .engine()
            .connection()
            .query_row("SELECT count(*) FROM control_events", [], |r| r.get(0))
            .unwrap()
    };
    let before = head(&rig);
    assert!(
        rig.authority
            .provider_turn_event(&b, &turn, Some("foreign-turn"), TurnState::Running, None,)
            .is_err()
    );
    assert_eq!(
        head(&rig),
        before,
        "valid launch B cannot progress A's turn"
    );
    rig.authority
        .provider_turn_event(&a, &turn, Some("provider-a"), TurnState::Running, None)
        .unwrap();
    let approved = rig.action_approval(&a, "owned-item");
    let pending_count = head(&rig);
    // Duplicate provider IDs must not write a second record or invalidate the first.
    assert!(
        rig.authority
            .provider_approval_request(
                &a,
                ApprovalKind::CommandExecution,
                "owned-item",
                FP,
                ApprovalStructure {
                    command_bytes: 12,
                    cwd_bytes: 6,
                    parsed_actions: 1,
                    files: 0,
                    diff_bytes: 0,
                },
            )
            .is_err()
    );
    assert_eq!(head(&rig), pending_count);
    assert_eq!(rig.authority.engine().pending_approvals(&session), 1);
    let respond = rig.call(
        "managedTurn/approval/respond",
        json!({"intent_id":uuid(),"managed_approval_id":approved.managed_approval_id,
            "expected_revision":1,"decision":"accept","runtime_fence":fence_a}),
    );
    assert_eq!(respond["result"]["kind"], "approval_receipt", "{respond}");
    let before_foreign_confirmation = head(&rig);
    assert!(
        rig.authority
            .provider_approval_confirmed(&b, &approved.managed_approval_id, "owned-item",)
            .is_err()
    );
    assert_eq!(head(&rig), before_foreign_confirmation);
    rig.authority
        .provider_approval_confirmed(&a, &approved.managed_approval_id, "owned-item")
        .unwrap();
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn reviewed_lost_pending_ticket_cannot_authorize_a_new_durable_accept() {
    let mut rig = Rig::new();
    rig.hello();
    let (a, fence) = rig.ready();
    let session = rig.sessions[0].clone();
    let granted = rig.call(
        "executionLease/grant",
        json!({"intent_id":uuid(),"session_id":session,"expected_session_revision":1,
            "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence}),
    );
    let lease = granted["result"]["value"]["lease_id"]
        .as_str()
        .unwrap()
        .to_owned();
    rig.bind(&a);
    let started = rig.call(
        "managedSession/turn/start",
        json!({"intent_id":uuid(),"managed_session_id":session,"expected_session_revision":1,
            "prompt":"pending","runtime_fence":fence}),
    );
    let turn = ManagedTurnId::parse(
        started["result"]["value"]["managed_turn_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    rig.authority
        .provider_turn_event(&a, &turn, Some("p"), TurnState::Running, None)
        .unwrap();
    let approval = rig.action_approval(&a, "pending-item");
    // The durable lease remains active, but volatile ticket ownership was lost.
    rig.authority.engine_mut().lease_lost(&session, &lease);
    let before: i64 = rig
        .authority
        .engine()
        .connection()
        .query_row("SELECT count(*) FROM control_events", [], |r| r.get(0))
        .unwrap();
    let frames = rig.sent().len();
    let reply = rig.call(
        "managedTurn/approval/respond",
        json!({"intent_id":uuid(),"managed_approval_id":approval.managed_approval_id,
            "expected_revision":1,"decision":"accept","runtime_fence":fence}),
    );
    assert_eq!(code(&reply), "stale_revision", "{reply}");
    let after: i64 = rig
        .authority
        .engine()
        .connection()
        .query_row("SELECT count(*) FROM control_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(
        rig.sent().len(),
        frames,
        "lost live ticket is not dispatch authority"
    );
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn reviewed_all_provider_decisions_report_failed_critical_dispatch_as_errors() {
    for decision in ["accept", "decline", "cancel"] {
        let mut rig = Rig::new();
        rig.hello();
        let (from, fence) = rig.ready();
        let session = rig.sessions[0].clone();
        rig.call(
            "executionLease/grant",
            json!({"intent_id":uuid(),"session_id":session,"expected_session_revision":1,
                "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence}),
        );
        rig.bind(&from);
        let started = rig.call(
            "managedSession/turn/start",
            json!({"intent_id":uuid(),"managed_session_id":session,"expected_session_revision":1,
                "prompt":"dispatch","runtime_fence":fence}),
        );
        let turn = ManagedTurnId::parse(
            started["result"]["value"]["managed_turn_id"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        rig.authority
            .provider_turn_event(&from, &turn, Some("p"), TurnState::Running, None)
            .unwrap();
        let approval = rig.action_approval(&from, "item-1");
        *rig.spawn.refuse_critical.lock().unwrap() = true;
        let sent = rig.sent().len();
        let response = rig.call(
            "managedTurn/approval/respond",
            json!({"intent_id":uuid(),"managed_approval_id":approval.managed_approval_id,
                "expected_revision":1,"decision":decision,"runtime_fence":fence}),
        );
        assert_ne!(
            code(&response),
            "<ok>",
            "{decision} falsely reported delivery: {response}"
        );
        assert_eq!(
            rig.sent().len(),
            sent,
            "no critical frame reached the test worker"
        );
        assert_eq!(
            rig.authority
                .engine()
                .runtime_view(&session)
                .map(|(s, _)| s),
            Some("stopping"),
            "a failed critical transport must fence this runtime only"
        );
        validate_v8(rig.authority.engine().connection()).unwrap();
    }
}

#[test]
fn review_cancel_refuses_stopping_launch_before_committing_a_new_intent() {
    let mut rig = Rig::new();
    rig.hello();
    let (launch, fence) = rig.ready();
    let session = rig.sessions[0].clone();
    rig.call(
        "executionLease/grant",
        json!({
            "intent_id": uuid(), "session_id": session, "expected_session_revision":1,
            "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence
        }),
    );
    rig.bind(&launch);
    let start = rig.call(
        "managedSession/turn/start",
        json!({
            "intent_id":uuid(),"managed_session_id":session,
            "expected_session_revision":1,"prompt":"cancel-check","runtime_fence":fence
        }),
    );
    let turn_id = start["result"]["value"]["managed_turn_id"]
        .as_str()
        .unwrap();
    let turn = ManagedTurnId::parse(turn_id).unwrap();
    rig.authority
        .provider_turn_event(
            &launch,
            &turn,
            Some("provider-cancel"),
            TurnState::Running,
            None,
        )
        .unwrap();
    rig.authority.engine_mut().transport_failed(&launch);
    let before: (i64,i64) = rig.authority.engine().connection().query_row(
        "SELECT (SELECT count(*) FROM control_events), (SELECT count(*) FROM processed_intents)",
        [], |r| Ok((r.get(0)?,r.get(1)?))
    ).unwrap();
    let denied = rig.call(
        "managedSession/turn/cancel",
        json!({
            "intent_id":uuid(),"managed_turn_id":turn_id,
            "expected_revision":2,"runtime_fence":fence
        }),
    );
    assert_ne!(code(&denied), "<ok>", "{denied}");
    let after: (i64,i64) = rig.authority.engine().connection().query_row(
        "SELECT (SELECT count(*) FROM control_events), (SELECT count(*) FROM processed_intents)",
        [], |r| Ok((r.get(0)?,r.get(1)?))
    ).unwrap();
    assert_eq!(
        before, after,
        "rejected cancellation must not mutate the journal"
    );
}

#[test]
fn review_failed_recovery_remains_pending_and_retries_after_storage_restoration() {
    let mut rig = Rig::new();
    rig.hello();
    let (launch, fence) = rig.ready();
    let session = rig.sessions[0].clone();
    rig.call(
        "executionLease/grant",
        json!({
            "intent_id":uuid(),"session_id":session,"expected_session_revision":1,
            "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence
        }),
    );
    rig.bind(&launch);
    let started = rig.call(
        "managedSession/turn/start",
        json!({
            "intent_id":uuid(),"managed_session_id":session,
            "expected_session_revision":1,"prompt":"recovery-check","runtime_fence":fence
        }),
    );
    let turn_id = started["result"]["value"]["managed_turn_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let turn = ManagedTurnId::parse(&turn_id).unwrap();
    rig.authority
        .provider_turn_event(
            &launch,
            &turn,
            Some("provider-recovery"),
            TurnState::Running,
            None,
        )
        .unwrap();
    // Only recovery writes to the stranded turn are rejected: runtime exit audit
    // itself succeeds so that the original one-shot ended marker is consumed.
    rig.authority
        .engine()
        .connection()
        .execute_batch(
            "CREATE TEMP TRIGGER f8_fail_recover BEFORE UPDATE ON main.managed_turns
         WHEN NEW.state='indeterminate'
         BEGIN SELECT RAISE(ABORT,'injected recovery storage refusal'); END;",
        )
        .unwrap();
    *rig.spawn.exited.lock().unwrap() = true;
    assert!(rig.authority.tick(Instant::now()).is_err());
    assert!(
        rig.authority.recovery_pending,
        "recovery must remain retryable"
    );
    rig.authority
        .engine()
        .connection()
        .execute_batch("DROP TRIGGER temp.f8_fail_recover")
        .unwrap();
    rig.authority.tick(Instant::now()).unwrap();
    assert!(!rig.authority.recovery_pending);
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
}

#[test]
fn review_inbound_scheduler_dispatches_only_from_its_owned_runtime() {
    let mut rig = Rig::with_capacity(2);
    rig.hello();
    let (a, fence_a) = rig.ready_for(0);
    let (b, _fence_b) = rig.ready_for(1);
    let session_a = rig.sessions[0].clone();
    let session_b = rig.sessions[1].clone();
    rig.call(
        "executionLease/grant",
        json!({
            "intent_id":uuid(),"session_id":session_a,"expected_session_revision":1,
            "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence_a
        }),
    );
    rig.bind(&a);
    let started = rig.call(
        "managedSession/turn/start",
        json!({
            "intent_id":uuid(),"managed_session_id":session_a,
            "expected_session_revision":1,"prompt":"inbound-check","runtime_fence":fence_a
        }),
    );
    let turn_id = started["result"]["value"]["managed_turn_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let head = |rig: &Rig| -> i64 {
        rig.authority
            .engine()
            .connection()
            .query_row("SELECT count(*) FROM control_events", [], |r| r.get(0))
            .unwrap()
    };
    // A legitimate B worker emits a validly encoded turn event carrying A's ID.
    // The transport, not the payload, determines its origin and must reject it.
    rig.spawn.inbound.lock().unwrap().push((
        session_b.clone(),
        serde_json::to_vec(&json!({
            "kind":"turn_running","managed_turn_id":turn_id,
            "provider_turn_id":"provider-foreign"
        }))
        .unwrap(),
    ));
    let prior = head(&rig);
    rig.authority.tick(Instant::now()).unwrap();
    assert_eq!(head(&rig) - prior, 0, "B cannot change A's durable turn");
    assert_eq!(
        rig.authority.engine().runtime_view(&session_b).map(|r| r.0),
        Some("stopping")
    );
    assert_eq!(
        rig.authority.engine().runtime_view(&session_a).map(|r| r.0),
        Some("ready")
    );

    rig.spawn.inbound.lock().unwrap().push((
        session_a.clone(),
        serde_json::to_vec(&json!({
            "kind":"turn_running","managed_turn_id":turn_id,
            "provider_turn_id":"provider-owned"
        }))
        .unwrap(),
    ));
    rig.authority.tick(Instant::now()).unwrap();
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
    assert_eq!(state, "running");
    assert_eq!(rig.authority.engine().reference(&session_a), Some(a));
    assert_eq!(
        rig.authority.engine().reference(&session_b),
        None,
        "faulted B was reaped while A continued"
    );
    assert!(rig.authority.engine().connection().query_row::<bool,_,_>(
        "SELECT EXISTS(SELECT 1 FROM control_events WHERE json_extract(envelope,'$.event_type')='authority.runtime.instance_exited' AND json_extract(envelope,'$.payload.runtime_instance_id')=?1)",
        [b.runtime_instance_id.as_str()], |r|r.get(0)).unwrap());
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn review_ambiguous_turn_start_send_never_claims_definitive_no_effect() {
    let mut rig = Rig::new();
    rig.hello();
    let (_launch, fence) = rig.ready();
    let session = rig.sessions[0].clone();
    rig.call(
        "executionLease/grant",
        json!({
            "intent_id":uuid(),"session_id":session,"expected_session_revision":1,
            "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence
        }),
    );
    rig.bind(&_launch);
    *rig.spawn.ambiguous_send.lock().unwrap() = true;
    let reply = rig.call(
        "managedSession/turn/start",
        json!({
            "intent_id":uuid(),"managed_session_id":session,
            "expected_session_revision":1,"prompt":"may-have-reached-provider",
            "runtime_fence":fence
        }),
    );
    assert_eq!(code(&reply), "external_effect_unknown", "{reply}");
    let (state, reason): (String, String) = rig
        .authority
        .engine()
        .connection()
        .query_row(
            "SELECT state,terminal_reason FROM managed_turns ORDER BY created_at DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (state.as_str(), reason.as_str()),
        ("indeterminate", "dispatch_unknown")
    );
    assert_eq!(
        rig.authority.engine().runtime_view(&session).map(|r| r.0),
        Some("stopping")
    );
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn review_provider_interrupt_send_failure_does_not_report_success() {
    let mut rig = Rig::new();
    rig.hello();
    let (launch, fence) = rig.ready();
    let session = rig.sessions[0].clone();
    rig.call(
        "executionLease/grant",
        json!({
            "intent_id":uuid(),"session_id":session,"expected_session_revision":1,
            "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence
        }),
    );
    rig.bind(&launch);
    let started = rig.call(
        "managedSession/turn/start",
        json!({
            "intent_id":uuid(),"managed_session_id":session,
            "expected_session_revision":1,"prompt":"running-work","runtime_fence":fence
        }),
    );
    let turn_id = started["result"]["value"]["managed_turn_id"]
        .as_str()
        .unwrap()
        .to_owned();
    rig.authority
        .provider_turn_event(
            &launch,
            &ManagedTurnId::parse(&turn_id).unwrap(),
            Some("provider-running"),
            TurnState::Running,
            None,
        )
        .unwrap();
    *rig.spawn.ambiguous_send.lock().unwrap() = true;
    let response = rig.call(
        "managedSession/turn/cancel",
        json!({
            "intent_id":uuid(),"managed_turn_id":turn_id,
            "expected_revision":2,"runtime_fence":fence
        }),
    );
    assert_eq!(code(&response), "external_effect_unknown", "{response}");
    assert_eq!(
        rig.authority.engine().runtime_view(&session).map(|r| r.0),
        Some("stopping")
    );
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn approval_kernel_write_followed_by_writer_constructor_failure_fences_without_replay() {
    let mut rig = Rig::new();
    rig.hello();
    let (origin, fence) = rig.ready();
    let session = rig.sessions[0].clone();
    let grant=rig.call("executionLease/grant",json!({"intent_id":uuid(),"session_id":session,"expected_session_revision":1,"scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence}));
    assert_eq!(grant["result"]["kind"], "lease", "{grant}");
    rig.bind(&origin);
    let started=rig.call("managedSession/turn/start",json!({"intent_id":uuid(),"managed_session_id":session,"expected_session_revision":1,"prompt":"effect","runtime_fence":fence}));
    let turn = ManagedTurnId::parse(
        started["result"]["value"]["managed_turn_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    rig.authority
        .provider_turn_event(&origin, &turn, Some("provider-t"), TurnState::Running, None)
        .unwrap();
    let approval = rig.action_approval(&origin, "effect-item");
    let intent = uuid();
    let params = json!({"intent_id":intent,"managed_approval_id":approval.managed_approval_id,"expected_revision":1,"decision":"accept","runtime_fence":fence});
    rig.authority.post_approval_write = Some(|c| c.pragma_update(None, "user_version", 7).unwrap());
    let before = rig.sent().len();
    let answer = rig.call("managedTurn/approval/respond", params.clone());
    assert_eq!(code(&answer), "storage_incompatible", "{answer}");
    assert_eq!(
        rig.sent().len(),
        before + 1,
        "one atomic provider write happened"
    );
    assert_eq!(
        rig.authority.engine().runtime_view(&session).map(|s| s.0),
        Some("stopping")
    );
    rig.authority
        .engine_mut()
        .connection_mut()
        .pragma_update(None, "user_version", 8)
        .unwrap();
    assert_eq!(
        super::super::writers::read_delivery(
            rig.authority.engine().connection(),
            approval.managed_approval_id.as_str()
        )
        .unwrap(),
        Some(ApprovalDelivery::DecisionAccepted)
    );
    let replay = rig.call("managedTurn/approval/respond", params);
    assert_eq!(replay["result"]["kind"], "approval_receipt", "{replay}");
    assert_eq!(
        rig.sent().len(),
        before + 1,
        "durable decision replay never dispatches"
    );
    rig.authority.tick(Instant::now()).unwrap();
    assert_eq!(
        super::super::writers::read_delivery(
            rig.authority.engine().connection(),
            approval.managed_approval_id.as_str()
        )
        .unwrap(),
        Some(ApprovalDelivery::Indeterminate)
    );
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn exact_turn_start_replay_survives_lease_revocation_and_runtime_exit_without_dispatch() {
    let mut rig = Rig::new();
    rig.hello();
    let (origin, fence) = rig.ready();
    let session = rig.sessions[0].clone();
    let grant=rig.call("executionLease/grant",json!({"intent_id":uuid(),"session_id":session,"expected_session_revision":1,"scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence}));
    rig.bind(&origin);
    let params = json!({"intent_id":uuid(),"managed_session_id":session,"expected_session_revision":1,"prompt":"once","runtime_fence":fence});
    let admitted = rig.call("managedSession/turn/start", params.clone());
    assert_eq!(admitted["result"]["kind"], "turn", "{admitted}");
    let revoked=rig.call("executionLease/revoke",json!({"intent_id":uuid(),"lease_id":grant["result"]["value"]["lease_id"],"expected_revision":1}));
    assert_eq!(revoked["result"]["kind"], "lease", "{revoked}");
    *rig.spawn.exited.lock().unwrap() = true;
    rig.authority.tick(Instant::now()).unwrap();
    assert!(rig.authority.engine().reference(&session).is_none());
    let sent = rig.sent().len();
    let replay = rig.call("managedSession/turn/start", params.clone());
    assert_eq!(
        replay["result"]["value"], admitted["result"]["value"],
        "{replay}"
    );
    assert_eq!(rig.sent().len(), sent);
    let mut changed = params.clone();
    changed["prompt"] = json!("different");
    assert_eq!(
        code(&rig.call("managedSession/turn/start", changed)),
        "intent_conflict"
    );
    let mut changed = params;
    changed["runtime_fence"]["generation"] = json!(origin.generation + 1);
    assert_eq!(
        code(&rig.call("managedSession/turn/start", changed)),
        "intent_conflict"
    );
    validate_v8(rig.authority.engine().connection()).unwrap();
}

/// Read-only observation: total SQLite changes on this connection, audit head and lease rows.
fn read_snapshot(rig: &Rig) -> (u64, String, String) {
    let c = rig.authority.engine().connection();
    let head: String = c
        .query_row(
            "SELECT last_sequence FROM authority_metadata WHERE id=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let leases: String = c
        .query_row(
            "SELECT coalesce(group_concat(lease_id||'|'||state||'|'||revision||'|'||expires_at||'|'||updated_at,';'),'') FROM (SELECT * FROM execution_leases ORDER BY lease_id)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    (c.total_changes(), head, leases)
}

/// A lease, a running turn and one provider approval `item`, all over one live, bound launch.
fn live_approval(
    rig: &mut Rig,
    ttl_seconds: u32,
    item: &str,
) -> (
    RuntimeRef,
    Value,
    String,
    crate::contracts::approval::ManagedApprovalRecord,
) {
    let s = rig.sessions[0].clone();
    let (reference, fence) = rig.ready();
    // Binding resumes under a lease, so the grant comes first, as in the full conversation.
    let grant = rig.call(
        "executionLease/grant",
        json!({"intent_id":uuid(),"session_id":s,"expected_session_revision":1,
        "scopes":all_scopes(),"ttl_seconds":ttl_seconds,"runtime_fence":fence}),
    );
    assert_eq!(grant["result"]["kind"], "lease", "{grant}");
    rig.bind(&reference);
    let turn = rig.call(
        "managedSession/turn/start",
        json!({"intent_id":uuid(),"managed_session_id":s,
        "expected_session_revision":1,"prompt":"approve","runtime_fence":fence}),
    );
    let turn_id = turn["result"]["value"]["managed_turn_id"]
        .as_str()
        .unwrap()
        .to_owned();
    rig.authority
        .provider_turn_event(
            &reference,
            &ManagedTurnId::parse(&turn_id).unwrap(),
            Some("provider-turn"),
            TurnState::Running,
            None,
        )
        .unwrap();
    let approval = rig.action_approval(&reference, item);
    (reference, fence, turn_id, approval)
}

#[test]
fn binding_status_is_attached_only_with_a_live_proven_launch() {
    let mut rig = Rig::new();
    rig.hello();
    let (bound, unbound) = (rig.sessions[0].clone(), rig.sessions[1].clone());
    let read = |rig: &mut Rig, s: &str| {
        rig.call(
            "managedSession/runtime/status",
            json!({"managed_session_id":s}),
        )
    };
    let before = read_snapshot(&rig);

    let u = read(&mut rig, &unbound);
    assert_eq!(u["result"]["kind"], "binding_status", "{u}");
    let v = &u["result"]["value"];
    assert_eq!(v["state"], "unbound", "{u}");
    for field in [
        "binding",
        "runtime_instance_id",
        "runtime_identity",
        "attachment_profile",
        "operation_id",
    ] {
        assert!(v[field].is_null(), "{field}: {u}");
    }

    // The fixture's durable binding is history: it never makes an absent runtime attached.
    let d = read(&mut rig, &bound);
    let v = &d["result"]["value"];
    assert_eq!(v["state"], "bound_detached", "{d}");
    assert_eq!(v["binding"]["managed_session_id"], bound, "{d}");
    assert!(v["runtime_instance_id"].is_null(), "{d}");
    assert!(v["runtime_identity"].is_null(), "{d}");

    assert_eq!(code(&read(&mut rig, &uuid())), "session_not_found");
    assert_eq!(code(&read(&mut rig, "not-a-uuid")), "invalid_request");
    assert_eq!(read_snapshot(&rig), before, "binding reads must not write");

    // A ready worker with no successful binding for its launch is still detached.
    let (reference, fence) = rig.ready();
    let grant = rig.call(
        "executionLease/grant",
        json!({"intent_id":uuid(),"session_id":bound,"expected_session_revision":1,
        "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence}),
    );
    assert_eq!(grant["result"]["kind"], "lease", "{grant}");
    let unproven = read(&mut rig, &bound);
    assert_eq!(
        unproven["result"]["value"]["state"], "bound_detached",
        "{unproven}"
    );
    assert!(
        unproven["result"]["value"]["runtime_instance_id"].is_null(),
        "{unproven}"
    );

    rig.bind(&reference);
    let attached = read(&mut rig, &bound);
    let v = &attached["result"]["value"];
    assert_eq!(v["state"], "bound_attached", "{attached}");
    assert_eq!(
        v["runtime_instance_id"],
        json!(reference.runtime_instance_id),
        "{attached}"
    );
    assert_eq!(
        v["runtime_identity"]["generation"], reference.generation,
        "{attached}"
    );
    assert_eq!(v["attachment_profile"], "local_tools_v1", "{attached}");

    // Stopping the launch withdraws the attachment and every identity it exposed.
    let (generation, revision) = rig.expect(&bound);
    let stop = rig.call(
        "runtime/stop",
        json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex",
        "managed_session_id":bound,"expected_generation":generation,
        "expected_revision":revision,"expected_runtime_instance_id":reference.runtime_instance_id}),
    );
    assert_eq!(stop["result"]["kind"], "runtime_control", "{stop}");
    *rig.spawn.exited.lock().unwrap() = true;
    rig.authority.tick(Instant::now()).unwrap();
    let stopped = read(&mut rig, &bound);
    let v = &stopped["result"]["value"];
    assert_eq!(v["state"], "bound_detached", "{stopped}");
    assert!(v["runtime_instance_id"].is_null(), "{stopped}");
    assert!(v["runtime_identity"].is_null(), "{stopped}");
    assert!(v["attachment_profile"].is_null(), "{stopped}");

    // A replacement generation must not inherit the old attachment: it needs its own binding.
    *rig.spawn.exited.lock().unwrap() = false;
    let (next, _) = rig.ready_for(0);
    assert!(next.generation > reference.generation);
    let replaced = read(&mut rig, &bound);
    let v = &replaced["result"]["value"];
    assert_eq!(v["state"], "bound_detached", "{replaced}");
    assert!(v["runtime_instance_id"].is_null(), "{replaced}");
    rig.bind(&next);
    let rebound = read(&mut rig, &bound);
    assert_eq!(
        rebound["result"]["value"]["runtime_identity"]["generation"],
        next.generation
    );
    assert_eq!(
        rebound["result"]["value"]["state"], "bound_attached",
        "{rebound}"
    );
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn approval_status_is_executable_only_for_its_live_pending_launch() {
    let mut rig = Rig::new();
    rig.hello();
    let (reference, fence, _turn, approval) = live_approval(&mut rig, 600, "item-1");
    let aid = json!(approval.managed_approval_id);
    let status = |rig: &mut Rig, id: Value| {
        rig.call(
            "managedTurn/approval/status",
            json!({"managed_approval_id":id}),
        )
    };
    let before = read_snapshot(&rig);

    let live = status(&mut rig, aid.clone());
    assert_eq!(live["result"]["kind"], "approval", "{live}");
    let v = &live["result"]["value"];
    assert_eq!(v["approval"]["state"], "pending", "{live}");
    // The body is exactly the strict contract once the protocol's launch projection, which the
    // response layer carries separately, is set aside.
    let mut body = v.clone();
    body.as_object_mut().unwrap().remove("runtime_ref");
    body.as_object_mut().unwrap().remove("attestation");
    assert_eq!(
        body.as_object_mut().unwrap().remove("decisions"),
        Some(json!(["accept", "decline", "cancel"]))
    );
    let strict: Result<crate::contracts::approval::ApprovalStatus, _> =
        serde_json::from_value(body);
    assert!(
        strict.is_ok(),
        "approval status must match its strict contract: {live}"
    );
    assert_eq!(v["acceptable"], true, "{live}");
    assert!(
        !v["command"].is_null(),
        "a live pending approval shows its action: {live}"
    );
    assert_eq!(
        v["runtime_ref"]["runtime_instance_id"],
        json!(reference.runtime_instance_id),
        "{live}"
    );

    assert_eq!(code(&status(&mut rig, json!(uuid()))), "approval_not_found");
    assert_eq!(
        code(&status(&mut rig, json!("not-a-uuid"))),
        "invalid_request"
    );
    let index = rig.call(
        "managedTurn/approval/status",
        json!({"managed_approval_id":aid,"file_index":3,"offset":0}),
    );
    assert_eq!(code(&index), "invalid_request", "{index}");
    assert_eq!(read_snapshot(&rig), before, "approval reads must not write");

    // Accepting makes the record final; its executable content is withdrawn.
    let accept = rig.call(
        "managedTurn/approval/respond",
        json!({"intent_id":uuid(),"managed_approval_id":aid,"expected_revision":approval.revision,
        "decision":"accept","runtime_fence":fence}),
    );
    assert_eq!(accept["result"]["kind"], "approval_receipt", "{accept}");
    let accepted = status(&mut rig, aid.clone());
    assert_eq!(
        accepted["result"]["value"]["approval"]["state"], "accepted",
        "{accepted}"
    );
    assert_eq!(
        accepted["result"]["value"]["acceptable"], false,
        "{accepted}"
    );
    assert!(
        accepted["result"]["value"]["command"].is_null(),
        "{accepted}"
    );

    // One undecided approval per session at a time; the second opens only after the first settles.
    let second = rig.action_approval(&reference, "item-2");
    let decline = rig.call(
        "managedTurn/approval/respond",
        json!({"intent_id":uuid(),"managed_approval_id":second.managed_approval_id,
        "expected_revision":second.revision,"decision":"decline","runtime_fence":fence}),
    );
    assert_eq!(decline["result"]["kind"], "approval_receipt", "{decline}");
    let declined = status(&mut rig, json!(second.managed_approval_id));
    assert_eq!(
        declined["result"]["value"]["approval"]["state"], "declined",
        "{declined}"
    );
    assert_eq!(
        declined["result"]["value"]["acceptable"], false,
        "{declined}"
    );
    assert!(
        declined["result"]["value"]["command"].is_null(),
        "{declined}"
    );
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn pending_approval_query_returns_only_an_undecided_unexpired_approval() {
    let mut rig = Rig::new();
    rig.hello();
    let legacy: String = rig
        .authority
        .engine()
        .connection()
        .query_row(
            "SELECT managed_turn_id FROM managed_turns WHERE state='completed' LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let pending = |rig: &mut Rig, turn: Value| {
        rig.call(
            "managedTurn/approval/pending",
            json!({"managed_turn_id":turn}),
        )
    };

    let none = pending(&mut rig, json!(legacy));
    assert_eq!(none["result"]["kind"], "pending_approval", "{none}");
    assert!(none["result"]["value"].is_null(), "{none}");
    assert_eq!(code(&pending(&mut rig, json!(uuid()))), "turn_not_found");
    assert_eq!(
        code(&pending(&mut rig, json!("not-a-uuid"))),
        "invalid_request"
    );

    let (_, fence, turn_id, approval) = live_approval(&mut rig, 600, "item-1");
    let before = read_snapshot(&rig);
    let one = pending(&mut rig, json!(turn_id));
    assert_eq!(
        one["result"]["value"]["approval"]["managed_approval_id"],
        json!(approval.managed_approval_id),
        "{one}"
    );
    assert_eq!(read_snapshot(&rig), before, "pending reads must not write");

    let accept = rig.call(
        "managedTurn/approval/respond",
        json!({"intent_id":uuid(),"managed_approval_id":approval.managed_approval_id,
        "expected_revision":approval.revision,"decision":"accept","runtime_fence":fence}),
    );
    assert_eq!(accept["result"]["kind"], "approval_receipt", "{accept}");
    let resolved = pending(&mut rig, json!(turn_id));
    assert!(resolved["result"]["value"].is_null(), "{resolved}");
}

#[test]
fn an_approval_past_its_deadline_is_never_pending_or_executable() {
    let mut rig = Rig::new();
    rig.hello();
    // The approval deadline is the earlier of its own TTL and the lease expiry; one second
    // is the shortest lease the contract allows. Nothing ticks, so no expiry is persisted.
    let (_, _, turn_id, approval) = live_approval(&mut rig, 1, "item-1");
    std::thread::sleep(std::time::Duration::from_millis(1500));

    let pending = rig.call(
        "managedTurn/approval/pending",
        json!({"managed_turn_id":turn_id}),
    );
    assert!(
        pending["result"]["value"].is_null(),
        "a deadline-passed approval must not read as pending: {pending}"
    );
    let status = rig.call(
        "managedTurn/approval/status",
        json!({"managed_approval_id":approval.managed_approval_id}),
    );
    assert_eq!(status["result"]["value"]["acceptable"], false, "{status}");
    assert!(status["result"]["value"]["command"].is_null(), "{status}");
}

#[test]
fn approval_reads_after_runtime_exit_and_lease_revocation_expose_no_action() {
    let mut rig = Rig::new();
    rig.hello();
    let (_, _, turn_id, approval) = live_approval(&mut rig, 600, "item-1");
    let s = rig.sessions[0].clone();
    let lease_id: String = rig
        .authority
        .engine()
        .connection()
        .query_row(
            "SELECT lease_id FROM execution_leases WHERE session_id=?1 AND state='active'",
            [&s],
            |r| r.get(0),
        )
        .unwrap();
    let revoke = rig.call(
        "executionLease/revoke",
        json!({"intent_id":uuid(),"lease_id":lease_id,"expected_revision":1}),
    );
    assert_eq!(revoke["result"]["kind"], "lease", "{revoke}");

    let status = |rig: &mut Rig| {
        rig.call(
            "managedTurn/approval/status",
            json!({"managed_approval_id":approval.managed_approval_id}),
        )
    };
    let revoked = status(&mut rig);
    assert_eq!(revoked["result"]["value"]["acceptable"], false, "{revoked}");
    assert!(revoked["result"]["value"]["command"].is_null(), "{revoked}");

    *rig.spawn.exited.lock().unwrap() = true;
    rig.authority.tick(Instant::now()).unwrap();
    let before = read_snapshot(&rig);
    let exited = status(&mut rig);
    assert_ne!(
        exited["result"]["value"]["approval"]["state"], "pending",
        "{exited}"
    );
    assert_eq!(exited["result"]["value"]["acceptable"], false, "{exited}");
    assert!(exited["result"]["value"]["command"].is_null(), "{exited}");
    let pending = rig.call(
        "managedTurn/approval/pending",
        json!({"managed_turn_id":turn_id}),
    );
    assert!(pending["result"]["value"].is_null(), "{pending}");
    assert_eq!(
        read_snapshot(&rig),
        before,
        "reads after exit must not write"
    );
    validate_v8(rig.authority.engine().connection()).unwrap();
}

fn send_to(authority: &mut Authority, client: &mut Client, spawn: &mut Spawn, v: Value) -> Value {
    let bytes = serde_json::to_vec(&v).unwrap();
    let out = authority.handle(client, &bytes, spawn, Instant::now());
    serde_json::from_slice(&out).unwrap()
}

/// Closure under a durable runtime: a live launch whose image is read back by a successor
/// authority that holds no live state. Only the durable rows say it may still run.
#[test]
fn a_successor_authority_never_closes_a_durably_running_runtime() {
    let mut rig = Rig::new();
    rig.hello();
    let (_, fence) = rig.ready();
    let s = rig.sessions[0].clone();
    let path = std::env::temp_dir().join(format!("wyss-f8-running-{}.sqlite3", uuid()));
    rig.authority
        .engine()
        .connection()
        .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    let auth = wire(AUTH);
    let engine = crate::f8::engine::Engine::new(
        conn,
        auth.clone(),
        crate::f8::registry::Registry::new(auth).unwrap(),
        None,
    )
    .unwrap();
    let mut successor = Authority::new(engine);
    let mut client = Client::default();
    let hello = send_to(
        &mut successor,
        &mut client,
        &mut rig.spawn,
        json!({"schema_version":2,"request_id":uuid(),
        "method":"authority/hello","params":{"protocol_version":2,"client_instance_id":uuid()}}),
    );
    assert_eq!(hello["result"]["kind"], "hello", "{hello}");

    let state = |a: &Authority| -> String {
        a.engine()
            .connection()
            .query_row(
                "SELECT control_state||':'||revision FROM managed_sessions WHERE session_id=?1",
                [&s],
                |r| r.get(0),
            )
            .unwrap()
    };
    let before = state(&successor);
    let closed = send_to(
        &mut successor,
        &mut client,
        &mut rig.spawn,
        json!({"schema_version":2,"request_id":uuid(),"authority_instance_id":AUTH,
        "method":"managedSession/close",
        "params":{"intent_id":uuid(),"managed_session_id":s,"expected_revision":1,
            "runtime_fence":{"managed_session_id":s,"generation":fence["generation"],
                "runtime_instance_id":null}}}),
    );
    assert_eq!(
        code(&closed),
        "runtime_operation_in_progress",
        "a durably running runtime must block closure: {closed}"
    );
    assert_eq!(
        state(&successor),
        before,
        "the refusal leaves the session open"
    );
    std::fs::remove_file(path).unwrap();
}

/// Closure against a live launch: a ready runtime refuses it, and an active turn refuses it.
#[test]
fn closure_refuses_a_ready_or_busy_runtime() {
    let mut rig = Rig::new();
    rig.hello();
    let (reference, fence) = rig.ready();
    let s = rig.sessions[0].clone();
    let close = |rig: &mut Rig, fence: Value| {
        rig.call(
            "managedSession/close",
            json!({"intent_id":uuid(),"managed_session_id":s,"expected_revision":1,
                "runtime_fence":fence}),
        )
    };
    let ready = close(&mut rig, fence.clone());
    assert_eq!(code(&ready), "runtime_operation_in_progress", "{ready}");

    let grant = rig.call(
        "executionLease/grant",
        json!({"intent_id":uuid(),"session_id":s,"expected_session_revision":1,
        "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence}),
    );
    assert_eq!(grant["result"]["kind"], "lease", "{grant}");
    rig.bind(&reference);
    let turn = rig.call(
        "managedSession/turn/start",
        json!({"intent_id":uuid(),"managed_session_id":s,
        "expected_session_revision":1,"prompt":"busy","runtime_fence":fence}),
    );
    assert_eq!(turn["result"]["kind"], "turn", "{turn}");
    let busy = close(&mut rig, fence.clone());
    assert_eq!(code(&busy), "active_turn_exists", "{busy}");
    validate_v8(rig.authority.engine().connection()).unwrap();
}

/// Historical identity: once the launch has stopped, its old instance proves nothing about the
/// present; only the exact generation with no instance can close the session.
#[test]
fn a_stopped_launch_closes_only_with_its_exact_generation_and_no_instance() {
    let mut rig = Rig::new();
    rig.hello();
    let (reference, _) = rig.ready();
    let s = rig.sessions[0].clone();
    let (generation, revision) = rig.expect(&s);
    let stop = rig.call(
        "runtime/stop",
        json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex",
        "managed_session_id":s,"expected_generation":generation,
        "expected_revision":revision,"expected_runtime_instance_id":reference.runtime_instance_id}),
    );
    assert_eq!(stop["result"]["kind"], "runtime_control", "{stop}");
    *rig.spawn.exited.lock().unwrap() = true;
    rig.authority.tick(Instant::now()).unwrap();

    let historical = rig.call(
        "managedSession/close",
        json!({"intent_id":uuid(),"managed_session_id":s,"expected_revision":1,
            "runtime_fence":{"managed_session_id":s,"generation":reference.generation,
                "runtime_instance_id":reference.runtime_instance_id}}),
    );
    assert_eq!(code(&historical), "stale_revision", "{historical}");
    let exact = rig.call(
        "managedSession/close",
        json!({"intent_id":uuid(),"managed_session_id":s,"expected_revision":1,
            "runtime_fence":{"managed_session_id":s,"generation":reference.generation,
                "runtime_instance_id":null}}),
    );
    assert_eq!(
        exact["result"]["value"]["control_state"], "closed",
        "{exact}"
    );
    validate_v8(rig.authority.engine().connection()).unwrap();
}

/// Everything an explicit stop could change, as text: the runtime row, launch sidecars, turns,
/// approvals, receipts, the audit stream and its head.
fn stop_state(c: &rusqlite::Connection, s: &str) -> String {
    c.query_row(
        "SELECT coalesce((SELECT desired_state||':'||revision||':'||generation FROM managed_runtimes WHERE managed_session_id=?1),'')||'|'||\
         (SELECT count(*) FROM managed_runtime_launches)||'|'||\
         (SELECT coalesce(group_concat(managed_turn_id||':'||state||':'||revision,';'),'') FROM (SELECT * FROM managed_turns ORDER BY managed_turn_id))||'|'||\
         (SELECT coalesce(group_concat(managed_approval_id||':'||state||':'||revision,';'),'') FROM (SELECT * FROM managed_approvals ORDER BY managed_approval_id))||'|'||\
         (SELECT count(*) FROM processed_intents)||'|'||(SELECT count(*) FROM control_events)||'|'||\
         (SELECT last_sequence FROM authority_metadata WHERE id=1)",
        [s],
        |r| r.get(0),
    )
    .unwrap()
}

fn explicit_stop(
    rig_expect: (u64, u64),
    reference: &RuntimeRef,
    s: &str,
    revision_bump: u64,
) -> Value {
    json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex",
        "managed_session_id":s,"expected_generation":rig_expect.0,
        "expected_revision":rig_expect.1 + revision_bump,
        "expected_runtime_instance_id":reference.runtime_instance_id})
}

/// A live, bound launch with one running turn, on the first session.
fn running_turn(rig: &mut Rig) -> (RuntimeRef, Value, String) {
    let s = rig.sessions[0].clone();
    let (reference, fence) = rig.ready();
    let grant = rig.call(
        "executionLease/grant",
        json!({"intent_id":uuid(),"session_id":s,"expected_session_revision":1,
        "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence}),
    );
    assert_eq!(grant["result"]["kind"], "lease", "{grant}");
    rig.bind(&reference);
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
            Some("provider-turn"),
            TurnState::Running,
            None,
        )
        .unwrap();
    (reference, fence, turn_id)
}

#[test]
fn explicit_stop_is_refused_while_a_turn_runs_and_changes_no_state() {
    let mut rig = Rig::new();
    rig.hello();
    let (reference, _, _) = running_turn(&mut rig);
    let s = rig.sessions[0].clone();
    let before = stop_state(rig.authority.engine().connection(), &s);
    let live_before = rig.authority.engine().active_turn(&s).map(str::to_owned);
    assert!(live_before.is_some());

    let stop = rig.call(
        "runtime/stop",
        explicit_stop(rig.expect(&s), &reference, &s, 0),
    );
    assert_eq!(code(&stop), "active_turn_exists", "{stop}");
    assert_eq!(
        stop_state(rig.authority.engine().connection(), &s),
        before,
        "no durable change"
    );
    assert_eq!(
        rig.authority.engine().active_turn(&s).map(str::to_owned),
        live_before,
        "the live ticket is intact"
    );
    assert_eq!(
        rig.authority.engine().reference(&s),
        Some(reference),
        "the launch is untouched"
    );
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn a_successor_without_live_state_still_refuses_a_stop_while_the_durable_turn_runs() {
    let mut rig = Rig::new();
    rig.hello();
    let (reference, _, _) = running_turn(&mut rig);
    let s = rig.sessions[0].clone();
    let path = std::env::temp_dir().join(format!("wyss-f8-stop-{}.sqlite3", uuid()));
    rig.authority
        .engine()
        .connection()
        .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    let auth = wire(AUTH);
    let engine = crate::f8::engine::Engine::new(
        conn,
        auth.clone(),
        crate::f8::registry::Registry::new(auth).unwrap(),
        None,
    )
    .unwrap();
    let mut successor = Authority::new(engine);
    let mut client = Client::default();
    let hello = send_to(
        &mut successor,
        &mut client,
        &mut rig.spawn,
        json!({"schema_version":2,"request_id":uuid(),
        "method":"authority/hello","params":{"protocol_version":2,"client_instance_id":uuid()}}),
    );
    assert_eq!(hello["result"]["kind"], "hello", "{hello}");
    assert!(
        successor.engine().active_turn(&s).is_none(),
        "no live ticket in the successor"
    );

    let (_, revision) = rig.expect(&s);
    let before = stop_state(successor.engine().connection(), &s);
    let stop = send_to(
        &mut successor,
        &mut client,
        &mut rig.spawn,
        json!({"schema_version":2,"request_id":uuid(),"authority_instance_id":AUTH,
        "method":"runtime/stop",
        "params":{"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex",
            "managed_session_id":s,"expected_generation":reference.generation,
            "expected_revision":revision,
            "expected_runtime_instance_id":reference.runtime_instance_id}}),
    );
    assert_eq!(code(&stop), "active_turn_exists", "{stop}");
    assert_eq!(
        stop_state(successor.engine().connection(), &s),
        before,
        "no durable change"
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn explicit_stop_is_refused_while_a_pending_approval_waits_on_its_turn() {
    let mut rig = Rig::new();
    rig.hello();
    let (reference, fence, _turn, approval) = live_approval(&mut rig, 600, "item-1");
    let s = rig.sessions[0].clone();
    let before = stop_state(rig.authority.engine().connection(), &s);

    let stop = rig.call(
        "runtime/stop",
        explicit_stop(rig.expect(&s), &reference, &s, 0),
    );
    assert_eq!(code(&stop), "active_turn_exists", "{stop}");
    assert_eq!(
        stop_state(rig.authority.engine().connection(), &s),
        before,
        "approval untouched"
    );
    let status = rig.call(
        "managedTurn/approval/status",
        json!({"managed_approval_id":approval.managed_approval_id}),
    );
    assert_eq!(
        status["result"]["value"]["approval"]["state"], "pending",
        "{status}"
    );
    assert_eq!(status["result"]["value"]["acceptable"], true, "{status}");
    let _ = fence;
}

/// A live, bound launch whose one turn has completed on the provider's word.
fn completed_turn(rig: &mut Rig) -> RuntimeRef {
    let (reference, _, turn_id) = running_turn(rig);
    rig.authority
        .provider_turn_event(
            &reference,
            &ManagedTurnId::parse(&turn_id).unwrap(),
            Some("provider-turn"),
            TurnState::Completed,
            Some(crate::contracts::turn::TurnReason::Completed),
        )
        .unwrap();
    reference
}

#[test]
fn explicit_stop_after_a_completed_turn_is_exact_replayable_and_reaps_cleanly() {
    let mut rig = Rig::new();
    rig.hello();
    let reference = completed_turn(&mut rig);
    let s = rig.sessions[0].clone();
    let params = explicit_stop(rig.expect(&s), &reference, &s, 0);
    let conn_state = |rig: &Rig| stop_state(rig.authority.engine().connection(), &s);

    let stop = rig.call("runtime/stop", params.clone());
    assert_eq!(stop["result"]["kind"], "runtime_control", "{stop}");
    assert_eq!(
        stop["result"]["value"]["desired_state"], "stopped",
        "{stop}"
    );
    validate_v8(rig.authority.engine().connection()).unwrap();

    let before = conn_state(&rig);
    let replay = rig.call("runtime/stop", params.clone());
    assert_eq!(
        replay["result"]["value"], stop["result"]["value"],
        "exact replay"
    );
    assert_eq!(conn_state(&rig), before, "a replay has no effect");

    *rig.spawn.exited.lock().unwrap() = true;
    rig.authority.tick(Instant::now()).unwrap();
    assert!(
        rig.authority.engine().reference(&s).is_none(),
        "the stopped launch is reaped"
    );
    validate_v8(rig.authority.engine().connection()).unwrap();

    let reaped = conn_state(&rig);
    let late = rig.call("runtime/stop", params.clone());
    assert_eq!(
        late["result"]["value"], stop["result"]["value"],
        "replay after the exit"
    );
    assert_eq!(
        conn_state(&rig),
        reaped,
        "replay after the exit writes nothing"
    );

    let mut changed = params.clone();
    changed["expected_revision"] = json!(params["expected_revision"].as_u64().unwrap() + 1);
    assert_eq!(code(&rig.call("runtime/stop", changed)), "intent_conflict");
    assert_eq!(
        conn_state(&rig),
        reaped,
        "a conflicting replay writes nothing"
    );
}

#[test]
fn a_failed_stop_transaction_leaves_no_partial_stop_and_no_receipt() {
    let mut rig = Rig::new();
    rig.hello();
    let reference = completed_turn(&mut rig);
    let s = rig.sessions[0].clone();
    let params = explicit_stop(rig.expect(&s), &reference, &s, 0);
    let before = stop_state(rig.authority.engine().connection(), &s);

    rig.authority
        .engine()
        .connection()
        .execute_batch(
            "CREATE TRIGGER stop_fault BEFORE INSERT ON processed_intents \
             BEGIN SELECT RAISE(ABORT,'injected fault'); END;",
        )
        .unwrap();
    let failed = rig.call("runtime/stop", params.clone());
    assert_ne!(code(&failed), "<ok>", "{failed}");
    assert_eq!(
        stop_state(rig.authority.engine().connection(), &s),
        before,
        "no partial stop"
    );
    assert_eq!(
        rig.authority.engine().reference(&s),
        Some(reference.clone()),
        "the launch is still live"
    );
    rig.authority
        .engine()
        .connection()
        .execute_batch("DROP TRIGGER stop_fault;")
        .unwrap();
    validate_v8(rig.authority.engine().connection()).unwrap();

    let retried = rig.call("runtime/stop", params);
    assert_eq!(retried["result"]["kind"], "runtime_control", "{retried}");
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn mandatory_shutdowns_still_contain_a_runtime_with_an_active_turn() {
    // An unexpected worker exit.
    let mut rig = Rig::new();
    rig.hello();
    let (_, _, turn_id) = running_turn(&mut rig);
    let s = rig.sessions[0].clone();
    *rig.spawn.exited.lock().unwrap() = true;
    rig.authority.tick(Instant::now()).unwrap();
    assert!(
        rig.authority.engine().reference(&s).is_none(),
        "the exited launch is contained"
    );
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
    assert_ne!(
        state, "completed",
        "a shutdown is never recorded as a successful turn"
    );
    validate_v8(rig.authority.engine().connection()).unwrap();

    // A failed transport while the provider decision is in flight.
    let mut rig = Rig::new();
    rig.hello();
    let (reference, _, turn_id) = running_turn(&mut rig);
    let s = rig.sessions[0].clone();
    rig.authority.engine_mut().transport_failed(&reference);
    *rig.spawn.exited.lock().unwrap() = true;
    rig.authority.tick(Instant::now()).unwrap();
    assert!(
        rig.authority.engine().reference(&s).is_none(),
        "the failed transport is contained"
    );
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
    assert_ne!(state, "completed");
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn review_exit_audit_retry_reconciles_stranded_work() {
    let mut rig = Rig::new();
    rig.hello();
    let (launch, fence) = rig.ready();
    let session = rig.sessions[0].clone();
    rig.call(
        "executionLease/grant",
        json!({
            "intent_id":uuid(),"session_id":session,"expected_session_revision":1,
            "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence
        }),
    );
    rig.bind(&launch);
    let started = rig.call(
        "managedSession/turn/start",
        json!({
            "intent_id":uuid(),"managed_session_id":session,
            "expected_session_revision":1,"prompt":"recovery-check","runtime_fence":fence
        }),
    );
    let turn_id = started["result"]["value"]["managed_turn_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let turn = ManagedTurnId::parse(&turn_id).unwrap();
    rig.authority
        .provider_turn_event(
            &launch,
            &turn,
            Some("provider-recovery"),
            TurnState::Running,
            None,
        )
        .unwrap();
    // The child is reaped, but its durable exit event cannot be recorded yet.
    rig.authority
        .engine()
        .connection()
        .execute_batch(
            "CREATE TEMP TRIGGER f8_fail_recover BEFORE INSERT ON main.control_events
         WHEN json_extract(NEW.envelope,'$.event_type')='authority.runtime.instance_exited'
         BEGIN SELECT RAISE(ABORT,'injected recovery storage refusal'); END;",
        )
        .unwrap();
    *rig.spawn.exited.lock().unwrap() = true;
    rig.authority.tick(Instant::now()).unwrap();
    assert_eq!(rig.authority.engine().unrecorded_exits(), 1);
    rig.authority
        .engine()
        .connection()
        .execute_batch("DROP TRIGGER temp.f8_fail_recover")
        .unwrap();
    rig.authority.tick(Instant::now()).unwrap();
    assert!(!rig.authority.recovery_pending);
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
}

#[test]
fn review_lease_replay_preserves_original_fence_after_runtime_loss() {
    let mut rig = Rig::new();
    rig.hello();
    let (launch, fence) = rig.ready();
    let session = rig.sessions[0].clone();
    let params = json!({"intent_id":uuid(),"session_id":session,"expected_session_revision":1,
        "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence});
    let original = rig.call("executionLease/grant", params.clone());
    assert_eq!(code(&original), "<ok>", "{original}");
    rig.authority.engine_mut().transport_failed(&launch);
    rig.authority.tick(Instant::now()).unwrap();
    let replay = rig.call("executionLease/grant", params.clone());
    assert_eq!(replay["result"], original["result"], "{replay}");
    let mut changed = params;
    changed["runtime_fence"]["generation"] = json!(launch.generation + 1);
    assert_eq!(
        code(&rig.call("executionLease/grant", changed)),
        "intent_conflict"
    );
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn review_record_replay_checks_the_original_fence_before_live_admission() {
    for approval in [false, true] {
        let mut rig = Rig::new();
        rig.hello();
        let (launch, fence, turn) = running_turn(&mut rig);
        let (method, mut params) = if approval {
            let a = rig.action_approval(&launch, "replay-bound-item");
            (
                "managedTurn/approval/respond",
                json!({"intent_id":uuid(),
                "managed_approval_id":a.managed_approval_id,"expected_revision":1,
                "decision":"decline","runtime_fence":fence}),
            )
        } else {
            (
                "managedSession/turn/cancel",
                json!({"intent_id":uuid(),
                "managed_turn_id":turn,"expected_revision":2,"runtime_fence":fence}),
            )
        };
        let original = rig.call(method, params.clone());
        assert_eq!(code(&original), "<ok>", "{method}: {original}");
        let writes = rig.sent().len();
        rig.authority.engine_mut().transport_failed(&launch);
        rig.authority.tick(Instant::now()).unwrap();
        let changes = rig.authority.engine().connection().total_changes();
        let replay = rig.call(method, params.clone());
        assert_eq!(replay["result"], original["result"], "{method}: {replay}");
        params["runtime_fence"]["generation"] = json!(launch.generation + 1);
        assert_eq!(
            code(&rig.call(method, params)),
            "intent_conflict",
            "{method}"
        );
        assert_eq!(rig.sent().len(), writes, "replay must never redispatch");
        assert_eq!(rig.authority.engine().connection().total_changes(), changes);
        validate_v8(rig.authority.engine().connection()).unwrap();
    }
}

#[test]
fn review_turn_start_exact_replay_survives_authority_succession_without_dispatch() {
    let mut rig = Rig::new();
    rig.hello();
    let (launch, fence, turn) = running_turn(&mut rig);
    let (intent, raw): (String, String) = rig.authority.engine().connection().query_row(
        "SELECT intent_id,result FROM processed_intents WHERE json_extract(result,'$.managed_turn_id')=?1",
        [&turn], |r| Ok((r.get(0)?, r.get(1)?)),
    ).unwrap();
    let original: Value = serde_json::from_str(&raw).unwrap();
    let conn = migration::clone_database(rig.authority.engine().connection()).unwrap();
    let auth = wire(&uuid());
    let engine = Engine::new(
        conn,
        auth.clone(),
        Registry::new(auth.clone()).unwrap(),
        None,
    )
    .unwrap();
    let mut successor = Authority::new(engine);
    let mut client = Client::default();
    let hello = send_to(
        &mut successor,
        &mut client,
        &mut rig.spawn,
        json!({"schema_version":2,"request_id":uuid(),"method":"authority/hello",
            "params":{"protocol_version":2,"client_instance_id":uuid()}}),
    );
    assert_eq!(hello["result"]["kind"], "hello", "{hello}");
    successor.tick(Instant::now()).unwrap();
    let changes = successor.engine().connection().total_changes();
    let sent = rig.sent().len();
    let mut params = json!({"intent_id":intent,"managed_session_id":launch.managed_session_id,
        "expected_session_revision":1,"prompt":"work","runtime_fence":fence});
    let replay = send_to(
        &mut successor,
        &mut client,
        &mut rig.spawn,
        json!({"schema_version":2,"request_id":uuid(),"authority_instance_id":auth,
            "method":"managedSession/turn/start","params":params}),
    );
    assert_eq!(replay["result"]["value"], original, "{replay}");
    params["runtime_fence"]["generation"] = json!(launch.generation + 1);
    let conflict = send_to(
        &mut successor,
        &mut client,
        &mut rig.spawn,
        json!({"schema_version":2,"request_id":uuid(),"authority_instance_id":auth,
            "method":"managedSession/turn/start","params":params}),
    );
    assert_eq!(code(&conflict), "intent_conflict", "{conflict}");
    assert_eq!(rig.sent().len(), sent);
    assert_eq!(successor.engine().connection().total_changes(), changes);
    assert!(
        successor
            .engine()
            .reference(launch.managed_session_id.as_str())
            .is_none()
    );
    validate_v8(successor.engine().connection()).unwrap();
}

#[path = "claude/parity_tests.rs"]
mod claude_parity;
