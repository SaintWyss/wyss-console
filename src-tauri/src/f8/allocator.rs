//! Durable per-session runtime allocator for a schema-8 database. Inactive: it has no process,
//! provider, socket or installed-store entry point and never opens a path itself.
//!
//! The database is the authority for identity monotonicity. Everything here happens in one
//! IMMEDIATE transaction that commits the runtime row, the launch reservation, the audit event
//! and the replay receipt together or not at all. The in-memory registry only mirrors a
//! committed allocation; it can never be the sole source of permission. A reservation is
//! made *before* any launch attempt, a failed attempt keeps its generation, a launch identity is
//! never reused and no lease, approval or turn is ever carried from one generation to the next.
use super::{
    audit::{self, AuditReplay},
    protocol::{Request, RuntimeRef, Target},
};
use crate::contracts::{
    WireUuid,
    control::{self as v1, ControlError as E, ControlResult, unique_json},
    identities::{AgentRuntimeInstanceId, ManagedSessionId},
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

/// Candidate lifetime limits (docs/contracts/f8/resources.json), also enforced by SQL triggers.
const MAX_RUNTIME_ROWS: i64 = 128;
const MAX_LAUNCH_ROWS: i64 = 4096;
const MAX_EVENT_BYTES: usize = 2048;
const MAX_RECEIPT_BYTES: usize = 1024;

fn db(_: impl std::fmt::Debug) -> E {
    E::StorageUnavailable
}
fn invalid(_: impl std::fmt::Debug) -> E {
    E::StorageIncompatible
}

/// What the caller independently observed in memory. Neither field grants anything; the
/// allocator uses them only to decide between retaining a launch and reserving a new one.
#[derive(Debug, Clone)]
pub struct Admission {
    /// The session's launch which the registry holds in any non-stopped phase.
    pub live: Option<RuntimeRef>,
    /// Whether the single active runtime slot is free for a new reservation.
    pub slot_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub record: v1::RuntimeControlRecord,
    pub managed_session_id: ManagedSessionId,
    pub generation: u64,
    /// The launch which was reserved or retained; `None` for a stop and an unlaunched runtime.
    pub runtime_instance_id: Option<AgentRuntimeInstanceId>,
    /// The original receipt was returned; no row, event or reservation was written.
    pub replayed: bool,
    /// A new launch identity was reserved by this call.
    pub reserved: bool,
}

pub struct Allocator<'c> {
    conn: &'c mut Connection,
    authority: WireUuid,
    stream: String,
    clock: Box<dyn Fn() -> String + 'c>,
    fault: Fault<'c>,
}

impl<'c> Allocator<'c> {
    /// `authority` is the instance writing events; it must differ from every session/instance.
    pub fn new(conn: &'c mut Connection, authority: WireUuid) -> ControlResult<Self> {
        super::protocol::canonical(authority.as_str())?;
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(invalid)?;
        let stream: String = conn
            .query_row(
                "SELECT stream_id FROM authority_metadata WHERE id=1",
                [],
                |r| r.get(0),
            )
            .map_err(invalid)?;
        if version != 8 {
            return Err(E::StorageIncompatible);
        }
        conn.pragma_update(None, "foreign_keys", true).map_err(db)?;
        Ok(Self {
            conn,
            authority,
            stream,
            clock: Box::new(|| {
                OffsetDateTime::now_utc()
                    .format(&Rfc3339)
                    .unwrap_or_default()
            }),
            fault: None,
        })
    }
    pub fn with_clock(mut self, clock: impl Fn() -> String + 'c) -> Self {
        self.clock = Box::new(clock);
        self
    }
    /// Fault injection: called after each transactional write step and before commit.
    pub fn with_fault(mut self, f: impl FnMut(usize) -> ControlResult<()> + 'c) -> Self {
        self.fault = Some(Box::new(f));
        self
    }

    /// Reserve a new launch generation, or retain the live one. Exact replays return the
    /// original receipt before any capacity, state or registry check.
    pub fn start(&mut self, request: &Request, admission: &Admission) -> ControlResult<Outcome> {
        self.mutate(request, admission, true)
    }
    pub fn stop(&mut self, request: &Request, admission: &Admission) -> ControlResult<Outcome> {
        self.mutate(request, admission, false)
    }

