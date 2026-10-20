//! Executable candidate contracts on ordinary in-memory SQL fixtures and routing oracles.
//! No environment replacement, provider, socket, filesystem database or production migration.
use rusqlite::{Connection, params};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use wyss_console::contracts;

#[allow(dead_code)]
#[path = "../src/control/schema.rs"]
mod baseline;

const SQL: &str = include_str!("../../docs/contracts/f8/v7-to-v8.sql");
const RESOURCES: &str = include_str!("../../docs/contracts/f8/resources.json");

fn statements() -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    for line in SQL.lines().filter(|l| !l.trim_start().starts_with("--")) {
        current.push_str(line);
        current.push('\n');
        if line.trim_end().ends_with(';') {
            result.push(std::mem::take(&mut current));
        }
    }
    assert!(current.trim().is_empty());
    result
}

fn v7() -> Connection {
    let mut c = Connection::open_in_memory().unwrap();
    c.pragma_update(None, "foreign_keys", true).unwrap();
    baseline::migrate(&mut c).unwrap();
    c.execute_batch("INSERT INTO managed_sessions VALUES('A','codex','codex','prepared',1,'t','t','1'); INSERT INTO managed_sessions VALUES('B','codex','codex','prepared',1,'t','t','2'); INSERT INTO managed_runtimes VALUES('codex','codex','running',9,'t','t'); INSERT INTO processed_intents VALUES('old-intent','{ \"old\": true }','{ \"result\": 1 }'); INSERT INTO control_events VALUES('old-event',(SELECT stream_id FROM authority_metadata),'00000000000000000001','A','legacy envelope bytes'); UPDATE authority_metadata SET last_sequence='1';").unwrap();
    c
}

fn snapshot(c: &Connection) -> String {
    let version: i64 = c
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    let mut q = c.prepare("SELECT name,sql FROM sqlite_master WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY name").unwrap();
    let objects: Vec<(String, String)> = q
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut out = format!("{version}:{objects:?}");
    for (name, sql) in objects {
        if sql.starts_with("CREATE TABLE") {
            let mut q = c
                .prepare(&format!("SELECT * FROM \"{name}\" ORDER BY rowid"))
                .unwrap();
            let cols = q.column_count();
            let values: Vec<Vec<String>> = q
                .query_map([], |r| {
                    (0..cols)
                        .map(|i| r.get_ref(i).map(|v| format!("{v:?}")))
                        .collect()
                })
                .unwrap()
                .map(Result::unwrap)
                .collect();
            out.push_str(&format!("{name}:{values:?}"));
        }
    }
    out
}

fn migrate(c: &Connection) {
    c.execute_batch("BEGIN IMMEDIATE").unwrap();
    for sql in statements() {
        c.execute_batch(&sql).unwrap();
    }
    let violations: i64 = c
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);
    c.execute_batch("COMMIT").unwrap();
}

fn launch(c: &Connection, session: &str, instance: &str) {
    c.execute(
        "INSERT INTO managed_runtimes VALUES(?1,'codex','codex','running',1,1,'t','t')",
        [session],
    )
    .unwrap();
    c.execute(
        "INSERT INTO managed_runtime_launches VALUES(?1,'codex','codex',1,?2,'authority','t')",
        params![session, instance],
    )
    .unwrap();
    c.execute(
        "INSERT INTO execution_leases VALUES(?1,?2,'active','[]',1,'t','z','t',NULL)",
        params![format!("lease-{session}"), session],
    )
    .unwrap();
}

fn turn(
    c: &Connection,
    session: &str,
    id: &str,
    instance: &str,
    generation: i64,
) -> rusqlite::Result<()> {
    c.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        c.execute(
            "INSERT INTO managed_turn_launches VALUES(?1,?2,?3,?4)",
            params![id, session, instance, generation],
        )?;
        c.execute("INSERT INTO managed_turns VALUES(?1,?2,?3,NULL,?4,1,?5,'starting',1,?6,1,?1,'{}',0,'t','t',NULL,NULL)", params![id,session,format!("thread-{session}"),format!("lease-{session}"),instance,"0".repeat(64)])?;
        c.execute_batch("COMMIT")
    })();
    if result.is_err() {
        c.execute_batch("ROLLBACK")?;
    }
    result
}

