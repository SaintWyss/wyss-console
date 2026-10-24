//! A rehearsal of the V8→V9 store migration that ADR0055 proposes. **Test builds only.**
//!
//! Nothing here opens a path, an installed store or an authority lifecycle: it works on private
//! in-memory images made from a synthetic V8 store, exactly as `migration` does for V7→V8, and it
//! is compiled only into test builds, so no packaged build carries or advertises V9. Its job is to
//! turn the ADR's list into something that can fail: every constraint, trigger and index that has
//! to be rebuilt, the metadata and boundary that have to move, the invariants the migration must
//! preserve, what an interruption at each stage leaves behind, and what a rollback is allowed to be.
//!
//! The candidate, in the order it runs on a private clone of a validated V8 image (foreign keys off
//! and the legacy rename rule on, as `control::schema` does for its table rebuilds):
//!
//! 1. each of `managed_runtimes`, `managed_runtime_launches`, `managed_session_bindings`,
//!    `authority_metadata` is renamed away, recreated from its V8 definition with only the Codex-only
//!    CHECKs widened, refilled `INSERT … SELECT *`, and its old copy dropped; the triggers and
//!    indexes that lived on it are recreated unchanged (they die with the old table);
//! 2. a second boundary, `migration_boundary_v9`, freezes the digest of every V8 event;
//! 3. the metadata moves to schema 9, and `PRAGMA user_version=9` is the last statement.
//!
//! The widened CHECKs: `adapter_id IN ('codex','claude')` and `runtime_kind=adapter_id`.
use super::{
    migration::{
        boundary_digest, clone_database, expected_v8, insert_rows, integrity, objects, rows,
    },
    v8::{V8Summary, validate_v8},
};
use crate::contracts::control::{ControlError as E, ControlResult};
use rusqlite::{Connection, TransactionBehavior, types::Value};
use std::collections::BTreeMap;

/// The tables whose Codex-only CHECKs are widened.
const WIDENED: [&str; 3] = [
    "managed_runtimes",
    "managed_runtime_launches",
    "managed_session_bindings",
];
/// The table whose version CHECK moves from 8 to 9.
const METADATA: &str = "authority_metadata";

const BOUNDARY_V9: [&str; 3] = [
    "CREATE TABLE migration_boundary_v9 (id INTEGER PRIMARY KEY CHECK(id=1), v8_last_sequence TEXT NOT NULL CHECK(length(v8_last_sequence) BETWEEN 1 AND 20), v8_events_sha256 TEXT NOT NULL CHECK(length(v8_events_sha256)=64), v8_stream_id TEXT NOT NULL) STRICT",
    "CREATE TRIGGER migration_boundary_v9_no_update BEFORE UPDATE ON migration_boundary_v9 BEGIN SELECT RAISE(ABORT,'append_only'); END",
    "CREATE TRIGGER migration_boundary_v9_no_delete BEFORE DELETE ON migration_boundary_v9 BEGIN SELECT RAISE(ABORT,'append_only'); END",
];

/// What the candidate changes in one table's definition, or `Err` if the V8 text is not what it
/// was built from (so a V8 schema that moved cannot be silently migrated by a stale rehearsal).
pub fn widen(name: &str, sql: &str) -> ControlResult<String> {
    let swap = |text: &str, old: &str, new: &str| -> ControlResult<String> {
        if text.matches(old).count() != 1 {
            return Err(E::StorageIncompatible);
        }
        Ok(text.replacen(old, new, 1))
    };
    if WIDENED.contains(&name) {
        let step = swap(
            sql,
            "CHECK(adapter_id='codex')",
            "CHECK(adapter_id IN ('codex','claude'))",
        )?;
        swap(
            &step,
            "CHECK(runtime_kind='codex')",
            "CHECK(runtime_kind=adapter_id)",
        )
    } else if name == METADATA {
        swap(sql, "CHECK(schema_version=8)", "CHECK(schema_version=9)")
    } else {
        Ok(sql.to_owned())
    }
}

