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
    fn containment(&mut self)->super::super::engine::Containment { super::super::engine::Containment::PidNamespace }
    fn preflight(&mut self, _context:&Attachment)->ControlResult<()> { Ok(()) }
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
struct Spawn {
    inbound: Inbound,
    sent: Sent,
    exited: Arc<Mutex<bool>>,
    refuse_critical: Arc<Mutex<bool>>,
    ambiguous_send: Arc<Mutex<bool>>,
}
impl Spawner for Spawn {
    fn descendant_containment(&self)->bool { true }
    fn spawn(
        &mut self,
        launch: &RuntimeRef,
        _staging: Option<&mut super::super::staging::LaunchStaging>,
    ) -> ControlResult<Spawned> {
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
        let workspace: String = conn.query_row("SELECT workspace_root FROM managed_session_bindings LIMIT 1",[],|r|r.get(0)).unwrap();
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
            },
            sessions,
            workspace,
        }
    }
    fn bind(&mut self, launch:&RuntimeRef) {
        let id=uuid();
        let reply=self.call("managedSession/runtime/resume",json!({"intent_id":id,"managed_session_id":launch.managed_session_id,"expected_session_revision":1,"runtime_fence":launch.fence()}));
        assert_eq!(code(&reply),"runtime_operation_in_progress","{reply}");
        let thread:String=self.authority.engine().connection().query_row("SELECT runtime_session_id FROM managed_session_bindings WHERE managed_session_id=?1",[launch.managed_session_id.as_str()],|r|r.get(0)).unwrap();
        super::super::operations::confirm_binding(self.authority.engine_mut(),launch,&wire(&id),&thread).unwrap();
    }
    fn action_approval(&mut self, launch:&RuntimeRef, item:&str) -> crate::contracts::approval::ManagedApprovalRecord {
        let action=super::super::operations::ProviderAction::Command {command:"true".into(),cwd:self.workspace.to_str().unwrap().into(),parsed_actions:vec![]};
        let (kind,fingerprint,structure)=action.metadata(item).unwrap();
        let record=self.authority.provider_approval_request(launch,kind,item,&fingerprint,structure).unwrap();
        self.authority.engine_mut().hold_approval_action(launch,item,action).unwrap();
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
impl Drop for Rig { fn drop(&mut self) { let _ = std::fs::remove_dir(&self.workspace); } }
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
        started["result"]["value"]["managed_turn_id"].as_str().unwrap(),
    ).unwrap();
    let head = |rig: &Rig| -> i64 {
        rig.authority.engine().connection().query_row(
            "SELECT count(*) FROM control_events", [], |r| r.get(0)
        ).unwrap()
    };
    let before = head(&rig);
    assert!(rig.authority.provider_turn_event(
        &b, &turn, Some("foreign-turn"), TurnState::Running, None,
    ).is_err());
    assert_eq!(head(&rig), before, "valid launch B cannot progress A's turn");
    rig.authority.provider_turn_event(
        &a, &turn, Some("provider-a"), TurnState::Running, None,
    ).unwrap();
    let approved = rig.action_approval(&a, "owned-item");
    let pending_count = head(&rig);
    // Duplicate provider IDs must not write a second record or invalidate the first.
    assert!(rig.authority.provider_approval_request(
        &a, ApprovalKind::CommandExecution, "owned-item", FP,
        ApprovalStructure {
            command_bytes: 12, cwd_bytes: 6, parsed_actions: 1, files: 0, diff_bytes: 0,
        },
    ).is_err());
    assert_eq!(head(&rig), pending_count);
    assert_eq!(rig.authority.engine().pending_approvals(&session), 1);
    let respond = rig.call(
        "managedTurn/approval/respond",
        json!({"intent_id":uuid(),"managed_approval_id":approved.managed_approval_id,
            "expected_revision":1,"decision":"accept","runtime_fence":fence_a}),
    );
    assert_eq!(respond["result"]["kind"], "approval_receipt", "{respond}");
    let before_foreign_confirmation = head(&rig);
    assert!(rig.authority.provider_approval_confirmed(
        &b, &approved.managed_approval_id, "owned-item",
    ).is_err());
    assert_eq!(head(&rig), before_foreign_confirmation);
    rig.authority.provider_approval_confirmed(
        &a, &approved.managed_approval_id, "owned-item",
    ).unwrap();
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
    let lease = granted["result"]["value"]["lease_id"].as_str().unwrap().to_owned();
    rig.bind(&a);
    let started = rig.call(
        "managedSession/turn/start",
        json!({"intent_id":uuid(),"managed_session_id":session,"expected_session_revision":1,
            "prompt":"pending","runtime_fence":fence}),
    );
    let turn = ManagedTurnId::parse(
        started["result"]["value"]["managed_turn_id"].as_str().unwrap()
    ).unwrap();
    rig.authority.provider_turn_event(
        &a, &turn, Some("p"), TurnState::Running, None,
    ).unwrap();
    let approval = rig.action_approval(&a, "pending-item");
    // The durable lease remains active, but volatile ticket ownership was lost.
    rig.authority.engine_mut().lease_lost(&session, &lease);
    let before: i64 = rig.authority.engine().connection().query_row(
        "SELECT count(*) FROM control_events", [], |r| r.get(0),
    ).unwrap();
    let frames = rig.sent().len();
    let reply = rig.call(
        "managedTurn/approval/respond",
        json!({"intent_id":uuid(),"managed_approval_id":approval.managed_approval_id,
            "expected_revision":1,"decision":"accept","runtime_fence":fence}),
    );
    assert_eq!(code(&reply), "stale_revision", "{reply}");
    let after: i64 = rig.authority.engine().connection().query_row(
        "SELECT count(*) FROM control_events", [], |r| r.get(0),
    ).unwrap();
    assert_eq!(before, after);
    assert_eq!(rig.sent().len(), frames, "lost live ticket is not dispatch authority");
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
            started["result"]["value"]["managed_turn_id"].as_str().unwrap(),
        ).unwrap();
        rig.authority.provider_turn_event(
            &from, &turn, Some("p"), TurnState::Running, None,
        ).unwrap();
        let approval = rig.action_approval(&from, "item-1");
        *rig.spawn.refuse_critical.lock().unwrap() = true;
        let sent = rig.sent().len();
        let response = rig.call(
            "managedTurn/approval/respond",
            json!({"intent_id":uuid(),"managed_approval_id":approval.managed_approval_id,
                "expected_revision":1,"decision":decision,"runtime_fence":fence}),
        );
        assert_ne!(code(&response), "<ok>", "{decision} falsely reported delivery: {response}");
        assert_eq!(rig.sent().len(), sent, "no critical frame reached the test worker");
        assert_eq!(
            rig.authority.engine().runtime_view(&session).map(|(s, _)| s),
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
    rig.call("executionLease/grant", json!({
        "intent_id": uuid(), "session_id": session, "expected_session_revision":1,
        "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence
    }));
    rig.bind(&launch);
    let start = rig.call("managedSession/turn/start", json!({
        "intent_id":uuid(),"managed_session_id":session,
        "expected_session_revision":1,"prompt":"cancel-check","runtime_fence":fence
    }));
    let turn_id = start["result"]["value"]["managed_turn_id"].as_str().unwrap();
    let turn = ManagedTurnId::parse(turn_id).unwrap();
    rig.authority.provider_turn_event(
        &launch, &turn, Some("provider-cancel"), TurnState::Running, None
    ).unwrap();
    rig.authority.engine_mut().transport_failed(&launch);
    let before: (i64,i64) = rig.authority.engine().connection().query_row(
        "SELECT (SELECT count(*) FROM control_events), (SELECT count(*) FROM processed_intents)",
        [], |r| Ok((r.get(0)?,r.get(1)?))
    ).unwrap();
    let denied = rig.call("managedSession/turn/cancel", json!({
        "intent_id":uuid(),"managed_turn_id":turn_id,
        "expected_revision":2,"runtime_fence":fence
    }));
    assert_ne!(code(&denied), "<ok>", "{denied}");
    let after: (i64,i64) = rig.authority.engine().connection().query_row(
        "SELECT (SELECT count(*) FROM control_events), (SELECT count(*) FROM processed_intents)",
        [], |r| Ok((r.get(0)?,r.get(1)?))
    ).unwrap();
    assert_eq!(before, after, "rejected cancellation must not mutate the journal");
}

