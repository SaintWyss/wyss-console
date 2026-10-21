//! Protocol-2 session lifecycle through `Authority::handle` on temporary V8 fixtures:
//! managedSession/prepare and managedSession/close, with replay, refusal, capacity, reopening and
//! fault injection. Faults come from SQLite triggers, so the production writer runs unmodified.
//! No runtime is launched: the spawner counts attempts and refuses every one.
use super::{
    authority::{Authority, Client},
    engine::{Engine, Spawned, Spawner},
    migration,
    protocol::RuntimeRef,
    registry::Registry,
    staging::LaunchStaging,
    v8::validate_v8,
    writers::Writer,
};
use crate::{
    contracts::{
        WireUuid,
        control::{ControlError as E, ControlResult, ExecutionScope, LeaseGrant, Operation},
    },
    control::f8_fixture,
};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{cell::Cell, rc::Rc, time::Instant};

const AUTH: &str = "00000000-0000-4000-8000-0000000000a1";

fn uuid() -> String {
    crate::control::store::uuid().as_str().to_owned()
}

fn code(v: &Value) -> &str {
    v["error"]["code"].as_str().unwrap_or("<ok>")
}

/// Counts every launch attempt and refuses it, so no test can start a runtime by accident.
struct Refuse(Rc<Cell<u32>>);
impl Spawner for Refuse {
    fn spawn(&mut self, _: &RuntimeRef, _: Option<&mut LaunchStaging>) -> ControlResult<Spawned> {
        self.0.set(self.0.get() + 1);
        Err(E::Unsupported)
    }
}

