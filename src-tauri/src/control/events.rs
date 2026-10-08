//! The durable event runtime: the single append path, event classification, the read-only
//! consistency projection and bounded replay.
//!
//! The stream is audit, history and observation. Canonical authority state lives in the
//! control tables and never derives from an event: nothing in this module starts, grants,
//! answers or repeats anything.
use super::*;
use crate::contracts::event_envelope::Persistence;
use std::collections::HashMap;

/// Largest encoded durable envelope. It equals what the schema's length check admits for
/// ASCII content and is twice the largest event of the schema v6 corpus (1024 bytes), so a
/// replay page always carries at least thirty events.
pub(super) const MAX_EVENT_BYTES: usize = 2048;
/// Bytes of one response frame reserved for everything that is not an event.
const PAGE_OVERHEAD: usize = 1024;
/// Encoded events one replay page may carry besides the count limit.
pub(super) const PAGE_BYTES: usize = MAX_FRAME - PAGE_OVERHEAD;
// A page of events at the append bound always fits one frame, thirty at the very least.
const _: () = assert!(PAGE_BYTES / (MAX_EVENT_BYTES + 1) >= 30);

/// Every event type this authority writes and can hold to a payload contract.
const KNOWN: [&str; 28] = [
    "authority.managed_session.prepared",
    "authority.managed_session.closed",
    "authority.managed_session.runtime_bound",
    "authority.managed_session.runtime_attached",
    "authority.managed_session.runtime_operation_abandoned",
    "authority.managed_session.runtime_operation_indeterminate",
    "authority.runtime.desired_state_changed",
    "authority.runtime.instance_ready",
    "authority.runtime.instance_exited",
    "authority.execution_lease.granted",
    "authority.execution_lease.renewed",
    "authority.execution_lease.revoked",
    "authority.execution_lease.expired",
    "authority.managed_turn.admitted",
    "authority.managed_turn.running",
    "authority.managed_turn.completed",
    "authority.managed_turn.interrupted",
    "authority.managed_turn.failed",
    "authority.managed_turn.indeterminate",
    "authority.managed_turn.cancel_requested",
    "authority.managed_approval.requested",
    "authority.managed_approval.accepted",
    "authority.managed_approval.declined",
    "authority.managed_approval.cancelled",
    "authority.managed_approval.expired",
    "authority.managed_approval.invalidated",
    "authority.managed_approval.indeterminate",
    "authority.managed_approval.confirmed",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Class {
    /// Written by this authority: its payload contract is enforced.
    Known,
    /// A valid envelope of a type this build does not define: an observation without
    /// effect. It is replayed as stored and never read as one of the known types.
    Unknown,
}
pub(super) fn classify(kind: &str) -> Class {
    if KNOWN.contains(&kind) {
        Class::Known
    } else {
        Class::Unknown
    }
}

#[cfg(not(test))]
const fn event_bytes() -> usize {
    MAX_EVENT_BYTES
}
#[cfg(test)]
thread_local! {
    /// No contract-valid event of this build reaches the bound, so its tests lower it.
    pub(super) static EVENT_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(MAX_EVENT_BYTES) };
}
#[cfg(test)]
fn event_bytes() -> usize {
    EVENT_BYTES.with(std::cell::Cell::get)
}

/// One durable event before it has a sequence. Identifiers are canonicalized on append.
pub(super) struct Draft<'a> {
    pub kind: &'a str,
    pub session: Option<&'a str>,
    pub correlation: Option<&'a str>,
    pub timestamp: String,
    pub payload: Value,
}

/// The only writer of `control_events`. It runs inside the caller's transaction, so the
/// event commits or rolls back together with the canonical mutation it describes and a
/// rollback never leaves a sequence gap: the head only moves with the row it numbers.
pub(super) fn append(
    tx: &Transaction<'_>,
    instance: &WireUuid,
    stream: &WireUuid,
    d: Draft<'_>,
) -> ControlResult<u64> {
    // The authority never appends a type whose contract it cannot enforce.
    if classify(d.kind) != Class::Known {
        return Err(ControlError::InternalError);
    }
    let n = next_sequence(tx)?;
    let id = uuid();
    let session = d.session.map(canonical);
    let value = json!({"schema_version":1,"event_id":id,"event_type":d.kind,"payload_schema_version":1,
        "timestamp":d.timestamp,"stream_id":stream,"sequence":n.to_string(),"session_id":session,
        "source":{"component":"wyss_control","instance_id":instance},
        "correlation_id":d.correlation.map(canonical),"causation_id":null,"payload":d.payload,
        "persistence":"durable","sensitivity":"internal","redaction":"none"});
    validate_audit(&value)?;
    let event = EventEnvelopeV1::from_value(value).map_err(invalid)?;
    let raw = serde_json::to_string(&event).map_err(db)?;
    // Durable security metadata is never truncated: an oversized event fails its mutation.
    if raw.len() > event_bytes() {
        return Err(ControlError::CapacityExceeded);
    }
    tx.execute(
        "INSERT INTO control_events VALUES(?1,?2,?3,?4,?5)",
        params![id.as_str(), stream.as_str(), key(n), session, raw],
    )
    .map_err(db)?;
    tx.execute(
        "UPDATE authority_metadata SET last_sequence=?1 WHERE id=1",
        [n.to_string()],
    )
    .map_err(db)?;
    Ok(n)
}

