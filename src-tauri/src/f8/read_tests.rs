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
};
use crate::{
    contracts::{WireUuid, control::ControlResult},
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
    /// One prepared session with an active lease and one completed legacy turn, then `hello`.
    fn new() -> Self {
        let mut source = f8_fixture::v7_with_sessions(1);
        let conn = migration::prepare(&mut source).unwrap().into_connection();
        let auth = WireUuid::try_from(AUTH.to_owned()).unwrap();
        let engine = Engine::new(conn, auth.clone(), Registry::new(auth).unwrap(), None).unwrap();
        let mut f = Self {
            authority: Authority::new(engine),
            client: Client::default(),
        };
        let hello = f.raw(json!({"schema_version":2,"request_id":uuid(),
            "method":"authority/hello","params":{"protocol_version":2,"client_instance_id":uuid()}}));
        assert_eq!(hello["result"]["kind"], "hello", "{hello}");
        f
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