    fn mutate(
        &mut self,
        request: &Request,
        admission: &Admission,
        start: bool,
    ) -> ControlResult<Outcome> {
        let (mutation, instance_expectation) = match (request.operation(), start) {
            (v1::Operation::RuntimeStart(m), true) | (v1::Operation::RuntimeStop(m), false) => {
                (m, request.target())
            }
            _ => return Err(E::InvalidRequest),
        };
        let Target::Runtime {
            session,
            generation: Some(expected_generation),
            instance: expected_instance,
        } = instance_expectation
        else {
            return Err(E::InvalidRequest);
        };
        let (session, expected_generation) = (session.as_str().to_owned(), *expected_generation);
        let expected_revision = mutation.expected_revision.ok_or(E::InvalidRequest)?;
        let intent = mutation.intent_id.as_str().to_owned();
        let canonical = String::from_utf8(request.canonical_intent()?).map_err(invalid)?;
        let now = (self.clock)();
        let authority = self.authority.as_str().to_owned();
        let stream = self.stream.clone();

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        // Replay first: a changed fence or payload under the same intent is a conflict, an
        // identical one returns the original receipt with zero side effects.
        if let Some(outcome) = replay(&tx, &intent, &canonical)? {
            tx.rollback().map_err(db)?;
            return Ok(outcome);
        }
        let state: Option<String> = tx
            .query_row(
                "SELECT control_state FROM managed_sessions WHERE session_id=?1",
                [&session],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        let closed = match state.as_deref() {
            None => return Err(E::SessionNotFound),
            Some("prepared") => false,
            Some(_) => true,
        };
        let row: Option<(String, i64, i64, String)> = tx
            .query_row(
                "SELECT desired_state,revision,generation,created_at FROM managed_runtimes WHERE managed_session_id=?1",
                [&session],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(db)?;
        let (desired, revision, generation, created) = match &row {
            Some((d, r, g, c)) => (
                d.as_str(),
                u64::try_from(*r).map_err(invalid)?,
                u64::try_from(*g).map_err(invalid)?,
                c.clone(),
            ),
            None => ("stopped", 0, 0, now.clone()),
        };
        if expected_revision != revision || expected_generation != generation {
            return Err(E::StaleRevision);
        }
        let current: Option<(String, String)> = tx
            .query_row(
                "SELECT runtime_instance_id,authority_instance_id FROM managed_runtime_launches WHERE managed_session_id=?1 AND generation=?2",
                params![session, generation as i64],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)?;
        let current_instance = current.as_ref().map(|c| c.0.clone());
        // A predecessor authority's launch is history, whatever the registry claims.
        let owned = current.as_ref().is_some_and(|c| c.1 == authority);
        let next_revision = revision
            .checked_add(1)
            .filter(|r| *r <= i64::MAX as u64)
            .ok_or(E::CapacityExceeded)?;

        let (new_generation, new_instance, running, reserved) = if start {
            if closed {
                return Err(E::InvalidTransition);
            }
            // A live registry observation only chooses retention; the durable row must agree.
            let retained = desired == "running"
                && owned
                && current_instance.as_deref().is_some_and(|i| {
                    admission.live.as_ref().is_some_and(|l| {
                        l.managed_session_id.as_str() == session
                            && l.generation == generation
                            && l.runtime_instance_id.as_str() == i
                    })
                });
            if retained {
                (generation, current_instance.clone(), true, false)
            } else {
                // Overflow refuses before any durable mutation.
                let next = generation
                    .checked_add(1)
                    .filter(|g| *g <= i64::MAX as u64)
                    .ok_or(E::CapacityExceeded)?;
                let rows: i64 = count(&tx, "managed_runtimes")?;
                let launches: i64 = count(&tx, "managed_runtime_launches")?;
                if !admission.slot_available
                    || (row.is_none() && rows >= MAX_RUNTIME_ROWS)
                    || launches >= MAX_LAUNCH_ROWS
                {
                    return Err(E::CapacityExceeded);
                }
                let fresh = loop {
                    let candidate = crate::control::store::uuid().as_str().to_owned();
                    let used: bool = tx
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM managed_runtime_launches WHERE runtime_instance_id=?1) OR ?1 IN (?2,?3)",
                            params![candidate, session, authority],
                            |r| r.get(0),
                        )
                        .map_err(db)?;
                    if !used {
                        break candidate;
                    }
                };
                (next, Some(fresh), true, true)
            }
        } else {
            // A stop fences the launch it observed: a concrete instance must be the current one,
            // and null means explicitly "no launch", never a wildcard.
            let expected = expected_instance.as_ref().map(|i| i.as_str().to_owned());
            let live_now = desired == "running" && current_instance.is_some();
            if expected
                != if live_now {
                    current_instance.clone()
                } else {
                    None
                }
            {
                return Err(E::StaleRevision);
            }
            (generation, None, false, false)
        };

        let desired_text = if running { "running" } else { "stopped" };
        if row.is_some() {
            tx.execute(
                "UPDATE managed_runtimes SET desired_state=?2,revision=?3,generation=?4,updated_at=?5 WHERE managed_session_id=?1",
                params![session, desired_text, next_revision as i64, new_generation as i64, now],
            )
            .map_err(db)?;
        } else {
            tx.execute(
                "INSERT INTO managed_runtimes VALUES(?1,'codex','codex',?2,?3,?4,?5,?5)",
                params![
                    session,
                    desired_text,
                    next_revision as i64,
                    new_generation as i64,
                    now
                ],
            )
            .map_err(db)?;
        }
        step(&mut self.fault, 0)?;
        if reserved {
            tx.execute(
                "INSERT INTO managed_runtime_launches VALUES(?1,'codex','codex',?2,?3,?4,?5)",
                params![session, new_generation as i64, new_instance, authority, now],
            )
            .map_err(db)?;
        }
        step(&mut self.fault, 1)?;
        append(
            &tx,
            (&authority, &stream, &now),
            Draft {
                session: &session,
                kind: "authority.runtime.desired_state_changed",
                correlation: Some(&intent),
                payload: json!({"adapter_id":"codex","runtime_kind":"codex","desired_state":desired_text,
                    "revision":next_revision,"generation":new_generation,"runtime_instance_id":new_instance}),
            },
        )?;
        step(&mut self.fault, 2)?;
        let record = json!({"adapter_id":"codex","runtime_kind":"codex","desired_state":desired_text,
            "revision":next_revision,"created_at":created,"updated_at":now});
        let mut result = record.clone();
        result["managed_session_id"] = json!(session);
        result["generation"] = json!(new_generation);
        let result = result.to_string();
        if canonical.len() > MAX_RECEIPT_BYTES || result.len() > MAX_RECEIPT_BYTES {
            return Err(E::CapacityExceeded);
        }
        tx.execute(
            "INSERT INTO processed_intents VALUES(?1,?2,?3)",
            params![intent, canonical, result],
        )
        .map_err(db)?;
        step(&mut self.fault, 3)?;
        let record: v1::RuntimeControlRecord = serde_json::from_value(record).map_err(invalid)?;
        record.validate()?;
        tx.commit().map_err(db)?;
        Ok(Outcome {
            record,
            managed_session_id: ManagedSessionId::parse(&session).map_err(invalid)?,
            generation: new_generation,
            runtime_instance_id: new_instance
                .as_deref()
                .map(AgentRuntimeInstanceId::parse)
                .transpose()
                .map_err(invalid)?,
            replayed: false,
            reserved,
        })
    }