struct Facade {
    authority: Authority,
    client: Client,
    launches: Rc<Cell<u32>>,
}
impl Facade {
    /// Migrates `source` to V8, then greets the authority.
    fn over(mut source: Connection) -> Self {
        let conn = migration::prepare(&mut source).unwrap().into_connection();
        let auth = WireUuid::try_from(AUTH.to_owned()).unwrap();
        let engine = Engine::new(conn, auth.clone(), Registry::new(auth).unwrap(), None).unwrap();
        let mut f = Self {
            authority: Authority::new(engine),
            client: Client::default(),
            launches: Rc::default(),
        };
        let hello = f.raw(json!({"schema_version":2,"request_id":uuid(),
            "method":"authority/hello","params":{"protocol_version":2,"client_instance_id":uuid()}}));
        assert_eq!(hello["result"]["kind"], "hello", "{hello}");
        f
    }
    /// Two prepared sessions: a completed legacy session with a revoked lease, and one more.
    fn fixture() -> Self {
        Self::over(f8_fixture::v7_with_sessions(1))
    }
    fn raw(&mut self, v: Value) -> Value {
        let bytes = serde_json::to_vec(&v).unwrap();
        let mut spawner = Refuse(self.launches.clone());
        let out = self
            .authority
            .handle(&mut self.client, &bytes, &mut spawner, Instant::now());
        serde_json::from_slice(&out).unwrap()
    }
    fn call(&mut self, method: &str, params: Value) -> Value {
        self.raw(
            json!({"schema_version":2,"request_id":uuid(),"authority_instance_id":AUTH,
            "method":method,"params":params}),
        )
    }
    fn prepare(&mut self, intent: &str) -> Value {
        self.call(
            "managedSession/prepare",
            json!({"intent_id":intent,"adapter_id":"codex","runtime_kind":"codex"}),
        )
    }
    fn close(&mut self, intent: &str, session: &str, revision: u64) -> Value {
        self.call(
            "managedSession/close",
            json!({"intent_id":intent,"managed_session_id":session,"expected_revision":revision,
                "runtime_fence":{"managed_session_id":session,"generation":0,"runtime_instance_id":null}}),
        )
    }
    fn conn(&self) -> &Connection {
        self.authority.engine().connection()
    }
    fn scalar(&self, sql: &str) -> String {
        self.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }
    fn count(&self, sql: &str) -> i64 {
        self.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }
    fn sessions(&self) -> Vec<String> {
        let mut q = self
            .conn()
            .prepare("SELECT session_id FROM managed_sessions ORDER BY created_sequence")
            .unwrap();
        q.query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }
    /// Every durable row the lifecycle can touch, as text. Compared before and after refusals,
    /// and after rolled-back transactions. (`total_changes` is not used: it counts writes that a
    /// rollback then undoes.)
    fn durable(&self) -> String {
        self.scalar(
            "SELECT (SELECT last_sequence FROM authority_metadata WHERE id=1)||'|'||\
             (SELECT coalesce(group_concat(session_id||':'||control_state||':'||revision||':'||updated_at,';'),'') FROM (SELECT * FROM managed_sessions ORDER BY session_id))||'|'||\
             (SELECT coalesce(group_concat(lease_id||':'||state||':'||revision||':'||coalesce(revocation_reason,''),';'),'') FROM (SELECT * FROM execution_leases ORDER BY lease_id))||'|'||\
             (SELECT count(*) FROM processed_intents)||'|'||\
             (SELECT count(*) FROM control_events)",
        )
    }
    /// The authority's own audit kinds for one session, in durable sequence order.
    fn audit_kinds(&self, session: &str) -> Vec<String> {
        let mut q = self
            .conn()
            .prepare("SELECT envelope FROM control_events WHERE session_id=?1 ORDER BY sequence")
            .unwrap();
        q.query_map([session], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|e| {
                let v: Value = serde_json::from_str(&e.unwrap()).unwrap();
                v["event_type"].as_str().unwrap().to_owned()
            })
            .collect()
    }
    fn grant_active_lease(&mut self, session: &str) {
        let auth = WireUuid::try_from(AUTH.to_owned()).unwrap();
        let grant = Operation::LeaseGrant(LeaseGrant {
            intent_id: WireUuid::try_from(uuid()).unwrap(),
            session_id: crate::contracts::identities::ManagedSessionId::parse(session).unwrap(),
            expected_session_revision: 1,
            scopes: vec![
                ExecutionScope::SessionCreate,
                ExecutionScope::SessionResume,
                ExecutionScope::TurnStart,
            ],
            ttl_seconds: 600,
        });
        Writer::new(self.authority.engine_mut().connection_mut(), auth)
            .unwrap()
            .lease(&grant)
            .unwrap();
    }
    /// Copies the durable image to a file and reopens it as a new authority, as after a restart.
    fn reopen(&mut self, tag: &str) -> (Self, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("wyss-f8-lifecycle-{tag}-{}.sqlite3", uuid()));
        self.conn()
            .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
            .unwrap();
        let conn = Connection::open(&path).unwrap();
        let auth = WireUuid::try_from(AUTH.to_owned()).unwrap();
        let engine = Engine::new(conn, auth.clone(), Registry::new(auth).unwrap(), None).unwrap();
        let mut f = Self {
            authority: Authority::new(engine),
            client: Client::default(),
            launches: Rc::default(),
        };
        let hello = f.raw(json!({"schema_version":2,"request_id":uuid(),
            "method":"authority/hello","params":{"protocol_version":2,"client_instance_id":uuid()}}));
        assert_eq!(hello["result"]["kind"], "hello", "{hello}");
        (f, path)
    }
}

#[test]
fn prepare_creates_one_session_with_its_audit_event_and_replays_exactly() {
    let mut f = Facade::fixture();
    let before_sessions = f.sessions().len();
    let intent = uuid();
    let first = f.prepare(&intent);
    assert_eq!(first["result"]["kind"], "session", "{first}");
    let v = &first["result"]["value"];
    assert_eq!(v["control_state"], "prepared");
    assert_eq!(v["revision"], 1);
    assert!(
        v["created_sequence"]
            .as_str()
            .is_some_and(|s| !s.is_empty()),
        "{first}"
    );
    let session = v["managed_session_id"].as_str().unwrap().to_owned();
    assert_eq!(
        f.sessions().len(),
        before_sessions + 1,
        "exactly one new session"
    );
    assert_eq!(
        f.audit_kinds(&session),
        vec!["authority.managed_session.prepared".to_owned()]
    );

    let durable = f.durable();
    let replay = f.prepare(&intent);
    assert_eq!(
        replay["result"]["value"], first["result"]["value"],
        "exact replay"
    );
    assert_eq!(f.durable(), durable, "a replay has no effect");

    let conflict = f.call(
        "managedSession/prepare",
        json!({"intent_id":intent,"adapter_id":"other","runtime_kind":"codex"}),
    );
    assert_eq!(code(&conflict), "intent_conflict", "{conflict}");
    assert_eq!(f.durable(), durable, "a conflicting replay has no effect");
    assert_eq!(f.launches.get(), 0);
}

