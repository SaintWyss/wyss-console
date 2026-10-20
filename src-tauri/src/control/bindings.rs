//! Sole-writer journal for non-transactional provider effects. Never retries uncertain creates.
use super::*;
use crate::adapters::codex::CodexThreadId;
use crate::contracts::identities::AgentRuntimeInstanceId;

#[derive(Clone)]
pub(in crate::control) struct Reservation {
    pub operation_id: WireUuid,
    pub session_id: ManagedSessionId,
    pub request_id: String,
    pub workspace: String,
    pub thread_id: Option<CodexThreadId>,
}
fn mutation(op: &Operation) -> ControlResult<String> {
    let mut v = serde_json::to_value(op).map_err(db)?;
    v["params"]["intent_id"] = json!(canonical(
        intent(op).ok_or(ControlError::InvalidRequest)?.as_str()
    ));
    if let Some(id) = v["params"]["managed_session_id"].as_str() {
        v["params"]["managed_session_id"] = json!(canonical(id));
    }
    if let Some(id) = v["params"]["operation_id"].as_str() {
        v["params"]["operation_id"] = json!(canonical(id));
    }
    let raw = serde_json::to_string(&v).map_err(db)?;
    if raw.len() > 1024 {
        return Err(ControlError::InvalidRequest);
    }
    Ok(raw)
}
pub(in crate::control) fn canonical_workspace(raw: &str) -> ControlResult<String> {
    validate_workspace_text(raw)?;
    let p = std::fs::canonicalize(raw).map_err(|_| ControlError::WorkspaceInvalid)?;
    if !p.is_dir() {
        return Err(ControlError::WorkspaceInvalid);
    }
    let s = p.to_str().ok_or(ControlError::WorkspaceInvalid)?.to_owned();
    validate_workspace_text(&s)?;
    Ok(s)
}
fn read(c: &Connection, id: &str) -> ControlResult<Option<SessionBinding>> {
    let v:Option<String>=c.query_row("SELECT json_object('managed_session_id',managed_session_id,'adapter_id',adapter_id,'runtime_kind',runtime_kind,'runtime_session_id',runtime_session_id,'workspace_root',workspace_root,'binding_revision',binding_revision,'created_at',created_at,'updated_at',updated_at) FROM managed_session_bindings WHERE managed_session_id=?1",[canonical(id)],|r|r.get(0)).optional().map_err(db)?;
    v.map(|v| {
        let b: SessionBinding = serde_json::from_str(&v).map_err(invalid)?;
        b.validate()?;
        Ok(b)
    })
    .transpose()
}
impl Store {
    pub(in crate::control) fn binding(
        &self,
        id: &ManagedSessionId,
    ) -> ControlResult<Option<SessionBinding>> {
        read(&self.conn, id.as_str())
    }
    pub(in crate::control) fn binding_status(
        &self,
        id: &ManagedSessionId,
    ) -> ControlResult<BindingStatus> {
        let session = get_session(&self.conn, id.as_str())?;
        let binding = self.binding(id)?;
        let pending:Option<(String,String)>=self.conn.query_row("SELECT operation_id,state FROM managed_session_operations WHERE managed_session_id=?1 AND state IN ('reserved','dispatching','indeterminate')",[canonical(id.as_str())],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db)?;
        Ok(BindingStatus {
            managed_session_id: session.managed_session_id,
            state: if binding.is_some() {
                BindingState::BoundDetached
            } else if pending.as_ref().is_some_and(|(_, s)| s == "indeterminate") {
                BindingState::Indeterminate
            } else if pending.is_some() {
                BindingState::Creating
            } else {
                BindingState::Unbound
            },
            binding,
            runtime_instance_id: None,
            attachment_profile: None,
            observed_model: None,
            operation_id: pending
                .map(|(id, _)| WireUuid::try_from(id).map_err(invalid))
                .transpose()?,
        })
    }
    pub(in crate::control) fn binding_receipt(
        &self,
        op: &Operation,
    ) -> ControlResult<Option<ResultValue>> {
        let id = canonical(intent(op).ok_or(ControlError::InvalidRequest)?.as_str());
        let m = mutation(op)?;
        let prior: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT mutation,result FROM processed_intents WHERE intent_id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)?;
        if let Some((old, result)) = prior {
            if old != m {
                return Err(ControlError::IntentConflict);
            }
            return decode_receipt(op, &result).map(Some);
        }
        let journal: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT mutation,state FROM managed_session_operations WHERE operation_id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)?;
        if let Some((old, state)) = journal {
            if old != m {
                return Err(ControlError::IntentConflict);
            }
            return Err(match state.as_str() {
                "reserved" | "dispatching" => ControlError::RuntimeOperationInProgress,
                "indeterminate" => ControlError::BindingIndeterminate,
                _ => ControlError::InvalidTransition,
            });
        }
        Ok(None)
    }
    pub(in crate::control) fn reserve_binding(
        &mut self,
        op: &Operation,
    ) -> ControlResult<Reservation> {
        op.validate()?;
        let (id, session, revision, workspace, kind) = match op {
            Operation::BindingCreate(p) => (
                p.intent_id.clone(),
                p.managed_session_id.clone(),
                p.expected_session_revision,
                Some(p.workspace_root.clone()),
                "create",
            ),
            Operation::BindingResume(p) => (
                p.intent_id.clone(),
                p.managed_session_id.clone(),
                p.expected_session_revision,
                None,
                "resume",
            ),
            _ => return Err(ControlError::InvalidRequest),
        };
        let m = mutation(op)?;
        let time = now()?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let s = get_session(&tx, session.as_str())?;
        let session = s.managed_session_id.clone();
        if s.revision != revision {
            return Err(ControlError::StaleRevision);
        }
        if s.control_state != ControlState::Prepared {
            return Err(ControlError::InvalidTransition);
        }
        let binding = read(&tx, session.as_str())?;
        let (workspace, thread_id) = if kind == "create" {
            if binding.is_some() {
                return Err(ControlError::BindingAlreadyExists);
            }
            (workspace.unwrap(), None)
        } else {
            let b = binding.ok_or(ControlError::BindingNotFound)?;
            (b.workspace_root, Some(b.runtime_session_id))
        };
        if tx.query_row("SELECT EXISTS(SELECT 1 FROM managed_session_operations WHERE managed_session_id=?1 AND state IN ('reserved','dispatching','indeterminate'))",[canonical(session.as_str())],|r|r.get::<_,bool>(0)).map_err(db)? {return Err(ControlError::BindingIndeterminate)}
        let request_id = format!("wyss-session-{}", uuid().as_str());
        tx.execute(
            "INSERT INTO managed_session_operations VALUES(?1,?2,?3,?4,'reserved',?5,?6,?6,NULL,'none')",
            params![
                canonical(id.as_str()),
                canonical(session.as_str()),
                kind,
                m,
                request_id,
                time
            ],
        )
        .map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(Reservation {
            operation_id: id,
            session_id: session,
            request_id,
            workspace,
            thread_id,
        })
    }
    pub(in crate::control) fn dispatch_binding(&mut self, r: &Reservation) -> ControlResult<()> {
        let n=self.conn.execute("UPDATE managed_session_operations SET state='dispatching',updated_at=?1 WHERE operation_id=?2 AND state='reserved'",params![now()?,canonical(r.operation_id.as_str())]).map_err(db)?;
        if n != 1 {
            return Err(ControlError::InvalidTransition);
        }
        Ok(())
    }
    pub(in crate::control) fn checkpoint_binding(
        &mut self,
        r: &Reservation,
        thread: &CodexThreadId,
        stage: super::super::runtime::MaterializationStage,
    ) -> ControlResult<()> {
        let (previous, next) = match stage {
            super::super::runtime::MaterializationStage::Started => ("none", "started"),
            super::super::runtime::MaterializationStage::Archived => ("started", "archived"),
            super::super::runtime::MaterializationStage::Unarchived => ("archived", "unarchived"),
        };
        let n = self.conn.execute("UPDATE managed_session_operations SET provider_thread_id=?1,provider_stage=?2,updated_at=?3 WHERE operation_id=?4 AND kind='create' AND state='dispatching' AND provider_stage=?5 AND (provider_thread_id IS NULL OR provider_thread_id=?1)",params![thread.as_str(),next,now()?,canonical(r.operation_id.as_str()),previous]).map_err(db)?;
        if n != 1 {
            return Err(ControlError::InvalidTransition);
        }
        Ok(())
    }
    pub(in crate::control) fn fail_binding(
        &mut self,
        r: &Reservation,
        uncertain: bool,
    ) -> ControlResult<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        settle_operation(
            &tx,
            &self.instance,
            &self.stream,
            &canonical(r.operation_id.as_str()),
            uncertain,
        )?;
        tx.commit().map_err(db)
    }
    pub(in crate::control) fn recover_binding_operations(&mut self) -> ControlResult<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let open = {
            let mut q = tx
                .prepare("SELECT operation_id,state FROM managed_session_operations WHERE state IN ('reserved','dispatching') ORDER BY created_at,operation_id")
                .map_err(db)?;
            q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(db)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(db)?
        };
        for (id, state) in open {
            // A dispatch marker is committed before enqueue/write; conservatively uncertain
            // even if the parent died before the worker received it. Reserved is provably
            // not dispatched.
            settle_operation(
                &tx,
                &self.instance,
                &self.stream,
                &id,
                state == "dispatching",
            )?;
        }
        tx.commit().map_err(db)
    }
    pub(in crate::control) fn commit_binding(
        &mut self,
        r: &Reservation,
        thread: &CodexThreadId,
        runtime: &AgentRuntimeInstanceId,
    ) -> ControlResult<SessionBinding> {
        let time = now()?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let creating = r.thread_id.is_none();
        if creating {
            let proven: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM managed_session_operations WHERE operation_id=?1 AND state='dispatching' AND provider_stage='unarchived' AND provider_thread_id=?2)",params![canonical(r.operation_id.as_str()),thread.as_str()],|r|r.get(0)).map_err(db)?;
            if !proven {
                return Err(ControlError::InvalidTransition);
            }
        }
        let b = if creating {
            SessionBinding {
                managed_session_id: r.session_id.clone(),
                adapter_id: RuntimeTarget::codex().adapter_id,
                runtime_kind: RuntimeTarget::codex().runtime_kind,
                runtime_session_id: thread.clone(),
                workspace_root: r.workspace.clone(),
                binding_revision: 1,
                created_at: time.clone().try_into().map_err(invalid)?,
                updated_at: time.clone().try_into().map_err(invalid)?,
            }
        } else {
            read(&tx, r.session_id.as_str())?.ok_or(ControlError::BindingNotFound)?
        };
        if b.runtime_session_id != *thread {
            return Err(ControlError::CorrelationError);
        }
        let raw = serde_json::to_string(&b).map_err(db)?;
        if raw.len() > 1024 {
            return Err(ControlError::CapacityExceeded);
        }
        if creating {
            tx.execute(
                "INSERT INTO managed_session_bindings VALUES(?1,'codex','codex',?2,?3,1,?4,?4)",
                params![
                    canonical(r.session_id.as_str()),
                    thread.as_str(),
                    r.workspace,
                    time
                ],
            )
            .map_err(db)?;
        }
        let kind = if creating {
            "authority.managed_session.runtime_bound"
        } else {
            "authority.managed_session.runtime_attached"
        };
        let payload = if creating {
            json!({"adapter_id":"codex","runtime_kind":"codex","runtime_session_id":thread,"binding_revision":1})
        } else {
            json!({"runtime_session_id":thread,"runtime_instance_id":runtime})
        };
        append_binding_event(
            &tx,
            &self.instance,
            &self.stream,
            &r.session_id,
            r.operation_id.as_str(),
            kind,
            payload,
        )?;
        let n=tx.execute("UPDATE managed_session_operations SET state='succeeded',updated_at=?1 WHERE operation_id=?2 AND state='dispatching'",params![time,canonical(r.operation_id.as_str())]).map_err(db)?;
        if n != 1 {
            return Err(ControlError::InvalidTransition);
        }
        tx.execute("INSERT INTO processed_intents SELECT operation_id,mutation,?1 FROM managed_session_operations WHERE operation_id=?2",params![raw,canonical(r.operation_id.as_str())]).map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(b)
    }
    pub(in crate::control) fn abandon_binding(
        &mut self,
        p: &BindingAbandon,
    ) -> ControlResult<ResultValue> {
        let op = Operation::BindingAbandon(p.clone());
        if let Some(r) = self.binding_receipt(&op)? {
            return Ok(r);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let s = get_session(&tx, p.managed_session_id.as_str())?;
        if s.control_state != ControlState::Prepared
            || read(&tx, p.managed_session_id.as_str())?.is_some()
        {
            return Err(ControlError::InvalidTransition);
        }
        let n=tx.execute("UPDATE managed_session_operations SET state='failed',updated_at=?1 WHERE operation_id=?2 AND managed_session_id=?3 AND kind='create' AND state='indeterminate'",params![now()?,canonical(p.operation_id.as_str()),canonical(p.managed_session_id.as_str())]).map_err(db)?;
        if n != 1 {
            return Err(ControlError::InvalidTransition);
        }
        append_binding_event(
            &tx,
            &self.instance,
            &self.stream,
            &p.managed_session_id,
            p.intent_id.as_str(),
            "authority.managed_session.runtime_operation_abandoned",
            json!({"operation_id":p.operation_id}),
        )?;
        let result = AbandonedOperation {
            operation_id: p.operation_id.clone(),
        };
        tx.execute(
            "INSERT INTO processed_intents VALUES(?1,?2,?3)",
            params![
                canonical(p.intent_id.as_str()),
                mutation(&op)?,
                serde_json::to_string(&result).map_err(db)?
            ],
        )
        .map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(ResultValue::BindingAbandoned(result))
    }
    /// Every binding row is the one the log recorded, and an operation the log declared
    /// uncertain is still uncertain or was abandoned.
    pub(super) fn verify_bindings(&self, replay: &events::Replay, v8: bool) -> ControlResult<()> {
        let mut q = self
            .conn
            .prepare("SELECT managed_session_id FROM managed_session_bindings")
            .map_err(invalid)?;
        let ids = q
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(invalid)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(invalid)?;
        if ids.len() != replay.bound.len() {
            return Err(ControlError::StorageIncompatible);
        }
        for id in ids {
            let b = read(&self.conn, &id)?.ok_or(ControlError::StorageIncompatible)?;
            if replay.bound.get(&id).map(String::as_str) != Some(b.runtime_session_id.as_str()) {
                return Err(ControlError::StorageIncompatible);
            }
        }
        for (operation, abandoned) in &replay.uncertain {
            let state: Option<String> = self
                .conn
                .query_row(
                    "SELECT state FROM managed_session_operations WHERE operation_id=?1 AND kind='create'",
                    [operation],
                    |r| r.get(0),
                )
                .optional()
                .map_err(invalid)?;
            if state.as_deref()
                != Some(if *abandoned {
                    "failed"
                } else {
                    "indeterminate"
                })
            {
                return Err(ControlError::StorageIncompatible);
            }
        }
        let mut q=self.conn.prepare("SELECT operation_id,managed_session_id,kind,mutation,state,provider_request_id,created_at,updated_at,provider_thread_id,provider_stage FROM managed_session_operations").map_err(invalid)?;
        let mut rows = q.query([]).map_err(invalid)?;
        while let Some(r) = rows.next().map_err(invalid)? {
            let id: String = r.get(0).map_err(invalid)?;
            let session: String = r.get(1).map_err(invalid)?;
            let kind: String = r.get(2).map_err(invalid)?;
            let raw: String = r.get(3).map_err(invalid)?;
            let decoded = unique_json(raw.as_bytes()).map_err(invalid)?;
            let fenced = v8 && decoded["params"].get("runtime_fence").is_some();
            let request = if fenced {
                Some(crate::f8::operations::decode_mutation(&raw)?)
            } else {
                None
            };
            let op: Operation = if let Some(request) = &request {
                if request.canonical_intent().map_err(invalid)? != raw.as_bytes() {
                    return Err(ControlError::StorageIncompatible);
                }
                let crate::f8::protocol::Target::Fenced(fence) = request.target() else {
                    return Err(ControlError::StorageIncompatible);
                };
                let sidecar: Option<(String,String,i64)> = self.conn.query_row("SELECT managed_session_id,runtime_instance_id,generation FROM managed_operation_launches WHERE operation_id=?1", [&id], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(invalid)?;
                if sidecar
                    != Some((
                        fence.managed_session_id.as_str().to_owned(),
                        fence
                            .runtime_instance_id
                            .as_ref()
                            .ok_or(ControlError::StorageIncompatible)?
                            .as_str()
                            .to_owned(),
                        fence.generation as i64,
                    ))
                {
                    return Err(ControlError::StorageIncompatible);
                }
                request.operation().clone()
            } else {
                serde_json::from_value(decoded).map_err(invalid)?
            };
            op.validate().map_err(invalid)?;
            let (oid, sid, k) = match &op {
                Operation::BindingCreate(p) => (
                    p.intent_id.as_str(),
                    p.managed_session_id.as_str(),
                    "create",
                ),
                Operation::BindingResume(p) => (
                    p.intent_id.as_str(),
                    p.managed_session_id.as_str(),
                    "resume",
                ),
                _ => return Err(ControlError::StorageIncompatible),
            };
            if canonical(oid) != id
                || canonical(sid) != session
                || k != kind
                || (!fenced && mutation(&op)? != raw)
            {
                return Err(ControlError::StorageIncompatible);
            }
            let request: String = r.get(5).map_err(invalid)?;
            crate::adapters::codex::CodexRequestId::from_value(&json!(request)).map_err(invalid)?;
            for col in [6, 7] {
                let t: String = r.get(col).map_err(invalid)?;
                crate::contracts::UtcTimestamp::try_from(t).map_err(invalid)?;
            }
            let state: String = r.get(4).map_err(invalid)?;
            let thread: Option<String> = r.get(8).map_err(invalid)?;
            let stage: String = r.get(9).map_err(invalid)?;
            if thread.is_some() != (stage != "none")
                || (kind == "resume" && stage != "none")
                || (kind == "create" && state == "succeeded" && stage != "unarchived")
            {
                return Err(ControlError::StorageIncompatible);
            }
            if let Some(thread) = thread {
                CodexThreadId::parse(&thread).map_err(invalid)?;
            }
            if state == "succeeded" && read(&self.conn, &session)?.is_none() {
                return Err(ControlError::StorageIncompatible);
            }
        }
        Ok(())
    }
}
/// Ends one open operation without a binding. A resume has no external effect to lose and
/// a definite failure leaves the authority exactly as before the attempt, so neither is
/// an event. An uncertain create may have left a provider thread behind: that outcome
/// blocks the session until it is abandoned, and it is audited in the same transaction.
fn settle_operation(
    tx: &Transaction<'_>,
    instance: &WireUuid,
    stream: &WireUuid,
    operation: &str,
    uncertain: bool,
) -> ControlResult<()> {
    let open: Option<(String, String)> = tx
        .query_row(
            "SELECT managed_session_id,kind FROM managed_session_operations WHERE operation_id=?1 AND state IN ('reserved','dispatching')",
            [operation],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db)?;
    let Some((session, kind)) = open else {
        return Ok(());
    };
    let uncertain = uncertain && kind == "create";
    tx.execute(
        "UPDATE managed_session_operations SET state=?1,updated_at=?2 WHERE operation_id=?3",
        params![
            if uncertain { "indeterminate" } else { "failed" },
            now()?,
            operation
        ],
    )
    .map_err(db)?;
    if uncertain {
        append_binding_event(
            tx,
            instance,
            stream,
            &ManagedSessionId::parse(&session).map_err(invalid)?,
            operation,
            "authority.managed_session.runtime_operation_indeterminate",
            json!({"operation_id":operation}),
        )?;
    }
    Ok(())
}
fn append_binding_event(
    tx: &Transaction<'_>,
    instance: &WireUuid,
    stream: &WireUuid,
    session: &ManagedSessionId,
    intent: &str,
    kind: &str,
    payload: Value,
) -> ControlResult<()> {
    events::append(
        tx,
        instance,
        stream,
        events::Draft {
            kind,
            session: Some(session.as_str()),
            correlation: Some(intent),
            timestamp: now()?,
            payload,
        },
    )?;
    Ok(())
}
pub(super) fn validate_audit(v: &Value) -> ControlResult<()> {
    let kind = v["event_type"]
        .as_str()
        .ok_or(ControlError::StorageIncompatible)?;
    if v["session_id"].is_null()
        || v["correlation_id"].is_null()
        || v["persistence"] != "durable"
        || v["sensitivity"] != "internal"
        || v["redaction"] != "none"
        || v["source"]["component"] != "wyss_control"
        || !v["causation_id"].is_null()
        || v["payload_schema_version"] != 1
    {
        return Err(ControlError::StorageIncompatible);
    }
    let p = v["payload"]
        .as_object()
        .ok_or(ControlError::StorageIncompatible)?;
    let keys: &[&str] = match kind {
        "authority.managed_session.runtime_bound" => &[
            "adapter_id",
            "runtime_kind",
            "runtime_session_id",
            "binding_revision",
        ],
        "authority.managed_session.runtime_attached" => {
            &["runtime_session_id", "runtime_instance_id"]
        }
        "authority.managed_session.runtime_operation_abandoned"
        | "authority.managed_session.runtime_operation_indeterminate" => &["operation_id"],
        _ => return Err(ControlError::StorageIncompatible),
    };
    if p.len() != keys.len() || keys.iter().any(|k| !p.contains_key(*k)) {
        return Err(ControlError::StorageIncompatible);
    }
    if kind.ends_with("runtime_bound")
        && (p["adapter_id"] != "codex"
            || p["runtime_kind"] != "codex"
            || p["binding_revision"] != 1)
    {
        return Err(ControlError::StorageIncompatible);
    }
    if let Some(id) = p.get("runtime_session_id") {
        CodexThreadId::parse(id.as_str().ok_or(ControlError::StorageIncompatible)?)
            .map_err(invalid)?;
    }
    if let Some(id) = p.get("runtime_instance_id") {
        AgentRuntimeInstanceId::parse(id.as_str().ok_or(ControlError::StorageIncompatible)?)
            .map_err(invalid)?;
    }
    if let Some(id) = p.get("operation_id") {
        WireUuid::try_from(
            id.as_str()
                .ok_or(ControlError::StorageIncompatible)?
                .to_owned(),
        )
        .map_err(invalid)?;
    }
    Ok(())
}
#[cfg(test)]
#[path = "binding_tests.rs"]
mod tests;
