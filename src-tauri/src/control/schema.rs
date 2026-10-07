//! Explicit v0/v1/v2/v3 -> v4 migrations. Unknown/corrupt schemas fail closed.
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
pub(crate) const V2_OBJECTS: &[(&str, &str)] = &[
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
pub(crate) const V3_OBJECTS: &[(&str, &str)] = &[
    (
        "authority_metadata",
        "CREATE TABLE authority_metadata (id INTEGER PRIMARY KEY CHECK(id=1), schema_version INTEGER NOT NULL CHECK(schema_version=3), stream_id TEXT NOT NULL, last_sequence TEXT NOT NULL) STRICT",
    ),
    V2_OBJECTS[1],
    V2_OBJECTS[2],
    V2_OBJECTS[3],
    V2_OBJECTS[4],
    V2_OBJECTS[5],
    V2_OBJECTS[6],
    (
        "execution_leases",
        "CREATE TABLE execution_leases (lease_id TEXT PRIMARY KEY NOT NULL, session_id TEXT NOT NULL REFERENCES managed_sessions(session_id), state TEXT NOT NULL CHECK(state IN ('active','revoked','expired')), scopes TEXT NOT NULL CHECK(length(scopes)<=64), revision INTEGER NOT NULL CHECK(revision>0), issued_at TEXT NOT NULL, expires_at TEXT NOT NULL, updated_at TEXT NOT NULL, revocation_reason TEXT CHECK(revocation_reason IN ('user_request','session_closed')), CHECK((state='revoked')=(revocation_reason IS NOT NULL)), CHECK(lease_id<>session_id)) STRICT",
    ),
    (
        "execution_leases_one_active",
        "CREATE UNIQUE INDEX execution_leases_one_active ON execution_leases(session_id) WHERE state='active'",
    ),
];
pub(crate) const OBJECTS: &[(&str, &str)] = &[
    (
        "authority_metadata",
        "CREATE TABLE authority_metadata (id INTEGER PRIMARY KEY CHECK(id=1), schema_version INTEGER NOT NULL CHECK(schema_version=4), stream_id TEXT NOT NULL, last_sequence TEXT NOT NULL) STRICT",
    ),
    V3_OBJECTS[1],
    V3_OBJECTS[2],
    V3_OBJECTS[3],
    V3_OBJECTS[4],
    V3_OBJECTS[5],
    V3_OBJECTS[6],
    V3_OBJECTS[7],
    V3_OBJECTS[8],
    (
        "managed_session_bindings",
        "CREATE TABLE managed_session_bindings (managed_session_id TEXT PRIMARY KEY NOT NULL REFERENCES managed_sessions(session_id), adapter_id TEXT NOT NULL CHECK(adapter_id='codex'), runtime_kind TEXT NOT NULL CHECK(runtime_kind='codex'), runtime_session_id TEXT NOT NULL UNIQUE CHECK(length(runtime_session_id) BETWEEN 1 AND 256), workspace_root TEXT NOT NULL CHECK(length(workspace_root) BETWEEN 1 AND 256), binding_revision INTEGER NOT NULL CHECK(binding_revision=1), created_at TEXT NOT NULL, updated_at TEXT NOT NULL) STRICT",
    ),
    (
        "managed_session_operations",
        "CREATE TABLE managed_session_operations (operation_id TEXT PRIMARY KEY NOT NULL, managed_session_id TEXT NOT NULL REFERENCES managed_sessions(session_id), kind TEXT NOT NULL CHECK(kind IN ('create','resume')), mutation TEXT NOT NULL CHECK(length(mutation)<=1024), state TEXT NOT NULL CHECK(state IN ('reserved','dispatching','succeeded','failed','indeterminate')), provider_request_id TEXT NOT NULL UNIQUE CHECK(length(provider_request_id)<=256), created_at TEXT NOT NULL, updated_at TEXT NOT NULL, provider_thread_id TEXT CHECK(provider_thread_id IS NULL OR length(provider_thread_id) BETWEEN 1 AND 256), provider_stage TEXT NOT NULL CHECK(provider_stage IN ('none','started','archived','unarchived'))) STRICT",
    ),
    (
        "managed_session_operations_one_pending",
        "CREATE UNIQUE INDEX managed_session_operations_one_pending ON managed_session_operations(managed_session_id) WHERE state IN ('reserved','dispatching','indeterminate')",
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
                "INSERT INTO authority_metadata VALUES(1,4,?1,'0')",
                [uuid::Uuid::new_v4().to_string()],
            )
            .map_err(|_| E::StorageUnavailable)?;
            tx.pragma_update(None, "user_version", 4)
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
            for i in [0, 2, 4, 5, 6, 7, 8, 9, 10, 11] {
                tx.execute_batch(OBJECTS[i].1)
                    .map_err(|_| E::StorageUnavailable)?;
            }
            tx.execute_batch("INSERT INTO authority_metadata SELECT id,4,stream_id,last_sequence FROM authority_metadata_v1;
                INSERT INTO control_events SELECT * FROM control_events_v1;
                DROP TABLE control_events_v1; DROP TABLE authority_metadata_v1;").map_err(|_|E::StorageUnavailable)?;
            tx.pragma_update(None, "user_version", 4)
                .map_err(|_| E::StorageUnavailable)?;
            before_commit()?;
            tx.commit().map_err(|_| E::StorageUnavailable)?;
        }
        2 => {
            validate(c, V2_OBJECTS)?;
            let tx = c.transaction().map_err(|_| E::StorageUnavailable)?;
            tx.execute_batch("ALTER TABLE authority_metadata RENAME TO authority_metadata_v2;")
                .map_err(|_| E::StorageUnavailable)?;
            for i in [0, 7, 8, 9, 10, 11] {
                tx.execute_batch(OBJECTS[i].1)
                    .map_err(|_| E::StorageUnavailable)?;
            }
            tx.execute_batch("INSERT INTO authority_metadata SELECT id,4,stream_id,last_sequence FROM authority_metadata_v2; DROP TABLE authority_metadata_v2;").map_err(|_|E::StorageUnavailable)?;
            tx.pragma_update(None, "user_version", 4)
                .map_err(|_| E::StorageUnavailable)?;
            before_commit()?;
            tx.commit().map_err(|_| E::StorageUnavailable)?;
        }
        3 => {
            validate(c, V3_OBJECTS)?;
            let tx = c.transaction().map_err(|_| E::StorageUnavailable)?;
            tx.execute_batch("ALTER TABLE authority_metadata RENAME TO authority_metadata_v3;")
                .map_err(|_| E::StorageUnavailable)?;
            for i in [0, 9, 10, 11] {
                tx.execute_batch(OBJECTS[i].1)
                    .map_err(|_| E::StorageUnavailable)?;
            }
            tx.execute_batch("INSERT INTO authority_metadata SELECT id,4,stream_id,last_sequence FROM authority_metadata_v3; DROP TABLE authority_metadata_v3;").map_err(|_|E::StorageUnavailable)?;
            tx.pragma_update(None, "user_version", 4)
                .map_err(|_| E::StorageUnavailable)?;
            before_commit()?;
            tx.commit().map_err(|_| E::StorageUnavailable)?;
        }
        4 => (),
        _ => return Err(E::StorageIncompatible),
    }
    validate(c, OBJECTS)
}
#[cfg(test)]
pub(super) fn fail_migration(c: &mut Connection) -> ControlResult<()> {
    migrate_with(c, || Err(E::StorageUnavailable))
}
