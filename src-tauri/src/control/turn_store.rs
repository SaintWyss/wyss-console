//! Sole-writer durable turn journal. No prompt, output or provider frames enter this module.
use super::*;
use crate::adapters::codex::CodexTurnId;
use crate::contracts::identities::{AgentRuntimeInstanceId, ManagedTurnId};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::control) struct StartFingerprint {
    pub intent_id: WireUuid,
    pub managed_session_id: ManagedSessionId,
    pub expected_session_revision: u64,
    pub prompt_fingerprint: String,
    pub prompt_byte_length: usize,
}
impl StartFingerprint {
    pub fn compute(p: &TurnStart) -> ControlResult<Self> {
        validate_prompt(&p.prompt)?;
        valid_revision(p.expected_session_revision)?;
        Ok(Self {
            intent_id: canonical(p.intent_id.as_str())
                .try_into()
                .map_err(invalid)?,
            managed_session_id: ManagedSessionId::parse(&canonical(p.managed_session_id.as_str()))
                .map_err(invalid)?,
            expected_session_revision: p.expected_session_revision,
            prompt_fingerprint: format!("{:x}", Sha256::digest(p.prompt.as_bytes())),
            prompt_byte_length: p.prompt.len(),
        })
    }
    fn raw(&self) -> ControlResult<String> {
        serde_json::to_string(&json!({"method":"managedSession/turn/start","params":self}))
            .map_err(db)
    }
}
pub(super) fn read(c: &Connection, id: &str) -> ControlResult<ManagedTurnRecord> {
    let raw: Option<String> = c.query_row("SELECT json_object('managed_turn_id',managed_turn_id,'managed_session_id',managed_session_id,'codex_thread_id',codex_thread_id,'codex_turn_id',codex_turn_id,'admitted_lease_id',admitted_lease_id,'admitted_lease_revision',admitted_lease_revision,'runtime_instance_id',runtime_instance_id,'state',state,'revision',revision,'created_at',created_at,'updated_at',updated_at,'terminal_at',terminal_at,'terminal_reason',terminal_reason) FROM managed_turns WHERE managed_turn_id=?1", [canonical(id)], |r|r.get(0)).optional().map_err(db)?;
    let t: ManagedTurnRecord =
        serde_json::from_str(&raw.ok_or(ControlError::TurnNotFound)?).map_err(invalid)?;
    t.validate().map_err(invalid)?;
    Ok(t)
}
/// One turn event. `at` is when what the event records happened: the turn's own update for
/// a state change, the authority clock for an intent that changes no state.
fn audit(
    tx: &Transaction<'_>,
    instance: &WireUuid,
    stream: &WireUuid,
    t: &ManagedTurnRecord,
    kind: &str,
    correlation: Option<&str>,
    (reason, at): (Option<TurnReason>, &crate::contracts::UtcTimestamp),
) -> ControlResult<()> {
    events::append(
        tx,
        instance,
        stream,
        events::Draft {
            kind: &format!("authority.managed_turn.{kind}"),
            session: Some(t.managed_session_id.as_str()),
            correlation,
            timestamp: enum_text(at)?,
            payload: json!({"managed_turn_id":t.managed_turn_id,"codex_turn_id":t.codex_turn_id,"lease_id":t.admitted_lease_id,"lease_revision":t.admitted_lease_revision,"runtime_instance_id":t.runtime_instance_id,"state":t.state,"revision":t.revision,"reason":reason}),
        },
    )?;
    Ok(())
}
pub(super) fn validate_audit(v: &Value) -> ControlResult<()> {
    let p = v["payload"]
        .as_object()
        .ok_or(ControlError::StorageIncompatible)?;
    let kind = v["event_type"]
        .as_str()
        .ok_or(ControlError::StorageIncompatible)?;
    if ![
        "admitted",
        "running",
        "completed",
        "interrupted",
        "failed",
        "indeterminate",
        "cancel_requested",
    ]
    .iter()
    .any(|k| kind == format!("authority.managed_turn.{k}"))
        || p.len() != 8
        || [
            "managed_turn_id",
            "codex_turn_id",
            "lease_id",
            "lease_revision",
            "runtime_instance_id",
            "state",
            "revision",
            "reason",
        ]
        .iter()
        .any(|k| !p.contains_key(*k))
        || v["persistence"] != "durable"
        || v["sensitivity"] != "internal"
        || v["redaction"] != "none"
        || v["source"]["component"] != "wyss_control"
        || !v["causation_id"].is_null()
        || v["payload_schema_version"] != 1
    {
        return Err(ControlError::StorageIncompatible);
    }
    serde_json::from_value::<ManagedSessionId>(v["session_id"].clone()).map_err(invalid)?;
    serde_json::from_value::<ManagedTurnId>(p["managed_turn_id"].clone()).map_err(invalid)?;
    serde_json::from_value::<Option<CodexTurnId>>(p["codex_turn_id"].clone()).map_err(invalid)?;
    serde_json::from_value::<crate::contracts::identities::ExecutionLeaseId>(p["lease_id"].clone())
        .map_err(invalid)?;
    serde_json::from_value::<AgentRuntimeInstanceId>(p["runtime_instance_id"].clone())
        .map_err(invalid)?;
    serde_json::from_value::<TurnState>(p["state"].clone()).map_err(invalid)?;
    serde_json::from_value::<Option<TurnReason>>(p["reason"].clone()).map_err(invalid)?;
    valid_revision(
        p["revision"]
            .as_u64()
            .ok_or(ControlError::StorageIncompatible)?,
    )?;
    valid_revision(
        p["lease_revision"]
            .as_u64()
            .ok_or(ControlError::StorageIncompatible)?,
    )?;
    Ok(())
}
pub(super) fn verify_fingerprint_receipt(
    id: &str,
    mutation: &str,
    result: &str,
) -> ControlResult<()> {
    let v = unique_json(mutation.as_bytes()).map_err(invalid)?;
    if v.as_object().is_none_or(|o| o.len() != 2) || v["method"] != "managedSession/turn/start" {
        return Err(ControlError::StorageIncompatible);
    }
    let p: StartFingerprint = serde_json::from_value(v["params"].clone()).map_err(invalid)?;
    if p.intent_id.as_str() != id
        || !(1..=8192).contains(&p.prompt_byte_length)
        || p.prompt_fingerprint.len() != 64
        || !p
            .prompt_fingerprint
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(ControlError::StorageIncompatible);
    }
    valid_revision(p.expected_session_revision)?;
    let t: ManagedTurnRecord = serde_json::from_str(result).map_err(invalid)?;
    t.validate()?;
    if t.managed_session_id != p.managed_session_id {
        return Err(ControlError::StorageIncompatible);
    }
    Ok(())
}
impl Store {
    pub(in crate::control) fn turn(&self, id: &ManagedTurnId) -> ControlResult<ManagedTurnRecord> {
        read(&self.conn, id.as_str())
    }
    pub(in crate::control) fn active_turn(&self) -> ControlResult<Option<ManagedTurnRecord>> {
        let id: Option<String> = self
            .conn
            .query_row(
                "SELECT managed_turn_id FROM managed_turns WHERE state IN ('starting','running')",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        id.map(|id| read(&self.conn, &id)).transpose()
    }
    pub(in crate::control) fn turn_receipt(
        &self,
        f: &StartFingerprint,
    ) -> ControlResult<Option<ManagedTurnRecord>> {
        let prior: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT mutation,result FROM processed_intents WHERE intent_id=?1",
                [f.intent_id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)?;
        prior
            .map(|(m, r)| {
                if m != f.raw()? {
                    return Err(ControlError::IntentConflict);
                }
                let t: ManagedTurnRecord = serde_json::from_str(&r).map_err(invalid)?;
                t.validate()?;
                Ok(t)
            })
            .transpose()
    }
    pub(in crate::control) fn reserve_turn(
        &mut self,
        f: &StartFingerprint,
        binding: &SessionBinding,
        lease: &ExecutionLease,
        runtime: &AgentRuntimeInstanceId,
    ) -> ControlResult<ManagedTurnRecord> {
        if self.active_turn()?.is_some() {
            return Err(ControlError::ActiveTurnExists);
        }
        let session = get_session(&self.conn, f.managed_session_id.as_str())?;
        if session.revision != f.expected_session_revision {
            return Err(ControlError::StaleRevision);
        }
        if session.control_state != ControlState::Prepared {
            return Err(ControlError::InvalidTransition);
        }
        let at: crate::contracts::UtcTimestamp = self
            .lease_now()
            .format(&Rfc3339)
            .map_err(invalid)?
            .try_into()
            .map_err(invalid)?;
        let t = ManagedTurnRecord {
            managed_turn_id: ManagedTurnId::parse(uuid().as_str()).map_err(invalid)?,
            managed_session_id: session.managed_session_id,
            codex_thread_id: binding.runtime_session_id.clone(),
            codex_turn_id: None,
            admitted_lease_id: lease.lease_id.clone(),
            admitted_lease_revision: lease.revision,
            runtime_instance_id: runtime.clone(),
            state: TurnState::Starting,
            revision: 1,
            created_at: at.clone(),
            updated_at: at,
            terminal_at: None,
            terminal_reason: None,
        };
        let mutation = f.raw()?;
        let result = serde_json::to_string(&t).map_err(db)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        tx.execute("INSERT INTO managed_turns VALUES(?1,?2,?3,NULL,?4,?5,?6,'starting',1,?7,?8,?9,?10,1,?11,?11,NULL,NULL)",params![t.managed_turn_id.as_str(),t.managed_session_id.as_str(),t.codex_thread_id.as_str(),t.admitted_lease_id.as_str(),t.admitted_lease_revision as i64,t.runtime_instance_id.as_str(),f.prompt_fingerprint,f.prompt_byte_length as i64,f.intent_id.as_str(),mutation,enum_text(&t.created_at)?]).map_err(db)?;
        audit(
            &tx,
            &self.instance,
            &self.stream,
            &t,
            "admitted",
            Some(f.intent_id.as_str()),
            (None, &t.updated_at),
        )?;
        tx.execute(
            "INSERT INTO processed_intents VALUES(?1,?2,?3)",
            params![f.intent_id.as_str(), mutation, result],
        )
        .map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(t)
    }
    pub(in crate::control) fn transition_turn(
        &mut self,
        id: &ManagedTurnId,
        provider: Option<&CodexTurnId>,
        state: TurnState,
        reason: Option<TurnReason>,
    ) -> ControlResult<ManagedTurnRecord> {
        let mut t = self.turn(id)?;
        if t.state.terminal() {
            return Ok(t);
        }
        if let Some(p) = provider {
            if t.codex_turn_id.as_ref().is_some_and(|old| old != p) {
                return Err(ControlError::CorrelationError);
            }
            t.codex_turn_id = Some(p.clone());
        }
        if t.state == state {
            return Ok(t);
        }
        t.state = state;
        t.revision = t
            .revision
            .checked_add(1)
            .ok_or(ControlError::CapacityExceeded)?;
        t.updated_at = self
            .lease_now()
            .format(&Rfc3339)
            .map_err(invalid)?
            .try_into()
            .map_err(invalid)?;
        if state.terminal() {
            t.terminal_at = Some(t.updated_at.clone());
            t.terminal_reason = reason;
        }
        t.validate()?;
        let tx = self.conn.transaction().map_err(db)?;
        tx.execute("UPDATE managed_turns SET codex_turn_id=?2,state=?3,revision=?4,updated_at=?5,terminal_at=?6,terminal_reason=?7 WHERE managed_turn_id=?1",params![id.as_str(),t.codex_turn_id.as_ref().map(CodexTurnId::as_str),enum_text(&t.state)?,t.revision as i64,enum_text(&t.updated_at)?,t.terminal_at.as_ref().map(enum_text).transpose()?,t.terminal_reason.map(|v|enum_text(&v)).transpose()?]).map_err(db)?;
        audit(
            &tx,
            &self.instance,
            &self.stream,
            &t,
            &enum_text(&state)?,
            None,
            (reason, &t.updated_at),
        )?;
        tx.commit().map_err(db)?;
        Ok(t)
    }
    pub(in crate::control) fn cancel_turn_receipt(
        &mut self,
        p: &TurnCancel,
        reason: TurnReason,
    ) -> ControlResult<ManagedTurnRecord> {
        let mut p = p.clone();
        p.intent_id = canonical(p.intent_id.as_str())
            .try_into()
            .map_err(invalid)?;
        p.managed_turn_id =
            ManagedTurnId::parse(&canonical(p.managed_turn_id.as_str())).map_err(invalid)?;
        let op = Operation::TurnCancel(p.clone());
        let mutation = serde_json::to_string(&op).map_err(db)?;
        let old: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT mutation,result FROM processed_intents WHERE intent_id=?1",
                [p.intent_id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)?;
        if let Some((m, r)) = old {
            if m != mutation {
                return Err(ControlError::IntentConflict);
            }
            return serde_json::from_str(&r).map_err(invalid);
        }
        let t = self.turn(&p.managed_turn_id)?;
        if t.state.terminal() {
            return Err(ControlError::InvalidTransition);
        }
        if t.revision != p.expected_revision {
            return Err(ControlError::StaleRevision);
        }
        // The request is recorded when it was made. The turn itself is unchanged, and the
        // journal clock never runs behind the turn it describes.
        let requested: crate::contracts::UtcTimestamp = self
            .lease_now()
            .max(timestamp(&t.updated_at)?)
            .format(&Rfc3339)
            .map_err(invalid)?
            .try_into()
            .map_err(invalid)?;
        let tx = self.conn.transaction().map_err(db)?;
        audit(
            &tx,
            &self.instance,
            &self.stream,
            &t,
            "cancel_requested",
            Some(p.intent_id.as_str()),
            (Some(reason), &requested),
        )?;
        tx.execute(
            "INSERT INTO processed_intents VALUES(?1,?2,?3)",
            params![
                p.intent_id.as_str(),
                mutation,
                serde_json::to_string(&t).map_err(db)?
            ],
        )
        .map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(t)
    }
    pub(in crate::control) fn turn_lease(
        &self,
        t: &ManagedTurnRecord,
    ) -> ControlResult<ExecutionLease> {
        leases::read(&self.conn, t.admitted_lease_id.as_str())
    }
    pub(in crate::control) fn recover_turns(&mut self) -> ControlResult<()> {
        if let Some(t) = self.active_turn()? {
            self.transition_turn(
                &t.managed_turn_id,
                None,
                TurnState::Indeterminate,
                Some(TurnReason::AuthorityRestarted),
            )?;
        }
        Ok(())
    }
    /// Every turn row is the latest revision the log recorded for it, and nothing else is.
    pub(super) fn verify_turns(
        &self,
        heads: &std::collections::HashMap<String, events::Head>,
    ) -> ControlResult<()> {
        let count: i64 = self
            .conn
            .query_row("SELECT count(*) FROM managed_turns", [], |r| r.get(0))
            .map_err(invalid)?;
        if usize::try_from(count).map_err(invalid)? != heads.len() {
            return Err(ControlError::StorageIncompatible);
        }
        let mut q = self
            .conn
            .prepare("SELECT managed_turn_id,intent_id,intent_mutation,prompt_fingerprint,prompt_byte_length FROM managed_turns")
            .map_err(invalid)?;
        let rows = q
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            })
            .map_err(invalid)?;
        for row in rows {
            let (id, intent, mutation, digest, bytes) = row.map_err(invalid)?;
            let t = read(&self.conn, &id)?;
            if heads.get(&id)
                != Some(&events::Head {
                    state: enum_text(&t.state).map_err(invalid)?,
                    revision: t.revision,
                })
            {
                return Err(ControlError::StorageIncompatible);
            }
            let manifest: Value = serde_json::from_str(&mutation).map_err(invalid)?;
            let fingerprint: StartFingerprint =
                serde_json::from_value(manifest["params"].clone()).map_err(invalid)?;
            let binding = self
                .binding(&t.managed_session_id)?
                .ok_or(ControlError::StorageIncompatible)?;
            let lease = self.turn_lease(&t)?;
            let receipt: (String, String) = self
                .conn
                .query_row(
                    "SELECT mutation,result FROM processed_intents WHERE intent_id=?1",
                    [&intent],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(invalid)?;
            let original: ManagedTurnRecord = serde_json::from_str(&receipt.1).map_err(invalid)?;
            if fingerprint.prompt_fingerprint != digest
                || fingerprint.prompt_byte_length as i64 != bytes
                || binding.runtime_session_id != t.codex_thread_id
                || lease.session_id != t.managed_session_id
                || lease.revision < t.admitted_lease_revision
                || receipt.0 != mutation
                || original.managed_turn_id != t.managed_turn_id
                || original.admitted_lease_id != t.admitted_lease_id
                || original.admitted_lease_revision != t.admitted_lease_revision
                || original.runtime_instance_id != t.runtime_instance_id
                || original.codex_thread_id != t.codex_thread_id
                || original.state != TurnState::Starting
                || original.revision != 1
            {
                return Err(ControlError::StorageIncompatible);
            }
            verify_fingerprint_receipt(
                &intent,
                &mutation,
                &serde_json::to_string(&t).map_err(invalid)?,
            )?;
        }
        Ok(())
    }
}
