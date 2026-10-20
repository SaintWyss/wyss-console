//! Protocol-2 read-only requests through the real `Authority::handle` facade on an in-memory V8
//! fixture. Every test also proves that the read changed no row: SQLite's own change counter must
//! not move. Ordinary in-memory SQLite only; no provider, workspace, socket or installed database.
use super::{
    authority::{Authority, Client},
    engine::{Engine, Spawned, Spawner},
    migration,
    protocol::RuntimeRef,
    registry::Registry,
    staging::LaunchStaging,
    writers::Writer,
};
use crate::{
    contracts::{
        WireUuid,
        control::{ControlResult, ExecutionScope, LeaseGrant, Operation},
        identities::ManagedSessionId,
    },
    control::f8_fixture,
};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::time::Instant;

const AUTH: &str = "00000000-0000-4000-8000-0000000000a1";

fn uuid() -> String {
    crate::control::store::uuid().as_str().to_owned()
}

/// Reads never launch a runtime, so any spawn is a test failure rather than a silent launch.
struct NoSpawn;
impl Spawner for NoSpawn {
    fn spawn(&mut self, _: &RuntimeRef, _: Option<&mut LaunchStaging>) -> ControlResult<Spawned> {
        panic!("a read-only request must never launch a runtime")
    }
}