/// Decodes one stored envelope. Duplicate members, a malformed envelope or a non-durable
/// event in the durable log are corruption.
pub(super) fn decode(raw: &str) -> ControlResult<(Value, EventEnvelopeV1)> {
    let value = unique_json(raw.as_bytes()).map_err(invalid)?;
    let event = EventEnvelopeV1::from_value(value.clone()).map_err(invalid)?;
    if event.persistence() != Persistence::Durable {
        return Err(ControlError::StorageIncompatible);
    }
    Ok((value, event))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Head {
    pub state: String,
    pub revision: u64,
}
/// A deterministic in-memory view of what the durable stream says happened. It is a
/// diagnostic: it grants nothing, executes nothing and is rebuilt from sequence 1 every
/// time. Where it disagrees with the canonical tables the store is refused, never repaired.
#[derive(Default)]
pub(super) struct Replay {
    pub sessions: HashMap<String, Head>,
    pub leases: HashMap<String, Head>,
    pub turns: HashMap<String, Head>,
    pub approvals: approvals::AuditHeads,
    /// Session to the provider thread its binding event recorded.
    pub bound: HashMap<String, String>,
    /// Operations the log declared uncertain, and whether they were later abandoned.
    pub uncertain: HashMap<String, bool>,
    pub runtime: Option<Head>,
    pub known: u64,
    pub unknown: u64,
}
fn text(v: &Value) -> ControlResult<String> {
    v.as_str()
        .map(str::to_owned)
        .ok_or(ControlError::StorageIncompatible)
}
fn number(v: &Value) -> ControlResult<u64> {
    v.as_u64().ok_or(ControlError::StorageIncompatible)
}
/// Advances one entity by exactly one revision, starting at revision 1.
fn advance(
    heads: &mut HashMap<String, Head>,
    id: String,
    state: String,
    revision: u64,
) -> ControlResult<()> {
    let expected = heads.get(&id).map_or(1, |h| h.revision + 1);
    if revision != expected {
        return Err(ControlError::StorageIncompatible);
    }
    heads.insert(id, Head { state, revision });
    Ok(())
}
impl Replay {
    /// Folds one decoded envelope. An unknown future event is counted and nothing else.
    pub(super) fn observe(&mut self, v: &Value) -> ControlResult<()> {
        let kind = text(&v["event_type"])?;
        if classify(&kind) == Class::Unknown {
            self.unknown += 1;
            return Ok(());
        }
        validate_audit(v)?;
        self.known += 1;
        let p = &v["payload"];
        let session = v["session_id"].as_str().map(str::to_owned);
        let Some(name) = kind.rsplit('.').next() else {
            return Err(ControlError::StorageIncompatible);
        };
        if kind.starts_with("authority.managed_turn.") {
            let id = text(&p["managed_turn_id"])?;
            if name == "cancel_requested" {
                // A recorded intent: it must describe the turn as the log last left it.
                return match self.turns.get(&id) {
                    Some(h)
                        if h.revision == number(&p["revision"])?
                            && h.state == text(&p["state"])? =>
                    {
                        Ok(())
                    }
                    _ => Err(ControlError::StorageIncompatible),
                };
            }
            return advance(
                &mut self.turns,
                id,
                text(&p["state"])?,
                number(&p["revision"])?,
            );
        }
        if kind.starts_with("authority.managed_approval.") {
            return approvals::fold_audit(&mut self.approvals, v);
        }
        if kind.starts_with("authority.execution_lease.") {
            return advance(
                &mut self.leases,
                text(&p["lease_id"])?,
                text(&p["lease_state"])?,
                number(&p["revision"])?,
            );
        }
        if kind.starts_with("authority.runtime.") {
            if name == "desired_state_changed" {
                let revision = number(&p["revision"])?;
                if revision != self.runtime.as_ref().map_or(1, |h| h.revision + 1) {
                    return Err(ControlError::StorageIncompatible);
                }
                self.runtime = Some(Head {
                    state: text(&p["desired_state"])?,
                    revision,
                });
            }
            return Ok(());
        }
        let session = session.ok_or(ControlError::StorageIncompatible)?;
        match name {
            "prepared" | "closed" => advance(
                &mut self.sessions,
                session,
                text(&p["control_state"])?,
                number(&p["revision"])?,
            ),
            "runtime_bound" => {
                if !self.sessions.contains_key(&session)
                    || self
                        .bound
                        .insert(session, text(&p["runtime_session_id"])?)
                        .is_some()
                {
                    return Err(ControlError::StorageIncompatible);
                }
                Ok(())
            }
            "runtime_attached" => {
                if self.bound.get(&session) != Some(&text(&p["runtime_session_id"])?) {
                    return Err(ControlError::StorageIncompatible);
                }
                Ok(())
            }
            "runtime_operation_indeterminate" => {
                if self
                    .uncertain
                    .insert(text(&p["operation_id"])?, false)
                    .is_some()
                {
                    return Err(ControlError::StorageIncompatible);
                }
                Ok(())
            }
            "runtime_operation_abandoned" => {
                // Stores written before the uncertainty event existed abandon an operation
                // the log never announced; one that was announced is abandoned at most once.
                match self.uncertain.insert(text(&p["operation_id"])?, true) {
                    Some(true) => Err(ControlError::StorageIncompatible),
                    _ => Ok(()),
                }
            }
            _ => Err(ControlError::StorageIncompatible),
        }
    }
}

impl Store {
    /// The committed durable head. Zero before the first event.
    pub(in crate::control) fn head(&self) -> ControlResult<u64> {
        // Never observed from inside a transaction: only a commit moves what watchers see.
        if !self.conn.is_autocommit() {
            return Err(ControlError::InternalError);
        }
        let head: String = self
            .conn
            .query_row(
                "SELECT last_sequence FROM authority_metadata WHERE id=1",
                [],
                |r| r.get(0),
            )
            .map_err(db)?;
        parse_head(&head)
    }
    /// One page of committed history strictly after `after`, ascending, bounded by `limit`
    /// events and by the response frame. `next` is the exact continuation cursor and is
    /// present only while a later committed event exists. Read-only.
    pub(in crate::control) fn replay(
        &self,
        stream: &WireUuid,
        after: Option<Sequence>,
        limit: u16,
    ) -> ControlResult<ResultValue> {
        if limit == 0 || limit > MAX_PAGE || canonical(stream.as_str()) != self.stream.as_str() {
            return Err(ControlError::InvalidRequest);
        }
        let after = after.map_or(0, |s| s.get());
        // A cursor ahead of the durable head names history that does not exist.
        if after > self.head()? {
            return Err(ControlError::InvalidRequest);
        }
        let mut q = self
            .conn
            .prepare("SELECT sequence,envelope FROM control_events WHERE stream_id=?1 AND sequence>?2 ORDER BY sequence LIMIT ?3")
            .map_err(db)?;
        let mut rows = q
            .query(params![self.stream.as_str(), key(after), limit + 1])
            .map_err(db)?;
        let (mut events, mut bytes, mut more) = (Vec::new(), 0usize, false);
        while let Some(r) = rows.next().map_err(db)? {
            let order: String = r.get(0).map_err(db)?;
            let raw: String = r.get(1).map_err(db)?;
            let (_, event) = decode(&raw)?;
            // A committed event is never skipped: the page is contiguous from the cursor.
            let expected = after + events.len() as u64 + 1;
            if order != key(expected) || event.sequence().get() != expected {
                return Err(ControlError::StorageIncompatible);
            }
            let size = serde_json::to_vec(&event).map_err(db)?.len() + 1;
            if events.len() == usize::from(limit)
                || (!events.is_empty() && bytes + size > PAGE_BYTES)
            {
                more = true;
                break;
            }
            bytes += size;
            events.push(event);
        }
        let next = if more {
            events.last().map(EventEnvelopeV1::sequence)
        } else {
            None
        };
        Ok(ResultValue::Audit { events, next })
    }
}
#[cfg(test)]
#[path = "event_tests.rs"]
mod tests;