fn approval(
    c: &Connection,
    session: &str,
    turn: &str,
    id: &str,
    instance: &str,
) -> rusqlite::Result<()> {
    c.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        c.execute(
            "INSERT INTO managed_approval_launches VALUES(?1,?2,?3,?4,1)",
            params![id, turn, session, instance],
        )?;
        let sequence: i64 =
            c.query_row("SELECT count(*)+1 FROM managed_approvals", [], |r| r.get(0))?;
        c.execute("INSERT INTO managed_approvals VALUES(?1,?2,?3,?4,'command_execution',?1,?5,1,'pending',1,?6,'{}',NULL,'none','t','t','z',NULL,NULL,?7)", params![id,turn,session,instance,format!("lease-{session}"),"0".repeat(64),format!("{sequence:020}")])?;
        c.execute_batch("COMMIT")
    })();
    if result.is_err() {
        c.execute_batch("ROLLBACK")?;
    }
    result
}

#[test]
fn every_sql_boundary_rolls_back_schema_data_and_version() {
    let steps = statements();
    for fail_after in 0..=steps.len() {
        let c = v7();
        let before = snapshot(&c);
        c.execute_batch("BEGIN IMMEDIATE").unwrap();
        for sql in steps.iter().take(fail_after) {
            c.execute_batch(sql).unwrap();
        }
        c.execute_batch("ROLLBACK").unwrap();
        assert_eq!(snapshot(&c), before, "after statement {fail_after}");
    }
}