    /// Records that this authority observed the reserved launch handshake. Only the allocating
    /// authority may do so, only for the current running generation, and at most once.
    pub fn record_ready(
        &mut self,
        launch: &RuntimeRef,
        runtime_version: &str,
        compatibility: &str,
    ) -> ControlResult<()> {
        self.observe(launch, "authority.runtime.instance_ready", |_| {
            Ok(json!({"adapter_id":"codex","runtime_kind":"codex",
                "runtime_instance_id":launch.runtime_instance_id,"generation":launch.generation,
                "runtime_version":runtime_version,"compatibility":compatibility}))
        })
    }
    /// Records the reaped end of a launch. It never fabricates an exit for a predecessor's
    /// launch: the authority recorded at reservation must be this one.
    pub fn record_exit(
        &mut self,
        launch: &RuntimeRef,
        exit_kind: &str,
        expected: bool,
        budget_failure: Option<&str>,
    ) -> ControlResult<()> {
        self.observe(launch, "authority.runtime.instance_exited", |desired| {
            Ok(json!({"adapter_id":"codex","runtime_kind":"codex",
                "runtime_instance_id":launch.runtime_instance_id,"generation":launch.generation,
                "exit_kind":exit_kind,"expected":expected,"desired_state":desired,
                "budget_failure":budget_failure}))
        })
    }
    fn observe(
        &mut self,
        launch: &RuntimeRef,
        kind: &str,
        payload: impl FnOnce(&str) -> ControlResult<Value>,
    ) -> ControlResult<()> {
        launch.validate()?;
        if launch.authority_instance_id != self.authority {
            return Err(E::CorrelationError);
        }
        let (authority, stream, now) = (
            self.authority.as_str().to_owned(),
            self.stream.clone(),
            (self.clock)(),
        );
        let session = launch.managed_session_id.as_str();
        let instance = launch.runtime_instance_id.as_str();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let owner: Option<String> = tx
            .query_row(
                "SELECT authority_instance_id FROM managed_runtime_launches WHERE managed_session_id=?1 AND runtime_instance_id=?2 AND generation=?3",
                params![session, instance, launch.generation as i64],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        if owner.as_deref() != Some(authority.as_str()) {
            return Err(E::StaleRevision);
        }
        let (desired, generation): (String, i64) = tx
            .query_row(
                "SELECT desired_state,generation FROM managed_runtimes WHERE managed_session_id=?1",
                [session],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db)?;
        let ready = kind.ends_with("instance_ready");
        if ready && (desired != "running" || generation as u64 != launch.generation) {
            return Err(E::StaleRevision);
        }
        for prior in ["instance_ready", "instance_exited"] {
            // Ready once; exit once; neither after an exit.
            if (prior == "instance_exited" || kind.ends_with(prior))
                && events_for(&tx, &format!("authority.runtime.{prior}"), instance)? != 0
            {
                return Err(E::InvalidTransition);
            }
        }
        append(
            &tx,
            (&authority, &stream, &now),
            Draft {
                session,
                kind,
                correlation: None,
                payload: payload(&desired)?,
            },
        )?;
        step(&mut self.fault, 0)?;
        tx.commit().map_err(db)
    }

    /// Launches reserved by an earlier authority instance which never recorded an exit. They
    /// are unowned history: nothing adopts, resumes or inherits them, and no exit is invented.
    pub fn unowned_launches(&self) -> ControlResult<Vec<RuntimeRef>> {
        let mut q = self
            .conn
            .prepare("SELECT managed_session_id,runtime_instance_id,generation,authority_instance_id FROM managed_runtime_launches WHERE authority_instance_id<>?1 ORDER BY runtime_instance_id")
            .map_err(db)?;
        let rows = q
            .query_map([self.authority.as_str()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .map_err(db)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db)?;
        let mut out = Vec::new();
        for (session, instance, generation, authority) in rows {
            if events_for(self.conn, "authority.runtime.instance_exited", &instance)? == 0 {
                out.push(RuntimeRef {
                    schema: 2,
                    authority_instance_id: WireUuid::try_from(authority).map_err(invalid)?,
                    managed_session_id: ManagedSessionId::parse(&session).map_err(invalid)?,
                    runtime_instance_id: AgentRuntimeInstanceId::parse(&instance)
                        .map_err(invalid)?,
                    generation: u64::try_from(generation).map_err(invalid)?,
                });
            }
        }
        Ok(out)
    }

    /// Everything the durable log leaves unresolved on launches that are not live. Read-only.
    pub fn unresolved_work(&self) -> ControlResult<audit::Unresolved> {
        let mut replay = AuditReplay::default();
        let mut q = self
            .conn
            .prepare("SELECT envelope FROM control_events ORDER BY sequence")
            .map_err(db)?;
        let mut rows = q.query([]).map_err(db)?;
        while let Some(r) = rows.next().map_err(db)? {
            let raw: String = r.get(0).map_err(db)?;
            replay.observe(&unique_json(raw.as_bytes()).map_err(invalid)?)?;
        }
        // As seen by this authority: a predecessor's launch is never live to it.
        Ok(replay.unresolved_for(self.authority.as_str()))
    }

    /// Durable identity history to prime a fresh in-memory registry after restart. Every
    /// listed launch is dead to this process; the registry may only use it to refuse reuse.
    pub fn registry_history(&self) -> ControlResult<RegistryHistory> {
        let used = self
            .conn
            .prepare("SELECT runtime_instance_id FROM managed_runtime_launches")
            .map_err(db)?
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(db)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db)?;
        let generations = self
            .conn
            .prepare("SELECT managed_session_id,generation FROM managed_runtimes")
            .map_err(db)?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(db)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db)?
            .into_iter()
            .map(|(s, g)| Ok((s, u64::try_from(g).map_err(invalid)?)))
            .collect::<ControlResult<Vec<_>>>()?;
        Ok((used, generations))
    }
}