#[test]
fn prepare_refuses_malformed_or_unsupported_input_without_any_write() {
    let mut f = Facade::fixture();
    let before = f.durable();
    let malformed = f.call(
        "managedSession/prepare",
        json!({"intent_id":"not-a-uuid","adapter_id":"codex","runtime_kind":"codex"}),
    );
    assert_eq!(code(&malformed), "invalid_request", "{malformed}");
    let other = f.call(
        "managedSession/prepare",
        json!({"intent_id":uuid(),"adapter_id":"other","runtime_kind":"codex"}),
    );
    assert_eq!(code(&other), "unsupported", "{other}");
    let mismatch = f.call(
        "managedSession/prepare",
        json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"other"}),
    );
    assert_eq!(code(&mismatch), "invalid_request", "{mismatch}");
    assert_eq!(f.durable(), before, "refusals leave no durable trace");
}

#[test]
fn prepare_capacity_refuses_without_partial_writes() {
    let mut f = Facade::fixture();
    while f.sessions().len() < 128 {
        let r = f.prepare(&uuid());
        assert_eq!(r["result"]["kind"], "session", "{r}");
    }
    let before = f.durable();
    let refused = f.prepare(&uuid());
    assert_eq!(code(&refused), "capacity_exceeded", "{refused}");
    assert_eq!(f.sessions().len(), 128);
    assert_eq!(
        f.durable(),
        before,
        "the rolled-back attempt left no durable trace"
    );
    validate_v8(f.conn()).unwrap();
}

