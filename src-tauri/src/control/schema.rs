//! Explicit v0(empty) -> v1 migration. No runtime schema repair or destructive downgrade.
use crate::contracts::control::{ControlError as E, ControlResult};
use rusqlite::Connection;
pub(crate) const OBJECTS: &[(&str, &str)] = &[
    (
        "authority_metadata",
        "CREATE TABLE authority_metadata (id INTEGER PRIMARY KEY CHECK(id=1), schema_version INTEGER NOT NULL CHECK(schema_version=1), stream_id TEXT NOT NULL, last_sequence TEXT NOT NULL) STRICT",
    ),
    (
        "managed_sessions",
        "CREATE TABLE managed_sessions (session_id TEXT PRIMARY KEY NOT NULL, adapter_id TEXT NOT NULL, runtime_kind TEXT NOT NULL, control_state TEXT NOT NULL CHECK(control_state IN ('prepared','closed')), revision INTEGER NOT NULL CHECK(revision IN (1,2)), created_at TEXT NOT NULL, updated_at TEXT NOT NULL, created_sequence TEXT NOT NULL UNIQUE) STRICT",
    ),
    (
        "control_events",
        "CREATE TABLE control_events (event_id TEXT PRIMARY KEY NOT NULL, stream_id TEXT NOT NULL, sequence TEXT NOT NULL CHECK(length(sequence)=20), session_id TEXT NOT NULL REFERENCES managed_sessions(session_id), envelope TEXT NOT NULL CHECK(length(envelope)<=2048), UNIQUE(stream_id,sequence)) STRICT",
    ),
    (
        "processed_intents",
        "CREATE TABLE processed_intents (intent_id TEXT PRIMARY KEY NOT NULL, mutation TEXT NOT NULL CHECK(length(mutation)<=1024), result TEXT NOT NULL CHECK(length(result)<=1024)) STRICT",
    ),
    (
        "control_events_no_update",
        "CREATE TRIGGER control_events_no_update BEFORE UPDATE ON control_events BEGIN SELECT RAISE(ABORT,'append_only'); END",
    ),
    (
        "control_events_no_delete",
        "CREATE TRIGGER control_events_no_delete BEFORE DELETE ON control_events BEGIN SELECT RAISE(ABORT,'append_only'); END",
    ),
];
pub(crate) fn migrate(c: &mut Connection) -> ControlResult<()> {
    let version: i64 = c
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|_| E::StorageIncompatible)?;
    if version == 0 {
        let count: i64 = c
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )
            .map_err(|_| E::StorageIncompatible)?;
        if count != 0 {
            return Err(E::StorageIncompatible);
        }
        let tx = c.transaction().map_err(|_| E::StorageUnavailable)?;
        for (_, sql) in OBJECTS {
            tx.execute_batch(sql).map_err(|_| E::StorageUnavailable)?;
        }
        tx.execute(
            "INSERT INTO authority_metadata VALUES(1,1,?1,'0')",
            [uuid::Uuid::new_v4().to_string()],
        )
        .map_err(|_| E::StorageUnavailable)?;
        tx.pragma_update(None, "user_version", 1)
            .map_err(|_| E::StorageUnavailable)?;
        tx.commit().map_err(|_| E::StorageUnavailable)?;
    } else if version != 1 {
        return Err(E::StorageIncompatible);
    }
    let integrity: String = c
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .map_err(|_| E::StorageIncompatible)?;
    if integrity != "ok" {
        return Err(E::StorageIncompatible);
    }
    let mut q=c.prepare("SELECT name,sql FROM sqlite_master WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY name").map_err(|_|E::StorageIncompatible)?;
    let objects = q
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|_| E::StorageIncompatible)?;
    let mut count = 0;
    for object in objects {
        let (name, sql) = object.map_err(|_| E::StorageIncompatible)?;
        if !OBJECTS.iter().any(|(n, s)| *n == name && *s == sql) {
            return Err(E::StorageIncompatible);
        }
        count += 1;
    }
    if count != OBJECTS.len() {
        return Err(E::StorageIncompatible);
    }
    Ok(())
}