#[test]
fn migration_preserves_sql_history_without_assigning_it_to_a_session() {
    let c = v7();
    let old: String = c
        .query_row("SELECT envelope FROM control_events", [], |r| r.get(0))
        .unwrap();
    migrate(&c);
    assert_eq!(
        c.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        8
    );
    assert_eq!(
        c.query_row::<String, _, _>("SELECT envelope FROM control_events", [], |r| r.get(0))
            .unwrap(),
        old
    );
    assert_eq!(
        c.query_row::<String, _, _>("SELECT mutation FROM processed_intents", [], |r| r.get(0))
            .unwrap(),
        "{ \"old\": true }"
    );
    assert_eq!(
        c.query_row::<i64, _, _>("SELECT revision FROM managed_runtimes_legacy_v7", [], |r| r
            .get(0))
            .unwrap(),
        9
    );
    assert_eq!(
        c.query_row::<i64, _, _>("SELECT count(*) FROM managed_runtimes", [], |r| r.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        c.query_row::<i64, _, _>("SELECT event_vocabulary FROM authority_metadata", [], |r| r
            .get(0))
            .unwrap(),
        2
    );
    for sql in [
        "UPDATE managed_runtimes_legacy_v7 SET revision=10",
        "DELETE FROM managed_runtimes_legacy_v7",
        "UPDATE control_events SET envelope='changed'",
        "DELETE FROM control_events",
    ] {
        assert!(c.execute_batch(sql).is_err(), "{sql}");
    }
}

#[test]
fn an_active_lease_refuses_migration_without_changing_v7() {
    let c = v7();
    c.execute_batch(
        "INSERT INTO execution_leases VALUES('lease','A','active','[]',1,'t','z','t',NULL)",
    )
    .unwrap();
    let before = snapshot(&c);
    c.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(c.execute_batch(SQL).is_err());
    c.execute_batch("ROLLBACK").unwrap();
    assert_eq!(snapshot(&c), before);
}

#[test]
fn turns_and_pending_approvals_are_per_session_and_cross_session_sidecars_refuse() {
    let c = v7();
    migrate(&c);
    launch(&c, "A", "instance-A");
    launch(&c, "B", "instance-B");
    turn(&c, "A", "turn-A", "instance-A", 1).unwrap();
    turn(&c, "B", "turn-B", "instance-B", 1).unwrap();
    assert!(turn(&c, "A", "turn-A2", "instance-A", 1).is_err());
    assert!(turn(&c, "A", "wrong-B", "instance-B", 1).is_err());
    approval(&c, "A", "turn-A", "approval-A", "instance-A").unwrap();
    approval(&c, "B", "turn-B", "approval-B", "instance-B").unwrap();
    assert!(approval(&c, "A", "turn-A", "approval-A2", "instance-A").is_err());
    assert!(approval(&c, "A", "turn-B", "crossed", "instance-A").is_err());
    assert_eq!(
        c.query_row::<i64, _, _>(
            "SELECT count(*) FROM managed_approvals WHERE state='pending'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        2
    );
    assert!(c.execute_batch("UPDATE managed_approvals SET runtime_instance_id='instance-B',revision=2 WHERE managed_approval_id='approval-A'").is_err());
}

#[test]
fn old_generation_new_work_dangling_sidecar_and_counter_reset_refuse() {
    let c = v7();
    migrate(&c);
    launch(&c, "A", "instance-A");
    c.execute_batch("UPDATE managed_runtimes SET generation=2,revision=2 WHERE managed_session_id='A'; INSERT INTO managed_runtime_launches VALUES('A','codex','codex',2,'instance-A2','authority','t');").unwrap();
    assert!(turn(&c, "A", "stale", "instance-A", 1).is_err());
    assert!(
        c.execute_batch(
            "UPDATE managed_runtimes SET generation=1,revision=3 WHERE managed_session_id='A'"
        )
        .is_err()
    );
    assert!(
        c.execute_batch("UPDATE managed_runtime_launches SET generation=3")
            .is_err()
    );
    c.execute_batch(
        "BEGIN IMMEDIATE; INSERT INTO managed_turn_launches VALUES('orphan','A','instance-A2',2)",
    )
    .unwrap();
    assert!(c.execute_batch("COMMIT").is_err());
    c.execute_batch("ROLLBACK").unwrap();
    assert_eq!(
        c.query_row::<i64, _, _>("SELECT count(*) FROM managed_turn_launches", [], |r| r
            .get(0))
            .unwrap(),
        0
    );
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Fence {
    managed_session_id: String,
    generation: u64,
    runtime_instance_id: Option<String>,
}

/// Reference oracle only: the future core must perform these checks again at dispatch.
fn route<'a>(
    authority: &str,
    expected_authority: &str,
    fence: &Fence,
    registry: &'a BTreeMap<String, Fence>,
) -> Option<&'a Fence> {
    if authority != expected_authority
        || fence.generation == 0
        || fence.generation > i64::MAX as u64
        || fence.runtime_instance_id.is_none()
    {
        return None;
    }
    registry
        .get(&fence.managed_session_id)
        .filter(|live| *live == fence)
}

#[test]
fn routing_oracle_refuses_cross_session_stale_instance_generation_and_authority() {
    let a = Fence {
        managed_session_id: "A".into(),
        generation: 1,
        runtime_instance_id: Some("instance-A".into()),
    };
    let b = Fence {
        managed_session_id: "B".into(),
        generation: 1,
        runtime_instance_id: Some("instance-B".into()),
    };
    let registry = BTreeMap::from([("A".into(), a.clone()), ("B".into(), b.clone())]);
    assert!(route("authority", "authority", &a, &registry).is_some());
    for bad in [
        Fence {
            managed_session_id: "B".into(),
            ..a.clone()
        },
        Fence {
            generation: 2,
            ..a.clone()
        },
        Fence {
            runtime_instance_id: b.runtime_instance_id.clone(),
            ..a.clone()
        },
        Fence {
            runtime_instance_id: None,
            ..a.clone()
        },
    ] {
        assert!(route("authority", "authority", &bad, &registry).is_none());
    }
    assert!(route("old", "authority", &a, &registry).is_none());
    assert!(serde_json::from_value::<Fence>(json!({"managed_session_id":"A","generation":1,"runtime_instance_id":"instance-A","pid":1})).is_err());
}

#[test]
fn resource_contract_is_finite_and_stopping_keeps_its_slot_until_reaped() {
    let r: Value = serde_json::from_str(RESOURCES).unwrap();
    assert_eq!(r["max_active_runtimes"], 1);
    for (name, value) in r.as_object().unwrap() {
        if name != "status" {
            assert!(value.as_u64().is_some_and(|n| n > 0), "{name}");
        }
    }
    assert_eq!(r["client_frame_bytes"], contracts::control::MAX_FRAME);
    assert_eq!(
        r["data_queue_events_per_runtime"].as_u64().unwrap()
            * r["client_frame_bytes"].as_u64().unwrap(),
        r["data_queue_bytes_per_runtime"].as_u64().unwrap()
    );
    let mut reservations = BTreeMap::from([("A", "starting")]);
    for state in ["discovering", "initializing", "ready", "stopping"] {
        reservations.insert("A", state);
        assert!(reservations.len() >= r["max_active_runtimes"].as_u64().unwrap() as usize);
    }
    reservations.remove("A");
    assert!(reservations.is_empty());
}

#[test]
fn durable_limits_are_lifetime_global_and_refuse_before_an_extra_row() {
    let c = v7();
    migrate(&c);
    c.execute_batch("BEGIN IMMEDIATE").unwrap();
    for i in 0..129 {
        let session = format!("session-{i}");
        c.execute(
            "INSERT INTO managed_sessions VALUES(?1,'codex','codex','prepared',1,'t','t',?1)",
            [&session],
        )
        .unwrap();
        let result = c.execute(
            "INSERT INTO managed_runtimes VALUES(?1,'codex','codex','running',1,0,'t','t')",
            [&session],
        );
        assert_eq!(result.is_ok(), i < 128);
    }
    c.execute_batch("COMMIT; UPDATE managed_runtimes SET generation=1,revision=2 WHERE managed_session_id='session-0'").unwrap();
    c.execute_batch("BEGIN IMMEDIATE").unwrap();
    for i in 1..=4096 {
        if i > 1 {
            c.execute("UPDATE managed_runtimes SET generation=?1,revision=revision+1 WHERE managed_session_id='session-0'",[i]).unwrap();
        }
        c.execute("INSERT INTO managed_runtime_launches VALUES('session-0','codex','codex',?1,?2,'authority','t')",params![i,format!("instance-{i}")]).unwrap();
    }
    c.execute_batch("COMMIT").unwrap();
    let before = snapshot(&c);
    c.execute_batch("BEGIN IMMEDIATE; UPDATE managed_runtimes SET generation=4097,revision=revision+1 WHERE managed_session_id='session-0'").unwrap();
    assert!(c.execute_batch("INSERT INTO managed_runtime_launches VALUES('session-0','codex','codex',4097,'extra-instance','authority','t')").is_err());
    c.execute_batch("ROLLBACK").unwrap();
    assert_eq!(snapshot(&c), before);
    // Historical receipt remains readable at capacity; this is not a replay-resolver test.
    assert_eq!(
        c.query_row::<String, _, _>(
            "SELECT result FROM processed_intents WHERE intent_id='old-intent'",
            [],
            |r| r.get(0)
        )
        .unwrap(),
        "{ \"result\": 1 }"
    );
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateEnvelope {
    schema_version: u8,
    request_id: uuid::Uuid,
    authority_instance_id: Option<uuid::Uuid>,
    method: String,
    params: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireFence {
    managed_session_id: uuid::Uuid,
    generation: u64,
    runtime_instance_id: Option<uuid::Uuid>,
}

const ROUTED: [&str; 9] = [
    "managedSession/runtime/create",
    "managedSession/runtime/resume",
    "managedSession/runtime/abandon",
    "managedSession/turn/start",
    "managedSession/turn/cancel",
    "managedSession/close",
    "executionLease/grant",
    "executionLease/renew",
    "managedTurn/approval/respond",
];

/// Candidate request-delta oracle. Production Request::parse remains protocol 1, untouched.
fn candidate_request(raw: Value) -> Result<contracts::control::Operation, &'static str> {
    if raw["schema_version"] != 2 {
        return Err("protocol_version_unsupported");
    }
    if raw["method"] == "authority/hello" && raw.get("authority_instance_id").is_some() {
        return Err("invalid_request");
    }
    let mut r: CandidateEnvelope = serde_json::from_value(raw).map_err(|_| "invalid_request")?;
    assert_eq!(r.schema_version, 2);
    let _correlation = r.request_id;
    if !contracts::control::METHODS.contains(&r.method.as_str()) {
        return Err("unsupported_method");
    }
    if r.method == "authority/hello" {
        if r.params["protocol_version"] != 2 {
            return Err("protocol_version_unsupported");
        }
    } else if r.authority_instance_id.is_none() {
        return Err("correlation_error");
    }
    let p = r.params.as_object_mut().ok_or("invalid_request")?;
    if ROUTED.contains(&r.method.as_str()) {
        let fence: WireFence =
            serde_json::from_value(p.remove("runtime_fence").ok_or("invalid_request")?)
                .map_err(|_| "invalid_request")?;
        let optional =
            ["managedSession/close", "managedSession/runtime/abandon"].contains(&r.method.as_str());
        if fence.generation > i64::MAX as u64
            || (!optional && (fence.generation == 0 || fence.runtime_instance_id.is_none()))
            || fence.runtime_instance_id == Some(fence.managed_session_id)
            || r.authority_instance_id == Some(fence.managed_session_id)
            || fence
                .runtime_instance_id
                .is_some_and(|id| Some(id) == r.authority_instance_id)
        {
            return Err("invalid_request");
        }
        if let Some(sid) = p.get("managed_session_id")
            && sid.as_str() != Some(fence.managed_session_id.to_string().as_str())
        {
            return Err("invalid_request");
        }
    }
    if r.method.starts_with("runtime/") {
        let session = p.remove("managed_session_id").ok_or("invalid_request")?;
        let session: uuid::Uuid = serde_json::from_value(session).map_err(|_| "invalid_request")?;
        if r.authority_instance_id == Some(session) {
            return Err("invalid_request");
        }
        if r.method != "runtime/status" {
            let generation = p
                .remove("expected_generation")
                .and_then(|v| v.as_u64())
                .filter(|n| *n <= i64::MAX as u64)
                .ok_or("invalid_request")?;
            if r.method == "runtime/model/list" && generation == 0 {
                return Err("invalid_request");
            }
            if ["runtime/start", "runtime/stop"].contains(&r.method.as_str()) {
                p.get("expected_revision")
                    .and_then(Value::as_u64)
                    .filter(|n| *n <= i64::MAX as u64)
                    .ok_or("invalid_request")?;
            }
            if r.method != "runtime/start" {
                let id = p
                    .remove("expected_runtime_instance_id")
                    .ok_or("invalid_request")?;
                if id.is_null() {
                    if r.method != "runtime/stop" {
                        return Err("invalid_request");
                    }
                } else {
                    let id: uuid::Uuid =
                        serde_json::from_value(id).map_err(|_| "invalid_request")?;
                    if id == session || Some(id) == r.authority_instance_id {
                        return Err("invalid_request");
                    }
                }
            }
        }
    }
    let op: contracts::control::Operation =
        serde_json::from_value(json!({"method":r.method,"params":r.params}))
            .map_err(|_| "invalid_request")?;
    // Hello's version is the explicit candidate delta; baseline validate intentionally accepts v1 only.
    if !matches!(op, contracts::control::Operation::Hello(_)) {
        op.validate().map_err(|_| "invalid_request")?;
    }
    Ok(op)
}

fn request(method: &str, params: Value) -> Value {
    json!({"schema_version":2,"request_id":"00000000-0000-4000-8000-000000000010","authority_instance_id":"00000000-0000-4000-8000-000000000011","method":method,"params":params})
}

#[test]
fn protocol_two_delta_oracle_refuses_implicit_target_unknown_fields_and_generic_execution() {
    let valid = request(
        "runtime/start",
        json!({"intent_id":"00000000-0000-4000-8000-000000000012","adapter_id":"codex","runtime_kind":"codex","managed_session_id":"00000000-0000-4000-8000-000000000001","expected_revision":0,"expected_generation":0}),
    );
    assert!(candidate_request(valid.clone()).is_ok());
    for field in [
        "managed_session_id",
        "expected_generation",
        "expected_revision",
    ] {
        let mut bad = valid.clone();
        bad["params"].as_object_mut().unwrap().remove(field);
        assert!(candidate_request(bad).is_err(), "{field}");
    }
    for field in ["model", "effort", "argv", "env", "pid", "prompt"] {
        let mut bad = valid.clone();
        bad["params"][field] = json!("not permitted");
        assert!(candidate_request(bad).is_err(), "{field}");
    }
    let mut old = valid.clone();
    old["schema_version"] = json!(1);
    assert!(matches!(
        candidate_request(old),
        Err("protocol_version_unsupported")
    ));
    let mut missing = valid.clone();
    missing
        .as_object_mut()
        .unwrap()
        .remove("authority_instance_id");
    assert!(candidate_request(missing).is_err());
    let mut extra = valid.clone();
    extra["selected_project"] = json!("A");
    assert!(candidate_request(extra).is_err());
    for generation in [
        json!(-1),
        json!(1.5),
        json!(true),
        json!("1"),
        json!(u64::MAX),
    ] {
        let mut bad = valid.clone();
        bad["params"]["expected_generation"] = generation;
        assert!(candidate_request(bad).is_err());
    }
    let hello = json!({"schema_version":2,"request_id":"00000000-0000-4000-8000-000000000010","method":"authority/hello","params":{"protocol_version":2,"client_instance_id":"00000000-0000-4000-8000-000000000013"}});
    assert!(candidate_request(hello.clone()).is_ok());
    let mut bad = hello;
    bad["params"]["protocol_version"] = json!(1);
    assert!(candidate_request(bad).is_err());
    assert_eq!(contracts::control::METHODS.len() - ROUTED.len() - 4 - 1, 15);
}

#[test]
fn turn_start_requires_exact_session_launch_fence_without_relaxing_original_payload() {
    let valid = request(
        "managedSession/turn/start",
        json!({"intent_id":"00000000-0000-4000-8000-000000000012","managed_session_id":"00000000-0000-4000-8000-000000000001","expected_session_revision":1,"prompt":"unit fixture","runtime_fence":{"managed_session_id":"00000000-0000-4000-8000-000000000001","generation":1,"runtime_instance_id":"00000000-0000-4000-8000-000000000002"}}),
    );
    assert!(candidate_request(valid.clone()).is_ok());
    for field in ["runtime_fence", "prompt"] {
        let mut bad = valid.clone();
        bad["params"].as_object_mut().unwrap().remove(field);
        assert!(candidate_request(bad).is_err());
    }
    let mut bad = valid.clone();
    bad["params"]["runtime_fence"]["managed_session_id"] =
        json!("00000000-0000-4000-8000-000000000003");
    assert!(candidate_request(bad).is_err());
    let mut bad = valid.clone();
    bad["params"]["runtime_fence"]["runtime_instance_id"] = Value::Null;
    assert!(candidate_request(bad).is_err());
    let mut bad = valid;
    bad["params"]["runtime_fence"]["pid"] = json!(123);
    assert!(candidate_request(bad).is_err());
}