#[test]
fn review_failed_recovery_remains_pending_and_retries_after_storage_restoration() {
    let mut rig = Rig::new();
    rig.hello();
    let (launch, fence) = rig.ready();
    let session = rig.sessions[0].clone();
    rig.call("executionLease/grant", json!({
        "intent_id":uuid(),"session_id":session,"expected_session_revision":1,
        "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence
    }));
    rig.bind(&launch);
    let started = rig.call("managedSession/turn/start", json!({
        "intent_id":uuid(),"managed_session_id":session,
        "expected_session_revision":1,"prompt":"recovery-check","runtime_fence":fence
    }));
    let turn_id = started["result"]["value"]["managed_turn_id"].as_str().unwrap().to_owned();
    let turn = ManagedTurnId::parse(&turn_id).unwrap();
    rig.authority.provider_turn_event(
        &launch, &turn, Some("provider-recovery"), TurnState::Running, None
    ).unwrap();
    // Only recovery writes to the stranded turn are rejected: runtime exit audit
    // itself succeeds so that the original one-shot ended marker is consumed.
    rig.authority.engine().connection().execute_batch(
        "CREATE TEMP TRIGGER f8_fail_recover BEFORE UPDATE ON main.managed_turns
         WHEN NEW.state='indeterminate'
         BEGIN SELECT RAISE(ABORT,'injected recovery storage refusal'); END;"
    ).unwrap();
    *rig.spawn.exited.lock().unwrap() = true;
    assert!(rig.authority.tick(Instant::now()).is_err());
    assert!(rig.authority.recovery_pending, "recovery must remain retryable");
    rig.authority.engine().connection().execute_batch(
        "DROP TRIGGER temp.f8_fail_recover"
    ).unwrap();
    rig.authority.tick(Instant::now()).unwrap();
    assert!(!rig.authority.recovery_pending);
    let state: String = rig.authority.engine().connection().query_row(
        "SELECT state FROM managed_turns WHERE managed_turn_id=?1",
        [&turn_id], |r| r.get(0)
    ).unwrap();
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
    rig.call("executionLease/grant", json!({
        "intent_id":uuid(),"session_id":session_a,"expected_session_revision":1,
        "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence_a
    }));
    rig.bind(&a);
    let started = rig.call("managedSession/turn/start", json!({
        "intent_id":uuid(),"managed_session_id":session_a,
        "expected_session_revision":1,"prompt":"inbound-check","runtime_fence":fence_a
    }));
    let turn_id = started["result"]["value"]["managed_turn_id"].as_str().unwrap().to_owned();
    let head = |rig: &Rig| -> i64 {
        rig.authority.engine().connection().query_row(
            "SELECT count(*) FROM control_events", [], |r| r.get(0)
        ).unwrap()
    };
    // A legitimate B worker emits a validly encoded turn event carrying A's ID.
    // The transport, not the payload, determines its origin and must reject it.
    rig.spawn.inbound.lock().unwrap().push((session_b.clone(),
        serde_json::to_vec(&json!({
            "kind":"turn_running","managed_turn_id":turn_id,
            "provider_turn_id":"provider-foreign"
        })).unwrap()
    ));
    let prior = head(&rig);
    rig.authority.tick(Instant::now()).unwrap();
    assert_eq!(head(&rig) - prior, 0, "B cannot change A's durable turn");
    assert_eq!(rig.authority.engine().runtime_view(&session_b).map(|r|r.0),Some("stopping"));
    assert_eq!(rig.authority.engine().runtime_view(&session_a).map(|r|r.0),Some("ready"));

    rig.spawn.inbound.lock().unwrap().push((session_a.clone(),
        serde_json::to_vec(&json!({
            "kind":"turn_running","managed_turn_id":turn_id,
            "provider_turn_id":"provider-owned"
        })).unwrap()
    ));
    rig.authority.tick(Instant::now()).unwrap();
    let state: String = rig.authority.engine().connection().query_row(
        "SELECT state FROM managed_turns WHERE managed_turn_id=?1",
        [&turn_id], |r| r.get(0)
    ).unwrap();
    assert_eq!(state, "running");
    assert_eq!(rig.authority.engine().reference(&session_a), Some(a));
    assert_eq!(rig.authority.engine().reference(&session_b), None, "faulted B was reaped while A continued");
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
    rig.call("executionLease/grant", json!({
        "intent_id":uuid(),"session_id":session,"expected_session_revision":1,
        "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence
    }));
    rig.bind(&_launch);
    *rig.spawn.ambiguous_send.lock().unwrap() = true;
    let reply = rig.call("managedSession/turn/start", json!({
        "intent_id":uuid(),"managed_session_id":session,
        "expected_session_revision":1,"prompt":"may-have-reached-provider",
        "runtime_fence":fence
    }));
    assert_eq!(code(&reply), "external_effect_unknown", "{reply}");
    let (state, reason): (String, String) = rig.authority.engine().connection().query_row(
        "SELECT state,terminal_reason FROM managed_turns ORDER BY created_at DESC LIMIT 1",
        [], |r| Ok((r.get(0)?,r.get(1)?))
    ).unwrap();
    assert_eq!((state.as_str(), reason.as_str()),
        ("indeterminate", "dispatch_unknown"));
    assert_eq!(rig.authority.engine().runtime_view(&session).map(|r|r.0),Some("stopping"));
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn review_provider_interrupt_send_failure_does_not_report_success() {
    let mut rig = Rig::new();
    rig.hello();
    let (launch, fence) = rig.ready();
    let session = rig.sessions[0].clone();
    rig.call("executionLease/grant", json!({
        "intent_id":uuid(),"session_id":session,"expected_session_revision":1,
        "scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence
    }));
    rig.bind(&launch);
    let started = rig.call("managedSession/turn/start", json!({
        "intent_id":uuid(),"managed_session_id":session,
        "expected_session_revision":1,"prompt":"running-work","runtime_fence":fence
    }));
    let turn_id = started["result"]["value"]["managed_turn_id"].as_str().unwrap().to_owned();
    rig.authority.provider_turn_event(
        &launch, &ManagedTurnId::parse(&turn_id).unwrap(),
        Some("provider-running"), TurnState::Running, None
    ).unwrap();
    *rig.spawn.ambiguous_send.lock().unwrap() = true;
    let response = rig.call("managedSession/turn/cancel", json!({
        "intent_id":uuid(),"managed_turn_id":turn_id,
        "expected_revision":2,"runtime_fence":fence
    }));
    assert_eq!(code(&response), "external_effect_unknown", "{response}");
    assert_eq!(rig.authority.engine().runtime_view(&session).map(|r|r.0),Some("stopping"));
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn approval_kernel_write_followed_by_writer_constructor_failure_fences_without_replay() {
    let mut rig=Rig::new();rig.hello();let (origin,fence)=rig.ready();
    let session=rig.sessions[0].clone();
    let grant=rig.call("executionLease/grant",json!({"intent_id":uuid(),"session_id":session,"expected_session_revision":1,"scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence}));
    assert_eq!(grant["result"]["kind"],"lease","{grant}");
    rig.bind(&origin);
    let started=rig.call("managedSession/turn/start",json!({"intent_id":uuid(),"managed_session_id":session,"expected_session_revision":1,"prompt":"effect","runtime_fence":fence}));
    let turn=ManagedTurnId::parse(started["result"]["value"]["managed_turn_id"].as_str().unwrap()).unwrap();
    rig.authority.provider_turn_event(&origin,&turn,Some("provider-t"),TurnState::Running,None).unwrap();
    let approval=rig.action_approval(&origin,"effect-item");
    let intent=uuid();let params=json!({"intent_id":intent,"managed_approval_id":approval.managed_approval_id,"expected_revision":1,"decision":"accept","runtime_fence":fence});
    rig.authority.post_approval_write=Some(|c|c.pragma_update(None,"user_version",7).unwrap());
    let before=rig.sent().len();let answer=rig.call("managedTurn/approval/respond",params.clone());
    assert_eq!(code(&answer),"storage_incompatible","{answer}");
    assert_eq!(rig.sent().len(),before+1,"one atomic provider write happened");
    assert_eq!(rig.authority.engine().runtime_view(&session).map(|s|s.0),Some("stopping"));
    rig.authority.engine_mut().connection_mut().pragma_update(None,"user_version",8).unwrap();
    assert_eq!(super::super::writers::read_delivery(rig.authority.engine().connection(),approval.managed_approval_id.as_str()).unwrap(),Some(ApprovalDelivery::DecisionAccepted));
    let replay=rig.call("managedTurn/approval/respond",params);
    assert_eq!(replay["result"]["kind"],"approval_receipt","{replay}");
    assert_eq!(rig.sent().len(),before+1,"durable decision replay never dispatches");
    rig.authority.tick(Instant::now()).unwrap();
    assert_eq!(super::super::writers::read_delivery(rig.authority.engine().connection(),approval.managed_approval_id.as_str()).unwrap(),Some(ApprovalDelivery::Indeterminate));
    validate_v8(rig.authority.engine().connection()).unwrap();
}

#[test]
fn exact_turn_start_replay_survives_lease_revocation_and_runtime_exit_without_dispatch() {
    let mut rig=Rig::new();rig.hello();let (origin,fence)=rig.ready();let session=rig.sessions[0].clone();
    let grant=rig.call("executionLease/grant",json!({"intent_id":uuid(),"session_id":session,"expected_session_revision":1,"scopes":all_scopes(),"ttl_seconds":600,"runtime_fence":fence}));
    rig.bind(&origin);
    let params=json!({"intent_id":uuid(),"managed_session_id":session,"expected_session_revision":1,"prompt":"once","runtime_fence":fence});
    let admitted=rig.call("managedSession/turn/start",params.clone());assert_eq!(admitted["result"]["kind"],"turn","{admitted}");
    let revoked=rig.call("executionLease/revoke",json!({"intent_id":uuid(),"lease_id":grant["result"]["value"]["lease_id"],"expected_revision":1}));assert_eq!(revoked["result"]["kind"],"lease","{revoked}");
    *rig.spawn.exited.lock().unwrap()=true;rig.authority.tick(Instant::now()).unwrap();
    assert!(rig.authority.engine().reference(&session).is_none());let sent=rig.sent().len();
    let replay=rig.call("managedSession/turn/start",params.clone());assert_eq!(replay["result"]["value"],admitted["result"]["value"],"{replay}");assert_eq!(rig.sent().len(),sent);
    let mut changed=params.clone();changed["prompt"]=json!("different");assert_eq!(code(&rig.call("managedSession/turn/start",changed)),"intent_conflict");
    let mut changed=params;changed["runtime_fence"]["generation"]=json!(origin.generation+1);assert_eq!(code(&rig.call("managedSession/turn/start",changed)),"intent_conflict");
    validate_v8(rig.authority.engine().connection()).unwrap();
}
