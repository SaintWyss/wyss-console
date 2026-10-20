//! Post-migration schema-8 validator: a database which may hold legitimate new work.
//!
//! `migration::validate_migrated` deliberately refuses every nonempty V8 table. This validator
//! accepts new runtimes, launches, sidecars and generation-aware events, and refuses anything
//! the durable stream does not prove. It is read-only: it never repairs, migrates or opens a
//! path, and it validates a private in-memory copy for the canonical (baseline) verification.
use super::{
    audit::AuditReplay,
    migration::{clone_database, expected_v8, integrity, objects},
    protocol::{Request, Target, canonical},
};
use crate::{
    contracts::control::{self as v1, ControlError as E, ControlResult, unique_json},
    control::store::Store,
};
use rusqlite::Connection;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

type Replayed = BTreeMap<String, (String, Option<(String, u64)>)>;

#[derive(Debug, PartialEq, Eq)]
pub struct V8Summary {
    pub events: u64,
    pub runtimes: usize,
    pub launches: usize,
    pub assigned_turns: usize,
    pub assigned_approvals: usize,
    pub legacy_turns: usize,
}

fn invalid(_: impl std::fmt::Debug) -> E {
    E::StorageIncompatible
}
fn bad() -> E {
    E::StorageIncompatible
}

/// Full validation. Any refusal means the image must not be used; nothing is modified.
pub fn validate_v8(c: &Connection) -> ControlResult<V8Summary> {
    if c.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))
        .map_err(invalid)?
        != 8
        || objects(c)? != expected_v8()?
    {
        return Err(bad());
    }
    integrity(c)?;
    // Baseline canonical verification of sessions, leases, bindings, turns, approvals,
    // receipts and the legacy runtime, on a private copy so the caller's connection is untouched.
    Store::validate_offline_connection_v8(clone_database(c)?)?;

    let mut replay = AuditReplay::default();
    let mut events = 0u64;
    {
        let mut q = c
            .prepare("SELECT envelope FROM control_events ORDER BY sequence")
            .map_err(invalid)?;
        let mut rows = q.query([]).map_err(invalid)?;
        while let Some(r) = rows.next().map_err(invalid)? {
            let raw: String = r.get(0).map_err(invalid)?;
            let v = unique_json(raw.as_bytes()).map_err(invalid)?;
            replay.observe(&v)?;
            events += 1;
        }
    }
    let head: String = c
        .query_row(
            "SELECT last_sequence FROM authority_metadata WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(invalid)?;
    if head.parse::<u64>().map_err(invalid)? != events || replay.head() != events {
        return Err(bad());
    }

    // Runtime rows are exactly what the stream says, one per session, never deleted.
    let mut rows: BTreeMap<String, (u64, bool, u64)> = BTreeMap::new();
    {
        let mut q = c
            .prepare(
                "SELECT managed_session_id,desired_state,revision,generation FROM managed_runtimes",
            )
            .map_err(invalid)?;
        let mut r = q.query([]).map_err(invalid)?;
        while let Some(row) = r.next().map_err(invalid)? {
            let session: String = row.get(0).map_err(invalid)?;
            let desired: String = row.get(1).map_err(invalid)?;
            let revision: i64 = row.get(2).map_err(invalid)?;
            let generation: i64 = row.get(3).map_err(invalid)?;
            canonical(&session)?;
            rows.insert(
                session,
                (
                    u64::try_from(revision).map_err(invalid)?,
                    desired == "running",
                    u64::try_from(generation).map_err(invalid)?,
                ),
            );
        }
    }
    if rows != replay.runtimes() {
        return Err(bad());
    }

    // Allocation history: exact set, authority attribution, contiguous generations 1..=current.
    let mut launches: BTreeMap<String, (String, u64, String)> = BTreeMap::new();
    {
        let mut q = c
            .prepare("SELECT runtime_instance_id,managed_session_id,generation,authority_instance_id FROM managed_runtime_launches")
            .map_err(invalid)?;
        let mut r = q.query([]).map_err(invalid)?;
        while let Some(row) = r.next().map_err(invalid)? {
            let generation: i64 = row.get(2).map_err(invalid)?;
            launches.insert(
                row.get(0).map_err(invalid)?,
                (
                    row.get(1).map_err(invalid)?,
                    u64::try_from(generation).map_err(invalid)?,
                    row.get(3).map_err(invalid)?,
                ),
            );
        }
    }
    if launches != replay.launches() {
        return Err(bad());
    }
    let mut per_session: BTreeMap<&str, BTreeSet<u64>> = BTreeMap::new();
    for (session, generation, _) in launches.values() {
        per_session.entry(session).or_default().insert(*generation);
    }
    for (session, (_, _, generation)) in &rows {
        let expected: BTreeSet<u64> = (1..=*generation).collect();
        if per_session.remove(session.as_str()).unwrap_or_default() != expected {
            return Err(bad());
        }
    }
    if !per_session.is_empty() {
        return Err(bad());
    }

    let assigned_turns = sidecars(
        c,
        "managed_turns",
        "managed_turn_id",
        "managed_turn_launches",
        &replay.turn_launches(),
    )?;
    let approval_map = replay
        .approval_launches()
        .into_iter()
        .map(|(id, (s, _, l))| (id, (s, l)))
        .collect();
    let assigned_approvals = sidecars(
        c,
        "managed_approvals",
        "managed_approval_id",
        "managed_approval_launches",
        &approval_map,
    )?;
    // An approval's sidecar names the same turn as the approval itself.
    let mismatched: i64 = c
        .query_row(
            "SELECT count(*) FROM managed_approval_launches l JOIN managed_approvals a ON a.managed_approval_id=l.managed_approval_id WHERE a.managed_turn_id<>l.managed_turn_id",
            [],
            |r| r.get(0),
        )
        .map_err(invalid)?;
    if mismatched != 0 {
        return Err(bad());
    }
    operations(c, &launches)?;
    runtime_receipts(c, &replay)?;
    Ok(V8Summary {
        events,
        runtimes: rows.len(),
        launches: launches.len(),
        assigned_turns,
        assigned_approvals,
        legacy_turns: replay
            .turn_launches()
            .values()
            .filter(|(_, l)| l.is_none())
            .count(),
    })
}

