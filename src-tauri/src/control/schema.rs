//! Explicit v0/v1/v2/v3/v4/v5 -> v6 migrations. Unknown/corrupt schemas fail closed.
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
pub(crate) const V4_OBJECTS: &[(&str, &str)] = &[
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
pub(crate) const V5_OBJECTS: &[(&str, &str)] = &[
    (
        "authority_metadata",
        "CREATE TABLE authority_metadata (id INTEGER PRIMARY KEY CHECK(id=1), schema_version INTEGER NOT NULL CHECK(schema_version=5), stream_id TEXT NOT NULL, last_sequence TEXT NOT NULL) STRICT",
    ),
    V4_OBJECTS[1],
    V4_OBJECTS[2],
    V4_OBJECTS[3],
    V4_OBJECTS[4],
    V4_OBJECTS[5],
    V4_OBJECTS[6],
    V4_OBJECTS[7],
    V4_OBJECTS[8],
    V4_OBJECTS[9],
    V4_OBJECTS[10],
    V4_OBJECTS[11],
    (
        "managed_turns",
        "CREATE TABLE managed_turns (managed_turn_id TEXT PRIMARY KEY NOT NULL, managed_session_id TEXT NOT NULL REFERENCES managed_sessions(session_id), codex_thread_id TEXT NOT NULL CHECK(length(codex_thread_id) BETWEEN 1 AND 256), codex_turn_id TEXT CHECK(codex_turn_id IS NULL OR length(codex_turn_id) BETWEEN 1 AND 256), admitted_lease_id TEXT NOT NULL REFERENCES execution_leases(lease_id), admitted_lease_revision INTEGER NOT NULL CHECK(admitted_lease_revision>0), runtime_instance_id TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('starting','running','completed','interrupted','failed','indeterminate')), revision INTEGER NOT NULL CHECK(revision>0), prompt_fingerprint TEXT NOT NULL CHECK(length(prompt_fingerprint)=64 AND prompt_fingerprint NOT GLOB '*[^0-9a-f]*'), prompt_byte_length INTEGER NOT NULL CHECK(prompt_byte_length BETWEEN 1 AND 8192), intent_id TEXT NOT NULL UNIQUE, intent_mutation TEXT NOT NULL CHECK(length(intent_mutation)<=1024), dispatched INTEGER NOT NULL CHECK(dispatched IN (0,1)), created_at TEXT NOT NULL, updated_at TEXT NOT NULL, terminal_at TEXT, terminal_reason TEXT, CHECK((state IN ('starting','running'))=(terminal_at IS NULL)), CHECK((terminal_at IS NULL)=(terminal_reason IS NULL))) STRICT",
    ),
    (
        "managed_turns_one_active",
        "CREATE UNIQUE INDEX managed_turns_one_active ON managed_turns((1)) WHERE state IN ('starting','running')",
    ),
];
/// v6 widens the stored scope encoding to the exact canonical maximum
/// (`ExecutionScope` now has five members) and adds approval control metadata.
/// `execution_leases` is rebuilt; no constraint is relaxed beyond that bound.
pub(crate) const OBJECTS: &[(&str, &str)] = &[
    (
        "authority_metadata",
        "CREATE TABLE authority_metadata (id INTEGER PRIMARY KEY CHECK(id=1), schema_version INTEGER NOT NULL CHECK(schema_version=6), stream_id TEXT NOT NULL, last_sequence TEXT NOT NULL) STRICT",
    ),
    V5_OBJECTS[1],
    V5_OBJECTS[2],
    V5_OBJECTS[3],
    V5_OBJECTS[4],
    V5_OBJECTS[5],
    V5_OBJECTS[6],
    (
        "execution_leases",
        "CREATE TABLE execution_leases (lease_id TEXT PRIMARY KEY NOT NULL, session_id TEXT NOT NULL REFERENCES managed_sessions(session_id), state TEXT NOT NULL CHECK(state IN ('active','revoked','expired')), scopes TEXT NOT NULL CHECK(length(scopes)<=80), revision INTEGER NOT NULL CHECK(revision>0), issued_at TEXT NOT NULL, expires_at TEXT NOT NULL, updated_at TEXT NOT NULL, revocation_reason TEXT CHECK(revocation_reason IN ('user_request','session_closed')), CHECK((state='revoked')=(revocation_reason IS NOT NULL)), CHECK(lease_id<>session_id)) STRICT",
    ),
    V5_OBJECTS[8],
    V5_OBJECTS[9],
    V5_OBJECTS[10],
    V5_OBJECTS[11],
    V5_OBJECTS[12],
    V5_OBJECTS[13],
    (
        "managed_approvals",
        "CREATE TABLE managed_approvals (managed_approval_id TEXT PRIMARY KEY NOT NULL, managed_turn_id TEXT NOT NULL REFERENCES managed_turns(managed_turn_id), managed_session_id TEXT NOT NULL REFERENCES managed_sessions(session_id), runtime_instance_id TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('command_execution','file_change')), provider_item_id TEXT NOT NULL CHECK(length(provider_item_id) BETWEEN 1 AND 128), admitted_lease_id TEXT NOT NULL REFERENCES execution_leases(lease_id), admitted_lease_revision INTEGER NOT NULL CHECK(admitted_lease_revision>0), state TEXT NOT NULL CHECK(state IN ('pending','accepted','declined','cancelled','expired','invalidated','indeterminate')), revision INTEGER NOT NULL CHECK(revision>0), action_fingerprint TEXT NOT NULL CHECK(length(action_fingerprint)=64 AND action_fingerprint NOT GLOB '*[^0-9a-f]*'), structure TEXT NOT NULL CHECK(length(structure)<=160), decision TEXT CHECK(decision IN ('accept','decline','cancel')), effect TEXT NOT NULL CHECK(effect IN ('none','dispatched','confirmed','unknown')), created_at TEXT NOT NULL, updated_at TEXT NOT NULL, expires_at TEXT NOT NULL, terminal_at TEXT, terminal_reason TEXT, created_sequence TEXT NOT NULL UNIQUE CHECK(length(created_sequence)=20), UNIQUE(managed_turn_id,provider_item_id), CHECK((state='pending')=(terminal_at IS NULL)), CHECK((terminal_at IS NULL)=(terminal_reason IS NULL)), CHECK(CASE state WHEN 'accepted' THEN decision IS 'accept' AND effect IN ('dispatched','confirmed') WHEN 'indeterminate' THEN decision IS 'accept' AND effect='unknown' WHEN 'declined' THEN decision IS 'decline' AND effect='none' WHEN 'cancelled' THEN decision IS 'cancel' AND effect='none' ELSE decision IS NULL AND effect='none' END), CHECK(managed_approval_id NOT IN (managed_turn_id,managed_session_id,admitted_lease_id,runtime_instance_id))) STRICT",
    ),
    (
        "managed_approvals_one_pending",
        "CREATE UNIQUE INDEX managed_approvals_one_pending ON managed_approvals((1)) WHERE state='pending'",
    ),
    // Identity, binding, fingerprint and deadline never change. Only a pending approval
    // may be decided or terminalized; only an accepted one whose provider outcome is still
    // open may be confirmed or become indeterminate. Every write is exactly one revision.
    (
        "managed_approvals_transition",
        "CREATE TRIGGER managed_approvals_transition BEFORE UPDATE ON managed_approvals WHEN NOT (NEW.revision=OLD.revision+1 AND NEW.managed_approval_id=OLD.managed_approval_id AND NEW.managed_turn_id=OLD.managed_turn_id AND NEW.managed_session_id=OLD.managed_session_id AND NEW.runtime_instance_id=OLD.runtime_instance_id AND NEW.kind=OLD.kind AND NEW.provider_item_id=OLD.provider_item_id AND NEW.admitted_lease_id=OLD.admitted_lease_id AND NEW.admitted_lease_revision=OLD.admitted_lease_revision AND NEW.action_fingerprint=OLD.action_fingerprint AND NEW.structure=OLD.structure AND NEW.created_at=OLD.created_at AND NEW.expires_at=OLD.expires_at AND NEW.created_sequence=OLD.created_sequence AND ((OLD.state='pending' AND NEW.state IN ('accepted','declined','cancelled','expired','invalidated')) OR (OLD.state='accepted' AND OLD.effect='dispatched' AND NEW.state IN ('accepted','indeterminate') AND NEW.effect IN ('confirmed','unknown') AND NEW.terminal_at=OLD.terminal_at))) BEGIN SELECT RAISE(ABORT,'approval_transition'); END",
    ),
    (
        "managed_approvals_no_delete",
        "CREATE TRIGGER managed_approvals_no_delete BEFORE DELETE ON managed_approvals BEGIN SELECT RAISE(ABORT,'append_only'); END",
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
    if version == 6 {
        return validate(c, OBJECTS);
    }
    let old = match version {
        0 => &[][..],
        1 => V1_OBJECTS,
        2 => V2_OBJECTS,
        3 => V3_OBJECTS,
        4 => V4_OBJECTS,
        5 => V5_OBJECTS,
        _ => return Err(E::StorageIncompatible),
    };
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
    } else {
        validate(c, old)?;
    }
    // Rebuilding a referenced parent table: with enforcement on, or without the legacy
    // rename rule, SQLite would rewrite the child REFERENCES clauses to the temporary
    // name. Both pragmas are connection settings and cannot change inside a transaction;
    // they are restored on every path and the result is checked before returning.
    let rebuild_leases = old.iter().any(|(n, _)| *n == "execution_leases");
    let enforced: bool = c
        .pragma_query_value(None, "foreign_keys", |r| r.get(0))
        .map_err(|_| E::StorageUnavailable)?;
    c.execute_batch("PRAGMA foreign_keys=OFF; PRAGMA legacy_alter_table=ON;")
        .map_err(|_| E::StorageUnavailable)?;
    let result = migrate_in_transaction(c, version, old, rebuild_leases, before_commit);
    let restored = c
        .execute_batch("PRAGMA legacy_alter_table=OFF;")
        .and_then(|()| c.pragma_update(None, "foreign_keys", enforced));
    result?;
    restored.map_err(|_| E::StorageUnavailable)?;
    let violations: i64 = c
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .map_err(|_| E::StorageIncompatible)?;
    if violations != 0 {
        return Err(E::StorageIncompatible);
    }
    validate(c, OBJECTS)
}
fn migrate_in_transaction(
    c: &mut Connection,
    version: i64,
    old: &[(&str, &str)],
    rebuild_leases: bool,
    before_commit: impl FnOnce() -> ControlResult<()>,
) -> ControlResult<()> {
    let tx = c.transaction().map_err(|_| E::StorageUnavailable)?;
    if rebuild_leases {
        tx.execute_batch("DROP INDEX execution_leases_one_active; ALTER TABLE execution_leases RENAME TO execution_leases_old;")
            .map_err(|_| E::StorageUnavailable)?;
    }
    if version != 0 {
        tx.execute_batch("ALTER TABLE authority_metadata RENAME TO authority_metadata_old;")
            .map_err(|_| E::StorageUnavailable)?;
    }
    if version == 1 {
        tx.execute_batch("DROP TRIGGER control_events_no_update; DROP TRIGGER control_events_no_delete; ALTER TABLE control_events RENAME TO control_events_old;").map_err(|_| E::StorageUnavailable)?;
    }
    for (name, sql) in OBJECTS {
        if *name == "authority_metadata"
            || !old.iter().any(|(n, _)| n == name)
            || (rebuild_leases
                && ["execution_leases", "execution_leases_one_active"].contains(name))
            || (version == 1
                && [
                    "control_events",
                    "control_events_no_update",
                    "control_events_no_delete",
                ]
                .contains(name))
        {
            tx.execute_batch(sql).map_err(|_| E::StorageUnavailable)?;
        }
    }
    if version == 0 {
        tx.execute(
            "INSERT INTO authority_metadata VALUES(1,6,?1,'0')",
            [uuid::Uuid::new_v4().to_string()],
        )
        .map_err(|_| E::StorageUnavailable)?;
    } else {
        tx.execute_batch("INSERT INTO authority_metadata SELECT id,6,stream_id,last_sequence FROM authority_metadata_old; DROP TABLE authority_metadata_old;").map_err(|_| E::StorageUnavailable)?;
    }
    if version == 1 {
        tx.execute_batch("INSERT INTO control_events SELECT * FROM control_events_old; DROP TABLE control_events_old;").map_err(|_| E::StorageUnavailable)?;
    }
    if rebuild_leases {
        tx.execute_batch("INSERT INTO execution_leases SELECT * FROM execution_leases_old; DROP TABLE execution_leases_old;")
            .map_err(|_| E::StorageUnavailable)?;
    }
    tx.pragma_update(None, "user_version", 6)
        .map_err(|_| E::StorageUnavailable)?;
    before_commit()?;
    tx.commit().map_err(|_| E::StorageUnavailable)
}
#[cfg(test)]
pub(super) fn fail_migration(c: &mut Connection) -> ControlResult<()> {
    migrate_with(c, || Err(E::StorageUnavailable))
}