/// The exact V9 schema, derived from the V8 one by text, independently of how the migration gets
/// there: what the migration produced is checked against this.
pub fn expected_v9() -> ControlResult<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for (name, sql) in expected_v8()? {
        let sql = widen(&name, &sql)?;
        out.insert(name, sql);
    }
    for sql in BOUNDARY_V9 {
        let name = sql
            .split_whitespace()
            .nth(2)
            .ok_or(E::StorageIncompatible)?;
        out.insert(name.to_owned(), sql.to_owned());
    }
    Ok(out)
}

/// The migration's stages, for saying where an interruption happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// The i-th statement of the rebuild ran.
    Statement(usize),
    /// The V8 boundary row was inserted.
    Boundary,
    /// The image was validated, just before it is handed back.
    Validated,
}

/// The statements that rebuild the affected tables, read off the V8 image itself so that nothing
/// the V8 definition has (a trigger, an index, a column) can be forgotten.
fn rebuild_statements(c: &Connection) -> ControlResult<Vec<String>> {
    let mut out = Vec::new();
    for table in WIDENED.iter().copied().chain([METADATA]) {
        let old: String = c
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get(0),
            )
            .map_err(invalid)?;
        let mut q = c
            .prepare(
                "SELECT sql FROM sqlite_master WHERE tbl_name=?1 AND type IN ('index','trigger') \
                 AND sql IS NOT NULL ORDER BY name",
            )
            .map_err(invalid)?;
        let dependents: Vec<String> = q
            .query_map([table], |r| r.get(0))
            .map_err(invalid)?
            .collect::<Result<_, _>>()
            .map_err(invalid)?;
        out.push(format!("ALTER TABLE {table} RENAME TO {table}_v8_old"));
        out.push(widen(table, &old)?);
        out.push(if table == METADATA {
            // The one value that changes on the way across: the schema version.
            format!(
                "INSERT INTO {table} SELECT id,9,stream_id,last_sequence,event_vocabulary FROM {table}_v8_old"
            )
        } else {
            format!("INSERT INTO {table} SELECT * FROM {table}_v8_old")
        });
        out.push(format!("DROP TABLE {table}_v8_old"));
        out.extend(dependents);
    }
    out.extend(BOUNDARY_V9.iter().map(|s| (*s).to_owned()));
    Ok(out)
}

