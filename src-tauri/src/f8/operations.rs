//! Protocol-2 session operations and historical projections on the V8 sole writer.
//! Provider effects are journaled before dispatch and are never replayed automatically.
use super::{
    allocator::{Draft, append, attest, db, invalid},
    engine::Engine,
    protocol::{Request, RuntimeFence, RuntimeRef, Target},
};
use crate::{
    contracts::{
        WireUuid,
        control::{self as v1, ControlError as E, ControlResult},
    },
    control::turn_stream::Streams,
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

fn now() -> ControlResult<String> {
    OffsetDateTime::now_utc().format(&Rfc3339).map_err(invalid)
}
fn result(kind: &str, value: impl serde::Serialize) -> ControlResult<Value> {
    Ok(json!({"kind":kind,"value":serde_json::to_value(value).map_err(invalid)?}))
}
fn context(c: &Connection) -> ControlResult<String> {
    c.query_row(
        "SELECT stream_id FROM authority_metadata WHERE id=1",
        [],
        |r| r.get(0),
    )
    .map_err(db)
}
fn receipt(c: &Connection, id: &str, mutation: &str) -> ControlResult<Option<Value>> {
    let old: Option<(String, String)> = c
        .query_row(
            "SELECT mutation,result FROM processed_intents WHERE intent_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db)?;
    old.map(|(m, r)| {
        if m == mutation {
            serde_json::from_str(&r).map_err(invalid)
        } else {
            Err(E::IntentConflict)
        }
    })
    .transpose()
}
fn save(tx: &Transaction<'_>, id: &str, mutation: &str, value: &Value) -> ControlResult<()> {
    let raw = serde_json::to_string(value).map_err(invalid)?;
    if raw.len() > 1024 || mutation.len() > 1024 {
        return Err(E::CapacityExceeded);
    }
    tx.execute(
        "INSERT INTO processed_intents VALUES(?1,?2,?3)",
        params![id, mutation, raw],
    )
    .map_err(db)?;
    Ok(())
}
fn mutation(r: &Request) -> ControlResult<String> {
    String::from_utf8(r.canonical_intent()?).map_err(invalid)
}
fn fence(r: &Request) -> ControlResult<&RuntimeFence> {
    if let Target::Fenced(f) = r.target() {
        Ok(f)
    } else {
        Err(E::InvalidRequest)
    }
}
/// An absent fence is exact: it cannot name a stopped generation's old live instance.
fn check_fence(engine: &Engine, f: &RuntimeFence) -> ControlResult<()> {
    f.validate(true)?;
    if let Some(reference) = engine.reference(f.managed_session_id.as_str()) {
        if reference.fence() != *f {
            return Err(E::StaleRevision);
        }
        return Ok(());
    }
    let generation: Option<i64> = engine
        .connection()
        .query_row(
            "SELECT generation FROM managed_runtimes WHERE managed_session_id=?1",
            [f.managed_session_id.as_str()],
            |r| r.get(0),
        )
        .optional()
        .map_err(db)?;
    if f.runtime_instance_id.is_some() || generation.unwrap_or(0) as u64 != f.generation {
        return Err(E::StaleRevision);
    }
    Ok(())
}
pub(super) fn session(c: &Connection, id: &str) -> ControlResult<v1::ManagedSessionRecord> {
    let raw:Option<String>=c.query_row("SELECT json_object('managed_session_id',session_id,'adapter_id',adapter_id,'runtime_kind',runtime_kind,'control_state',control_state,'revision',revision,'created_at',created_at,'updated_at',updated_at,'created_sequence',ltrim(created_sequence,'0')) FROM managed_sessions WHERE session_id=?1",[id],|r|r.get(0)).optional().map_err(db)?;
    let s: v1::ManagedSessionRecord =
        serde_json::from_str(&raw.ok_or(E::SessionNotFound)?).map_err(invalid)?;
    s.validate()?;
    Ok(s)
}
fn binding(c: &Connection, id: &str) -> ControlResult<Option<v1::SessionBinding>> {
    let raw:Option<String>=c.query_row("SELECT json_object('managed_session_id',managed_session_id,'adapter_id',adapter_id,'runtime_kind',runtime_kind,'runtime_session_id',runtime_session_id,'workspace_root',workspace_root,'binding_revision',binding_revision,'created_at',created_at,'updated_at',updated_at) FROM managed_session_bindings WHERE managed_session_id=?1",[id],|r|r.get(0)).optional().map_err(db)?;
    raw.map(|r| {
        let b: v1::SessionBinding = serde_json::from_str(&r).map_err(invalid)?;
        b.validate()?;
        Ok(b)
    })
    .transpose()
}
pub(super) fn read_turn(c: &Connection, id: &str) -> ControlResult<v1::ManagedTurnRecord> {
    let raw:Option<String>=c.query_row("SELECT json_object('managed_turn_id',managed_turn_id,'managed_session_id',managed_session_id,'codex_thread_id',codex_thread_id,'codex_turn_id',codex_turn_id,'admitted_lease_id',admitted_lease_id,'admitted_lease_revision',admitted_lease_revision,'runtime_instance_id',runtime_instance_id,'state',state,'revision',revision,'created_at',created_at,'updated_at',updated_at,'terminal_at',terminal_at,'terminal_reason',terminal_reason) FROM managed_turns WHERE managed_turn_id=?1",[id],|r|r.get(0)).optional().map_err(db)?;
    let t: v1::ManagedTurnRecord =
        serde_json::from_str(&raw.ok_or(E::TurnNotFound)?).map_err(invalid)?;
    t.validate()?;
    Ok(t)
}
fn read_approval(c: &Connection, id: &str) -> ControlResult<v1::ManagedApprovalRecord> {
    let raw:Option<String>=c.query_row("SELECT json_object('managed_approval_id',managed_approval_id,'managed_turn_id',managed_turn_id,'managed_session_id',managed_session_id,'runtime_instance_id',runtime_instance_id,'kind',kind,'provider_item_id',provider_item_id,'admitted_lease_id',admitted_lease_id,'admitted_lease_revision',admitted_lease_revision,'state',state,'revision',revision,'action_fingerprint',action_fingerprint,'structure',json(structure),'decision',decision,'effect',effect,'created_at',created_at,'updated_at',updated_at,'expires_at',expires_at,'terminal_at',terminal_at,'terminal_reason',terminal_reason,'created_sequence',ltrim(created_sequence,'0')) FROM managed_approvals WHERE managed_approval_id=?1",[id],|r|r.get(0)).optional().map_err(db)?;
    let a: v1::ManagedApprovalRecord =
        serde_json::from_str(&raw.ok_or(E::ApprovalNotFound)?).map_err(invalid)?;
    a.validate()?;
    Ok(a)
}
fn project(
    engine: &Engine,
    table: &str,
    key: &str,
    id: &str,
    value: &mut Value,
) -> ControlResult<()> {
    let sql = format!(
        "SELECT a.authority_instance_id,l.managed_session_id,l.runtime_instance_id,l.generation FROM {table} l JOIN managed_runtime_launches a ON a.runtime_instance_id=l.runtime_instance_id AND a.managed_session_id=l.managed_session_id AND a.generation=l.generation WHERE l.{key}=?1"
    );
    let reference: Option<(String, String, String, i64)> = engine
        .connection()
        .query_row(&sql, [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .optional()
        .map_err(db)?;
    value["runtime_ref"] = Value::Null;
    value["attestation"] = Value::Null;
    if let Some((authority, session, instance, generation)) = reference {
        let r:RuntimeRef=serde_json::from_value(json!({"schema":2,"authority_instance_id":authority,"managed_session_id":session,"runtime_instance_id":instance,"generation":generation})).map_err(invalid)?;
        r.validate()?;
        value["runtime_ref"] = json!(r);
        if let Ok(live) = engine.observe_fence(&r.fence())
            && live.authority_instance_id == r.authority_instance_id
        {
            value["attestation"] = json!(live)
        }
    }
    Ok(())
}
fn approval_status(
    engine: &Engine,
    id: &str,
    index: Option<u16>,
    offset: Option<u32>,
) -> ControlResult<Value> {
    let approval = read_approval(engine.connection(), id)?;
    let mut v = json!({"approval":approval,"acceptable":false,"decisions":[],"command":null,"file_count":null,"file":null,"delivery":super::writers::read_delivery(engine.connection(),id)?});
    project(
        engine,
        "managed_approval_launches",
        "managed_approval_id",
        id,
        &mut v,
    )?;
    if approval.state == v1::ApprovalState::Pending
        && v1::timestamp(&approval.expires_at)? > OffsetDateTime::now_utc()
        && let Some(origin) = engine.reference(approval.managed_session_id.as_str())
    {
        let same_launch = origin.runtime_instance_id == approval.runtime_instance_id
            && v["runtime_ref"] == json!(origin)
            && engine.active_turn(approval.managed_session_id.as_str())
                == Some(approval.managed_turn_id.as_str())
            && engine
                .pending_fingerprint(
                    approval.managed_session_id.as_str(),
                    &approval.provider_item_id,
                )
                .as_deref()
                == Some(approval.action_fingerprint.as_str());
        if same_launch {
            let action = engine.approval_presentation(
                &origin.fence(),
                &approval.provider_item_id,
                index,
                offset,
            );
            match action {
                Ok(presentation) => {
                    v["decisions"] = json!(engine.approval_decisions(
                        approval.managed_session_id.as_str(),
                        &approval.provider_item_id
                    ));
                    v["command"] = presentation["command"].clone();
                    v["file_count"] = presentation["file_count"].clone();
                    v["file"] = presentation["file"].clone();
                    let scope = match approval.kind {
                        v1::ApprovalKind::CommandExecution => v1::ExecutionScope::CommandExecute,
                        v1::ApprovalKind::FileChange => v1::ExecutionScope::FileChange,
                    };
                    let lease = super::writers::read_lease(
                        engine.connection(),
                        approval.admitted_lease_id.as_str(),
                    )?;
                    let permitted = lease.session_id == approval.managed_session_id
                        && lease.state == v1::LeaseState::Active
                        && v1::timestamp(&lease.expires_at)? > OffsetDateTime::now_utc()
                        && lease.scopes.contains(&scope);
                    v["acceptable"] = json!(
                        permitted
                            && gate(
                                engine,
                                &v1::GateQuery {
                                    session_id: approval.managed_session_id.clone(),
                                    action: scope
                                }
                            )? == v1::GateReason::Allowed
                    );
                }
                Err(E::InvalidRequest) => return Err(E::InvalidRequest),
                Err(_) => (),
            }
        }
    }
    Ok(v)
}

pub(crate) fn handle(
    engine: &mut Engine,
    r: &Request,
    streams: &mut Streams,
) -> ControlResult<Value> {
    use v1::Operation as O;
    match r.operation() {
        O::Prepare(_) | O::Close(_) => session_mutation(engine, r),
        O::Get(p) => result(
            "session",
            session(engine.connection(), p.managed_session_id.as_str())?,
        ),
        O::List(p) => {
            let mut q=engine.connection().prepare("SELECT session_id FROM managed_sessions WHERE created_sequence>?1 ORDER BY created_sequence LIMIT ?2").map_err(db)?;
            let ids = q
                .query_map(
                    params![
                        format!("{:020}", p.after.map_or(0, |s| s.get())),
                        i64::from(p.limit) + 1
                    ],
                    |r| r.get::<_, String>(0),
                )
                .map_err(db)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(db)?;
            let more = ids.len() > usize::from(p.limit);
            let sessions = ids
                .iter()
                .take(usize::from(p.limit))
                .map(|id| session(engine.connection(), id))
                .collect::<ControlResult<Vec<_>>>()?;
            let next = if more {
                sessions.last().map(|s| s.created_sequence)
            } else {
                None
            };
            result("sessions", json!({"sessions":sessions,"next":next}))
        }
        O::BindingStatus(p) => binding_status(engine, p.managed_session_id.as_str()),
        O::BindingCreate(_) | O::BindingResume(_) => binding_mutation(engine, r),
        O::BindingAbandon(_) => abandon(engine, r),
        O::GateStatus(p) => result(
            "gate_status",
            v1::GateStatus {
                session_id: p.session_id.clone(),
                action: p.action,
                reason: gate(engine, p)?,
            },
        ),
        O::TurnStatus(p) => {
            let turn = read_turn(engine.connection(), p.managed_turn_id.as_str())?;
            let page = streams.page(
                &turn,
                &v1::TurnEvents {
                    managed_turn_id: p.managed_turn_id.clone(),
                    after_sequence: None,
                    limit: 1,
                },
            );
            let mut value = json!({"turn":turn,"stream_earliest_sequence":page.earliest_sequence,"stream_latest_sequence":page.latest_sequence,"stream_retained":page.retained});
            project(
                engine,
                "managed_turn_launches",
                "managed_turn_id",
                p.managed_turn_id.as_str(),
                &mut value,
            )?;
            result("turn_status", value)
        }
        O::TurnEvents(p) => {
            let turn = read_turn(engine.connection(), p.managed_turn_id.as_str())?;
            result("turn_events", streams.page(&turn, p))
        }
        O::ApprovalStatus(p) => result(
            "approval",
            approval_status(
                engine,
                p.managed_approval_id.as_str(),
                p.file_index,
                p.offset,
            )?,
        ),
        O::ApprovalPending(p) => {
            read_turn(engine.connection(), p.managed_turn_id.as_str())?;
            // A row past its deadline is not awaiting a decision, even before the expiry tick
            // persists it; it is never reported as pending.
            let now = OffsetDateTime::now_utc();
            let candidates: Vec<(String, String)> = {
                let mut q = engine.connection().prepare("SELECT managed_approval_id,expires_at FROM managed_approvals WHERE managed_turn_id=?1 AND state='pending' ORDER BY created_sequence").map_err(db)?;
                q.query_map([p.managed_turn_id.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))
                    .map_err(db)?
                    .collect::<Result<_, _>>()
                    .map_err(db)?
            };
            let mut id = None;
            for (candidate, expires) in candidates {
                if OffsetDateTime::parse(&expires, &Rfc3339).map_err(invalid)? > now {
                    id = Some(candidate);
                    break;
                }
            }
            result(
                "pending_approval",
                id.map(|id| approval_status(engine, &id, None, None))
                    .transpose()?,
            )
        }
        O::RuntimeModels(p) => {
            let Target::Runtime {
                session,
                generation: Some(generation),
                instance: Some(instance),
            } = r.target()
            else {
                return Err(E::InvalidRequest);
            };
            let f = RuntimeFence {
                managed_session_id: session.clone(),
                generation: *generation,
                runtime_instance_id: Some(instance.clone()),
            };
            let identity = engine.observe_fence(&f)?;
            let (models, offers, complete) = engine.model_snapshot(&f)?;
            let index = usize::from(p.cursor.as_ref().map_or(0, |c| c.index));
            if index > models.len() {
                return Err(E::InvalidRequest);
            }
            let end = (index + usize::from(p.limit)).min(models.len());
            result(
                "runtime_models",
                json!({"runtime_instance_id":instance,"runtime_identity":identity,"models":&models[index..end],"offers":&offers[index..end],"complete":complete,"next":if end<models.len(){Some(v1::ModelCursor{runtime_instance_id:instance.clone(),index:end as u16})}else{None}}),
            )
        }
        _ => Err(E::InvalidRequest),
    }
}

fn session_mutation(engine: &mut Engine, r: &Request) -> ControlResult<Value> {
    let (id, closing) = match r.operation() {
        v1::Operation::Prepare(p) => (p.intent_id.as_str(), false),
        v1::Operation::Close(p) => (p.intent_id.as_str(), true),
        _ => return Err(E::InvalidRequest),
    };
    let mutation = mutation(r)?;
    if let Some(v) = receipt(engine.connection(), id, &mutation)? {
        return result("session", v);
    }
    if closing {
        check_fence(engine, fence(r)?)?;
        let f = fence(r)?;
        if engine.active_turn(f.managed_session_id.as_str()).is_some() {
            return Err(E::ActiveTurnExists);
        }
        if engine.reference(f.managed_session_id.as_str()).is_some() {
            return Err(E::RuntimeOperationInProgress);
        }
    }
    let authority = engine.authority_id().as_str().to_owned();
    let stream = context(engine.connection())?;
    let now = now()?;
    let tx = engine
        .connection_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    attest(&tx, &authority, &stream, &now)?;
    let (s, kind, changed) = match r.operation() {
        v1::Operation::Prepare(p) => {
            let kind = serde_json::to_value(p.runtime_kind).map_err(invalid)?;
            let provider = super::provider::Provider::parse(
                p.adapter_id.as_str(),
                kind.as_str().ok_or(E::InvalidRequest)?,
            )?;
            super::provider::admit(&tx, provider)?;
            let (adapter, runtime_kind) = provider.fields();
            let count: i64 = tx
                .query_row("SELECT count(*) FROM managed_sessions", [], |r| r.get(0))
                .map_err(db)?;
            if count >= 128 {
                return Err(E::CapacityExceeded);
            }
            let sequence: u64 = tx
                .query_row(
                    "SELECT last_sequence FROM authority_metadata WHERE id=1",
                    [],
                    |r| r.get::<_, String>(0),
                )
                .map_err(db)?
                .parse::<u64>()
                .map_err(invalid)?
                .checked_add(1)
                .ok_or(E::CapacityExceeded)?;
            let session_id = crate::control::store::uuid();
            tx.execute(
                "INSERT INTO managed_sessions VALUES(?1,?4,?5,'prepared',1,?2,?2,?3)",
                params![
                    session_id.as_str(),
                    now,
                    format!("{sequence:020}"),
                    adapter,
                    runtime_kind
                ],
            )
            .map_err(db)?;
            (
                session(&tx, session_id.as_str())?,
                "authority.managed_session.prepared",
                true,
            )
        }
        v1::Operation::Close(p) => {
            let mut s = session(&tx, p.managed_session_id.as_str())?;
            if s.revision != p.expected_revision {
                return Err(E::StaleRevision);
            }
            let changed = s.control_state != v1::ControlState::Closed;
            if changed {
                let pending:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM managed_turns WHERE managed_session_id=?1 AND state IN ('starting','running')) OR EXISTS(SELECT 1 FROM managed_session_operations WHERE managed_session_id=?1 AND state IN ('reserved','dispatching','indeterminate')) OR EXISTS(SELECT 1 FROM managed_runtimes WHERE managed_session_id=?1 AND desired_state='running')",[p.managed_session_id.as_str()],|r|r.get(0)).map_err(db)?;
                if pending {
                    return Err(E::RuntimeOperationInProgress);
                }
                if let Some(mut l) =
                    super::writers::active_lease(&tx, p.managed_session_id.as_str())?
                {
                    l.revision = l
                        .revision
                        .checked_add(1)
                        .filter(|n| *n <= i64::MAX as u64)
                        .ok_or(E::CapacityExceeded)?;
                    l.state = v1::LeaseState::Revoked;
                    l.updated_at = now.clone().try_into().map_err(invalid)?;
                    l.revocation_reason = Some(v1::RevocationReason::SessionClosed);
                    tx.execute("UPDATE execution_leases SET state='revoked',revision=?1,updated_at=?2,revocation_reason='session_closed' WHERE lease_id=?3",params![l.revision as i64,now,l.lease_id.as_str()]).map_err(db)?;
                    append(
                        &tx,
                        (&authority, &stream, &now),
                        Draft {
                            session: Some(p.managed_session_id.as_str()),
                            kind: "authority.execution_lease.revoked",
                            correlation: Some(id),
                            version: 1,
                            payload: json!({"lease_id":l.lease_id,"scopes":l.scopes,"lease_state":l.state,"revision":l.revision,"expires_at":l.expires_at,"revocation_reason":l.revocation_reason}),
                        },
                    )?;
                }
                tx.execute("UPDATE managed_sessions SET control_state='closed',revision=2,updated_at=?1 WHERE session_id=?2",params![now,p.managed_session_id.as_str()]).map_err(db)?;
                s = session(&tx, p.managed_session_id.as_str())?;
            }
            (s, "authority.managed_session.closed", changed)
        }
        _ => return Err(E::InvalidRequest),
    };
    if changed {
        append(
            &tx,
            (&authority, &stream, &now),
            Draft {
                session: Some(s.managed_session_id.as_str()),
                kind,
                correlation: Some(id),
                version: 1,
                payload: json!({"adapter_id":s.adapter_id,"runtime_kind":s.runtime_kind,"control_state":s.control_state,"revision":s.revision}),
            },
        )?;
    }
    let value = json!(s);
    save(&tx, id, &mutation, &value)?;
    tx.commit().map_err(db)?;
    result("session", value)
}

fn binding_status(engine: &Engine, id: &str) -> ControlResult<Value> {
    session(engine.connection(), id)?;
    let b = binding(engine.connection(), id)?;
    let pending:Option<(String,String)>=engine.connection().query_row("SELECT operation_id,state FROM managed_session_operations WHERE managed_session_id=?1 AND state IN ('reserved','dispatching','indeterminate')",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db)?;
    let identity = engine
        .reference(id)
        .and_then(|r| engine.observe_fence(&r.fence()).ok());
    let attachment = engine
        .attachment(id)
        .filter(|a| b.as_ref().is_some_and(|b| b.workspace_root == a.workspace));
    let attached = b.is_some()
        && identity.is_some()
        && attachment.is_some()
        && identity
            .as_ref()
            .is_some_and(|i| require_bound_launch(engine, &i.reference().fence()).is_ok());
    result(
        "binding_status",
        json!({"managed_session_id":id,"state":if attached{"bound_attached"}else if b.is_some(){"bound_detached"}else if pending.as_ref().is_some_and(|p|p.1=="indeterminate"){"indeterminate"}else if pending.is_some(){"creating"}else{"unbound"},"binding":b,"runtime_instance_id":if attached{identity.as_ref().map(|i|i.runtime_instance_id.clone())}else{None},"runtime_identity":if attached{identity}else{None},"operation_id":pending.map(|p|p.0),"attachment_profile":if attached{attachment.map(|a|if matches!(a.profile.as_str(),"codex_local_tools_v1"|"claude_local_tools_v1"){"local_tools_v1"}else{"inference_only_v1"})}else{None},"observed_model":null}),
    )
}
fn gate(engine: &Engine, p: &v1::GateQuery) -> ControlResult<v1::GateReason> {
    use v1::GateReason::*;
    let s = match session(engine.connection(), p.session_id.as_str()) {
        Ok(s) => s,
        Err(E::SessionNotFound) => return Ok(SessionNotFound),
        Err(e) => return Err(e),
    };
    if s.control_state != v1::ControlState::Prepared {
        return Ok(SessionNotPrepared);
    }
    let Some(l) = super::writers::active_lease(engine.connection(), p.session_id.as_str())? else {
        return Ok(LeaseMissing);
    };
    if v1::timestamp(&l.expires_at)? <= OffsetDateTime::now_utc() {
        return Ok(LeaseExpired);
    }
    if !l.scopes.contains(&p.action) {
        return Ok(ScopeNotGranted);
    }
    let Some(r) = engine.reference(p.session_id.as_str()) else {
        return Ok(RuntimeNotRunning);
    };
    if engine.observe_fence(&r.fence()).is_err() {
        return Ok(RuntimeNotReady);
    }
    if p.action == v1::ExecutionScope::TurnStart {
        if binding(engine.connection(), p.session_id.as_str())?.is_none() {
            return Ok(BindingNotFound);
        }
        if require_bound_launch(engine, &r.fence()).is_err() {
            return Ok(BindingDetached);
        }
        if engine.active_turn(p.session_id.as_str()).is_some() {
            return Ok(ActiveTurnExists);
        }
    }
    if matches!(
        p.action,
        v1::ExecutionScope::CommandExecute | v1::ExecutionScope::FileChange
    ) && engine.attachment(p.session_id.as_str()).is_none_or(|a| {
        !matches!(
            a.profile.as_str(),
            "codex_local_tools_v1" | "claude_local_tools_v1"
        )
    }) {
        return Ok(LocalToolsProfileMissing);
    }
    Ok(Allowed)
}

fn binding_mutation(engine: &mut Engine, r: &Request) -> ControlResult<Value> {
    let (id, sid, revision, workspace, create) = match r.operation() {
        v1::Operation::BindingCreate(p) => (
            p.intent_id.as_str(),
            p.managed_session_id.as_str(),
            p.expected_session_revision,
            Some(p.workspace_root.as_str()),
            true,
        ),
        v1::Operation::BindingResume(p) => (
            p.intent_id.as_str(),
            p.managed_session_id.as_str(),
            p.expected_session_revision,
            None,
            false,
        ),
        _ => return Err(E::InvalidRequest),
    };
    let m = mutation(r)?;
    if let Some(v) = receipt(engine.connection(), id, &m)? {
        return result("binding", v);
    }
    let prior: Option<(String, String)> = engine
        .connection()
        .query_row(
            "SELECT mutation,state FROM managed_session_operations WHERE operation_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db)?;
    if let Some((old, state)) = prior {
        return Err(if old != m {
            E::IntentConflict
        } else if state == "indeterminate" {
            E::BindingIndeterminate
        } else {
            E::RuntimeOperationInProgress
        });
    }
    let f = fence(r)?;
    engine.observe_fence(f)?;
    if f.managed_session_id.as_str() != sid {
        return Err(E::InvalidRequest);
    }
    let s = session(engine.connection(), sid)?;
    if s.revision != revision {
        return Err(E::StaleRevision);
    }
    if s.control_state != v1::ControlState::Prepared {
        return Err(E::InvalidTransition);
    }
    let action = if create {
        v1::ExecutionScope::SessionCreate
    } else {
        v1::ExecutionScope::SessionResume
    };
    if gate(
        engine,
        &v1::GateQuery {
            session_id: f.managed_session_id.clone(),
            action,
        },
    )? != v1::GateReason::Allowed
    {
        return Err(E::GateDenied);
    }
    let b = binding(engine.connection(), sid)?;
    if create && b.is_some() {
        return Err(E::BindingAlreadyExists);
    }
    if !create && b.is_none() {
        return Err(E::BindingNotFound);
    }
    let workspace = if let Some(w) = workspace {
        v1::validate_workspace_text(w)?;
        let canonical = std::fs::canonicalize(w).map_err(|_| E::WorkspaceInvalid)?;
        if !canonical.is_dir() {
            return Err(E::WorkspaceInvalid);
        }
        let w = canonical.to_str().ok_or(E::WorkspaceInvalid)?.to_owned();
        v1::validate_workspace_text(&w)?;
        w
    } else {
        b.as_ref().ok_or(E::BindingNotFound)?.workspace_root.clone()
    };
    let attachment = engine.attachment(sid).ok_or(E::GateDenied)?;
    if attachment.workspace != workspace {
        return Err(E::GateDenied);
    }
    engine.preflight(f, &attachment)?;
    let now = now()?;
    {
        let tx = engine
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let pending:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM managed_session_operations WHERE managed_session_id=?1 AND state IN ('reserved','dispatching','indeterminate'))",[sid],|r|r.get(0)).map_err(db)?;
        if pending {
            return Err(E::BindingIndeterminate);
        }
        if m.len() > 1024 {
            return Err(E::CapacityExceeded);
        }
        tx.execute(
            "INSERT INTO managed_operation_launches VALUES(?1,?2,?3,?4)",
            params![
                id,
                sid,
                f.runtime_instance_id
                    .as_ref()
                    .ok_or(E::InvalidRequest)?
                    .as_str(),
                f.generation as i64
            ],
        )
        .map_err(db)?;
        tx.execute("INSERT INTO managed_session_operations VALUES(?1,?2,?3,?4,'dispatching',?1,?5,?5,NULL,'none')",params![id,sid,if create{"create"}else{"resume"},m,now]).map_err(db)?;
        tx.commit().map_err(db)?;
    }
    let mut frame = json!({"method":if create{"thread/start"}else{"thread/resume"},"operation_id":id,"workspace":workspace});
    if let Some(b) = b {
        frame["thread_id"] = json!(b.runtime_session_id)
    }
    // Persisted dispatching is conservatively uncertain even if the transport refuses.
    // The same intent cannot cause a second dispatch on retry or authority succession.
    if engine
        .dispatch_binding(f, &serde_json::to_vec(&frame).map_err(invalid)?)
        .is_err()
    {
        mark_indeterminate(engine, id)?;
        return Err(E::ExternalEffectUnknown);
    }
    Err(E::RuntimeOperationInProgress)
}
fn mark_indeterminate(engine: &mut Engine, id: &str) -> ControlResult<()> {
    let authority = engine.authority_id().as_str().to_owned();
    let stream = context(engine.connection())?;
    let now = now()?;
    let tx = engine
        .connection_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    let row:Option<(String,String)>=tx.query_row("SELECT managed_session_id,kind FROM managed_session_operations WHERE operation_id=?1 AND state IN ('reserved','dispatching')",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db)?;
    if let Some((sid, kind)) = row {
        attest(&tx, &authority, &stream, &now)?;
        tx.execute(
            "UPDATE managed_session_operations SET state=?1,updated_at=?2 WHERE operation_id=?3",
            params![
                if kind == "create" {
                    "indeterminate"
                } else {
                    "failed"
                },
                now,
                id
            ],
        )
        .map_err(db)?;
        if kind == "create" {
            append(
                &tx,
                (&authority, &stream, &now),
                Draft {
                    session: Some(&sid),
                    kind: "authority.managed_session.runtime_operation_indeterminate",
                    correlation: Some(id),
                    version: 1,
                    payload: json!({"operation_id":id}),
                },
            )?;
        }
    }
    tx.commit().map_err(db)
}
/// Recovery never resends a provider operation whose frame might have left this process.
pub fn recover_bindings(engine: &mut Engine) -> ControlResult<usize> {
    let rows = {
        let mut q=engine.connection().prepare("SELECT o.operation_id,l.managed_session_id,l.runtime_instance_id,l.generation,a.authority_instance_id FROM managed_session_operations o JOIN managed_operation_launches l ON l.operation_id=o.operation_id JOIN managed_runtime_launches a ON a.runtime_instance_id=l.runtime_instance_id WHERE o.state IN ('reserved','dispatching') ORDER BY o.operation_id").map_err(db)?;
        q.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(db)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db)?
    };
    let mut count = 0;
    for (id, sid, instance, generation, authority) in rows {
        let live = engine.reference(&sid).is_some_and(|r| {
            r.authority_instance_id.as_str() == authority
                && r.runtime_instance_id.as_str() == instance
                && r.generation == generation as u64
                && !engine.provider_exited(&r)
        });
        if !live {
            mark_indeterminate(engine, &id)?;
            count += 1;
        }
    }
    Ok(count)
}
/// A confirmation is accepted only from the launch named in the original durable operation.
pub fn confirm_binding(
    engine: &mut Engine,
    origin: &RuntimeRef,
    operation_id: &WireUuid,
    provider_thread_id: &str,
) -> ControlResult<()> {
    let provider =
        super::provider::session(engine.connection(), origin.managed_session_id.as_str())?;
    if provider == super::provider::Provider::Claude
        && !crate::adapters::claude::frames::uuid_ok(provider_thread_id)
    {
        return Err(E::InvalidRequest);
    }
    let (adapter, runtime_kind) = provider.fields();
    let thread = crate::adapters::codex::CodexThreadId::parse(provider_thread_id)
        .map_err(|_| E::InvalidRequest)?;
    let identity = engine.observe_fence(&origin.fence())?;
    if identity.reference() != *origin {
        return Err(E::StaleRevision);
    }
    let(id,sid,kind,m,state):(String,String,String,String,String)=engine.connection().query_row("SELECT operation_id,managed_session_id,kind,mutation,state FROM managed_session_operations WHERE operation_id=?1",[operation_id.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(db)?.ok_or(E::InvalidRequest)?;
    let requested = decode_mutation(&m)?;
    if sid != origin.managed_session_id.as_str() || fence(&requested)? != &origin.fence() {
        return Err(E::InvalidRequest);
    }
    if state == "succeeded" {
        let b = binding(engine.connection(), &sid)?.ok_or(E::StorageIncompatible)?;
        return if b.runtime_session_id == thread {
            Ok(())
        } else {
            Err(E::CorrelationError)
        };
    }
    if state != "dispatching" {
        return Err(E::InvalidTransition);
    }
    let s = session(engine.connection(), &sid)?;
    if s.control_state != v1::ControlState::Prepared {
        return Err(E::InvalidTransition);
    }
    let action = if kind == "create" {
        v1::ExecutionScope::SessionCreate
    } else {
        v1::ExecutionScope::SessionResume
    };
    if gate(
        engine,
        &v1::GateQuery {
            session_id: origin.managed_session_id.clone(),
            action,
        },
    )? != v1::GateReason::Allowed
    {
        mark_indeterminate(engine, &id)?;
        return Err(E::GateDenied);
    }
    let attachment = engine.attachment(&sid).ok_or(E::GateDenied)?;
    let workspace = match requested.operation() {
        v1::Operation::BindingCreate(p) => std::fs::canonicalize(&p.workspace_root)
            .map_err(|_| E::WorkspaceInvalid)?
            .to_str()
            .ok_or(E::WorkspaceInvalid)?
            .to_owned(),
        v1::Operation::BindingResume(_) => {
            binding(engine.connection(), &sid)?
                .ok_or(E::BindingNotFound)?
                .workspace_root
        }
        _ => return Err(E::InvalidRequest),
    };
    if attachment.workspace != workspace {
        return Err(E::GateDenied);
    }
    let authority = engine.authority_id().as_str().to_owned();
    let stream = context(engine.connection())?;
    let now = now()?;
    let tx = engine
        .connection_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    attest(&tx, &authority, &stream, &now)?;
    let b = if kind == "create" {
        let collision: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM managed_session_bindings WHERE runtime_session_id=?1 AND adapter_id=?2 AND runtime_kind=?3)",
                params![thread.as_str(), adapter, runtime_kind],
                |r| r.get(0),
            )
            .map_err(db)?;
        if collision {
            return Err(E::BindingAlreadyExists);
        }
        tx.execute(
            "INSERT INTO managed_session_bindings VALUES(?1,?5,?6,?2,?3,1,?4,?4)",
            params![sid, thread.as_str(), workspace, now, adapter, runtime_kind],
        )
        .map_err(db)?;
        binding(&tx, &sid)?.ok_or(E::StorageIncompatible)?
    } else {
        let b = binding(&tx, &sid)?.ok_or(E::BindingNotFound)?;
        if b.runtime_session_id != thread {
            return Err(E::CorrelationError);
        }
        b
    };
    append(
        &tx,
        (&authority, &stream, &now),
        Draft {
            session: Some(&sid),
            kind: if kind == "create" {
                "authority.managed_session.runtime_bound"
            } else {
                "authority.managed_session.runtime_attached"
            },
            correlation: Some(&id),
            version: 1,
            payload: if kind == "create" {
                json!({"adapter_id":adapter,"runtime_kind":runtime_kind,"runtime_session_id":thread,"binding_revision":1})
            } else {
                json!({"runtime_session_id":thread,"runtime_instance_id":origin.runtime_instance_id})
            },
        },
    )?;
    tx.execute("UPDATE managed_session_operations SET state='succeeded',provider_thread_id=CASE WHEN kind='create' THEN ?1 ELSE NULL END,provider_stage=CASE WHEN kind='create' THEN 'unarchived' ELSE 'none' END,updated_at=?2 WHERE operation_id=?3 AND state='dispatching'",params![thread.as_str(),now,id]).map_err(db)?;
    save(&tx, &id, &m, &json!(b))?;
    tx.commit().map_err(db)
}
/// Reconstructs the strict protocol-2 request from its transport-independent durable body.
pub(crate) fn decode_mutation(raw: &str) -> ControlResult<Request> {
    let mut v = crate::contracts::control::unique_json(raw.as_bytes()).map_err(invalid)?;
    let o = v.as_object_mut().ok_or(E::StorageIncompatible)?;
    if o.len() != 2 || !o.contains_key("method") || !o.contains_key("params") {
        return Err(E::StorageIncompatible);
    }
    o.insert("schema_version".into(), json!(2));
    o.insert(
        "request_id".into(),
        json!("ffffffff-ffff-4fff-8fff-fffffffffffe"),
    );
    o.insert(
        "authority_instance_id".into(),
        json!("ffffffff-ffff-4fff-8fff-ffffffffffff"),
    );
    Request::parse(&serde_json::to_vec(&v).map_err(invalid)?).map_err(invalid)
}
fn abandon(engine: &mut Engine, r: &Request) -> ControlResult<Value> {
    let v1::Operation::BindingAbandon(p) = r.operation() else {
        return Err(E::InvalidRequest);
    };
    let m = mutation(r)?;
    if let Some(v) = receipt(engine.connection(), p.intent_id.as_str(), &m)? {
        return result("binding_abandoned", v);
    }
    check_fence(engine, fence(r)?)?;
    let s = session(engine.connection(), p.managed_session_id.as_str())?;
    if s.control_state != v1::ControlState::Prepared
        || binding(engine.connection(), p.managed_session_id.as_str())?.is_some()
    {
        return Err(E::InvalidTransition);
    }
    let authority = engine.authority_id().as_str().to_owned();
    let stream = context(engine.connection())?;
    let now = now()?;
    let tx = engine
        .connection_mut()
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    let n=tx.execute("UPDATE managed_session_operations SET state='failed',updated_at=?1 WHERE operation_id=?2 AND managed_session_id=?3 AND kind='create' AND state='indeterminate'",params![now,p.operation_id.as_str(),p.managed_session_id.as_str()]).map_err(db)?;
    if n != 1 {
        return Err(E::InvalidTransition);
    }
    attest(&tx, &authority, &stream, &now)?;
    append(
        &tx,
        (&authority, &stream, &now),
        Draft {
            session: Some(p.managed_session_id.as_str()),
            kind: "authority.managed_session.runtime_operation_abandoned",
            correlation: Some(p.intent_id.as_str()),
            version: 1,
            payload: json!({"operation_id":p.operation_id}),
        },
    )?;
    let v = json!({"operation_id":p.operation_id});
    save(&tx, p.intent_id.as_str(), &m, &v)?;
    tx.commit().map_err(db)?;
    result("binding_abandoned", v)
}

/// A handshake attests a connection; only an acknowledged thread operation attaches a binding.
pub(super) fn require_bound_launch(engine: &Engine, f: &RuntimeFence) -> ControlResult<()> {
    engine.observe_fence(f)?;
    let mut q=engine.connection().prepare("SELECT mutation FROM managed_session_operations WHERE managed_session_id=?1 AND state='succeeded' ORDER BY updated_at DESC").map_err(db)?;
    let mut rows = q.query([f.managed_session_id.as_str()]).map_err(db)?;
    while let Some(row) = rows.next().map_err(db)? {
        let raw: String = row.get(0).map_err(db)?;
        // Historical V1 attachment never authorizes a V8 launch.
        if let Ok(request) = decode_mutation(&raw)
            && fence(&request).is_ok_and(|old| old == f)
        {
            return Ok(());
        }
    }
    Err(E::GateDenied)
}

#[cfg(test)]
#[path = "operations_tests.rs"]
mod tests;

/// Complete bounded provider proposal; no Debug and no durable serialization.
#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderAction {
    Command {
        command: String,
        cwd: String,
        parsed_actions: Vec<String>,
    },
    Files {
        changes: Vec<ProviderFileChange>,
    },
}
#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderFileChange {
    path: String,
    kind: v1::FileChangeKind,
    move_path: Option<String>,
    diff: String,
}
pub(crate) enum HeldAction {
    Command(crate::control::approval_action::CommandAction),
    Files(crate::control::approval_action::FileAction),
}
impl ProviderAction {
    pub fn metadata(
        &self,
        item: &str,
    ) -> ControlResult<(v1::ApprovalKind, String, v1::ApprovalStructure)> {
        let h = self.clone().build(item)?;
        Ok((h.kind(), h.fingerprint(), h.structure()))
    }
    pub(crate) fn build(self, item: &str) -> ControlResult<HeldAction> {
        use crate::control::approval_action::{CommandAction, FileAction, FileChange};
        match self {
            Self::Command {
                command,
                cwd,
                parsed_actions,
            } => Ok(HeldAction::Command(CommandAction::new(
                item,
                &command,
                &cwd,
                parsed_actions,
            )?)),
            Self::Files { changes } => Ok(HeldAction::Files(FileAction::new(
                item,
                changes
                    .into_iter()
                    .map(|c| FileChange {
                        path: c.path,
                        kind: c.kind,
                        move_path: c.move_path,
                        diff: c.diff,
                    })
                    .collect(),
            )?)),
        }
    }
}
impl HeldAction {
    pub(crate) fn fingerprint(&self) -> String {
        match self {
            Self::Command(c) => c.fingerprint(),
            Self::Files(f) => f.fingerprint(),
        }
    }
    pub(crate) fn structure(&self) -> v1::ApprovalStructure {
        match self {
            Self::Command(c) => c.structure(),
            Self::Files(f) => f.structure(),
        }
    }
    pub(crate) fn kind(&self) -> v1::ApprovalKind {
        match self {
            Self::Command(_) => v1::ApprovalKind::CommandExecution,
            Self::Files(_) => v1::ApprovalKind::FileChange,
        }
    }
    pub(crate) fn confined(&self, workspace: &str) -> bool {
        match self {
            Self::Command(c) => c.confined(workspace),
            Self::Files(f) => f.confined(workspace),
        }
    }
    pub(crate) fn presentation(
        &self,
        index: Option<u16>,
        offset: Option<u32>,
    ) -> ControlResult<Value> {
        match self {
            Self::Command(c) => {
                if index.is_some() || offset.is_some() {
                    return Err(E::InvalidRequest);
                }
                Ok(json!({"command":c.presentation(),"file_count":null,"file":null}))
            }
            Self::Files(f) => Ok(
                json!({"command":null,"file_count":f.files(),"file":f.page(index.unwrap_or(0),offset.unwrap_or(0))?}),
            ),
        }
    }
}