#[test]
fn prepare_survives_reopening_the_durable_image_with_its_replay_receipt() {
    let mut f = Facade::fixture();
    let intent = uuid();
    let first = f.prepare(&intent);
    let session = first["result"]["value"]["managed_session_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (mut reopened, path) = f.reopen("prepare");

    assert!(
        reopened.sessions().contains(&session),
        "the session survives reopening"
    );
    let replay = reopened.prepare(&intent);
    assert_eq!(
        replay["result"]["value"], first["result"]["value"],
        "replay after reopening"
    );
    assert_eq!(
        reopened.sessions().len(),
        f.sessions().len(),
        "replay creates nothing"
    );
    validate_v8(reopened.conn()).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn close_is_refused_on_stale_unknown_or_mismatched_identity_and_changes_nothing() {
    let mut f = Facade::fixture();
    let session = f.sessions()[1].clone();
    let before = f.durable();

    assert_eq!(code(&f.close(&uuid(), &uuid(), 1)), "session_not_found");
    assert_eq!(code(&f.close(&uuid(), "not-a-uuid", 1)), "invalid_request");
    assert_eq!(code(&f.close(&uuid(), &session, 2)), "stale_revision");
    assert_eq!(code(&f.close(&uuid(), &session, 0)), "invalid_request");
    let wrong_generation = f.call(
        "managedSession/close",
        json!({"intent_id":uuid(),"managed_session_id":session,"expected_revision":1,
            "runtime_fence":{"managed_session_id":session,"generation":1,"runtime_instance_id":null}}),
    );
    assert_eq!(
        code(&wrong_generation),
        "stale_revision",
        "{wrong_generation}"
    );
    let foreign_instance = f.call(
        "managedSession/close",
        json!({"intent_id":uuid(),"managed_session_id":session,"expected_revision":1,
            "runtime_fence":{"managed_session_id":session,"generation":1,
                "runtime_instance_id":uuid()}}),
    );
    assert_eq!(
        code(&foreign_instance),
        "stale_revision",
        "{foreign_instance}"
    );
    assert_eq!(f.durable(), before, "refused closes write nothing");
    assert_eq!(f.launches.get(), 0);
}

#[test]
fn close_revokes_the_lease_in_the_same_commit_and_its_audit_order_is_exact() {
    let mut f = Facade::fixture();
    let session = f.sessions()[1].clone();
    f.grant_active_lease(&session);
    let intent = uuid();
    let closed = f.close(&intent, &session, 1);
    assert_eq!(closed["result"]["kind"], "session", "{closed}");
    assert_eq!(closed["result"]["value"]["control_state"], "closed");
    assert_eq!(closed["result"]["value"]["revision"], 2);

    let active = f.count(&format!(
        "SELECT count(*) FROM execution_leases WHERE session_id='{session}' AND state='active'"
    ));
    assert_eq!(active, 0, "no lease survives the closure");
    let revoked: String = f.scalar(&format!(
        "SELECT revocation_reason FROM execution_leases WHERE session_id='{session}' AND state='revoked' ORDER BY revision DESC LIMIT 1"
    ));
    assert_eq!(revoked, "session_closed");
    assert_eq!(
        f.audit_kinds(&session),
        vec![
            "authority.managed_session.prepared".to_owned(),
            "authority.execution_lease.granted".to_owned(),
            "authority.execution_lease.revoked".to_owned(),
            "authority.managed_session.closed".to_owned(),
        ],
        "the revocation is recorded before the closure, in one commit"
    );

    let durable = f.durable();
    let replay = f.close(&intent, &session, 1);
    assert_eq!(
        replay["result"]["value"], closed["result"]["value"],
        "exact replay"
    );
    assert_eq!(f.durable(), durable, "a replay has no effect");
    assert_eq!(
        code(&f.close(&uuid(), &session, 1)),
        "stale_revision",
        "closing twice is a stale close"
    );
    assert_eq!(f.durable(), durable);
    assert_eq!(code(&f.close(&intent, &session, 2)), "intent_conflict");
    assert_eq!(f.durable(), durable, "a conflicting replay has no effect");
}

#[test]
fn a_closed_session_is_never_reopened_by_prepare_start_turn_or_lease() {
    let mut f = Facade::fixture();
    let session = f.sessions()[1].clone();
    assert_eq!(f.close(&uuid(), &session, 1)["result"]["kind"], "session");

    let fresh = f.prepare(&uuid());
    let fresh_id = fresh["result"]["value"]["managed_session_id"]
        .as_str()
        .unwrap();
    assert_ne!(
        fresh_id, session,
        "prepare creates a new session, never reopens a closed one"
    );

    let start = f.call(
        "runtime/start",
        json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex",
            "managed_session_id":session,"expected_generation":0,"expected_revision":0}),
    );
    assert_ne!(
        code(&start),
        "<ok>",
        "a closed session must not start: {start}"
    );
    let turn = f.call(
        "managedSession/turn/start",
        json!({"intent_id":uuid(),"managed_session_id":session,"expected_session_revision":2,
            "prompt":"x","runtime_fence":{"managed_session_id":session,"generation":0,"runtime_instance_id":uuid()}}),
    );
    assert_ne!(code(&turn), "<ok>", "{turn}");
    let lease = f.call(
        "executionLease/grant",
        json!({"intent_id":uuid(),"session_id":session,"expected_session_revision":2,
            "scopes":["turn_start"],"ttl_seconds":60,
            "runtime_fence":{"managed_session_id":session,"generation":0,"runtime_instance_id":uuid()}}),
    );
    assert_ne!(code(&lease), "<ok>", "{lease}");
    assert_eq!(
        f.launches.get(),
        0,
        "no launch was attempted for a closed session"
    );
}

