//! Sole-writer durable approval journal. No command, output, path or diff enters this module:
//! only the authority-computed fingerprint and bounded structural counts.
// The approval state machine is the only caller of the write path.
#![cfg_attr(not(test), allow(dead_code))]
use super::*;
use crate::contracts::identities::{
    AgentRuntimeInstanceId, ExecutionLeaseId, ManagedApprovalId, ManagedTurnId,
};
use std::collections::{HashMap, HashSet};

pub(super) const METHOD: &str = "managedTurn/approval/respond";
/// Every audit event kind and the only `(state, effect)` it may record.
const EVENTS: [(&str, ApprovalState, ApprovalEffect); 8] = [
    ("requested", ApprovalState::Pending, ApprovalEffect::None),
    (
        "accepted",
        ApprovalState::Accepted,
        ApprovalEffect::Dispatched,
    ),
    ("declined", ApprovalState::Declined, ApprovalEffect::None),
    ("cancelled", ApprovalState::Cancelled, ApprovalEffect::None),
    ("expired", ApprovalState::Expired, ApprovalEffect::None),
    (
        "invalidated",
        ApprovalState::Invalidated,
        ApprovalEffect::None,
    ),
    (
        "indeterminate",
        ApprovalState::Indeterminate,
        ApprovalEffect::Unknown,
    ),
    (
        "confirmed",
        ApprovalState::Accepted,
        ApprovalEffect::Confirmed,
    ),
];
const PAYLOAD: [&str; 12] = [
    "managed_approval_id",
    "managed_turn_id",
    "kind",
    "lease_id",
    "lease_revision",
    "runtime_instance_id",
    "state",
    "revision",
    "decision",
    "effect",
    "reason",
    "action_fingerprint",
];
/// Latest audited revision of one approval, rebuilt from the event log.
#[derive(PartialEq, Eq)]
pub(super) struct AuditHead {
    revision: u64,
    state: ApprovalState,
    effect: ApprovalEffect,
    turn: String,
    fingerprint: String,
}
pub(super) type AuditHeads = HashMap<String, AuditHead>;

/// Outcome of a decision intent. Only `Fresh` entitles the caller to answer the provider.
#[derive(Debug)]
pub(in crate::control) enum Decided {
    Fresh(Box<ManagedApprovalRecord>, ApprovalReceipt),
    Replayed(ApprovalReceipt),
}

