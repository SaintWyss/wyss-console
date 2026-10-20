use super::*;
use crate::{control::f8_fixture, f8::migration};
use serde_json::json;

fn uuid() -> String {
    crate::control::store::uuid().as_str().to_owned()
}
fn one<T: rusqlite::types::FromSql>(c: &Connection, sql: &str) -> T {
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}

/// A migrated V8 image holding only legacy history.
fn legacy_image() -> Connection {
    let mut source = f8_fixture::v7_with_completed_turn();
    migration::prepare(&mut source).unwrap().into_connection()
}

struct Work {
    conn: Connection,
    session: String,
    stream: String,
    authority: String,
    instance: String,
    turn: String,
    intent: String,
}
fn head(c: &Connection) -> u64 {
    one::<String>(c, "SELECT last_sequence FROM authority_metadata")
        .parse()
        .unwrap()
}
fn append(
    c: &Connection,
    session: &str,
    stream: &str,
    kind: &str,
    corr: Option<&str>,
    authority: &str,
    payload: Value,
) {
    let n = head(c) + 1;
    let id = uuid();
    let envelope = json!({"schema_version":1,"event_id":id,"event_type":kind,"payload_schema_version":2,
        "timestamp":"2026-10-20T10:00:00.000Z","stream_id":stream,"sequence":n.to_string(),"session_id":session,
        "source":{"component":"wyss_control","instance_id":authority},"correlation_id":corr,"causation_id":null,
        "payload":payload,"persistence":"durable","sensitivity":"internal","redaction":"none"});
    c.execute(
        "INSERT INTO control_events VALUES(?1,?2,?3,?4,?5)",
        rusqlite::params![
            id,
            stream,
            format!("{n:020}"),
            session,
            envelope.to_string()
        ],
    )
    .unwrap();
    c.execute(
        "UPDATE authority_metadata SET last_sequence=?1",
        [n.to_string()],
    )
    .unwrap();
}
fn mutation_for(session: &str, intent: &str) -> (String, Value) {
    let request = json!({"schema_version":2,"request_id":uuid(),"authority_instance_id":uuid(),"method":"runtime/start",
        "params":{"intent_id":intent,"adapter_id":"codex","runtime_kind":"codex","managed_session_id":session,
            "expected_generation":0,"expected_revision":0}});
    let r = Request::parse(&serde_json::to_vec(&request).unwrap()).unwrap();
    let bytes = r.canonical_intent().unwrap();
    let result = json!({"adapter_id":"codex","runtime_kind":"codex","desired_state":"running","revision":1,
        "created_at":"2026-10-20T10:00:00Z","updated_at":"2026-10-20T10:00:00Z","managed_session_id":session,"generation":1});
    (String::from_utf8(bytes).unwrap(), result)
}
/// Legacy history plus one generation-aware runtime and one assigned, completed turn cloned from
/// the legacy turn through the same canonical row shapes.
fn with_work() -> Work {
    let c = legacy_image();
    let session: String = one(&c, "SELECT session_id FROM managed_sessions");
    let stream: String = one(&c, "SELECT stream_id FROM authority_metadata");
    let (authority, instance, intent) = (uuid(), uuid(), uuid());
    c.execute_batch("BEGIN IMMEDIATE").unwrap();
    c.execute("INSERT INTO managed_runtimes VALUES(?1,'codex','codex','running',1,1,'2026-10-20T10:00:00Z','2026-10-20T10:00:00Z')", [&session]).unwrap();
    c.execute("INSERT INTO managed_runtime_launches VALUES(?1,'codex','codex',1,?2,?3,'2026-10-20T10:00:00Z')", [&session, &instance, &authority]).unwrap();
    append(
        &c,
        &session,
        &stream,
        "authority.runtime.desired_state_changed",
        Some(&intent),
        &authority,
        json!({"adapter_id":"codex","runtime_kind":"codex","desired_state":"running","revision":1,"generation":1,"runtime_instance_id":instance}),
    );
    append(
        &c,
        &session,
        &stream,
        "authority.runtime.instance_ready",
        None,
        &authority,
        json!({"adapter_id":"codex","runtime_kind":"codex","runtime_instance_id":instance,"generation":1,"runtime_version":"0.159.0","compatibility":"compatible"}),
    );
    let (mutation, result) = mutation_for(&session, &intent);
    c.execute(
        "INSERT INTO processed_intents VALUES(?1,?2,?3)",
        [&intent, &mutation, &result.to_string()],
    )
    .unwrap();

    // Clone the legacy completed turn as new, assigned work.
    let (old_turn, old_intent, old_instance, old_codex): (String, String, String, Option<String>) =
        c.query_row(
            "SELECT managed_turn_id,intent_id,runtime_instance_id,codex_turn_id FROM managed_turns",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    let (turn, new_intent) = (uuid(), uuid());
    let new_codex = uuid();
    let mut pairs = vec![
        (&old_turn, &turn),
        (&old_intent, &new_intent),
        (&old_instance, &instance),
    ];
    if let Some(old) = &old_codex {
        pairs.push((old, &new_codex));
    }
    let swap = |col: &str| {
        pairs.iter().fold(col.to_owned(), |e, (o, n)| {
            format!("replace({e},'{o}','{n}')")
        })
    };
    let cols: Vec<(String, String)> = c
        .prepare("SELECT name,type FROM pragma_table_info('managed_turns')")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let select = cols
        .iter()
        .map(|(n, t)| if t == "TEXT" { swap(n) } else { n.clone() })
        .collect::<Vec<_>>()
        .join(",");
    c.execute(
        "INSERT INTO managed_turn_launches VALUES(?1,?2,?3,1)",
        [&turn, &session, &instance],
    )
    .unwrap();
    c.execute(&format!("INSERT INTO managed_turns SELECT {select} FROM managed_turns WHERE managed_turn_id='{old_turn}'"), []).unwrap();
    let receipt = format!(
        "INSERT INTO processed_intents SELECT {},{},{} FROM processed_intents WHERE intent_id='{old_intent}'",
        swap("intent_id"),
        swap("mutation"),
        swap("result")
    );
    c.execute(&receipt, []).unwrap();
    let events: Vec<String> = c.prepare("SELECT envelope FROM control_events WHERE json_extract(envelope,'$.event_type') LIKE 'authority.managed_turn.%' AND json_extract(envelope,'$.payload.managed_turn_id')=?1 ORDER BY sequence").unwrap()
        .query_map([&old_turn], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(events.len(), 3);
    for raw in events {
        let v: Value = serde_json::from_str(&raw).unwrap();
        let mut p = v["payload"].clone();
        p["managed_turn_id"] = json!(turn);
        p["runtime_instance_id"] = json!(instance);
        p["generation"] = json!(1);
        let corr = v["correlation_id"].as_str().map(|s| {
            if s == old_intent {
                new_intent.clone()
            } else {
                s.to_owned()
            }
        });
        append(
            &c,
            &session,
            &stream,
            v["event_type"].as_str().unwrap(),
            corr.as_deref(),
            &authority,
            p,
        );
    }
    c.execute_batch("COMMIT").unwrap();
    Work {
        conn: c,
        session,
        stream,
        authority,
        instance,
        turn,
        intent,
    }
}

/// Applies a statement with every trigger temporarily removed, then restores the exact schema.
fn tamper(c: &Connection, sql: &str) {
    let triggers: Vec<(String, String)> = c
        .prepare("SELECT name,sql FROM sqlite_master WHERE type='trigger'")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    c.pragma_update(None, "foreign_keys", false).unwrap();
    for (n, _) in &triggers {
        c.execute_batch(&format!("DROP TRIGGER \"{n}\"")).unwrap();
    }
    c.execute_batch(sql).unwrap();
    for (_, s) in &triggers {
        c.execute_batch(s).unwrap();
    }
    c.pragma_update(None, "foreign_keys", true).unwrap();
}

#[test]
fn legacy_only_image_validates_and_new_work_image_validates() {
    let c = legacy_image();
    let s = validate_v8(&c).unwrap();
    assert_eq!(
        (s.runtimes, s.launches, s.assigned_turns, s.legacy_turns),
        (0, 0, 0, 1)
    );
    // The immediate-migration validator keeps refusing any nonempty V8 table.
    let w = with_work();
    let s = validate_v8(&w.conn).unwrap();
    assert_eq!(
        (s.runtimes, s.launches, s.assigned_turns, s.legacy_turns),
        (1, 1, 1, 1)
    );
    assert_eq!(s.events, head(&w.conn));
    assert_eq!(
        migration::validate_migrated(&w.conn),
        Err(E::StorageIncompatible)
    );
}

#[test]
fn restart_and_reopen_validate_the_same_image() {
    let w = with_work();
    let dir = std::env::temp_dir().join(format!("wyss-f8-v8-{}", uuid()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("copy.db");
    w.conn
        .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
    let reopened = Connection::open(&path).unwrap();
    assert_eq!(
        validate_v8(&reopened).unwrap(),
        validate_v8(&w.conn).unwrap()
    );
    drop(reopened);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn tampering_with_runtime_launch_sidecar_event_receipt_or_schema_is_refused() {
    type Case = (&'static str, Box<dyn Fn(&Work) -> String>);
    let cases: Vec<Case> = vec![
        (
            "generation bumped",
            Box::new(|w| {
                format!(
                    "UPDATE managed_runtimes SET generation=2 WHERE managed_session_id='{}'",
                    w.session
                )
            }),
        ),
        (
            "revision bumped",
            Box::new(|w| {
                format!(
                    "UPDATE managed_runtimes SET revision=2 WHERE managed_session_id='{}'",
                    w.session
                )
            }),
        ),
        (
            "desired state flipped",
            Box::new(|w| {
                format!(
                    "UPDATE managed_runtimes SET desired_state='stopped' WHERE managed_session_id='{}'",
                    w.session
                )
            }),
        ),
        (
            "runtime row deleted",
            Box::new(|w| {
                format!(
                    "DELETE FROM managed_runtimes WHERE managed_session_id='{}'",
                    w.session
                )
            }),
        ),
        (
            "launch authority swapped",
            Box::new(|_| {
                format!(
                    "UPDATE managed_runtime_launches SET authority_instance_id='{}'",
                    uuid()
                )
            }),
        ),
        (
            "launch instance swapped",
            Box::new(|_| {
                format!(
                    "UPDATE managed_runtime_launches SET runtime_instance_id='{}'",
                    uuid()
                )
            }),
        ),
        (
            "launch deleted",
            Box::new(|_| "DELETE FROM managed_runtime_launches".into()),
        ),
        (
            "extra launch without event",
            Box::new(|w| {
                format!(
                    "INSERT INTO managed_runtime_launches VALUES('{}','codex','codex',2,'{}','{}','2026-10-20T10:00:00Z')",
                    w.session,
                    uuid(),
                    w.authority
                )
            }),
        ),
        (
            "turn sidecar instance",
            Box::new(|_| {
                format!(
                    "UPDATE managed_turn_launches SET runtime_instance_id='{}'",
                    uuid()
                )
            }),
        ),
        (
            "turn sidecar deleted",
            Box::new(|_| "DELETE FROM managed_turn_launches".into()),
        ),
        (
            "legacy turn gains a sidecar",
            Box::new(|w| {
                format!(
                    "INSERT INTO managed_turn_launches SELECT managed_turn_id,managed_session_id,'{}',1 FROM managed_turns WHERE managed_turn_id<>'{}'",
                    w.instance, w.turn
                )
            }),
        ),
        (
            "turn row reassigned",
            Box::new(|w| {
                format!(
                    "UPDATE managed_turns SET runtime_instance_id='{}' WHERE managed_turn_id='{}'",
                    uuid(),
                    w.turn
                )
            }),
        ),
        (
            "event removed",
            Box::new(|w| {
                format!(
                    "DELETE FROM control_events WHERE sequence='{:020}'",
                    head(&w.conn)
                )
            }),
        ),
        (
            "event generation forged",
            Box::new(|w| {
                format!(
                    "UPDATE control_events SET envelope=json_set(envelope,'$.payload.generation',2) WHERE sequence='{:020}'",
                    head(&w.conn)
                )
            }),
        ),
        (
            "event attributed to another session",
            Box::new(|w| {
                format!(
                    "UPDATE control_events SET envelope=json_set(envelope,'$.session_id','{}') WHERE sequence='{:020}'",
                    uuid(),
                    head(&w.conn)
                )
            }),
        ),
        (
            "event source authority forged",
            Box::new(|w| {
                format!(
                    "UPDATE control_events SET envelope=json_set(envelope,'$.source.instance_id','{}') WHERE sequence='{:020}'",
                    uuid(),
                    head(&w.conn) - 3
                )
            }),
        ),
        (
            "head rolled back",
            Box::new(|w| {
                format!(
                    "UPDATE authority_metadata SET last_sequence='{}'",
                    head(&w.conn) - 1
                )
            }),
        ),
        (
            "vocabulary lowered",
            Box::new(|_| "UPDATE authority_metadata SET event_vocabulary=1".into()),
        ),
        (
            "receipt generation forged",
            Box::new(|w| {
                format!(
                    "UPDATE processed_intents SET result=json_set(result,'$.generation',2) WHERE intent_id='{}'",
                    w.intent
                )
            }),
        ),
        (
            "receipt session forged",
            Box::new(|w| {
                format!(
                    "UPDATE processed_intents SET result=json_set(result,'$.managed_session_id','{}') WHERE intent_id='{}'",
                    uuid(),
                    w.intent
                )
            }),
        ),
        (
            "receipt deleted",
            Box::new(|w| {
                format!(
                    "DELETE FROM processed_intents WHERE intent_id='{}'",
                    w.intent
                )
            }),
        ),
        (
            "unexplained receipt",
            Box::new(|w| {
                let (m, r) = mutation_for(&w.session, &uuid());
                format!(
                    "INSERT INTO processed_intents VALUES('{}','{}','{}')",
                    uuid(),
                    m,
                    r
                )
            }),
        ),
        (
            "extra object",
            Box::new(|_| "CREATE TABLE shadow(x INTEGER)".into()),
        ),
        (
            "trigger removed",
            Box::new(|_| "DROP TRIGGER managed_runtime_launches_no_delete".into()),
        ),
    ];
    for (name, sql) in &cases {
        let w = with_work();
        validate_v8(&w.conn).unwrap();
        let sql = sql(&w);
        let triggers_edit = name.contains("trigger") || name.contains("object");
        if triggers_edit {
            w.conn.execute_batch(&sql).unwrap();
        } else {
            tamper(&w.conn, &sql);
        }
        assert!(validate_v8(&w.conn).is_err(), "{name}");
    }
}

#[test]
fn commit_validated_rolls_back_everything_on_a_refusal() {
    let mut w = with_work();
    let snapshot = |c: &Connection| -> (u64, i64, i64) {
        (
            head(c),
            one(c, "SELECT count(*) FROM control_events"),
            one(c, "SELECT count(*) FROM managed_runtime_launches"),
        )
    };
    let before = snapshot(&w.conn);
    // A launch with no stream explanation: the whole transaction, including its row, rolls back.
    let result = commit_validated(&mut w.conn, |tx| {
        tx.execute(
            "INSERT INTO managed_runtimes VALUES(?1,'codex','codex','stopped',1,0,'t','t')",
            [uuid()],
        )
        .map_err(|_| E::StorageIncompatible)
    });
    assert!(result.is_err());
    assert_eq!(snapshot(&w.conn), before);
    // A failing closure also leaves nothing behind.
    let result: ControlResult<()> = commit_validated(&mut w.conn, |tx| {
        tx.execute("UPDATE authority_metadata SET last_sequence='999'", [])
            .map_err(|_| E::StorageIncompatible)?;
        Err(E::InternalError)
    });
    assert_eq!(result, Err(E::InternalError));
    assert_eq!(snapshot(&w.conn), before);
    // A valid image commits.
    commit_validated(&mut w.conn, |_| Ok(())).unwrap();
    validate_v8(&w.conn).unwrap();
    let _ = (&w.stream,);
}