#[test]
fn a_closed_session_survives_reopening_and_its_replay_is_unchanged() {
    let mut f = Facade::fixture();
    let session = f.sessions()[1].clone();
    f.grant_active_lease(&session);
    let intent = uuid();
    let closed = f.close(&intent, &session, 1);
    let (mut reopened, path) = f.reopen("close");

    let replay = reopened.close(&intent, &session, 1);
    assert_eq!(
        replay["result"]["value"], closed["result"]["value"],
        "replay after reopening"
    );
    assert_eq!(
        reopened.durable(),
        f.durable(),
        "reopening and replaying changes nothing"
    );
    assert_eq!(
        reopened.audit_kinds(&session).last().map(String::as_str),
        Some("authority.managed_session.closed")
    );
    validate_v8(reopened.conn()).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn an_injected_fault_rolls_back_the_whole_closure_including_its_lease() {
    let mut f = Facade::fixture();
    let session = f.sessions()[1].clone();
    f.grant_active_lease(&session);
    let before = f.durable();
    let intent = uuid();

    // The closure's own audit record fails inside the transaction, after the lease was revoked.
    f.conn()
        .execute_batch(
            "CREATE TRIGGER lifecycle_fault BEFORE INSERT ON control_events \
             WHEN NEW.envelope LIKE '%authority.managed_session.closed%' \
             BEGIN SELECT RAISE(ABORT,'injected fault'); END;",
        )
        .unwrap();
    let failed = f.close(&intent, &session, 1);
    assert_ne!(code(&failed), "<ok>", "the fault must surface: {failed}");
    assert_eq!(
        f.durable(),
        before,
        "no partial closure, no revoked lease, no receipt"
    );

    f.conn()
        .execute_batch("DROP TRIGGER lifecycle_fault;")
        .unwrap();
    let retried = f.close(&intent, &session, 1);
    assert_eq!(
        retried["result"]["value"]["control_state"], "closed",
        "{retried}"
    );
    validate_v8(f.conn()).unwrap();
}

#[test]
fn an_injected_lease_write_fault_leaves_the_session_open() {
    let mut f = Facade::fixture();
    let session = f.sessions()[1].clone();
    f.grant_active_lease(&session);
    let before = f.durable();
    f.conn()
        .execute_batch(
            "CREATE TRIGGER lease_fault BEFORE UPDATE ON execution_leases \
             BEGIN SELECT RAISE(ABORT,'injected fault'); END;",
        )
        .unwrap();
    let failed = f.close(&uuid(), &session, 1);
    assert_ne!(code(&failed), "<ok>", "{failed}");
    assert_eq!(
        f.durable(),
        before,
        "the session stays prepared and its lease stays active"
    );
    f.conn().execute_batch("DROP TRIGGER lease_fault;").unwrap();
    validate_v8(f.conn()).unwrap();
}

#[test]
fn an_injected_receipt_fault_leaves_no_session_and_no_receipt_for_prepare() {
    let mut f = Facade::fixture();
    let before = f.durable();
    let sessions_before = f.sessions();
    let intent = uuid();
    f.conn()
        .execute_batch(
            "CREATE TRIGGER receipt_fault BEFORE INSERT ON processed_intents \
             BEGIN SELECT RAISE(ABORT,'injected fault'); END;",
        )
        .unwrap();
    let failed = f.prepare(&intent);
    assert_ne!(code(&failed), "<ok>", "{failed}");
    assert_eq!(f.sessions(), sessions_before, "no orphaned session");
    assert_eq!(f.durable(), before, "no orphaned receipt, no audit record");
    f.conn()
        .execute_batch("DROP TRIGGER receipt_fault;")
        .unwrap();

    let retried = f.prepare(&intent);
    assert_eq!(retried["result"]["kind"], "session", "{retried}");
    assert_eq!(
        f.sessions().len(),
        sessions_before.len() + 1,
        "the retry commits exactly once"
    );
    validate_v8(f.conn()).unwrap();
}

#[test]
fn prepare_replay_after_a_timeout_style_retry_creates_exactly_one_session() {
    let mut f = Facade::fixture();
    let intent = uuid();
    let first = f.prepare(&intent);
    let durable = f.durable();
    for _ in 0..3 {
        assert_eq!(
            f.prepare(&intent)["result"]["value"],
            first["result"]["value"]
        );
    }
    assert_eq!(
        f.durable(),
        durable,
        "retries never create a second session"
    );
}