struct Facade {
    authority: Authority,
    client: Client,
}
impl Facade {
    /// The V8 image of `v7_with_sessions(1)`: a completed legacy turn whose lease is revoked, plus
    /// one extra prepared session. The authority is then greeted with `authority/hello`.
    fn new() -> Self {
        Self::over(f8_fixture::v7_with_sessions(1))
    }
    /// Migrates `source` to V8 with a client that has not yet said hello.
    fn cold(mut source: Connection) -> Self {
        let conn = migration::prepare(&mut source).unwrap().into_connection();
        let auth = WireUuid::try_from(AUTH.to_owned()).unwrap();
        let engine = Engine::new(conn, auth.clone(), Registry::new(auth).unwrap(), None).unwrap();
        Self {
            authority: Authority::new(engine),
            client: Client::default(),
        }
    }
    /// Migrates `source` to V8 and greets the authority with `authority/hello`.
    fn over(source: Connection) -> Self {
        let mut f = Self::cold(source);
        let hello = f.raw(json!({"schema_version":2,"request_id":uuid(),
            "method":"authority/hello","params":{"protocol_version":2,"client_instance_id":uuid()}}));
        assert_eq!(hello["result"]["kind"], "hello", "{hello}");
        f
    }
    /// The V8 image of `v7_with_sessions(2)` whose last two prepared sessions each hold one active
    /// lease of `ttl_seconds`. Migration refuses an active lease, so the grants follow it, through
    /// the same V8 writer the authority uses. The fence check lives in the authority route, which
    /// this fixture does not exercise, and no runtime is launched.
    fn with_active_leases(ttl_seconds: u32) -> Self {
        let mut f = Self::over(f8_fixture::v7_with_sessions(2));
        let auth = WireUuid::try_from(AUTH.to_owned()).unwrap();
        for session in f.sessions_in_order().into_iter().skip(1) {
            let grant = Operation::LeaseGrant(LeaseGrant {
                intent_id: WireUuid::try_from(uuid()).unwrap(),
                session_id: ManagedSessionId::parse(&session).unwrap(),
                expected_session_revision: 1,
                scopes: vec![
                    ExecutionScope::SessionCreate,
                    ExecutionScope::SessionResume,
                    ExecutionScope::TurnStart,
                ],
                ttl_seconds,
            });
            Writer::new(f.authority.engine_mut().connection_mut(), auth.clone())
                .unwrap()
                .lease(&grant)
                .unwrap();
        }
        f
    }
    fn head(&self) -> String {
        self.conn()
            .query_row(
                "SELECT last_sequence FROM authority_metadata WHERE id=1",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }
    fn leases(&self) -> Vec<String> {
        self.column(
            "SELECT lease_id||'|'||session_id||'|'||state||'|'||revision||'|'||expires_at||'|'||updated_at FROM execution_leases ORDER BY lease_id",
        )
    }
    fn raw(&mut self, v: Value) -> Value {
        let bytes = serde_json::to_vec(&v).unwrap();
        let out = self
            .authority
            .handle(&mut self.client, &bytes, &mut NoSpawn, Instant::now());
        serde_json::from_slice(&out).unwrap()
    }
    fn call(&mut self, method: &str, params: Value) -> Value {
        self.raw(
            json!({"schema_version":2,"request_id":uuid(),"authority_instance_id":AUTH,
            "method":method,"params":params}),
        )
    }
    fn conn(&self) -> &Connection {
        self.authority.engine().connection()
    }
    fn changes(&self) -> u64 {
        self.conn().total_changes()
    }
    fn column(&self, sql: &str) -> Vec<String> {
        let mut q = self.conn().prepare(sql).unwrap();
        q.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }
    fn sessions_in_order(&self) -> Vec<String> {
        self.column("SELECT session_id FROM managed_sessions ORDER BY created_sequence")
    }
    fn prepare(&mut self) -> String {
        let r = self.call(
            "managedSession/prepare",
            json!({"intent_id":uuid(),"adapter_id":"codex","runtime_kind":"codex"}),
        );
        r["result"]["value"]["managed_session_id"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

fn code(v: &Value) -> &str {
    v["error"]["code"].as_str().unwrap_or("<ok>")
}

#[test]
fn get_returns_the_record_and_refuses_unknown_or_malformed_ids_without_mutation() {
    let mut f = Facade::new();
    let known = f.sessions_in_order()[0].clone();
    let before = f.changes();

    let ok = f.call("managedSession/get", json!({"managed_session_id":known}));
    assert_eq!(ok["result"]["kind"], "session", "{ok}");
    assert_eq!(ok["result"]["value"]["managed_session_id"], known);
    assert_eq!(ok["result"]["value"]["control_state"], "prepared");
    assert_eq!(ok["result"]["value"]["revision"], 1);

    let missing = f.call("managedSession/get", json!({"managed_session_id":uuid()}));
    assert_eq!(code(&missing), "session_not_found", "{missing}");
    let malformed = f.call(
        "managedSession/get",
        json!({"managed_session_id":"not-a-uuid"}),
    );
    assert_eq!(code(&malformed), "invalid_request", "{malformed}");
    let extra = f.call(
        "managedSession/get",
        json!({"managed_session_id":known,"expected_revision":1}),
    );
    assert_eq!(code(&extra), "invalid_request", "{extra}");

    assert_eq!(f.changes(), before, "managedSession/get must not write");
}

#[test]
fn list_pages_cover_each_session_exactly_once_in_creation_order() {
    let mut f = Facade::new();
    for _ in 0..4 {
        f.prepare();
    }
    let expected = f.sessions_in_order();
    // The fixture holds its base session plus one extra, and four were prepared above.
    assert_eq!(expected.len(), 6);
    let before = f.changes();

    for limit in [1u16, 2, 3, 5, 50] {
        let mut seen = Vec::new();
        let mut after = Value::Null;
        for _ in 0..=expected.len() {
            let page = f.call("managedSession/list", json!({"after":after,"limit":limit}));
            assert_eq!(page["result"]["kind"], "sessions", "limit {limit}: {page}");
            let items = page["result"]["value"]["sessions"].as_array().unwrap();
            assert!(items.len() <= usize::from(limit), "limit {limit}: {page}");
            seen.extend(
                items
                    .iter()
                    .map(|s| s["managed_session_id"].as_str().unwrap().to_owned()),
            );
            let next = page["result"]["value"]["next"].clone();
            if next.is_null() {
                break;
            }
            // A continuation names the last record returned, so the next page starts after it.
            assert_eq!(
                next,
                items.last().unwrap()["created_sequence"],
                "limit {limit}"
            );
            after = next;
        }
        assert_eq!(
            seen, expected,
            "limit {limit} skipped or duplicated a session"
        );
    }

    let past_end = f.call(
        "managedSession/list",
        json!({"after":"9999999999","limit":3}),
    );
    assert_eq!(past_end["result"]["kind"], "sessions", "{past_end}");
    assert_eq!(past_end["result"]["value"]["sessions"], json!([]));
    assert!(past_end["result"]["value"]["next"].is_null());

    for limit in [0u16, 51] {
        let bad = f.call("managedSession/list", json!({"after":null,"limit":limit}));
        assert_eq!(code(&bad), "invalid_request", "limit {limit}: {bad}");
    }
    for after in ["0", "01", "abc"] {
        let bad = f.call("managedSession/list", json!({"after":after,"limit":2}));
        assert_eq!(code(&bad), "invalid_request", "cursor {after}: {bad}");
    }
    assert_eq!(f.changes(), before, "managedSession/list must not write");
}

#[test]
fn turn_status_and_events_read_completed_history_after_its_runtime_is_gone() {
    let mut f = Facade::new();
    let turn = f
        .column("SELECT managed_turn_id FROM managed_turns WHERE state='completed' LIMIT 1")[0]
        .clone();
    let before = f.changes();

    let status = f.call(
        "managedSession/turn/status",
        json!({"managed_turn_id":turn}),
    );
    assert_eq!(status["result"]["kind"], "turn_status", "{status}");
    let value = &status["result"]["value"];
    assert_eq!(value["turn"]["managed_turn_id"], turn);
    assert_eq!(value["turn"]["state"], "completed");
    assert!(
        value["runtime_ref"].is_null(),
        "no live runtime may be invented: {status}"
    );
    assert!(value["attestation"].is_null(), "{status}");
    assert_eq!(value["stream_retained"], false);

    // With no retained stream the events read is one explicit gap marker, by contract. The
    // marker's sequence is `after + 1` and the cursor names it. `retained: false` is the signal
    // that tells a client to stop following that cursor.
    let page = f.call(
        "managedSession/turn/events",
        json!({"managed_turn_id":turn,"after_sequence":null,"limit":1}),
    );
    assert_eq!(page["result"]["kind"], "turn_events", "{page}");
    let v = &page["result"]["value"];
    assert_eq!(v["retained"], false, "{page}");
    assert_eq!(v["gap"], true, "{page}");
    assert_eq!(v["terminal"], true, "{page}");
    assert_eq!(v["events"].as_array().unwrap().len(), 1, "{page}");
    assert_eq!(v["events"][0]["kind"], "stream_gap", "{page}");
    assert_eq!(v["events"][0]["sequence"], "1", "{page}");
    assert_eq!(v["next_sequence"], "1", "{page}");

    let unknown = f.call(
        "managedSession/turn/events",
        json!({"managed_turn_id":uuid(),"after_sequence":null,"limit":1}),
    );
    assert_eq!(code(&unknown), "turn_not_found", "{unknown}");
    for limit in [0, 51] {
        let bad = f.call(
            "managedSession/turn/events",
            json!({"managed_turn_id":turn,"after_sequence":null,"limit":limit}),
        );
        assert_eq!(code(&bad), "invalid_request", "limit {limit}: {bad}");
    }
    let unknown_status = f.call(
        "managedSession/turn/status",
        json!({"managed_turn_id":uuid()}),
    );
    assert_eq!(code(&unknown_status), "turn_not_found", "{unknown_status}");
    assert_eq!(f.changes(), before, "turn reads must not write");
}

#[test]
fn execution_gate_status_is_observational_and_never_grants_or_renews() {
    let mut f = Facade::new();
    let session = f.sessions_in_order()[0].clone();
    // The fixture's only lease was revoked by its completed turn, so the session has no active
    // lease. A gate query must say so and must not revive, renew or grant anything.
    let leases = "SELECT lease_id||'|'||state||'|'||revision||'|'||expires_at FROM execution_leases ORDER BY lease_id";
    let lease_before = f.column(leases);
    assert_eq!(lease_before.len(), 1, "fixture carries exactly one lease");
    let before = f.changes();

    let missing = f.call(
        "executionGate/status",
        json!({"session_id":session,"action":"turn_start"}),
    );
    assert_eq!(missing["result"]["kind"], "gate_status", "{missing}");
    assert_eq!(
        missing["result"]["value"]["reason"], "lease_missing",
        "{missing}"
    );
    assert_eq!(missing["result"]["value"]["action"], "turn_start");

    let command = f.call(
        "executionGate/status",
        json!({"session_id":session,"action":"command_execute"}),
    );
    assert_eq!(
        command["result"]["value"]["reason"], "lease_missing",
        "{command}"
    );

    let unknown = f.call(
        "executionGate/status",
        json!({"session_id":uuid(),"action":"turn_start"}),
    );
    assert_eq!(
        unknown["result"]["value"]["reason"], "session_not_found",
        "{unknown}"
    );

    let malformed = f.call(
        "executionGate/status",
        json!({"session_id":session,"action":"grant_everything"}),
    );
    assert_eq!(code(&malformed), "invalid_request", "{malformed}");
    let malformed_session = f.call(
        "executionGate/status",
        json!({"session_id":"not-a-uuid","action":"turn_start"}),
    );
    assert_eq!(
        code(&malformed_session),
        "invalid_request",
        "{malformed_session}"
    );

    assert_eq!(
        f.column(leases),
        lease_before,
        "the gate must not renew, revive or grant"
    );
    assert_eq!(f.changes(), before, "executionGate/status must not write");
}

#[test]
fn authority_status_matches_storage_and_refuses_bad_correlation_before_and_after_hello() {
    let mut f = Facade::new();
    let (head, leases_before, before) = (f.head(), f.leases(), f.changes());

    let status = f.call("authority/status", json!({}));
    assert_eq!(status["result"]["kind"], "status", "{status}");
    let v = &status["result"]["value"];
    assert_eq!(v["protocol_version"], 2);
    assert_eq!(v["authority_instance_id"], AUTH);
    assert_eq!(v["storage_schema_version"], 8);
    assert_eq!(
        v["stream_id"],
        f.column("SELECT stream_id FROM authority_metadata WHERE id=1")[0]
    );
    assert_ne!(head, "0", "fixture must have committed events");
    assert_eq!(v["last_sequence"], head.as_str(), "{status}");
    let count = |state: &str| -> i64 {
        f.conn()
            .query_row(
                "SELECT count(*) FROM managed_sessions WHERE control_state=?1",
                [state],
                |r| r.get(0),
            )
            .unwrap()
    };
    assert_eq!(v["prepared_count"], count("prepared"), "{status}");
    assert_eq!(v["closed_count"], count("closed"), "{status}");
    assert_eq!(v["prepared_count"], 2);
    assert_eq!(v["closed_count"], 0);

    let wrong = f.raw(json!({"schema_version":2,"request_id":uuid(),
        "authority_instance_id":"00000000-0000-4000-8000-0000000000ff",
        "method":"authority/status","params":{}}));
    assert_eq!(code(&wrong), "correlation_error", "{wrong}");

    // Before hello no operation is answered.
    let mut cold = Facade::cold(f8_fixture::v7_with_sessions(0));
    let refused = cold.raw(json!({"schema_version":2,"request_id":uuid(),
        "authority_instance_id":AUTH,"method":"authority/status","params":{}}));
    assert_eq!(code(&refused), "handshake_required", "{refused}");

    assert_eq!(
        f.head(),
        head,
        "authority/status must not advance the audit head"
    );
    assert_eq!(f.leases(), leases_before);
    assert_eq!(f.changes(), before, "authority/status must not write");
}

#[test]
fn runtime_status_of_a_prepared_session_reports_no_runtime_and_refuses_unknown_targets() {
    let mut f = Facade::new();
    let session = f.prepare();
    let (head, before) = (f.head(), f.changes());
    let target =
        |s: &str| json!({"adapter_id":"codex","runtime_kind":"codex","managed_session_id":s});

    let status = f.call("runtime/status", target(&session));
    assert_eq!(status["result"]["kind"], "runtime_status", "{status}");
    let v = &status["result"]["value"];
    assert_eq!(v["managed_session_id"], session);
    assert_eq!(v["desired_state"], "stopped");
    assert_eq!(v["actual_state"], "stopped");
    assert_eq!(v["generation"], 0);
    // No managed_runtimes row exists. runtime/start compares expected_revision against the stored
    // revision, which is 0 for an absent row, so status must report that same 0.
    assert_eq!(v["revision"], 0, "{status}");
    assert!(v["runtime_instance_id"].is_null(), "{status}");
    assert!(v["runtime_identity"].is_null(), "{status}");
    assert_eq!(v["compatibility"], "unknown");

    let unknown = f.call("runtime/status", target(&uuid()));
    assert_eq!(code(&unknown), "session_not_found", "{unknown}");
    let malformed = f.call("runtime/status", target("not-a-uuid"));
    assert_eq!(code(&malformed), "invalid_request", "{malformed}");
    let absent = f.call(
        "runtime/status",
        json!({"adapter_id":"codex","runtime_kind":"codex"}),
    );
    assert_eq!(code(&absent), "invalid_request", "{absent}");

    assert_eq!(
        f.head(),
        head,
        "runtime/status must not advance the audit head"
    );
    assert_eq!(f.changes(), before, "runtime/status must not write");
}

#[test]
fn lease_get_returns_stored_records_including_historical_revoked_ones() {
    let mut f = Facade::new();
    let rows = f.column("SELECT lease_id FROM execution_leases ORDER BY lease_id");
    assert_eq!(rows.len(), 1, "fixture carries exactly one revoked lease");
    let (leases, before) = (f.leases(), f.changes());

    let got = f.call("executionLease/get", json!({"lease_id":rows[0]}));
    assert_eq!(got["result"]["kind"], "lease", "{got}");
    let v = &got["result"]["value"];
    let row = f.conn().query_row(
        "SELECT session_id,state,revision,issued_at,expires_at,updated_at,revocation_reason,scopes FROM execution_leases WHERE lease_id=?1",
        [&rows[0]],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, String>(7)?,
            ))
        },
    ).unwrap();
    assert_eq!(v["lease_id"], rows[0]);
    assert_eq!(v["session_id"], row.0);
    assert_eq!(
        v["state"], row.1,
        "historical revoked state must stay readable: {got}"
    );
    assert_eq!(v["revision"], row.2);
    assert_eq!(v["issued_at"], row.3.as_str());
    assert_eq!(v["expires_at"], row.4.as_str());
    assert_eq!(v["updated_at"], row.5.as_str());
    assert_eq!(
        v["revocation_reason"],
        row.6.map_or(Value::Null, Value::from)
    );
    let scopes: Value = serde_json::from_str(&row.7).unwrap();
    assert_eq!(v["scopes"], scopes);

    let unknown = f.call("executionLease/get", json!({"lease_id":uuid()}));
    assert_eq!(code(&unknown), "lease_not_found", "{unknown}");
    let malformed = f.call("executionLease/get", json!({"lease_id":"not-a-uuid"}));
    assert_eq!(code(&malformed), "invalid_request", "{malformed}");

    assert_eq!(
        f.leases(),
        leases,
        "executionLease/get must not touch lease rows"
    );
    assert_eq!(f.changes(), before, "executionLease/get must not write");
}

#[test]
fn lease_active_isolates_sessions_and_never_reports_a_time_expired_lease_as_usable() {
    // One second is the shortest grant the contract allows; the read must see it expire by time.
    let mut f = Facade::with_active_leases(1);
    let sessions = f.sessions_in_order();
    assert_eq!(sessions.len(), 3);
    let lease_of = |f: &Facade, s: &str| {
        f.column(&format!(
            "SELECT lease_id FROM execution_leases WHERE session_id='{s}' AND state='active'"
        ))
    };
    // sessions[0] carries the fixture's revoked lease; sessions[1] and [2] hold active ones.
    let (legacy, a, b) = (
        sessions[0].clone(),
        sessions[1].clone(),
        sessions[2].clone(),
    );
    assert!(lease_of(&f, &legacy).is_empty());
    let (lease_a, lease_b) = (lease_of(&f, &a)[0].clone(), lease_of(&f, &b)[0].clone());
    assert_ne!(lease_a, lease_b);
    let bare = f.prepare();
    let (before, rows_before) = (f.changes(), f.leases());

    let active_a = f.call("executionLease/active", json!({"session_id":a}));
    assert_eq!(active_a["result"]["kind"], "active_lease", "{active_a}");
    assert_eq!(
        active_a["result"]["value"]["lease_id"], lease_a,
        "{active_a}"
    );
    assert_eq!(active_a["result"]["value"]["session_id"], a, "{active_a}");
    assert_eq!(active_a["result"]["value"]["state"], "active");
    let active_b = f.call("executionLease/active", json!({"session_id":b}));
    assert_eq!(
        active_b["result"]["value"]["lease_id"], lease_b,
        "{active_b}"
    );
    assert_ne!(
        active_a["result"]["value"]["lease_id"], lease_b,
        "session A exposed B's lease"
    );

    let none = f.call("executionLease/active", json!({"session_id":bare}));
    assert!(
        none["result"]["value"].is_null(),
        "a session without a lease reads none: {none}"
    );
    let revoked_only = f.call("executionLease/active", json!({"session_id":legacy}));
    assert!(
        revoked_only["result"]["value"].is_null(),
        "a revoked lease is not active: {revoked_only}"
    );
    // Protocol 1 answers no lease for an unknown session rather than refusing; protocol 2 matches it.
    let unknown = f.call("executionLease/active", json!({"session_id":uuid()}));
    assert_eq!(unknown["result"]["kind"], "active_lease", "{unknown}");
    assert!(unknown["result"]["value"].is_null(), "{unknown}");
    let malformed = f.call("executionLease/active", json!({"session_id":"not-a-uuid"}));
    assert_eq!(code(&malformed), "invalid_request", "{malformed}");

    // Real time passes; the stored row stays 'active' because nothing wrote it.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let expired = f.call("executionLease/active", json!({"session_id":a}));
    assert!(
        expired["result"]["value"].is_null(),
        "a time-expired lease is not usable: {expired}"
    );
    let get = f.call("executionLease/get", json!({"lease_id":lease_a}));
    assert_eq!(get["result"]["value"]["state"], "expired", "{get}");
    assert_eq!(
        get["result"]["value"]["revision"], 1,
        "stored revision is not fabricated: {get}"
    );
    assert_eq!(
        lease_of(&f, &a),
        vec![lease_a.clone()],
        "the stored row is still active; expiry is a projection"
    );

    assert_eq!(
        f.leases(),
        rows_before,
        "lease reads must not renew, revoke or grant"
    );
    assert_eq!(f.changes(), before, "lease reads must not write");
}