/// Instances ever allocated, and each session's durable generation.
pub type RegistryHistory = (Vec<String>, Vec<(String, u64)>);
type Fault<'c> = Option<Box<dyn FnMut(usize) -> ControlResult<()> + 'c>>;
fn step(fault: &mut Fault<'_>, n: usize) -> ControlResult<()> {
    match fault.as_mut() {
        Some(f) => f(n),
        None => Ok(()),
    }
}
fn count(tx: &Transaction<'_>, table: &str) -> ControlResult<i64> {
    tx.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .map_err(db)
}
fn events_for(c: &Connection, kind: &str, instance: &str) -> ControlResult<i64> {
    c.query_row(
        "SELECT count(*) FROM control_events WHERE json_extract(envelope,'$.event_type')=?1 AND json_extract(envelope,'$.payload.runtime_instance_id')=?2",
        params![kind, instance],
        |r| r.get(0),
    )
    .map_err(db)
}

fn replay(tx: &Transaction<'_>, intent: &str, canonical: &str) -> ControlResult<Option<Outcome>> {
    let prior: Option<(String, String)> = tx
        .query_row(
            "SELECT mutation,result FROM processed_intents WHERE intent_id=?1",
            [intent],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db)?;
    let Some((mutation, result)) = prior else {
        return Ok(None);
    };
    if mutation != canonical {
        return Err(E::IntentConflict);
    }
    let mut value: Value = unique_json(result.as_bytes()).map_err(invalid)?;
    let o = value.as_object_mut().ok_or_else(|| invalid(()))?;
    let session = o
        .remove("managed_session_id")
        .and_then(|s| s.as_str().map(str::to_owned))
        .ok_or_else(|| invalid(()))?;
    let generation = o
        .remove("generation")
        .and_then(|g| g.as_u64())
        .ok_or_else(|| invalid(()))?;
    let record: v1::RuntimeControlRecord = serde_json::from_value(value).map_err(invalid)?;
    record.validate()?;
    let instance: Option<String> = tx
        .query_row(
            "SELECT runtime_instance_id FROM managed_runtime_launches WHERE managed_session_id=?1 AND generation=?2",
            params![session, generation as i64],
            |r| r.get(0),
        )
        .optional()
        .map_err(db)?;
    Ok(Some(Outcome {
        record: record.clone(),
        managed_session_id: ManagedSessionId::parse(&session).map_err(invalid)?,
        generation,
        // A stop receipt names no launch even though its generation had one.
        runtime_instance_id: instance
            .filter(|_| record.desired_state == v1::DesiredState::Running)
            .as_deref()
            .map(AgentRuntimeInstanceId::parse)
            .transpose()
            .map_err(invalid)?,
        replayed: true,
        reserved: false,
    }))
}

