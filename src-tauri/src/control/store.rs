#[path = "approval_store.rs"]
pub(in crate::control) mod approvals;
#[path = "bindings.rs"]
pub(in crate::control) mod bindings;
#[path = "events.rs"]
pub(in crate::control) mod events;
#[path = "leases.rs"]
mod leases;
#[cfg(test)]
pub(in crate::control) use leases::Clock;
#[path = "turn_store.rs"]
pub(in crate::control) mod turns;
use super::{paths, schema};
use crate::contracts::{
    Sequence, WireUuid, control::*, event_envelope::EventEnvelopeV1, identities::ManagedSessionId,
};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

fn db(_: impl std::fmt::Debug) -> ControlError {
    ControlError::StorageUnavailable
}
fn invalid(_: impl std::fmt::Debug) -> ControlError {
    ControlError::StorageIncompatible
}
pub(crate) fn uuid() -> WireUuid {
    WireUuid::try_from(uuid::Uuid::new_v4().to_string()).expect("generated UUID")
}
fn canonical(id: &str) -> String {
    id.to_ascii_lowercase()
}
fn key(n: u64) -> String {
    format!("{n:020}")
}
fn sequence(n: u64) -> ControlResult<Sequence> {
    n.to_string().parse().map_err(invalid)
}
fn now() -> ControlResult<String> {
    OffsetDateTime::now_utc().format(&Rfc3339).map_err(invalid)
}

