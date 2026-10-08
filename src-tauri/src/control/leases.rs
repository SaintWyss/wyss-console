//! The authority core owns all lease writes, including autonomous deadlines.
use super::*;
use crate::contracts::identities::ExecutionLeaseId;
#[cfg(test)]
use std::sync::Arc;

pub(in crate::control) trait Clock: Send + Sync {
    fn now(&self) -> OffsetDateTime;
}
pub(super) struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}
fn text(t: OffsetDateTime) -> ControlResult<String> {
    t.format(&Rfc3339).map_err(invalid)
}
fn utc(t: OffsetDateTime) -> ControlResult<crate::contracts::UtcTimestamp> {
    text(t)?.try_into().map_err(invalid)
}
fn increment(l: &mut ExecutionLease) -> ControlResult<()> {
    l.revision = l
        .revision
        .checked_add(1)
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or(ControlError::CapacityExceeded)?;
    Ok(())
}
pub(super) fn canonicalize(op: &Operation, v: &mut Value) {
    match op {
        Operation::LeaseGrant(p) => {
            let mut scopes = p.scopes.clone();
            scopes.sort();
            v["params"]["scopes"] = json!(scopes);
            v["params"]["session_id"] = json!(canonical(p.session_id.as_str()));
        }
        Operation::LeaseRenew(p) => v["params"]["lease_id"] = json!(canonical(p.lease_id.as_str())),
        Operation::LeaseRevoke(p) => {
            v["params"]["lease_id"] = json!(canonical(p.lease_id.as_str()))
        }
        _ => (),
    }
}
type Row = (
    String,
    String,
    String,
    String,
    i64,
    String,
    String,
    String,
    Option<String>,
);
pub(super) fn read(c: &Connection, id: &str) -> ControlResult<ExecutionLease> {
    let row:Option<Row>=c.query_row("SELECT lease_id,session_id,state,scopes,revision,issued_at,expires_at,updated_at,revocation_reason FROM execution_leases WHERE lease_id=?1",[canonical(id)],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).optional().map_err(db)?;
    let Some((id, session, state, scopes, revision, issued, expires, updated, reason)) = row else {
        return Err(ControlError::LeaseNotFound);
    };
    let scopes: Vec<ExecutionScope> =
        serde_json::from_value(unique_json(scopes.as_bytes()).map_err(invalid)?)
            .map_err(invalid)?;
    let l:ExecutionLease=serde_json::from_value(json!({"lease_id":id,"session_id":session,"state":state,"scopes":scopes,"revision":revision,"issued_at":issued,"expires_at":expires,"updated_at":updated,"revocation_reason":reason})).map_err(invalid)?;
    l.validate().map_err(invalid)?;
    if l.state != LeaseState::Active && l.revision < 2 {
        return Err(ControlError::StorageIncompatible);
    }
    Ok(l)
}
pub(super) fn active(c: &Connection, session: &str) -> ControlResult<Option<ExecutionLease>> {
    let id: Option<String> = c
        .query_row(
            "SELECT lease_id FROM execution_leases WHERE session_id=?1 AND state='active'",
            [canonical(session)],
            |r| r.get(0),
        )
        .optional()
        .map_err(db)?;
    id.map(|id| read(c, &id)).transpose()
}
fn write(c: &Connection, l: &ExecutionLease) -> ControlResult<()> {
    l.validate()?;
    c.execute("INSERT INTO execution_leases VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(lease_id) DO UPDATE SET state=excluded.state,revision=excluded.revision,expires_at=excluded.expires_at,updated_at=excluded.updated_at,revocation_reason=excluded.revocation_reason",params![l.lease_id.as_str(),l.session_id.as_str(),enum_text(&l.state)?,serde_json::to_string(&l.scopes).map_err(db)?,l.revision as i64,enum_text(&l.issued_at)?,enum_text(&l.expires_at)?,enum_text(&l.updated_at)?,l.revocation_reason.map(|r|enum_text(&r)).transpose()?]).map_err(db)?;
    Ok(())
}
fn event(
    tx: &Transaction<'_>,
    instance: &WireUuid,
    stream: &WireUuid,
    l: &ExecutionLease,
    intent: Option<&str>,
    kind: &str,
    now: OffsetDateTime,
) -> ControlResult<()> {
    let mut payload = json!({"lease_id":l.lease_id,"scopes":l.scopes,"lease_state":l.state,"revision":l.revision,"expires_at":l.expires_at});
    if let Some(reason) = l.revocation_reason {
        payload["revocation_reason"] = json!(reason);
    }
    events::append(
        tx,
        instance,
        stream,
        events::Draft {
            kind,
            session: Some(l.session_id.as_str()),
            correlation: intent,
            timestamp: text(now)?,
            payload,
        },
    )?;
    Ok(())
}
fn expire(
    tx: &Transaction<'_>,
    instance: &WireUuid,
    stream: &WireUuid,
    mut l: ExecutionLease,
    now: OffsetDateTime,
) -> ControlResult<ExecutionLease> {
    l.state = LeaseState::Expired;
    increment(&mut l)?;
    l.updated_at = utc(now)?;
    write(tx, &l)?;
    event(
        tx,
        instance,
        stream,
        &l,
        None,
        "authority.execution_lease.expired",
        now,
    )?;
    Ok(l)
}
pub(super) fn mutate(
    tx: &Transaction<'_>,
    instance: &WireUuid,
    stream: &WireUuid,
    op: &Operation,
    now: OffsetDateTime,
) -> ControlResult<ExecutionLease> {
    let intent = canonical(
        super::intent(op)
            .ok_or(ControlError::InvalidRequest)?
            .as_str(),
    );
    let (l, kind) = match op {
        Operation::LeaseGrant(p) => {
            let session = get_session(tx, p.session_id.as_str())?;
            if session.control_state != ControlState::Prepared {
                return Err(ControlError::InvalidTransition);
            }
            if session.revision != p.expected_session_revision {
                return Err(ControlError::StaleRevision);
            }
            if let Some(l) = active(tx, p.session_id.as_str())? {
                if now >= timestamp(&l.expires_at)? {
                    expire(tx, instance, stream, l, now)?;
                } else {
                    return Err(ControlError::LeaseAlreadyActive);
                }
            }
            let mut scopes = p.scopes.clone();
            scopes.sort();
            let mut id = ExecutionLeaseId::parse(uuid().as_str()).map_err(invalid)?;
            while id.as_str() == session.managed_session_id.as_str() {
                id = ExecutionLeaseId::parse(uuid().as_str()).map_err(invalid)?;
            }
            (
                ExecutionLease {
                    lease_id: id,
                    session_id: session.managed_session_id,
                    state: LeaseState::Active,
                    scopes,
                    revision: 1,
                    issued_at: utc(now)?,
                    expires_at: utc(now + time::Duration::seconds(i64::from(p.ttl_seconds)))?,
                    updated_at: utc(now)?,
                    revocation_reason: None,
                },
                "authority.execution_lease.granted",
            )
        }
        Operation::LeaseRenew(p) => {
            let mut l = read(tx, p.lease_id.as_str())?;
            if l.state != LeaseState::Active || now >= timestamp(&l.expires_at)? {
                return Err(ControlError::InvalidTransition);
            }
            if get_session(tx, l.session_id.as_str())?.control_state != ControlState::Prepared {
                return Err(ControlError::InvalidTransition);
            }
            if l.revision != p.expected_revision {
                return Err(ControlError::StaleRevision);
            }
            increment(&mut l)?;
            l.updated_at = utc(now)?;
            l.expires_at = utc(now + time::Duration::seconds(i64::from(p.ttl_seconds)))?;
            (l, "authority.execution_lease.renewed")
        }
        Operation::LeaseRevoke(p) => {
            let mut l = read(tx, p.lease_id.as_str())?;
            if l.state != LeaseState::Active || now >= timestamp(&l.expires_at)? {
                return Err(ControlError::InvalidTransition);
            }
            if l.revision != p.expected_revision {
                return Err(ControlError::StaleRevision);
            }
            increment(&mut l)?;
            l.state = LeaseState::Revoked;
            l.revocation_reason = Some(RevocationReason::UserRequest);
            l.updated_at = utc(now)?;
            (l, "authority.execution_lease.revoked")
        }
        _ => return Err(ControlError::InvalidRequest),
    };
    write(tx, &l)?;
    event(tx, instance, stream, &l, Some(&intent), kind, now)?;
    Ok(l)
}
pub(super) fn close(
    tx: &Transaction<'_>,
    instance: &WireUuid,
    stream: &WireUuid,
    session: &ManagedSessionRecord,
    intent: &str,
    now: OffsetDateTime,
) -> ControlResult<()> {
    if let Some(mut l) = active(tx, session.managed_session_id.as_str())? {
        if now >= timestamp(&l.expires_at)? {
            expire(tx, instance, stream, l, now)?;
        } else {
            increment(&mut l)?;
            l.state = LeaseState::Revoked;
            l.revocation_reason = Some(RevocationReason::SessionClosed);
            l.updated_at = utc(now)?;
            write(tx, &l)?;
            event(
                tx,
                instance,
                stream,
                &l,
                Some(intent),
                "authority.execution_lease.revoked",
                now,
            )?;
        }
    }
    Ok(())
}
impl Store {
    pub(in crate::control) fn lease_now(&self) -> OffsetDateTime {
        self.clock.now()
    }
    #[cfg(test)]
    pub(in crate::control) fn set_clock(&mut self, c: Arc<dyn Clock>) {
        self.clock = c;
    }
    /// Every lease row is the latest revision the log recorded for it, and nothing else is.
    pub(super) fn verify_leases(
        &self,
        heads: &std::collections::HashMap<String, events::Head>,
    ) -> ControlResult<()> {
        let mut q = self
            .conn
            .prepare("SELECT lease_id FROM execution_leases")
            .map_err(invalid)?;
        let ids = q
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(invalid)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(invalid)?;
        if ids.len() != heads.len() {
            return Err(ControlError::StorageIncompatible);
        }
        for id in ids {
            let l = read(&self.conn, &id)?;
            if l.state == LeaseState::Active
                && get_session(&self.conn, l.session_id.as_str())?.control_state
                    != ControlState::Prepared
            {
                return Err(ControlError::StorageIncompatible);
            }
            if heads.get(&id)
                != Some(&events::Head {
                    state: enum_text(&l.state).map_err(invalid)?,
                    revision: l.revision,
                })
            {
                return Err(ControlError::StorageIncompatible);
            }
        }
        Ok(())
    }
    pub(in crate::control) fn next_expiry(&self) -> ControlResult<Option<OffsetDateTime>> {
        let mut q = self
            .conn
            .prepare("SELECT expires_at FROM execution_leases WHERE state='active'")
            .map_err(db)?;
        let rows = q.query_map([], |r| r.get::<_, String>(0)).map_err(db)?;
        let mut earliest = None;
        for t in rows {
            let t = OffsetDateTime::parse(&t.map_err(db)?, &Rfc3339).map_err(invalid)?;
            earliest = Some(earliest.map_or(t, |a: OffsetDateTime| a.min(t)));
        }
        Ok(earliest)
    }
    pub(in crate::control) fn expire_due(&mut self) -> ControlResult<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let now = self.clock.now();
        // Collect identifiers only; no second writer or permission cache.
        let ids = {
            let mut q = tx
                .prepare(
                    "SELECT lease_id FROM execution_leases WHERE state='active' ORDER BY lease_id",
                )
                .map_err(db)?;
            q.query_map([], |r| r.get::<_, String>(0))
                .map_err(db)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(db)?
        };
        for id in ids {
            let l = read(&tx, &id)?;
            if now >= timestamp(&l.expires_at)? {
                expire(&tx, &self.instance, &self.stream, l, now)?;
            }
        }
        tx.commit().map_err(db)
    }
    pub(in crate::control) fn gate_session(
        &self,
        id: &ManagedSessionId,
    ) -> ControlResult<ManagedSessionRecord> {
        get_session(&self.conn, id.as_str())
    }
    pub(in crate::control) fn gate_lease(
        &self,
        id: &ManagedSessionId,
    ) -> ControlResult<Option<ExecutionLease>> {
        if let Some(l) = active(&self.conn, id.as_str())? {
            return Ok(Some(l));
        }
        let last:Option<String>=self.conn.query_row("SELECT lease_id FROM execution_leases WHERE session_id=?1 ORDER BY rowid DESC LIMIT 1",[canonical(id.as_str())],|r|r.get(0)).optional().map_err(db)?;
        last.map(|id| read(&self.conn, &id)).transpose()
    }
}
pub(super) fn validate_audit(v: &Value) -> ControlResult<()> {
    let kind = v["event_type"]
        .as_str()
        .ok_or(ControlError::StorageIncompatible)?;
    let state = match kind {
        "authority.execution_lease.granted" | "authority.execution_lease.renewed" => {
            LeaseState::Active
        }
        "authority.execution_lease.revoked" => LeaseState::Revoked,
        "authority.execution_lease.expired" => LeaseState::Expired,
        _ => return Err(ControlError::StorageIncompatible),
    };
    let p = v["payload"]
        .as_object()
        .ok_or(ControlError::StorageIncompatible)?;
    let required = [
        "lease_id",
        "scopes",
        "lease_state",
        "revision",
        "expires_at",
    ];
    if p.len() != required.len() + usize::from(state == LeaseState::Revoked)
        || required.iter().any(|k| !p.contains_key(*k))
        || p["lease_state"] != json!(state)
        || v["session_id"].is_null()
        || v["persistence"] != "durable"
        || v["sensitivity"] != "internal"
        || v["redaction"] != "none"
        || v["source"]["component"] != "wyss_control"
        || !v["causation_id"].is_null()
        || v["payload_schema_version"] != 1
        || (state == LeaseState::Expired) != v["correlation_id"].is_null()
    {
        return Err(ControlError::StorageIncompatible);
    }
    ExecutionLeaseId::parse(
        p["lease_id"]
            .as_str()
            .ok_or(ControlError::StorageIncompatible)?,
    )
    .map_err(invalid)?;
    let scopes: Vec<ExecutionScope> =
        serde_json::from_value(p["scopes"].clone()).map_err(invalid)?;
    validate_scopes(&scopes).map_err(invalid)?;
    if scopes.windows(2).any(|w| w[0] >= w[1]) {
        return Err(ControlError::StorageIncompatible);
    }
    valid_revision(
        p["revision"]
            .as_u64()
            .ok_or(ControlError::StorageIncompatible)?,
    )
    .map_err(invalid)?;
    serde_json::from_value::<crate::contracts::UtcTimestamp>(p["expires_at"].clone())
        .map_err(invalid)?;
    if state == LeaseState::Revoked {
        serde_json::from_value::<RevocationReason>(
            p.get("revocation_reason")
                .ok_or(ControlError::StorageIncompatible)?
                .clone(),
        )
        .map_err(invalid)?;
    }
    Ok(())
}
#[cfg(test)]
#[path = "lease_tests.rs"]
mod tests;
