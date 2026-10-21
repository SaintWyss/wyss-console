//! Offline migration preparation. The caller's database is a read-only source snapshot.
//! No path, installed-store opener, publication or authority lifecycle API exists here.
use crate::{
    contracts::control::{ControlError as E, ControlResult},
    control::{schema, store::Store},
};
use rusqlite::{Connection, TransactionBehavior, params_from_iter, types::Value};
use std::collections::BTreeMap;
const SQL: &str = include_str!("../../../docs/contracts/f8/v7-to-v8.sql");

/// A fully validated immediate-migration image. This API cannot publish it to an installation.
/// The validator deliberately refuses post-migration V8 work until its event replay is implemented.
pub struct PreparedMigration {
    connection: Connection,
}
impl PreparedMigration {
    pub fn validate(&self) -> ControlResult<()> {
        validate_migrated(&self.connection)
    }
    /// The image, for tests and for the development-only local application.
    #[cfg(any(test, feature = "f8-dev"))]
    pub(super) fn into_connection(self) -> Connection {
        self.connection
    }
    pub fn metadata(&self) -> ControlResult<(String, String)> {
        self.connection
            .query_row(
                "SELECT stream_id,last_sequence FROM authority_metadata WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(invalid)
    }
}

pub fn prepare(source: &mut Connection) -> ControlResult<PreparedMigration> {
    prepare_with(source, |_| Ok(()))
}
/// Failure hook runs after every candidate SQL statement and after validation, before commit.
/// It is useful for fault injection; it never receives a connection or process capability.
pub fn prepare_with(
    source: &mut Connection,
    mut after_step: impl FnMut(usize) -> ControlResult<()>,
) -> ControlResult<PreparedMigration> {
    if !source.is_autocommit() {
        return Err(E::StorageIncompatible);
    }
    let attached: i64 = source
        .query_row(
            "SELECT count(*) FROM pragma_database_list WHERE name NOT IN ('main','temp')",
            [],
            |r| r.get(0),
        )
        .map_err(invalid)?;
    let temp: i64 = source
        .query_row("SELECT count(*) FROM sqlite_temp_master", [], |r| r.get(0))
        .map_err(invalid)?;
    if attached != 0 || temp != 0 {
        return Err(E::StorageIncompatible);
    }
    let snapshot = source
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(db)?;
    let original_version: i64 = snapshot
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(invalid)?;
    let expected = match original_version {
        0 => &[][..],
        1 => schema::V1_OBJECTS,
        2 => schema::V2_OBJECTS,
        3 => schema::V3_OBJECTS,
        4 => schema::V4_OBJECTS,
        5 => schema::V5_OBJECTS,
        6 => schema::V6_OBJECTS,
        7 => schema::OBJECTS,
        _ => return Err(E::StorageIncompatible),
    };
    validate_objects(&snapshot, expected)?;
    let mut image = clone_database(&snapshot)?;
    // The unchanged supported migration path is applied only to the private image.
    schema::migrate(&mut image)?;
    image = Store::validate_offline_connection(image)?;
    let before = rows(&image)?;
    let old_meta = before
        .get("authority_metadata")
        .ok_or(E::StorageIncompatible)?
        .clone();
    let old_runtime = before
        .get("managed_runtimes")
        .ok_or(E::StorageIncompatible)?
        .clone();
    image
        .pragma_update(None, "foreign_keys", true)
        .map_err(db)?;
    let tx = image
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    let statements = statements();
    for (i, sql) in statements.iter().enumerate() {
        tx.execute_batch(sql).map_err(invalid)?;
        after_step(i)?;
    }
    // The trust boundary is computed from the validated V7 image, then frozen by triggers.
    let last_v7 = match old_meta[0].get(3) {
        Some(Value::Text(t)) => t.clone(),
        _ => return Err(E::StorageIncompatible),
    };
    let stream_v7 = match old_meta[0].get(2) {
        Some(Value::Text(t)) => t.clone(),
        _ => return Err(E::StorageIncompatible),
    };
    let digest = boundary_digest(&tx, last_v7.parse().map_err(invalid)?)?;
    tx.execute(
        "INSERT INTO migration_boundary VALUES(1,?1,?2,?3)",
        rusqlite::params![last_v7, digest, stream_v7],
    )
    .map_err(invalid)?;
    after_step(statements.len())?;
    validate_migrated(&tx)?;
    let after = rows(&tx)?;
    for (table, data) in before {
        if table != "authority_metadata"
            && table != "managed_runtimes"
            && after.get(&table) != Some(&data)
        {
            return Err(E::StorageIncompatible);
        }
    }
    if after.get("managed_runtimes_legacy_v7") != Some(&old_runtime) {
        return Err(E::StorageIncompatible);
    }
    let new_meta = after
        .get("authority_metadata")
        .ok_or(E::StorageIncompatible)?;
    if old_meta.len() != 1 || new_meta.len() != 1 || old_meta[0][2..4] != new_meta[0][2..4] {
        return Err(E::StorageIncompatible);
    }
    after_step(statements.len() + 1)?;
    tx.commit().map_err(db)?;
    snapshot.rollback().map_err(db)?;
    Ok(PreparedMigration { connection: image })
}

/// SHA-256 over every event up to `upto` as `"<sequence key>\n<envelope>\n"`, in sequence order.
pub(super) fn boundary_digest(c: &Connection, upto: u64) -> ControlResult<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut q = c
        .prepare("SELECT sequence,envelope FROM control_events ORDER BY sequence")
        .map_err(invalid)?;
    let mut r = q.query([]).map_err(invalid)?;
    let mut n = 0u64;
    while let Some(row) = r.next().map_err(invalid)? {
        n += 1;
        if n > upto {
            break;
        }
        let key: String = row.get(0).map_err(invalid)?;
        let envelope: String = row.get(1).map_err(invalid)?;
        hasher.update(key.as_bytes());
        hasher.update(b"\n");
        hasher.update(envelope.as_bytes());
        hasher.update(b"\n");
    }
    if n < upto {
        return Err(E::StorageIncompatible);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
/// The recorded boundary, verified against the stored events: the digest of the V7 prefix must
/// match, so rewritten or truncated pre-migration history is refused. Returns the V7 head.
pub(super) fn verify_boundary(c: &Connection) -> ControlResult<u64> {
    let (last, digest, stream): (String, String, String) = c
        .query_row(
            "SELECT v7_last_sequence,v7_events_sha256,v7_stream_id FROM migration_boundary WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(invalid)?;
    let upto: u64 = last.parse().map_err(invalid)?;
    let current: String = c
        .query_row(
            "SELECT stream_id FROM authority_metadata WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(invalid)?;
    if stream != current || boundary_digest(c, upto)? != digest {
        return Err(E::StorageIncompatible);
    }
    Ok(upto)
}
/// Exact V8 schema, FK/integrity, canonical legacy records/receipts/audit and zero-new-work check.
/// NOT a validator for a V8 authority that has started accepting new work: it refuses that image.
pub fn validate_migrated(c: &Connection) -> ControlResult<()> {
    if c.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
        .map_err(invalid)?
        != 8
    {
        return Err(E::StorageIncompatible);
    }
    let expected = expected_v8()?;
    if objects(c)? != expected {
        return Err(E::StorageIncompatible);
    }
    integrity(c)?;
    for table in [
        "managed_runtimes",
        "managed_runtime_launches",
        "managed_turn_launches",
        "managed_approval_launches",
        "managed_operation_launches",
        "managed_approval_delivery",
    ] {
        if c.query_row::<i64, _, _>(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .map_err(invalid)?
            != 0
        {
            return Err(E::StorageIncompatible);
        }
    }
    let version: i64 = c
        .query_row(
            "SELECT schema_version FROM authority_metadata WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(invalid)?;
    let vocabulary: i64 = c
        .query_row(
            "SELECT event_vocabulary FROM authority_metadata WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(invalid)?;
    if version != 8 || vocabulary < 2 {
        return Err(E::StorageIncompatible);
    }
    // An immediately migrated image has no event beyond the boundary.
    let upto = verify_boundary(c)?;
    let head: String = c
        .query_row(
            "SELECT last_sequence FROM authority_metadata WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(invalid)?;
    if head.parse::<u64>().map_err(invalid)? != upto {
        return Err(E::StorageIncompatible);
    }
    // Reconstruct the old canonical view, keeping original event/receipt bytes. Only the metadata
    // version and table name differ. No historical launch identity or provider version is invented.
    let mut legacy = Connection::open_in_memory().map_err(db)?;
    schema::migrate(&mut legacy)?;
    legacy
        .pragma_update(None, "foreign_keys", false)
        .map_err(db)?;
    legacy
        .execute("DELETE FROM authority_metadata", [])
        .map_err(db)?;
    let all = rows(c)?;
    for (name, data) in all {
        if name == "managed_runtimes"
            || name == "migration_boundary"
            || [
                "managed_runtime_launches",
                "managed_turn_launches",
                "managed_approval_launches",
                "managed_operation_launches",
                "managed_approval_delivery",
            ]
            .contains(&name.as_str())
        {
            continue;
        }
        let table = if name == "managed_runtimes_legacy_v7" {
            "managed_runtimes"
        } else {
            &name
        };
        let mut data = data;
        if table == "authority_metadata" {
            for row in &mut data {
                row[1] = Value::Integer(7);
            }
        }
        insert_rows(&legacy, table, &data)?;
    }
    legacy
        .pragma_update(None, "foreign_keys", true)
        .map_err(db)?;
    Store::validate_offline_connection(legacy)?;
    Ok(())
}
pub(super) fn expected_v8() -> ControlResult<BTreeMap<String, String>> {
    let mut c = Connection::open_in_memory().map_err(db)?;
    schema::migrate(&mut c)?;
    c.execute_batch(SQL).map_err(invalid)?;
    objects(&c)
}
fn statements() -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for line in SQL.lines().filter(|l| !l.trim_start().starts_with("--")) {
        current.push_str(line);
        current.push('\n');
        if line.trim_end().ends_with(';') {
            out.push(std::mem::take(&mut current));
        }
    }
    assert!(
        current.trim().is_empty(),
        "embedded migration SQL statement boundary"
    );
    out
}
pub(super) fn clone_database(c: &Connection) -> ControlResult<Connection> {
    integrity(c)?;
    let copy = Connection::open_in_memory().map_err(db)?;
    copy.pragma_update(None, "foreign_keys", false)
        .map_err(db)?;
    let objects = objects(c)?;
    // Trusted exact schema was validated by the caller; tables before dependent indexes/triggers.
    for sql in objects.values().filter(|s| s.starts_with("CREATE TABLE")) {
        copy.execute_batch(sql).map_err(invalid)?;
    }
    for (name, data) in rows(c)? {
        insert_rows(&copy, &name, &data)?;
    }
    for sql in objects.values().filter(|s| !s.starts_with("CREATE TABLE")) {
        copy.execute_batch(sql).map_err(invalid)?;
    }
    let version: i64 = c
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(invalid)?;
    copy.pragma_update(None, "user_version", version)
        .map_err(db)?;
    copy.pragma_update(None, "foreign_keys", true).map_err(db)?;
    integrity(&copy)?;
    Ok(copy)
}
fn insert_rows(c: &Connection, name: &str, data: &[Vec<Value>]) -> ControlResult<()> {
    for row in data {
        let marks = vec!["?"; row.len()].join(",");
        c.execute(
            &format!("INSERT INTO \"{name}\" VALUES({marks})"),
            params_from_iter(row),
        )
        .map_err(invalid)?;
    }
    Ok(())
}
fn rows(c: &Connection) -> ControlResult<BTreeMap<String, Vec<Vec<Value>>>> {
    let mut result = BTreeMap::new();
    for (name, sql) in objects(c)? {
        if !sql.starts_with("CREATE TABLE") {
            continue;
        }
        let mut q = c
            .prepare(&format!("SELECT * FROM \"{name}\" ORDER BY rowid"))
            .map_err(invalid)?;
        let count = q.column_count();
        let data = q
            .query_map([], |r| {
                (0..count)
                    .map(|i| r.get::<_, Value>(i))
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(invalid)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(invalid)?;
        result.insert(name, data);
    }
    Ok(result)
}
pub(super) fn objects(c: &Connection) -> ControlResult<BTreeMap<String, String>> {
    let mut q=c.prepare("SELECT name,sql FROM sqlite_master WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' ORDER BY name").map_err(invalid)?;
    q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(invalid)?
        .collect::<Result<_, _>>()
        .map_err(invalid)
}
fn validate_objects(c: &Connection, expected: &[(&str, &str)]) -> ControlResult<()> {
    let expected = expected
        .iter()
        .map(|(n, s)| (n.to_string(), s.to_string()))
        .collect();
    if objects(c)? != expected {
        return Err(E::StorageIncompatible);
    }
    integrity(c)
}
pub(super) fn integrity(c: &Connection) -> ControlResult<()> {
    let quick: String = c
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .map_err(invalid)?;
    let foreign: i64 = c
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .map_err(invalid)?;
    if quick != "ok" || foreign != 0 {
        Err(E::StorageIncompatible)
    } else {
        Ok(())
    }
}
fn db(_: impl std::fmt::Debug) -> E {
    E::StorageUnavailable
}
fn invalid(_: impl std::fmt::Debug) -> E {
    E::StorageIncompatible
}

#[cfg(test)]
#[path = "migration_tests.rs"]
mod tests;