/// Parent rows and sidecars correspond one to one with the stream's attribution: a sidecar
/// exists exactly for generation-aware records and repeats the same session, instance and
/// generation; unassigned baseline records never gain one.
fn sidecars(
    c: &Connection,
    table: &str,
    key: &str,
    sidecar: &str,
    replayed: &Replayed,
) -> ControlResult<usize> {
    let mut parents: BTreeMap<String, (String, String)> = BTreeMap::new();
    {
        let mut q = c
            .prepare(&format!(
                "SELECT {key},managed_session_id,runtime_instance_id FROM {table}"
            ))
            .map_err(invalid)?;
        let mut r = q.query([]).map_err(invalid)?;
        while let Some(row) = r.next().map_err(invalid)? {
            parents.insert(
                row.get(0).map_err(invalid)?,
                (row.get(1).map_err(invalid)?, row.get(2).map_err(invalid)?),
            );
        }
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut q = c
        .prepare(&format!(
            "SELECT {key},managed_session_id,runtime_instance_id,generation FROM {sidecar}"
        ))
        .map_err(invalid)?;
    let mut r = q.query([]).map_err(invalid)?;
    while let Some(row) = r.next().map_err(invalid)? {
        let id: String = row.get(0).map_err(invalid)?;
        let session: String = row.get(1).map_err(invalid)?;
        let instance: String = row.get(2).map_err(invalid)?;
        let generation = u64::try_from(row.get::<_, i64>(3).map_err(invalid)?).map_err(invalid)?;
        if parents.get(&id) != Some(&(session.clone(), instance.clone()))
            || replayed.get(&id) != Some(&(session, Some((instance, generation))))
        {
            return Err(bad());
        }
        seen.insert(id);
    }
    // Every parent is either assigned (sidecar present) or unassigned baseline history.
    for (id, (session, launch)) in replayed {
        if parents.get(id).map(|(s, _)| s) != Some(session) || launch.is_some() != seen.contains(id)
        {
            return Err(bad());
        }
    }
    if parents.len() != replayed.len() {
        return Err(bad());
    }
    Ok(seen.len())
}

fn operations(
    c: &Connection,
    launches: &BTreeMap<String, (String, u64, String)>,
) -> ControlResult<()> {
    let mut q = c
        .prepare("SELECT l.runtime_instance_id,l.managed_session_id,l.generation,o.managed_session_id FROM managed_operation_launches l JOIN managed_session_operations o ON o.operation_id=l.operation_id")
        .map_err(invalid)?;
    let mut r = q.query([]).map_err(invalid)?;
    while let Some(row) = r.next().map_err(invalid)? {
        let instance: String = row.get(0).map_err(invalid)?;
        let session: String = row.get(1).map_err(invalid)?;
        let generation = u64::try_from(row.get::<_, i64>(2).map_err(invalid)?).map_err(invalid)?;
        let owner: String = row.get(3).map_err(invalid)?;
        if session != owner
            || launches
                .get(&instance)
                .is_none_or(|l| l.0 != session || l.1 != generation)
        {
            return Err(bad());
        }
    }
    let total: i64 = c
        .query_row("SELECT count(*) FROM managed_operation_launches", [], |r| {
            r.get(0)
        })
        .map_err(invalid)?;
    let joined: i64 = c
        .query_row(
            "SELECT count(*) FROM managed_operation_launches l JOIN managed_session_operations o ON o.operation_id=l.operation_id",
            [],
            |r| r.get(0),
        )
        .map_err(invalid)?;
    if total != joined {
        return Err(bad());
    }
    Ok(())
}

/// Schema-8 runtime receipts: the canonical intent plus the session/generation it produced.
/// Receipts predating the migration keep their original bytes and shape; both must be
/// exactly justified, and each generation-aware mutation event has exactly one receipt.
fn runtime_receipts(c: &Connection, replay: &AuditReplay) -> ControlResult<()> {
    let mut q = c
        .prepare("SELECT intent_id,mutation,result FROM processed_intents")
        .map_err(invalid)?;
    let mut r = q.query([]).map_err(invalid)?;
    let mut matched = BTreeSet::new();
    let launches = replay.launches();
    while let Some(row) = r.next().map_err(invalid)? {
        let id: String = row.get(0).map_err(invalid)?;
        let mutation: String = row.get(1).map_err(invalid)?;
        let result: String = row.get(2).map_err(invalid)?;
        let m = unique_json(mutation.as_bytes()).map_err(invalid)?;
        if !matches!(m["method"].as_str(), Some("runtime/start" | "runtime/stop")) {
            continue;
        }
        let Some((session, revision, generation, running)) = replay.intents().get(&id) else {
            // Unassigned baseline receipt: original shape only.
            let op: v1::Operation = serde_json::from_value(m).map_err(invalid)?;
            op.validate().map_err(invalid)?;
            let record: v1::RuntimeControlRecord =
                serde_json::from_str(&result).map_err(invalid)?;
            record.validate()?;
            continue;
        };
        let envelope = serde_json::json!({
            "schema_version": 2,
            "request_id": id,
            "authority_instance_id": "ffffffff-ffff-4fff-8fff-ffffffffffff",
            "method": m["method"],
            "params": m["params"],
        });
        let request = Request::parse(&serde_json::to_vec(&envelope).map_err(invalid)?)?;
        let (mutation, start) = match request.operation() {
            v1::Operation::RuntimeStart(p) => (p, true),
            v1::Operation::RuntimeStop(p) => (p, false),
            _ => return Err(bad()),
        };
        let Target::Runtime {
            session: target,
            generation: Some(expected_generation),
            instance,
        } = request.target()
        else {
            return Err(bad());
        };
        let consistent_generation = if start {
            expected_generation == generation
                || expected_generation.checked_add(1) == Some(*generation)
        } else {
            expected_generation == generation
        };
        if mutation.intent_id.as_str() != id
            || target.as_str() != session
            || *running != start
            || !consistent_generation
            || mutation.expected_revision.and_then(|n| n.checked_add(1)) != Some(*revision)
            || instance
                .as_ref()
                .is_some_and(|i| launches.get(i.as_str()).is_none_or(|l| &l.0 != session))
        {
            return Err(bad());
        }
        let mut value: Value = unique_json(result.as_bytes()).map_err(invalid)?;
        let o = value.as_object_mut().ok_or_else(bad)?;
        if o.remove("managed_session_id")
            .as_ref()
            .and_then(Value::as_str)
            != Some(session)
            || o.remove("generation").and_then(|g| g.as_u64()) != Some(*generation)
        {
            return Err(bad());
        }
        let record: v1::RuntimeControlRecord = serde_json::from_value(value).map_err(invalid)?;
        record.validate()?;
        if record.revision != *revision
            || (record.desired_state == v1::DesiredState::Running) != start
        {
            return Err(bad());
        }
        matched.insert(id);
    }
    if matched.len() != replay.intents().len() {
        return Err(bad());
    }
    Ok(())
}

/// Runs `work` inside one IMMEDIATE transaction and commits only when the complete resulting
/// image validates. Any error or validation failure rolls back every row, including events.
/// The full validation is O(database); this is the offline/migration discipline, not a hot path.
pub fn commit_validated<T>(
    c: &mut Connection,
    work: impl FnOnce(&rusqlite::Transaction<'_>) -> ControlResult<T>,
) -> ControlResult<T> {
    let tx = c
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|_| E::StorageUnavailable)?;
    let out = work(&tx)?;
    validate_v8(&tx)?;
    tx.commit().map_err(|_| E::StorageUnavailable)?;
    Ok(out)
}

#[cfg(test)]
#[path = "v8_tests.rs"]
mod tests;