/// A V9 image made from a validated V8 one, or an error that leaves nothing behind: the V8 source
/// is only read, the work happens on a private clone, and the clone is returned only after the V9
/// validator accepts it. `after` is called at every stage and may refuse, to rehearse an
/// interruption there.
pub fn migrate(
    v8: &Connection,
    mut after: impl FnMut(Stage) -> ControlResult<()>,
) -> ControlResult<Connection> {
    validate_v8(v8)?;
    if !quiescent(v8)? {
        return Err(E::StorageIncompatible);
    }
    let mut image = clone_database(v8)?;
    image
        .execute_batch("PRAGMA foreign_keys=OFF; PRAGMA legacy_alter_table=ON;")
        .map_err(db)?;
    let tx = image
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    let statements = rebuild_statements(&tx)?;
    for (i, sql) in statements.iter().enumerate() {
        tx.execute_batch(sql).map_err(invalid)?;
        after(Stage::Statement(i))?;
    }
    let (last, stream): (String, String) = tx
        .query_row(
            "SELECT last_sequence,stream_id FROM authority_metadata WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(invalid)?;
    let digest = boundary_digest(&tx, last.parse().map_err(invalid)?)?;
    tx.execute(
        "INSERT INTO migration_boundary_v9 VALUES(1,?1,?2,?3)",
        rusqlite::params![last, digest, stream],
    )
    .map_err(invalid)?;
    after(Stage::Boundary)?;
    tx.execute_batch("PRAGMA user_version=9").map_err(invalid)?;
    tx.commit().map_err(db)?;
    image
        .execute_batch("PRAGMA legacy_alter_table=OFF; PRAGMA foreign_keys=ON;")
        .map_err(db)?;
    validate_v9(&image)?;
    after(Stage::Validated)?;
    Ok(image)
}

/// Exact V9 schema, integrity, boundary, and the V8 verifier's own opinion of the data.
pub fn validate_v9(c: &Connection) -> ControlResult<V8Summary> {
    if c.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
        .map_err(invalid)?
        != 9
        || objects(c)? != expected_v9()?
    {
        return Err(E::StorageIncompatible);
    }
    integrity(c)?;
    let version: i64 = c
        .query_row(
            "SELECT schema_version FROM authority_metadata WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(invalid)?;
    if version != 9 {
        return Err(E::StorageIncompatible);
    }
    // The frozen V8 history is still exactly what was frozen.
    let (last, digest, stream): (String, String, String) = c
        .query_row(
            "SELECT v8_last_sequence,v8_events_sha256,v8_stream_id FROM migration_boundary_v9 WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(invalid)?;
    let current: String = c
        .query_row(
            "SELECT stream_id FROM authority_metadata WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(invalid)?;
    if stream != current || boundary_digest(c, last.parse().map_err(invalid)?)? != digest {
        return Err(E::StorageIncompatible);
    }
    // Data and history, judged by the unchanged V8 verifier on the V8 view of this image.
    validate_v8(&v8_view(c)?)
}

/// Whether a row of a rebuilt table names a provider other than Codex.
fn has_non_codex_rows(c: &Connection) -> ControlResult<bool> {
    for table in WIDENED {
        let n: i64 = c
            .query_row(
                &format!(
                    "SELECT count(*) FROM {table} WHERE adapter_id<>'codex' OR runtime_kind<>'codex'"
                ),
                [],
                |r| r.get(0),
            )
            .map_err(invalid)?;
        if n != 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The V8 image this V9 one is the same as, or an error if it is not the same as one: refused when
/// any row names a provider V8 cannot hold. This is the whole of what an offline rollback is: it
/// exists only while the store holds nothing V8 could not have written.
pub fn v8_view(v9: &Connection) -> ControlResult<Connection> {
    if has_non_codex_rows(v9)? {
        return Err(E::StorageIncompatible);
    }
    let view = Connection::open_in_memory().map_err(db)?;
    view.pragma_update(None, "foreign_keys", false)
        .map_err(db)?;
    let schema = expected_v8()?;
    // Tables, then their rows, then the indexes and triggers that would refuse those rows.
    for sql in schema.values().filter(|s| s.starts_with("CREATE TABLE")) {
        view.execute_batch(sql).map_err(invalid)?;
    }
    for (name, mut data) in rows(v9)? {
        if name == "migration_boundary_v9" {
            continue;
        }
        if name == METADATA {
            for row in &mut data {
                row[1] = Value::Integer(8);
            }
        }
        insert_rows(&view, &name, &data)?;
    }
    for sql in schema.values().filter(|s| !s.starts_with("CREATE TABLE")) {
        view.execute_batch(sql).map_err(invalid)?;
    }
    view.pragma_update(None, "user_version", 8).map_err(db)?;
    view.pragma_update(None, "foreign_keys", true).map_err(db)?;
    integrity(&view)?;
    Ok(view)
}

/// The migration is offline and needs nothing in flight, as the V7→V8 one did.
pub fn quiescent(c: &Connection) -> ControlResult<bool> {
    let n: i64 = c
        .query_row(
            "SELECT \
               (SELECT count(*) FROM managed_turns WHERE state IN ('starting','running')) \
             + (SELECT count(*) FROM managed_approvals WHERE state='pending') \
             + (SELECT count(*) FROM execution_leases WHERE state='active') \
             + (SELECT count(*) FROM managed_session_operations WHERE state IN ('reserved','dispatching')) \
             + (SELECT count(*) FROM managed_runtimes WHERE desired_state='running')",
            [],
            |r| r.get(0),
        )
        .map_err(invalid)?;
    Ok(n == 0)
}

fn db(_: impl std::fmt::Debug) -> E {
    E::StorageUnavailable
}
fn invalid(_: impl std::fmt::Debug) -> E {
    E::StorageIncompatible
}