pub(crate) struct Store {
    pub(crate) conn: Connection,
    instance: WireUuid,
    stream: WireUuid,
    pub(super) clock: std::sync::Arc<dyn leases::Clock>,
    pub(super) execution_available: bool,
}
impl Store {
    /// Read-only canonical validation for an offline in-memory image. No open/migrate,
    /// vocabulary update, journal configuration or runtime reconciliation occurs.
    pub(crate) fn validate_offline_connection(conn: Connection) -> ControlResult<Connection> {
        if conn.path() != Some("") {
            return Err(ControlError::UnsafePath);
        }
        let (version, stream, head, vocabulary): (i64, String, String, i64) = conn.query_row(
            "SELECT schema_version,stream_id,last_sequence,event_vocabulary FROM authority_metadata WHERE id=1",
            [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(invalid)?;
        if version != 7 || vocabulary < 1 {
            return Err(ControlError::StorageIncompatible);
        }
        let store = Self {
            conn,
            instance: uuid(),
            stream: WireUuid::try_from(stream).map_err(invalid)?,
            clock: std::sync::Arc::new(leases::SystemClock),
            execution_available: false,
        };
        store.verify(&head, vocabulary > i64::from(events::VOCABULARY))?;
        Ok(store.conn)
    }
    #[cfg(test)]
    pub(crate) fn in_memory_fixture() -> Self {
        let mut conn = Connection::open_in_memory().unwrap();
        schema::migrate(&mut conn).unwrap();
        let stream: String = conn
            .query_row("SELECT stream_id FROM authority_metadata", [], |r| r.get(0))
            .unwrap();
        Self {
            conn,
            instance: uuid(),
            stream: WireUuid::try_from(stream).unwrap(),
            clock: std::sync::Arc::new(leases::SystemClock),
            execution_available: true,
        }
    }
    pub fn open(path: &Path, instance: WireUuid) -> ControlResult<Self> {
        paths::private_file(path)?;
        for suffix in ["-wal", "-shm", "-journal"] {
            let side = path.with_file_name(format!(
                "{}{}",
                path.file_name()
                    .ok_or(ControlError::UnsafePath)?
                    .to_string_lossy(),
                suffix
            ));
            if std::fs::symlink_metadata(&side).is_ok() {
                paths::validate_file(&side)?;
            }
        }
        let mut conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(invalid)?;
        // A store that is refused is left as it was found: closing it must not fold its
        // write-ahead log into the database. Checkpointing is restored on admission.
        conn.set_db_config(
            rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
            true,
        )
        .map_err(db)?;
        conn.busy_timeout(Duration::from_secs(1)).map_err(db)?;
        conn.pragma_update(None, "foreign_keys", true).map_err(db)?;
        schema::migrate(&mut conn)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(db)?;
        conn.pragma_update(None, "synchronous", "FULL")
            .map_err(db)?;
        conn.pragma_update(None, "trusted_schema", false)
            .map_err(db)?;
        let (version, stream, head, vocabulary): (i64, String, String, i64) = conn
            .query_row(
                "SELECT schema_version,stream_id,last_sequence,event_vocabulary FROM authority_metadata WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .map_err(invalid)?;
        if version != 7 || vocabulary < 1 {
            return Err(ControlError::StorageIncompatible);
        }
        // Before this build can append anything, the store records that its vocabulary
        // has been here. Only a store a larger vocabulary has opened may hold event types
        // this build does not know.
        let own = i64::from(events::VOCABULARY);
        if vocabulary < own {
            conn.execute(
                "UPDATE authority_metadata SET event_vocabulary=?1 WHERE id=1",
                [own],
            )
            .map_err(db)?;
        }
        let newer_writer = vocabulary > own;
        let stream = WireUuid::try_from(stream).map_err(invalid)?;
        let store = Self {
            conn,
            instance,
            stream,
            clock: std::sync::Arc::new(leases::SystemClock),
            execution_available: true,
        };
        store.verify(&head, newer_writer)?;
        store.checkpoint_on_close(true)?;
        Ok(store)
    }
    /// Whether closing this store may checkpoint and remove its write-ahead log. Off while
    /// the store is not, or not yet, admitted.
    pub(crate) fn checkpoint_on_close(&self, enabled: bool) -> ControlResult<()> {
        self.conn
            .set_db_config(
                rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
                !enabled,
            )
            .map(|_| ())
            .map_err(db)
    }
    /// `newer_writer`: a build with a larger event vocabulary has opened this store, so it
    /// may hold valid events of types this build cannot name.
    fn verify(&self, head: &str, newer_writer: bool) -> ControlResult<()> {
        let head = parse_head(head)?;
        let mut foreign = self
            .conn
            .prepare("PRAGMA foreign_key_check")
            .map_err(invalid)?;
        if foreign
            .query([])
            .map_err(invalid)?
            .next()
            .map_err(invalid)?
            .is_some()
        {
            return Err(ControlError::StorageIncompatible);
        }
        let mut query=self.conn.prepare("SELECT event_id,stream_id,sequence,session_id,envelope FROM control_events ORDER BY sequence").map_err(invalid)?;
        let mut rows = query.query([]).map_err(invalid)?;
        let mut n = 0u64;
        // A read-only view of what the log says happened, compared below with the tables.
        let mut replay = events::Replay::default();
        while let Some(r) = rows.next().map_err(invalid)? {
            n = n.checked_add(1).ok_or(ControlError::StorageIncompatible)?;
            let id: String = r.get(0).map_err(invalid)?;
            let stream: String = r.get(1).map_err(invalid)?;
            let order: String = r.get(2).map_err(invalid)?;
            let session: Option<String> = r.get(3).map_err(invalid)?;
            let raw: String = r.get(4).map_err(invalid)?;
            let (v, e) = events::decode(&raw)?;
            if id != v["event_id"]
                || stream != self.stream.as_str()
                || v["stream_id"] != self.stream.as_str()
                || order != key(n)
                || e.sequence().get() != n
                || serde_json::to_value(session).map_err(invalid)? != v["session_id"]
            {
                return Err(ControlError::StorageIncompatible);
            }
            // A known event is held to its contract; an unknown future one is only counted.
            replay.observe(&v)?;
        }
        if head != n {
            return Err(ControlError::StorageIncompatible);
        }
        // Inside this authority's own vocabulary every event has a type it knows: one that
        // it cannot name was not written by any build that ever opened this store.
        if replay.unknown != 0 && !newer_writer {
            return Err(ControlError::StorageIncompatible);
        }
        let mut q = self
            .conn
            .prepare("SELECT session_id FROM managed_sessions")
            .map_err(invalid)?;
        let ids = q
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(invalid)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(invalid)?;
        if ids.len() != replay.sessions.len() {
            return Err(ControlError::StorageIncompatible);
        }
        for id in ids {
            let s = get_session(&self.conn, &id)?;
            s.validate()?;
            if replay.sessions.get(&id)
                != Some(&events::Head {
                    state: enum_text(&s.control_state).map_err(invalid)?,
                    revision: s.revision,
                })
            {
                return Err(ControlError::StorageIncompatible);
            }
        }
        self.verify_leases(&replay.leases)?;
        self.verify_bindings(&replay)?;
        self.verify_turns(&replay.turns)?;
        // Receipts contain only typed minimal mutations and their original session projection.
        let mut q = self
            .conn
            .prepare("SELECT intent_id,mutation,result FROM processed_intents")
            .map_err(invalid)?;
        let mut rows = q.query([]).map_err(invalid)?;
        let mut decided_approvals = std::collections::HashSet::new();
        while let Some(r) = rows.next().map_err(invalid)? {
            let id: String = r.get(0).map_err(invalid)?;
            WireUuid::try_from(id.clone()).map_err(invalid)?;
            let mutation: String = r.get(1).map_err(invalid)?;
            let result: String = r.get(2).map_err(invalid)?;
            let method = unique_json(mutation.as_bytes()).map_err(invalid)?["method"].clone();
            if method == "managedSession/turn/start" {
                turns::verify_fingerprint_receipt(&id, &mutation, &result)?;
                continue;
            }
            if method == approvals::METHOD {
                // One decision receipt per approval, ever.
                if !decided_approvals.insert(approvals::verify_receipt(&id, &mutation, &result)?) {
                    return Err(ControlError::StorageIncompatible);
                }
                continue;
            }
            let op: Operation =
                serde_json::from_value(unique_json(mutation.as_bytes()).map_err(invalid)?)
                    .map_err(invalid)?;
            op.validate().map_err(invalid)?;
            let Some(intent) = intent(&op) else {
                return Err(ControlError::StorageIncompatible);
            };
            if canonical(intent.as_str()) != id {
                return Err(ControlError::StorageIncompatible);
            }
            let result: String = r.get(2).map_err(invalid)?;
            decode_receipt(&op, &result)?;
        }
        self.verify_approvals(&replay.approvals, &decided_approvals)?;
        let runtime = self.runtime_control()?;
        if let Some(runtime) = &runtime {
            runtime.validate()?;
        }
        let desired = runtime
            .map(|r| {
                Ok::<_, ControlError>(events::Head {
                    state: enum_text(&r.desired_state).map_err(invalid)?,
                    revision: r.revision,
                })
            })
            .transpose()?;
        if desired != replay.runtime {
            return Err(ControlError::StorageIncompatible);
        }
        let count: i64 = self
            .conn
            .query_row("SELECT count(*) FROM authority_metadata", [], |r| r.get(0))
            .map_err(invalid)?;
        if count != 1 {
            return Err(ControlError::StorageIncompatible);
        }
        Ok(())
    }
    pub fn status(&self) -> ControlResult<AuthorityStatus> {
        let head: String = self
            .conn
            .query_row(
                "SELECT last_sequence FROM authority_metadata WHERE id=1",
                [],
                |r| r.get(0),
            )
            .map_err(db)?;
        let head = parse_head(&head)?;
        let count = |state| {
            self.conn
                .query_row(
                    "SELECT count(*) FROM managed_sessions WHERE control_state=?1",
                    [state],
                    |r| r.get::<_, i64>(0),
                )
                .map_err(db)
                .and_then(|n| u64::try_from(n).map_err(invalid))
        };
        Ok(AuthorityStatus {
            protocol_version: 1,
            authority_instance_id: self.instance.clone(),
            service_version: env!("CARGO_PKG_VERSION").into(),
            storage_schema_version: 7,
            stream_id: self.stream.clone(),
            last_sequence: if head == 0 {
                None
            } else {
                Some(sequence(head)?)
            },
            prepared_count: count("prepared")?,
            closed_count: count("closed")?,
        })
    }
    pub fn apply(&mut self, op: &Operation) -> ControlResult<ResultValue> {
        op.validate()?;
        match op {
            Operation::Hello(_) => Ok(ResultValue::Hello(self.status()?)),
            Operation::Status(_) => Ok(ResultValue::Status(self.status()?)),
            Operation::Get(p) => Ok(ResultValue::Session(get_session(
                &self.conn,
                p.managed_session_id.as_str(),
            )?)),
            Operation::List(p) => {
                let mut q=self.conn.prepare("SELECT session_id FROM managed_sessions WHERE created_sequence>?1 ORDER BY created_sequence LIMIT ?2").map_err(db)?;
                let rows = q
                    .query_map(params![key(p.after.map_or(0, |s| s.get())), p.limit], |r| {
                        r.get::<_, String>(0)
                    })
                    .map_err(db)?;
                let mut sessions = Vec::new();
                for row in rows {
                    sessions.push(get_session(&self.conn, &row.map_err(db)?)?);
                }
                let next = if sessions.len() == p.limit as usize {
                    sessions.last().map(|s| s.created_sequence)
                } else {
                    None
                };
                Ok(ResultValue::Sessions { sessions, next })
            }
            Operation::AuditRead(p) => self.replay(&p.stream_id, p.after_sequence, p.limit),
            // The wait happens outside the core; here a watch is the same bounded read.
            Operation::AuditWatch(p) => self.replay(&p.stream_id, p.after_sequence, p.limit),
            Operation::LeaseGet(p) => Ok(ResultValue::Lease(
                leases::read(&self.conn, p.lease_id.as_str())?.effective_at(self.clock.now())?,
            )),
            Operation::LeaseActive(p) => Ok(ResultValue::ActiveLease(
                leases::active(&self.conn, p.session_id.as_str())?
                    .map(|l| l.effective_at(self.clock.now()))
                    .transpose()?
                    .filter(|l| l.state == LeaseState::Active),
            )),
            Operation::LeaseGrant(_) | Operation::LeaseRenew(_) | Operation::LeaseRevoke(_) => {
                self.mutate(op)
            }
            Operation::Prepare(_)
            | Operation::Close(_)
            | Operation::RuntimeStart(_)
            | Operation::RuntimeStop(_) => self.mutate(op),
            _ => Err(ControlError::InvalidRequest),
        }
    }
    pub fn runtime_control(&self) -> ControlResult<Option<RuntimeControlRecord>> {
        read_runtime(&self.conn)
    }
    pub fn runtime_event(&mut self, kind: &str, payload: Value) -> ControlResult<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        events::append(
            &tx,
            &self.instance,
            &self.stream,
            events::Draft {
                kind,
                session: None,
                correlation: None,
                timestamp: now()?,
                payload,
            },
        )?;
        tx.commit().map_err(db)
    }
    fn mutate(&mut self, op: &Operation) -> ControlResult<ResultValue> {
        let intent_id = canonical(intent(op).ok_or(ControlError::InvalidRequest)?.as_str());
        let mut canonical_op = serde_json::to_value(op).map_err(db)?;
        canonical_op["params"]["intent_id"] = json!(intent_id);
        if let Operation::Close(p) = op {
            canonical_op["params"]["managed_session_id"] =
                json!(canonical(p.managed_session_id.as_str()));
        }
        leases::canonicalize(op, &mut canonical_op);
        let mutation = serde_json::to_string(&canonical_op).map_err(db)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let receipt: Option<(String, String)> = tx
            .query_row(
                "SELECT mutation,result FROM processed_intents WHERE intent_id=?1",
                [&intent_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)?;
        if let Some((original, result)) = receipt {
            if original != mutation {
                return Err(ControlError::IntentConflict);
            }
            return decode_receipt(op, &result);
        }
        if tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM managed_session_operations WHERE operation_id=?1)",
                [&intent_id],
                |r| r.get::<_, bool>(0),
            )
            .map_err(db)?
        {
            return Err(ControlError::IntentConflict);
        }
        let lease_now = self.clock.now();
        if matches!(
            op,
            Operation::LeaseGrant(_) | Operation::LeaseRenew(_) | Operation::LeaseRevoke(_)
        ) {
            if !self.execution_available {
                return Err(ControlError::AuthorityUnavailable);
            }
            let lease = leases::mutate(&tx, &self.instance, &self.stream, op, lease_now)?;
            tx.execute(
                "INSERT INTO processed_intents VALUES(?1,?2,?3)",
                params![
                    intent_id,
                    mutation,
                    serde_json::to_string(&lease).map_err(db)?
                ],
            )
            .map_err(db)?;
            tx.commit().map_err(db)?;
            return Ok(ResultValue::Lease(lease));
        }
        if let Operation::RuntimeStart(p) | Operation::RuntimeStop(p) = op {
            p.target().supported()?;
            let previous = read_runtime(&tx)?;
            let revision = previous.as_ref().map_or(0, |r| r.revision);
            if p.expected_revision.is_some_and(|r| r != revision) {
                return Err(ControlError::StaleRevision);
            }
            let revision = revision
                .checked_add(1)
                .filter(|r| *r <= i64::MAX as u64)
                .ok_or(ControlError::CapacityExceeded)?;
            let time = now()?;
            let runtime = RuntimeControlRecord {
                adapter_id: p.adapter_id.clone(),
                runtime_kind: p.runtime_kind,
                desired_state: if matches!(op, Operation::RuntimeStart(_)) {
                    DesiredState::Running
                } else {
                    DesiredState::Stopped
                },
                revision,
                created_at: previous.map_or_else(
                    || time.clone().try_into().map_err(invalid),
                    |r| Ok(r.created_at),
                )?,
                updated_at: time.clone().try_into().map_err(invalid)?,
            };
            tx.execute("INSERT INTO managed_runtimes VALUES('codex','codex',?1,?2,?3,?4) ON CONFLICT(adapter_id) DO UPDATE SET desired_state=excluded.desired_state,revision=excluded.revision,updated_at=excluded.updated_at",params![enum_text(&runtime.desired_state)?,revision as i64,enum_text(&runtime.created_at)?,time]).map_err(db)?;
            events::append(
                &tx,
                &self.instance,
                &self.stream,
                events::Draft {
                    kind: "authority.runtime.desired_state_changed",
                    session: None,
                    correlation: Some(&intent_id),
                    timestamp: now()?,
                    payload: json!({"adapter_id":"codex","runtime_kind":"codex","desired_state":runtime.desired_state,"revision":revision}),
                },
            )?;
            tx.execute(
                "INSERT INTO processed_intents VALUES(?1,?2,?3)",
                params![
                    intent_id,
                    mutation,
                    serde_json::to_string(&runtime).map_err(db)?
                ],
            )
            .map_err(db)?;
            tx.commit().map_err(db)?;
            return Ok(ResultValue::RuntimeControl(runtime));
        }
        let time = now()?;
        let mut append = true;
        // The sequence a new session records as its own must be the one its event takes.
        let mut created = None;
        let (session, kind) = match op {
            Operation::Prepare(p) => {
                let n = next_sequence(&tx)?;
                created = Some(n);
                let session = ManagedSessionRecord {
                    managed_session_id: ManagedSessionId::parse(uuid().as_str())
                        .map_err(invalid)?,
                    adapter_id: p.adapter_id.clone(),
                    runtime_kind: p.runtime_kind,
                    control_state: ControlState::Prepared,
                    revision: 1,
                    created_at: time.clone().try_into().map_err(invalid)?,
                    updated_at: time.clone().try_into().map_err(invalid)?,
                    created_sequence: sequence(n)?,
                };
                tx.execute(
                    "INSERT INTO managed_sessions VALUES(?1,?2,?3,'prepared',1,?4,?4,?5)",
                    params![
                        session.managed_session_id.as_str(),
                        session.adapter_id.as_str(),
                        enum_text(&session.runtime_kind)?,
                        time,
                        key(n)
                    ],
                )
                .map_err(db)?;
                (session, "authority.managed_session.prepared")
            }
            Operation::Close(p) => {
                let mut session = get_session(&tx, p.managed_session_id.as_str())?;
                if session.revision != p.expected_revision {
                    return Err(ControlError::StaleRevision);
                }
                if session.control_state == ControlState::Closed {
                    append = false;
                } else {
                    leases::close(
                        &tx,
                        &self.instance,
                        &self.stream,
                        &session,
                        &intent_id,
                        lease_now,
                    )?;
                    session.control_state = ControlState::Closed;
                    session.revision = 2;
                    session.updated_at = time.clone().try_into().map_err(invalid)?;
                    tx.execute("UPDATE managed_sessions SET control_state='closed', revision=2, updated_at=?1 WHERE session_id=?2",params![time,session.managed_session_id.as_str()]).map_err(db)?;
                }
                (session, "authority.managed_session.closed")
            }
            _ => return Err(ControlError::InvalidRequest),
        };
        if append {
            let appended = events::append(
                &tx,
                &self.instance,
                &self.stream,
                events::Draft {
                    kind,
                    session: Some(session.managed_session_id.as_str()),
                    correlation: Some(&intent_id),
                    timestamp: time.clone(),
                    payload: json!({"adapter_id":session.adapter_id,"runtime_kind":session.runtime_kind,"control_state":session.control_state,"revision":session.revision}),
                },
            )?;
            if created.is_some_and(|n| n != appended) {
                return Err(ControlError::InternalError);
            }
        }
        let result = serde_json::to_string(&session).map_err(db)?;
        tx.execute(
            "INSERT INTO processed_intents VALUES(?1,?2,?3)",
            params![intent_id, mutation, result],
        )
        .map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(ResultValue::Session(session))
    }
}
fn intent(op: &Operation) -> Option<&WireUuid> {
    match op {
        Operation::TurnCancel(p) => Some(&p.intent_id),
        Operation::ApprovalRespond(p) => Some(&p.intent_id),
        Operation::BindingCreate(p) => Some(&p.intent_id),
        Operation::BindingResume(p) => Some(&p.intent_id),
        Operation::BindingAbandon(p) => Some(&p.intent_id),
        Operation::LeaseGrant(p) => Some(&p.intent_id),
        Operation::LeaseRenew(p) => Some(&p.intent_id),
        Operation::LeaseRevoke(p) => Some(&p.intent_id),
        Operation::Prepare(p) => Some(&p.intent_id),
        Operation::Close(p) => Some(&p.intent_id),
        Operation::RuntimeStart(p) | Operation::RuntimeStop(p) => Some(&p.intent_id),
        _ => None,
    }
}
fn enum_text(v: &impl serde::Serialize) -> ControlResult<String> {
    serde_json::to_value(v)
        .map_err(db)?
        .as_str()
        .map(str::to_owned)
        .ok_or(ControlError::InternalError)
}
fn parse_head(s: &str) -> ControlResult<u64> {
    if s == "0" {
        Ok(0)
    } else {
        s.parse::<Sequence>().map(|s| s.get()).map_err(invalid)
    }
}
fn next_sequence(tx: &Transaction<'_>) -> ControlResult<u64> {
    let head: String = tx
        .query_row(
            "SELECT last_sequence FROM authority_metadata WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(db)?;
    parse_head(&head)?
        .checked_add(1)
        .ok_or(ControlError::CapacityExceeded)
}
fn decode_receipt(op: &Operation, raw: &str) -> ControlResult<ResultValue> {
    if matches!(op, Operation::TurnCancel(_)) {
        let t: ManagedTurnRecord = serde_json::from_str(raw).map_err(invalid)?;
        t.validate()?;
        Ok(ResultValue::Turn(t))
    } else if matches!(op, Operation::ApprovalRespond(_)) {
        let r: ApprovalReceipt = serde_json::from_str(raw).map_err(invalid)?;
        r.validate()?;
        Ok(ResultValue::ApprovalReceipt(r))
    } else if matches!(
        op,
        Operation::BindingCreate(_) | Operation::BindingResume(_)
    ) {
        let b: SessionBinding = serde_json::from_str(raw).map_err(invalid)?;
        b.validate()?;
        Ok(ResultValue::Binding(b))
    } else if matches!(op, Operation::BindingAbandon(_)) {
        Ok(ResultValue::BindingAbandoned(
            serde_json::from_str(raw).map_err(invalid)?,
        ))
    } else if matches!(
        op,
        Operation::LeaseGrant(_) | Operation::LeaseRenew(_) | Operation::LeaseRevoke(_)
    ) {
        let l: ExecutionLease = serde_json::from_str(raw).map_err(invalid)?;
        l.validate()?;
        Ok(ResultValue::Lease(l))
    } else if matches!(op, Operation::RuntimeStart(_) | Operation::RuntimeStop(_)) {
        let r: RuntimeControlRecord = serde_json::from_str(raw).map_err(invalid)?;
        r.validate()?;
        Ok(ResultValue::RuntimeControl(r))
    } else {
        let s: ManagedSessionRecord = serde_json::from_str(raw).map_err(invalid)?;
        s.validate()?;
        Ok(ResultValue::Session(s))
    }
}
fn read_runtime(c: &Connection) -> ControlResult<Option<RuntimeControlRecord>> {
    let row:Option<(String,i64,String,String)>=c.query_row("SELECT desired_state,revision,created_at,updated_at FROM managed_runtimes WHERE adapter_id='codex'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(db)?;
    row.map(|(state,revision,created,updated)| {
        let r:RuntimeControlRecord=serde_json::from_value(json!({"adapter_id":"codex","runtime_kind":"codex","desired_state":state,"revision":revision,"created_at":created,"updated_at":updated})).map_err(invalid)?;
        r.validate()?; Ok(r)
    }).transpose()
}
fn validate_audit(v: &Value) -> ControlResult<()> {
    let kind = v["event_type"]
        .as_str()
        .ok_or(ControlError::StorageIncompatible)?;
    if kind.starts_with("authority.managed_turn.") {
        return turns::validate_audit(v);
    }
    if kind.starts_with("authority.managed_approval.") {
        return approvals::validate_audit(v);
    }
    if kind.starts_with("authority.managed_session.runtime_") {
        return bindings::validate_audit(v);
    }
    if kind.starts_with("authority.execution_lease.") {
        return leases::validate_audit(v);
    }
    if kind.starts_with("authority.runtime.") {
        if v["persistence"] != "durable"
            || v["sensitivity"] != "internal"
            || v["redaction"] != "none"
            || v["source"]["component"] != "wyss_control"
            || !v["session_id"].is_null()
            || !v["causation_id"].is_null()
            || v["payload_schema_version"] != 1
        {
            return Err(ControlError::StorageIncompatible);
        }
        let p = v["payload"]
            .as_object()
            .ok_or(ControlError::StorageIncompatible)?;
        let keys: &[&str] = match kind {
            "authority.runtime.desired_state_changed" => {
                &["adapter_id", "runtime_kind", "desired_state", "revision"]
            }
            "authority.runtime.instance_ready" => &[
                "adapter_id",
                "runtime_kind",
                "runtime_instance_id",
                "runtime_version",
                "compatibility",
            ],
            "authority.runtime.instance_exited" => &[
                "adapter_id",
                "runtime_kind",
                "runtime_instance_id",
                "exit_kind",
                "expected",
                "desired_state",
            ],
            _ => return Err(ControlError::StorageIncompatible),
        };
        if p.len() != keys.len()
            || keys.iter().any(|k| !p.contains_key(*k))
            || p["adapter_id"] != "codex"
            || p["runtime_kind"] != "codex"
        {
            return Err(ControlError::StorageIncompatible);
        }
        if kind.ends_with("desired_state_changed") {
            if v["correlation_id"].is_null()
                || !matches!(p["desired_state"].as_str(), Some("running" | "stopped"))
                || !p["revision"]
                    .as_u64()
                    .is_some_and(|n| n > 0 && n <= i64::MAX as u64)
            {
                return Err(ControlError::StorageIncompatible);
            }
        } else {
            crate::contracts::identities::AgentRuntimeInstanceId::parse(
                p["runtime_instance_id"]
                    .as_str()
                    .ok_or(ControlError::StorageIncompatible)?,
            )
            .map_err(invalid)?;
            if !v["correlation_id"].is_null() {
                return Err(ControlError::StorageIncompatible);
            }
            if kind.ends_with("instance_ready") {
                serde_json::from_value::<crate::contracts::agent_adapter::CompatibilityState>(
                    p["compatibility"].clone(),
                )
                .map_err(invalid)?;
                if p["runtime_version"]
                    .as_str()
                    .is_some_and(|s| s.len() > 128 || s.chars().any(char::is_control))
                    || !(p["runtime_version"].is_null() || p["runtime_version"].is_string())
                {
                    return Err(ControlError::StorageIncompatible);
                }
            } else {
                serde_json::from_value::<RuntimeDiagnostic>(p["exit_kind"].clone())
                    .map_err(invalid)?;
                if !p["expected"].is_boolean()
                    || !matches!(p["desired_state"].as_str(), Some("running" | "stopped"))
                {
                    return Err(ControlError::StorageIncompatible);
                }
            }
        }
        return Ok(());
    }
    let state = match kind {
        "authority.managed_session.prepared" => "prepared",
        "authority.managed_session.closed" => "closed",
        _ => return Err(ControlError::StorageIncompatible),
    };
    let p = v["payload"]
        .as_object()
        .ok_or(ControlError::StorageIncompatible)?;
    if p.len() != 4
        || p.get("control_state") != Some(&json!(state))
        || p.get("revision") != Some(&json!(if state == "prepared" { 1 } else { 2 }))
        || v["persistence"] != "durable"
        || v["sensitivity"] != "internal"
        || v["redaction"] != "none"
        || v["source"]["component"] != "wyss_control"
        || v["session_id"].is_null()
        || v["correlation_id"].is_null()
        || !v["causation_id"].is_null()
        || v["payload_schema_version"] != 1
    {
        return Err(ControlError::StorageIncompatible);
    }
    serde_json::from_value::<crate::contracts::agent_adapter::AgentAdapterId>(
        p.get("adapter_id")
            .ok_or(ControlError::StorageIncompatible)?
            .clone(),
    )
    .map_err(invalid)?;
    serde_json::from_value::<crate::contracts::agent_adapter::AgentRuntimeKind>(
        p.get("runtime_kind")
            .ok_or(ControlError::StorageIncompatible)?
            .clone(),
    )
    .map_err(invalid)?;
    Ok(())
}
type SessionRow = (String, String, String, String, i64, String, String, String);
fn get_session(c: &Connection, id: &str) -> ControlResult<ManagedSessionRecord> {
    let v:Option<SessionRow>=c.query_row("SELECT session_id,adapter_id,runtime_kind,control_state,revision,created_at,updated_at,created_sequence FROM managed_sessions WHERE session_id=?1",[canonical(id)],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).optional().map_err(db)?;
    let Some((id, adapter, runtime, state, revision, created, updated, seq)) = v else {
        return Err(ControlError::SessionNotFound);
    };
    let s:ManagedSessionRecord=serde_json::from_value(json!({"managed_session_id":id,"adapter_id":adapter,"runtime_kind":runtime,"control_state":state,"revision":u64::try_from(revision).map_err(invalid)?,"created_at":created,"updated_at":updated,"created_sequence":seq.trim_start_matches('0')})).map_err(invalid)?;
    s.validate()?;
    Ok(s)
}
