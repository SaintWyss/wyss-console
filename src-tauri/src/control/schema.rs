//! Explicit v0 -> v2 and v1 -> v2 migrations. Unknown/corrupt schemas fail closed.
use crate::contracts::control::{ControlError as E, ControlResult};
use rusqlite::Connection;
pub(crate) const V1_OBJECTS: &[(&str, &str)] = &[
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
pub(crate) const OBJECTS: &[(&str, &str)] = &[
    (
        "authority_metadata",
        "CREATE TABLE authority_metadata (id INTEGER PRIMARY KEY CHECK(id=1), schema_version INTEGER NOT NULL CHECK(schema_version=2), stream_id TEXT NOT NULL, last_sequence TEXT NOT NULL) STRICT",
    ),
    V1_OBJECTS[1],
    (
        "control_events",
        "CREATE TABLE control_events (event_id TEXT PRIMARY KEY NOT NULL, stream_id TEXT NOT NULL, sequence TEXT NOT NULL CHECK(length(sequence)=20), session_id TEXT REFERENCES managed_sessions(session_id), envelope TEXT NOT NULL CHECK(length(envelope)<=2048), UNIQUE(stream_id,sequence)) STRICT",
    ),
    V1_OBJECTS[3],
    V1_OBJECTS[4],
    V1_OBJECTS[5],
    (
        "managed_runtimes",
        "CREATE TABLE managed_runtimes (adapter_id TEXT PRIMARY KEY NOT NULL CHECK(adapter_id='codex'), runtime_kind TEXT NOT NULL CHECK(runtime_kind='codex'), desired_state TEXT NOT NULL CHECK(desired_state IN ('running','stopped')), revision INTEGER NOT NULL CHECK(revision>0), created_at TEXT NOT NULL, updated_at TEXT NOT NULL) STRICT",
    ),
];
fn validate(c: &Connection, objects: &[(&str, &str)]) -> ControlResult<()> {
    let integrity: String = c
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .map_err(|_| E::StorageIncompatible)?;
    if integrity != "ok" {
        return Err(E::StorageIncompatible);
    }
    let mut q=c.prepare("SELECT name,sql FROM sqlite_master WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY name").map_err(|_|E::StorageIncompatible)?;
    let rows = q
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|_| E::StorageIncompatible)?;
    let mut count = 0;
    for row in rows {
        let (name, sql) = row.map_err(|_| E::StorageIncompatible)?;
        if !objects.iter().any(|(n, s)| *n == name && *s == sql) {
            return Err(E::StorageIncompatible);
        }
        count += 1;
    }
    if count != objects.len() {
        return Err(E::StorageIncompatible);
    }
    Ok(())
}
pub(crate) fn migrate(c: &mut Connection) -> ControlResult<()> {
    migrate_with(c, || Ok(()))
}
fn migrate_with(
    c: &mut Connection,
    before_commit: impl FnOnce() -> ControlResult<()>,
) -> ControlResult<()> {
    let version: i64 = c
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|_| E::StorageIncompatible)?;
    match version {
        0 => {
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
                "INSERT INTO authority_metadata VALUES(1,2,?1,'0')",
                [uuid::Uuid::new_v4().to_string()],
            )
            .map_err(|_| E::StorageUnavailable)?;
            tx.pragma_update(None, "user_version", 2)
                .map_err(|_| E::StorageUnavailable)?;
            before_commit()?;
            tx.commit().map_err(|_| E::StorageUnavailable)?;
        }
        1 => {
            validate(c, V1_OBJECTS)?;
            let tx = c.transaction().map_err(|_| E::StorageUnavailable)?;
            // Rebuild only tables whose constraints change. Original envelope bytes remain intact.
            tx.execute_batch(
                "DROP TRIGGER control_events_no_update; DROP TRIGGER control_events_no_delete;
                ALTER TABLE authority_metadata RENAME TO authority_metadata_v1;
                ALTER TABLE control_events RENAME TO control_events_v1;",
            )
            .map_err(|_| E::StorageUnavailable)?;
            for i in [0, 2, 4, 5, 6] {
                tx.execute_batch(OBJECTS[i].1)
                    .map_err(|_| E::StorageUnavailable)?;
            }
            tx.execute_batch("INSERT INTO authority_metadata SELECT id,2,stream_id,last_sequence FROM authority_metadata_v1;
                INSERT INTO control_events SELECT * FROM control_events_v1;
                DROP TABLE control_events_v1; DROP TABLE authority_metadata_v1;").map_err(|_|E::StorageUnavailable)?;
            tx.pragma_update(None, "user_version", 2)
                .map_err(|_| E::StorageUnavailable)?;
            before_commit()?;
            tx.commit().map_err(|_| E::StorageUnavailable)?;
        }
        2 => (),
        _ => return Err(E::StorageIncompatible),
    }
    validate(c, OBJECTS)
}
#[cfg(test)]
pub(super) fn fail_migration(c: &mut Connection) -> ControlResult<()> {
    migrate_with(c, || Err(E::StorageUnavailable))
}