fn read(c: &Connection, id: &str) -> ControlResult<ManagedApprovalRecord> {
    let raw: Option<String> = c.query_row("SELECT json_object('managed_approval_id',managed_approval_id,'managed_turn_id',managed_turn_id,'managed_session_id',managed_session_id,'runtime_instance_id',runtime_instance_id,'kind',kind,'provider_item_id',provider_item_id,'admitted_lease_id',admitted_lease_id,'admitted_lease_revision',admitted_lease_revision,'state',state,'revision',revision,'action_fingerprint',action_fingerprint,'structure',json(structure),'decision',decision,'effect',effect,'created_at',created_at,'updated_at',updated_at,'expires_at',expires_at,'terminal_at',terminal_at,'terminal_reason',terminal_reason,'created_sequence',ltrim(created_sequence,'0')) FROM managed_approvals WHERE managed_approval_id=?1", [canonical(id)], |r| r.get(0)).optional().map_err(db)?;
    let a: ManagedApprovalRecord =
        serde_json::from_str(&raw.ok_or(ControlError::ApprovalNotFound)?).map_err(invalid)?;
    a.validate().map_err(invalid)?;
    Ok(a)
}
fn insert(tx: &Transaction<'_>, a: &ManagedApprovalRecord) -> ControlResult<()> {
    a.validate()?;
    tx.execute("INSERT INTO managed_approvals VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)", params![
        a.managed_approval_id.as_str(),
        a.managed_turn_id.as_str(),
        a.managed_session_id.as_str(),
        a.runtime_instance_id.as_str(),
        enum_text(&a.kind)?,
        a.provider_item_id,
        a.admitted_lease_id.as_str(),
        a.admitted_lease_revision as i64,
        enum_text(&a.state)?,
        a.revision as i64,
        a.action_fingerprint,
        serde_json::to_string(&a.structure).map_err(db)?,
        a.decision.map(|d| enum_text(&d)).transpose()?,
        enum_text(&a.effect)?,
        enum_text(&a.created_at)?,
        enum_text(&a.updated_at)?,
        enum_text(&a.expires_at)?,
        a.terminal_at.as_ref().map(enum_text).transpose()?,
        a.terminal_reason.map(|r| enum_text(&r)).transpose()?,
        key(a.created_sequence.get()),
    ]).map_err(db)?;
    Ok(())
}
/// Writes the next revision of an existing approval. The schema trigger independently
/// refuses any change that is not a legal single-revision transition.
fn update(tx: &Transaction<'_>, a: &ManagedApprovalRecord) -> ControlResult<()> {
    a.validate()?;
    let changed = tx.execute("UPDATE managed_approvals SET state=?2,revision=?3,decision=?4,effect=?5,updated_at=?6,terminal_at=?7,terminal_reason=?8 WHERE managed_approval_id=?1 AND revision=?9", params![
        a.managed_approval_id.as_str(),
        enum_text(&a.state)?,
        a.revision as i64,
        a.decision.map(|d| enum_text(&d)).transpose()?,
        enum_text(&a.effect)?,
        enum_text(&a.updated_at)?,
        a.terminal_at.as_ref().map(enum_text).transpose()?,
        a.terminal_reason.map(|r| enum_text(&r)).transpose()?,
        (a.revision - 1) as i64,
    ]).map_err(|_| ControlError::InvalidTransition)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(ControlError::StaleRevision)
    }
}
fn audit(
    tx: &Transaction<'_>,
    instance: &WireUuid,
    stream: &WireUuid,
    a: &ManagedApprovalRecord,
    kind: &str,
    correlation: Option<&str>,
) -> ControlResult<u64> {
    let n = next_sequence(tx)?;
    let value = json!({"schema_version":1,"event_id":uuid(),"event_type":format!("authority.managed_approval.{kind}"),"payload_schema_version":1,"timestamp":a.updated_at,"stream_id":stream,"sequence":n.to_string(),"session_id":a.managed_session_id,"source":{"component":"wyss_control","instance_id":instance},"correlation_id":correlation,"causation_id":null,"payload":{"managed_approval_id":a.managed_approval_id,"managed_turn_id":a.managed_turn_id,"kind":a.kind,"lease_id":a.admitted_lease_id,"lease_revision":a.admitted_lease_revision,"runtime_instance_id":a.runtime_instance_id,"state":a.state,"revision":a.revision,"decision":a.decision,"effect":a.effect,"reason":a.terminal_reason,"action_fingerprint":a.action_fingerprint},"persistence":"durable","sensitivity":"internal","redaction":"none"});
    validate_audit(&value)?;
    tx.execute(
        "INSERT INTO control_events VALUES(?1,?2,?3,?4,?5)",
        params![
            value["event_id"].as_str().unwrap(),
            stream.as_str(),
            key(n),
            a.managed_session_id.as_str(),
            serde_json::to_string(&value).map_err(db)?
        ],
    )
    .map_err(db)?;
    tx.execute(
        "UPDATE authority_metadata SET last_sequence=?1 WHERE id=1",
        [n.to_string()],
    )
    .map_err(db)?;
    Ok(n)
}
pub(super) fn validate_audit(v: &Value) -> ControlResult<()> {
    let p = v["payload"]
        .as_object()
        .ok_or(ControlError::StorageIncompatible)?;
    let kind = v["event_type"]
        .as_str()
        .and_then(|k| k.strip_prefix("authority.managed_approval."))
        .ok_or(ControlError::StorageIncompatible)?;
    let Some((_, expected_state, expected_effect)) = EVENTS.iter().find(|(k, _, _)| *k == kind)
    else {
        return Err(ControlError::StorageIncompatible);
    };
    if p.len() != PAYLOAD.len()
        || PAYLOAD.iter().any(|k| !p.contains_key(*k))
        || v["persistence"] != "durable"
        || v["sensitivity"] != "internal"
        || v["redaction"] != "none"
        || v["source"]["component"] != "wyss_control"
        || v["payload_schema_version"] != 1
        || !v["causation_id"].is_null()
    {
        return Err(ControlError::StorageIncompatible);
    }
    serde_json::from_value::<ManagedSessionId>(v["session_id"].clone()).map_err(invalid)?;
    serde_json::from_value::<ManagedApprovalId>(p["managed_approval_id"].clone())
        .map_err(invalid)?;
    serde_json::from_value::<ManagedTurnId>(p["managed_turn_id"].clone()).map_err(invalid)?;
    serde_json::from_value::<ApprovalKind>(p["kind"].clone()).map_err(invalid)?;
    serde_json::from_value::<ExecutionLeaseId>(p["lease_id"].clone()).map_err(invalid)?;
    serde_json::from_value::<AgentRuntimeInstanceId>(p["runtime_instance_id"].clone())
        .map_err(invalid)?;
    let state: ApprovalState = serde_json::from_value(p["state"].clone()).map_err(invalid)?;
    let decision: Option<ApprovalDecision> =
        serde_json::from_value(p["decision"].clone()).map_err(invalid)?;
    let effect: ApprovalEffect = serde_json::from_value(p["effect"].clone()).map_err(invalid)?;
    let reason: Option<ApprovalReason> =
        serde_json::from_value(p["reason"].clone()).map_err(invalid)?;
    for field in ["revision", "lease_revision"] {
        valid_revision(p[field].as_u64().ok_or(ControlError::StorageIncompatible)?)
            .map_err(invalid)?;
    }
    let expected_decision = match state {
        ApprovalState::Accepted | ApprovalState::Indeterminate => Some(ApprovalDecision::Accept),
        ApprovalState::Declined => Some(ApprovalDecision::Decline),
        ApprovalState::Cancelled => Some(ApprovalDecision::Cancel),
        ApprovalState::Pending | ApprovalState::Expired | ApprovalState::Invalidated => None,
    };
    // A client decision is the only approval event caused by an intent.
    let caused_by_intent = matches!(kind, "accepted" | "declined" | "cancelled");
    if state != *expected_state
        || effect != *expected_effect
        || decision != expected_decision
        || reason.is_some() != state.terminal()
        || (reason == Some(ApprovalReason::ClientDecision))
            != (caused_by_intent || kind == "confirmed")
        || v["correlation_id"].is_null() == caused_by_intent
        || (kind == "requested" && p["revision"] != 1)
        || !p["action_fingerprint"]
            .as_str()
            .is_some_and(valid_fingerprint)
    {
        return Err(ControlError::StorageIncompatible);
    }
    Ok(())
}
/// Folds one already validated approval event into the per-approval audit heads, refusing
/// any log in which an approval does not advance by exactly one revision per event or
/// changes the turn or the action it is bound to.
pub(super) fn fold_audit(heads: &mut AuditHeads, v: &Value) -> ControlResult<()> {
    let p = &v["payload"];
    let text = |k: &str| {
        p[k].as_str()
            .map(str::to_owned)
            .ok_or(ControlError::StorageIncompatible)
    };
    let next = AuditHead {
        revision: p["revision"]
            .as_u64()
            .ok_or(ControlError::StorageIncompatible)?,
        state: serde_json::from_value(p["state"].clone()).map_err(invalid)?,
        effect: serde_json::from_value(p["effect"].clone()).map_err(invalid)?,
        turn: text("managed_turn_id")?,
        fingerprint: text("action_fingerprint")?,
    };
    let continuous = match heads.get(&text("managed_approval_id")?) {
        None => next.revision == 1,
        Some(old) => {
            next.revision == old.revision + 1
                && next.turn == old.turn
                && next.fingerprint == old.fingerprint
        }
    };
    if !continuous {
        return Err(ControlError::StorageIncompatible);
    }
    heads.insert(text("managed_approval_id")?, next);
    Ok(())
}
/// Canonical identifier case, so a retried intent compares equal byte for byte.
fn canonical_respond(p: &ApprovalRespond) -> ControlResult<ApprovalRespond> {
    valid_revision(p.expected_revision)?;
    Ok(ApprovalRespond {
        intent_id: canonical(p.intent_id.as_str())
            .try_into()
            .map_err(invalid)?,
        managed_approval_id: ManagedApprovalId::parse(&canonical(p.managed_approval_id.as_str()))
            .map_err(invalid)?,
        expected_revision: p.expected_revision,
        decision: p.decision,
    })
}
fn mutation(p: &ApprovalRespond) -> ControlResult<String> {
    serde_json::to_string(&Operation::ApprovalRespond(p.clone())).map_err(db)
}
/// Structural check of one stored decision receipt; returns the approval it decided.
pub(super) fn verify_receipt(id: &str, mutation: &str, result: &str) -> ControlResult<String> {
    let v = unique_json(mutation.as_bytes()).map_err(invalid)?;
    if v.as_object().is_none_or(|o| o.len() != 2) || v["method"] != METHOD {
        return Err(ControlError::StorageIncompatible);
    }
    let p: ApprovalRespond = serde_json::from_value(v["params"].clone()).map_err(invalid)?;
    valid_revision(p.expected_revision).map_err(invalid)?;
    let receipt: ApprovalReceipt = serde_json::from_str(result).map_err(invalid)?;
    receipt.validate()?;
    if p.intent_id.as_str() != id
        || p.managed_approval_id.as_str() != canonical(p.managed_approval_id.as_str())
        || receipt.managed_approval_id != p.managed_approval_id
        || receipt.decision != p.decision
        || receipt.revision != p.expected_revision + 1
    {
        return Err(ControlError::StorageIncompatible);
    }
    Ok(receipt.managed_approval_id.as_str().to_owned())
}
fn stamp(t: OffsetDateTime) -> ControlResult<crate::contracts::UtcTimestamp> {
    t.format(&Rfc3339)
        .map_err(invalid)?
        .try_into()
        .map_err(invalid)
}
fn scope(kind: ApprovalKind) -> ExecutionScope {
    match kind {
        ApprovalKind::CommandExecution => ExecutionScope::CommandExecute,
        ApprovalKind::FileChange => ExecutionScope::FileChange,
    }
}
fn bump(a: &mut ManagedApprovalRecord, at: OffsetDateTime) -> ControlResult<()> {
    a.revision = a
        .revision
        .checked_add(1)
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or(ControlError::CapacityExceeded)?;
    // The journal clock never runs backwards within one approval.
    a.updated_at = stamp(at.max(timestamp(&a.updated_at)?))?;
    if a.terminal_at.is_none() && a.state.terminal() {
        a.terminal_at = Some(a.updated_at.clone());
    }
    a.validate()
}
impl Store {
    pub(in crate::control) fn approval(
        &self,
        id: &ManagedApprovalId,
    ) -> ControlResult<ManagedApprovalRecord> {
        read(&self.conn, id.as_str())
    }
    pub(in crate::control) fn pending_approval(
        &self,
    ) -> ControlResult<Option<ManagedApprovalRecord>> {
        let id: Option<String> = self
            .conn
            .query_row(
                "SELECT managed_approval_id FROM managed_approvals WHERE state='pending'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        id.map(|id| read(&self.conn, &id)).transpose()
    }
    /// The approval that mediates one provider item of one turn, if any.
    pub(in crate::control) fn approval_for_item(
        &self,
        turn: &ManagedTurnId,
        provider_item_id: &str,
    ) -> ControlResult<Option<ManagedApprovalRecord>> {
        let id: Option<String> = self
            .conn
            .query_row(
                "SELECT managed_approval_id FROM managed_approvals WHERE managed_turn_id=?1 AND provider_item_id=?2",
                params![canonical(turn.as_str()), provider_item_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        id.map(|id| read(&self.conn, &id)).transpose()
    }
    /// One page of a turn's approvals in request order. `next` is set only when the page
    /// is full, exactly like every other listing of this authority.
    pub(in crate::control) fn approvals(
        &self,
        turn: &ManagedTurnId,
        after: Option<Sequence>,
        limit: u16,
    ) -> ControlResult<(Vec<ManagedApprovalRecord>, Option<Sequence>)> {
        if limit == 0 || limit > MAX_PAGE {
            return Err(ControlError::InvalidRequest);
        }
        let mut q = self
            .conn
            .prepare("SELECT managed_approval_id FROM managed_approvals WHERE managed_turn_id=?1 AND created_sequence>?2 ORDER BY created_sequence LIMIT ?3")
            .map_err(db)?;
        let ids = q
            .query_map(
                params![
                    canonical(turn.as_str()),
                    key(after.map_or(0, |s| s.get())),
                    limit
                ],
                |r| r.get::<_, String>(0),
            )
            .map_err(db)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db)?;
        let mut page = Vec::with_capacity(ids.len());
        for id in ids {
            page.push(read(&self.conn, &id)?);
        }
        let next = if page.len() == usize::from(limit) {
            page.last().map(|a| a.created_sequence)
        } else {
            None
        };
        Ok((page, next))
    }
    /// Records one provider approval request against the turn's ORIGINAL admitted lease.
    /// The deadline is the stricter of the approval TTL and that lease's expiry. A provider
    /// item is mediated at most once: a repeated request for it is never a new approval.
    pub(in crate::control) fn open_approval(
        &mut self,
        turn: &ManagedTurnId,
        runtime: &AgentRuntimeInstanceId,
        kind: ApprovalKind,
        provider_item_id: &str,
        action_fingerprint: &str,
        structure: ApprovalStructure,
    ) -> ControlResult<ManagedApprovalRecord> {
        if !valid_provider_item(provider_item_id)
            || !valid_fingerprint(action_fingerprint)
            || structure.validate(kind).is_err()
        {
            return Err(ControlError::InvalidRequest);
        }
        let now = self.clock.now();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let turn = turns::read(&tx, turn.as_str())?;
        if turn.state != TurnState::Running {
            return Err(ControlError::InvalidTransition);
        }
        if turn.runtime_instance_id != *runtime {
            return Err(ControlError::CorrelationError);
        }
        let (pending, repeated): (bool, bool) = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM managed_approvals WHERE state='pending'),EXISTS(SELECT 1 FROM managed_approvals WHERE managed_turn_id=?1 AND provider_item_id=?2)",
                params![turn.managed_turn_id.as_str(), provider_item_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db)?;
        if repeated {
            return Err(ControlError::CorrelationError);
        }
        if pending {
            return Err(ControlError::CapacityExceeded);
        }
        let lease = leases::read(&tx, turn.admitted_lease_id.as_str())?;
        let deadline = (now + time::Duration::seconds(APPROVAL_TTL_SECONDS))
            .min(timestamp(&lease.expires_at)?);
        if lease.state != LeaseState::Active
            || lease.session_id != turn.managed_session_id
            || deadline <= now
        {
            return Err(ControlError::GateDenied);
        }
        let at = stamp(now)?;
        let a = ManagedApprovalRecord {
            managed_approval_id: ManagedApprovalId::parse(uuid().as_str()).map_err(invalid)?,
            managed_turn_id: turn.managed_turn_id,
            managed_session_id: turn.managed_session_id,
            runtime_instance_id: turn.runtime_instance_id,
            kind,
            provider_item_id: provider_item_id.to_owned(),
            admitted_lease_id: turn.admitted_lease_id,
            admitted_lease_revision: turn.admitted_lease_revision,
            state: ApprovalState::Pending,
            revision: 1,
            action_fingerprint: action_fingerprint.to_owned(),
            structure,
            decision: None,
            effect: ApprovalEffect::None,
            created_at: at.clone(),
            updated_at: at,
            expires_at: stamp(deadline)?,
            terminal_at: None,
            terminal_reason: None,
            created_sequence: sequence(next_sequence(&tx)?)?,
        };
        insert(&tx, &a)?;
        if audit(&tx, &self.instance, &self.stream, &a, "requested", None)?
            != a.created_sequence.get()
        {
            return Err(ControlError::InternalError);
        }
        tx.commit().map_err(db)?;
        Ok(a)
    }
    /// Commits one client decision: state, decision, the external-effect dispatch marker for
    /// Accept, minimal audit and the fingerprint-only receipt, in a single transaction.
    ///
    /// Accept is granted only against the exact pending action, before its deadline, while
    /// the turn still runs under its ORIGINAL admitted lease and that lease is active,
    /// unexpired and carries the scope of this kind of action. The caller answers the
    /// provider only after a `Fresh` result; a `Replayed` receipt never causes a response.
    pub(in crate::control) fn decide_approval(
        &mut self,
        p: &ApprovalRespond,
        current_fingerprint: &str,
    ) -> ControlResult<Decided> {
        let p = canonical_respond(p)?;
        let mutation = mutation(&p)?;
        let now = self.clock.now();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let prior: Option<(String, String)> = tx
            .query_row(
                "SELECT mutation,result FROM processed_intents WHERE intent_id=?1",
                [p.intent_id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)?;
        if let Some((original, result)) = prior {
            if original != mutation {
                return Err(ControlError::IntentConflict);
            }
            let receipt: ApprovalReceipt = serde_json::from_str(&result).map_err(invalid)?;
            receipt.validate()?;
            return Ok(Decided::Replayed(receipt));
        }
        if tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM managed_session_operations WHERE operation_id=?1)",
                [p.intent_id.as_str()],
                |r| r.get::<_, bool>(0),
            )
            .map_err(db)?
        {
            return Err(ControlError::IntentConflict);
        }
        let mut a = read(&tx, p.managed_approval_id.as_str())?;
        if a.state != ApprovalState::Pending {
            return Err(ControlError::InvalidTransition);
        }
        if a.revision != p.expected_revision {
            return Err(ControlError::StaleRevision);
        }
        if a.action_fingerprint != current_fingerprint {
            return Err(ControlError::CorrelationError);
        }
        if now >= timestamp(&a.expires_at)? {
            return Err(ControlError::GateDenied);
        }
        if p.decision == ApprovalDecision::Accept {
            let turn = turns::read(&tx, a.managed_turn_id.as_str())?;
            let lease = leases::read(&tx, a.admitted_lease_id.as_str())?;
            let session = get_session(&tx, a.managed_session_id.as_str())?;
            if turn.state != TurnState::Running
                || turn.admitted_lease_id != a.admitted_lease_id
                || turn.admitted_lease_revision != a.admitted_lease_revision
                || turn.runtime_instance_id != a.runtime_instance_id
                || session.control_state != ControlState::Prepared
                || lease.session_id != a.managed_session_id
                || lease.state != LeaseState::Active
                || now >= timestamp(&lease.expires_at)?
                || !lease.scopes.contains(&scope(a.kind))
            {
                return Err(ControlError::GateDenied);
            }
        }
        a.state = p.decision.state();
        a.decision = Some(p.decision);
        a.effect = if p.decision == ApprovalDecision::Accept {
            ApprovalEffect::Dispatched
        } else {
            ApprovalEffect::None
        };
        a.terminal_reason = Some(ApprovalReason::ClientDecision);
        bump(&mut a, now)?;
        let receipt = ApprovalReceipt {
            managed_approval_id: a.managed_approval_id.clone(),
            managed_turn_id: a.managed_turn_id.clone(),
            kind: a.kind,
            decision: p.decision,
            state: a.state,
            revision: a.revision,
            action_fingerprint: a.action_fingerprint.clone(),
            decided_at: a.updated_at.clone(),
        };
        receipt.validate()?;
        update(&tx, &a)?;
        audit(
            &tx,
            &self.instance,
            &self.stream,
            &a,
            &enum_text(&a.state)?,
            Some(p.intent_id.as_str()),
        )?;
        tx.execute(
            "INSERT INTO processed_intents VALUES(?1,?2,?3)",
            params![
                p.intent_id.as_str(),
                mutation,
                serde_json::to_string(&receipt).map_err(db)?
            ],
        )
        .map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(Decided::Fresh(Box::new(a), receipt))
    }
    /// The provider resolved the accepted request and reported that exact item complete.
    pub(in crate::control) fn confirm_approval(
        &mut self,
        id: &ManagedApprovalId,
        turn: &ManagedTurnId,
        runtime: &AgentRuntimeInstanceId,
        provider_item_id: &str,
    ) -> ControlResult<ManagedApprovalRecord> {
        let now = self.clock.now();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let mut a = read(&tx, id.as_str())?;
        if a.managed_turn_id.as_str() != canonical(turn.as_str())
            || a.runtime_instance_id != *runtime
            || a.provider_item_id != provider_item_id
        {
            return Err(ControlError::CorrelationError);
        }
        if a.state != ApprovalState::Accepted {
            return Err(ControlError::InvalidTransition);
        }
        if a.effect == ApprovalEffect::Confirmed {
            return Ok(a);
        }
        a.effect = ApprovalEffect::Confirmed;
        bump(&mut a, now)?;
        update(&tx, &a)?;
        audit(&tx, &self.instance, &self.stream, &a, "confirmed", None)?;
        tx.commit().map_err(db)?;
        Ok(a)
    }
    /// Authority-side terminalization without a client decision, for the approvals of one
    /// turn or of every turn. An undecided approval expires (deadline reasons) or is
    /// invalidated (every other reason). An accepted approval whose provider outcome was
    /// never confirmed becomes indeterminate unless the reason is a mere deadline, which no
    /// longer concerns it. Final approvals are never touched and nothing is ever replayed.
    pub(in crate::control) fn settle_approvals(
        &mut self,
        turn: Option<&ManagedTurnId>,
        reason: ApprovalReason,
    ) -> ControlResult<Vec<ManagedApprovalRecord>> {
        if reason == ApprovalReason::ClientDecision {
            return Err(ControlError::InvalidRequest);
        }
        let deadline = matches!(
            reason,
            ApprovalReason::ApprovalTimeout | ApprovalReason::LeaseExpired
        );
        let now = self.clock.now();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let ids = {
            let mut q = tx
                .prepare("SELECT managed_approval_id FROM managed_approvals WHERE (state='pending' OR (state='accepted' AND effect='dispatched')) AND (?1 IS NULL OR managed_turn_id=?1) ORDER BY created_sequence")
                .map_err(db)?;
            q.query_map([turn.map(|t| canonical(t.as_str()))], |r| {
                r.get::<_, String>(0)
            })
            .map_err(db)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db)?
        };
        let mut settled = Vec::new();
        for id in ids {
            let mut a = read(&tx, &id)?;
            a.state = match a.state {
                ApprovalState::Pending if deadline => ApprovalState::Expired,
                ApprovalState::Pending => ApprovalState::Invalidated,
                _ if deadline => continue,
                _ => ApprovalState::Indeterminate,
            };
            if a.state == ApprovalState::Indeterminate {
                a.effect = ApprovalEffect::Unknown;
            }
            a.terminal_reason = Some(reason);
            bump(&mut a, now)?;
            update(&tx, &a)?;
            audit(
                &tx,
                &self.instance,
                &self.stream,
                &a,
                &enum_text(&a.state)?,
                None,
            )?;
            settled.push(a);
        }
        tx.commit().map_err(db)?;
        Ok(settled)
    }
    /// Deadline of the single pending approval, for the authority's own scheduler.
    pub(in crate::control) fn next_approval_expiry(&self) -> ControlResult<Option<OffsetDateTime>> {
        self.pending_approval()?
            .map(|a| timestamp(&a.expires_at))
            .transpose()
    }
    /// Expires the pending approval once its deadline has passed. The reason says which
    /// bound was the binding one: the approval TTL or the admitted lease's own expiry.
    pub(in crate::control) fn expire_approvals(
        &mut self,
    ) -> ControlResult<Vec<ManagedApprovalRecord>> {
        let Some(a) = self.pending_approval()? else {
            return Ok(Vec::new());
        };
        if self.clock.now() < timestamp(&a.expires_at)? {
            return Ok(Vec::new());
        }
        let lease = leases::read(&self.conn, a.admitted_lease_id.as_str())?;
        let reason = if timestamp(&lease.expires_at)? <= timestamp(&a.expires_at)? {
            ApprovalReason::LeaseExpired
        } else {
            ApprovalReason::ApprovalTimeout
        };
        self.settle_approvals(Some(&a.managed_turn_id), reason)
    }
    /// Startup reconciliation. No provider response is ever replayed: an undecided approval
    /// is invalidated and an accepted one without a confirmed outcome is indeterminate.
    pub(in crate::control) fn recover_approvals(&mut self) -> ControlResult<()> {
        self.settle_approvals(None, ApprovalReason::AuthorityRestarted)
            .map(|_| ())
    }
    /// Every approval row must agree with its turn, its lease, its request event, the
    /// latest audited revision of itself and, when decided, exactly one decision receipt.
    pub(super) fn verify_approvals(
        &self,
        heads: &AuditHeads,
        decided: &HashSet<String>,
    ) -> ControlResult<()> {
        let mut q = self
            .conn
            .prepare("SELECT managed_approval_id FROM managed_approvals")
            .map_err(invalid)?;
        let ids = q
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(invalid)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(invalid)?;
        if ids.len() != heads.len() {
            return Err(ControlError::StorageIncompatible);
        }
        let mut receipts = 0;
        for id in ids {
            let a = read(&self.conn, &id).map_err(invalid)?;
            let turn = turns::read(&self.conn, a.managed_turn_id.as_str()).map_err(invalid)?;
            let lease = leases::read(&self.conn, a.admitted_lease_id.as_str()).map_err(invalid)?;
            let requested: Option<String> = self
                .conn
                .query_row(
                    "SELECT json_extract(envelope,'$.event_type')||'|'||json_extract(envelope,'$.payload.managed_approval_id') FROM control_events WHERE sequence=?1",
                    [key(a.created_sequence.get())],
                    |r| r.get(0),
                )
                .optional()
                .map_err(invalid)?;
            if a.decision.is_some() != decided.contains(&id) {
                return Err(ControlError::StorageIncompatible);
            }
            receipts += usize::from(a.decision.is_some());
            let head = AuditHead {
                revision: a.revision,
                state: a.state,
                effect: a.effect,
                turn: a.managed_turn_id.as_str().to_owned(),
                fingerprint: a.action_fingerprint.clone(),
            };
            if id != canonical(&id)
                || heads.get(&id) != Some(&head)
                || requested != Some(format!("authority.managed_approval.requested|{id}"))
                || turn.managed_session_id != a.managed_session_id
                || turn.admitted_lease_id != a.admitted_lease_id
                || turn.admitted_lease_revision != a.admitted_lease_revision
                || turn.runtime_instance_id != a.runtime_instance_id
                || lease.session_id != a.managed_session_id
                || lease.revision < a.admitted_lease_revision
                || timestamp(&a.created_at)? < timestamp(&turn.created_at)?
            {
                return Err(ControlError::StorageIncompatible);
            }
        }
        if receipts != decided.len() {
            return Err(ControlError::StorageIncompatible);
        }
        Ok(())
    }
}
#[cfg(test)]
#[path = "approval_tests.rs"]
mod tests;