/// The single event writer of this module: validates the exact vocabulary-2 contract, the
/// unchanged durable size bound and sequence continuity before advancing the stream head.
struct Draft<'a> {
    session: &'a str,
    kind: &'a str,
    correlation: Option<&'a str>,
    payload: Value,
}
fn append(
    tx: &Transaction<'_>,
    (authority, stream, now): (&str, &str, &str),
    d: Draft<'_>,
) -> ControlResult<u64> {
    let Draft {
        session,
        kind,
        correlation,
        payload,
    } = d;
    let head: String = tx
        .query_row(
            "SELECT last_sequence FROM authority_metadata WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(db)?;
    let next = head
        .parse::<u64>()
        .map_err(invalid)?
        .checked_add(1)
        .ok_or(E::CapacityExceeded)?;
    let id = crate::control::store::uuid().as_str().to_owned();
    let value = json!({"schema_version":1,"event_id":id,"event_type":kind,"payload_schema_version":2,
        "timestamp":now,"stream_id":stream,"sequence":next.to_string(),"session_id":session,
        "source":{"component":"wyss_control","instance_id":authority},
        "correlation_id":correlation,"causation_id":null,"payload":payload,
        "persistence":"durable","sensitivity":"internal","redaction":"none"});
    audit::validate_event(&value)?;
    let raw = serde_json::to_string(&value).map_err(invalid)?;
    // Durable security metadata is never truncated: an oversized event fails its mutation.
    if raw.len() > MAX_EVENT_BYTES {
        return Err(E::CapacityExceeded);
    }
    tx.execute(
        "INSERT INTO control_events VALUES(?1,?2,?3,?4,?5)",
        params![id, stream, format!("{next:020}"), session, raw],
    )
    .map_err(db)?;
    tx.execute(
        "UPDATE authority_metadata SET last_sequence=?1 WHERE id=1",
        [next.to_string()],
    )
    .map_err(db)?;
    Ok(next)
}

#[cfg(test)]
#[path = "allocator_tests.rs"]
mod tests;
