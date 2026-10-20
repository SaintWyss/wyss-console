//! Inactive protocol-2 / schema-8 authority facade. It owns an `Engine` and routes strict
//! protocol-2 requests to the durable writers, the engine and its fences. There is no socket, no
//! server loop, no installed-store opener and no provider launcher here; `handle` is a pure
//! bytes-in/bytes-out function for tests and for the future server.
//!
//! Nothing is inferred: the target of every routed request is the explicit runtime fence in the
//! request, checked against the registry (live work) or the immutable sidecar (history) before
//! any state changes; leases, approvals and cancellation re-run their gates at the moment of the
//! effect; a provider answer is sent only after its durable decision committed and the engine
//! revalidated the ticket. Methods which still need a schema-8 port (session prepare/close,
//! bindings, model catalogue, turn/approval reads, gate query) answer `unsupported`.
use super::{
    engine::{Attachment, Engine, LeaseClaim, LeaseView, Spawner, TurnTicket},
    protocol::{Request, Response, RuntimeFence, RuntimeRef, Target, encode},
    writers::{Decided, TurnIntent, Writer},
};
use crate::contracts::{
    WireUuid,
    approval::{ApprovalDecision, ApprovalKind, ApprovalReason, ApprovalStructure},
    control::{self as v1, ControlError as E, ControlResult, unique_json},
    identities::ManagedTurnId,
    turn::{TurnReason, TurnState},
};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, time::Instant};

const NIL: &str = "00000000-0000-4000-8000-000000000000";
const PAGE_BYTES: usize = 65_536 - 1024;

/// Per-connection state: the authority instance observed in the handshake.
#[derive(Default)]
pub struct Client {
    observed: Option<WireUuid>,
}

pub struct Authority {
    engine: Engine,
    /// In-memory admission tickets by turn id: transient, launch-scoped, never durable.
    tickets: BTreeMap<String, TurnTicket>,
}

/// Lease facts read from the database at the moment of an effect.
struct Snapshot {
    session: String,
    lease: String,
    active: bool,
}
impl LeaseView for Snapshot {
    fn active(&self, session: &str, lease: &str, _now_ms: u64) -> bool {
        self.active && self.session == session && self.lease == lease
    }
}
fn lease_snapshot(c: &Connection, session: &str, lease: &str) -> Snapshot {
    let active = super::writers::read_lease(c, lease).is_ok_and(|l| {
        l.session_id.as_str() == session
            && l.state == crate::contracts::execution::LeaseState::Active
            && crate::contracts::execution::timestamp(&l.expires_at)
                .is_ok_and(|t| t > time::OffsetDateTime::now_utc())
    });
    Snapshot {
        session: session.to_owned(),
        lease: lease.to_owned(),
        active,
    }
}

fn request_id_of(bytes: &[u8]) -> WireUuid {
    unique_json(bytes)
        .ok()
        .and_then(|v| v["request_id"].as_str().map(str::to_owned))
        .and_then(|s| WireUuid::try_from(s).ok())
        .unwrap_or_else(|| WireUuid::try_from(NIL.to_owned()).expect("nil uuid"))
}

impl Authority {
    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            tickets: BTreeMap::new(),
        }
    }
    pub fn engine(&self) -> &Engine {
        &self.engine
    }
    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    fn authority(&self) -> WireUuid {
        self.engine.authority_id().clone()
    }
    fn fail(&self, id: &WireUuid, e: E) -> Vec<u8> {
        Response::error(id.clone(), self.authority(), e)
            .and_then(|r| r.encode())
            .unwrap_or_default()
    }
    fn ok(&self, id: &WireUuid, result: Value) -> Vec<u8> {
        let built = json!({"schema_version":2,"request_id":id,
            "authority_instance_id":self.authority(),"result":result});
        encode(&built)
            .and_then(|b| Response::parse(&b))
            .and_then(|r| r.encode())
            .unwrap_or_else(|e| self.fail(id, e))
    }

    /// One strict protocol-2 exchange. Always returns a well-formed response frame.
    pub fn handle(
        &mut self,
        client: &mut Client,
        bytes: &[u8],
        spawner: &mut dyn Spawner,
        now: Instant,
    ) -> Vec<u8> {
        let id = request_id_of(bytes);
        let request = match Request::parse(bytes) {
            Ok(r) => r,
            Err(e) => return self.fail(&id, e),
        };
        if let Err(e) = request.correlate(client.observed.as_ref()) {
            return self.fail(request.request_id(), e);
        }
        match self.route(client, &request, spawner, now) {
            Ok(result) => self.ok(request.request_id(), result),
            Err(e) => self.fail(request.request_id(), e),
        }
    }

    fn route(
        &mut self,
        client: &mut Client,
        request: &Request,
        spawner: &mut dyn Spawner,
        now: Instant,
    ) -> ControlResult<Value> {
        use v1::Operation as O;
        match request.operation() {
            O::Hello(_) => {
                client.observed = Some(self.authority());
                self.status("hello")
            }
            O::Status(_) => self.status("status"),
            O::RuntimeStart(_) => {
                let o = self.engine.start(request, spawner)?;
                Ok(runtime_control(&o))
            }
            O::RuntimeStop(_) => {
                let o = self.engine.stop(request, now)?;
                Ok(runtime_control(&o))
            }
            O::RuntimeStatus(_) => self.runtime_status(request),
            O::LeaseGrant(_) | O::LeaseRenew(_) | O::LeaseRevoke(_) => self.lease_op(request),
            O::LeaseGet(p) => {
                let l = super::writers::read_lease(self.engine.connection(), p.lease_id.as_str())?;
                lease_value(&l)
            }
            O::LeaseActive(p) => {
                let l =
                    super::writers::active_lease(self.engine.connection(), p.session_id.as_str())?;
                Ok(json!({"kind":"active_lease","value":l}))
            }
            O::TurnStart(p) => self.turn_start(request, p),
            O::TurnCancel(p) => self.turn_cancel(request, p),
            O::ApprovalRespond(p) => self.approval_respond(request, p, now),
            O::AuditRead(p) => self.audit(&p.stream_id, p.after_sequence, p.limit),
            O::AuditWatch(p) => self.audit(&p.stream_id, p.after_sequence, p.limit),
            _ => Err(E::Unsupported),
        }
    }

    fn status(&self, kind: &str) -> ControlResult<Value> {
        let c = self.engine.connection();
        let (stream, head): (String, String) = c
            .query_row(
                "SELECT stream_id,last_sequence FROM authority_metadata WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| E::StorageUnavailable)?;
        let count = |state: &str| -> ControlResult<i64> {
            c.query_row(
                "SELECT count(*) FROM managed_sessions WHERE control_state=?1",
                [state],
                |r| r.get(0),
            )
            .map_err(|_| E::StorageUnavailable)
        };
        Ok(json!({"kind":kind,"value":{"protocol_version":2,
            "authority_instance_id":self.authority(),"service_version":env!("CARGO_PKG_VERSION"),
            "storage_schema_version":8,"stream_id":stream,
            "last_sequence":if head == "0" { Value::Null } else { json!(head) },
            "prepared_count":count("prepared")?,"closed_count":count("closed")?}}))
    }

    fn runtime_status(&self, request: &Request) -> ControlResult<Value> {
        let Target::Runtime { session, .. } = request.target() else {
            return Err(E::InvalidRequest);
        };
        let s = session.as_str();
        let row: Option<(String, i64, i64)> = self
            .engine
            .connection()
            .query_row(
                "SELECT desired_state,revision,generation FROM managed_runtimes WHERE managed_session_id=?1",
                [s],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|_| E::StorageUnavailable)?;
        let (desired, revision, generation) = row.unwrap_or(("stopped".into(), 0, 0));
        let view = self.engine.runtime_view(s);
        let (actual, identity) = match &view {
            Some((phase, id)) => (*phase, id.clone()),
            None => ("stopped", None),
        };
        let mut value = json!({"adapter_id":"codex","runtime_kind":"codex","desired_state":desired,
            "revision":revision.max(0),"actual_state":actual,
            "runtime_instance_id":identity.as_ref().map(|i| i.runtime_instance_id.clone()),
            "runtime_version":identity.as_ref().map(|i| i.observed_version.clone()),
            "compatibility":if identity.is_some() { "compatible" } else { "unknown" },
            "model_count":0,"diagnostic":null,
            "outbound_methods":{"initialize":0,"initialized":0,"model_list":0},
            "managed_session_id":s,"generation":generation,
            "runtime_identity":identity,"budget_failure":null});
        if revision == 0 {
            // A never-allocated runtime has no desired-state record to report.
            value["revision"] = json!(1);
            value["desired_state"] = json!("stopped");
        }
        Ok(json!({"kind":"runtime_status","value":value}))
    }

    /// The fence names the live attested launch, or the request refuses before anything changes.
    fn fenced(&self, request: &Request) -> ControlResult<RuntimeFence> {
        match request.target() {
            Target::Fenced(f) => {
                self.engine.observe_fence(f)?;
                Ok(f.clone())
            }
            _ => Err(E::InvalidRequest),
        }
    }

    fn lease_op(&mut self, request: &Request) -> ControlResult<Value> {
        // Grant and renewal are launch-fenced (protocol 2). Revocation deliberately is not: a
        // grant must stay revocable when its runtime is gone.
        let revoke = matches!(request.operation(), v1::Operation::LeaseRevoke(_));
        let fence = if revoke {
            None
        } else {
            Some(self.fenced(request)?)
        };
        if let (Some(fence), v1::Operation::LeaseRenew(p)) = (&fence, request.operation()) {
            // A renewal targets a lease of the fence's own session, never another's.
            let l = super::writers::read_lease(self.engine.connection(), p.lease_id.as_str())?;
            if l.session_id.as_str() != fence.managed_session_id.as_str() {
                return Err(E::GateDenied);
            }
        }
        let authority = self.authority();
        let lease =
            Writer::new(self.engine.connection_mut(), authority)?.lease(request.operation())?;
        if lease.state != crate::contracts::execution::LeaseState::Active {
            let session = lease.session_id.as_str().to_owned();
            self.lease_lost(&session, lease.lease_id.as_str());
        }
        lease_value(&lease)
    }
    fn lease_lost(&mut self, session: &str, lease_id: &str) {
        self.engine.lease_lost(session, lease_id);
        self.tickets.retain(|_, t| t.lease_id != lease_id);
    }

    fn turn_start(
        &mut self,
        request: &Request,
        p: &crate::contracts::turn::TurnStart,
    ) -> ControlResult<Value> {
        let fence = self.fenced(request)?;
        let session = fence.managed_session_id.as_str().to_owned();
        let reference = self.engine.reference(&session).ok_or(E::StaleRevision)?;
        let lease = super::writers::active_lease(self.engine.connection(), &session)?
            .ok_or(E::GateDenied)?;
        let intent = TurnIntent {
            intent_id: p.intent_id.clone(),
            expected_session_revision: p.expected_session_revision,
            prompt_fingerprint: format!("{:x}", Sha256::digest(p.prompt.as_bytes())),
            prompt_byte_length: p.prompt.len(),
        };
        let authority = self.authority();
        let (record, fresh) = Writer::new(self.engine.connection_mut(), authority.clone())?
            .admit_turn_ex(&reference, &intent, lease.lease_id.as_str())?;
        if !fresh {
            return Ok(json!({"kind":"turn","value":record}));
        }
        let attachment: Attachment = self.engine.attachment(&session).ok_or(E::GateDenied)?;
        let snapshot = lease_snapshot(self.engine.connection(), &session, lease.lease_id.as_str());
        let admitted = self.engine.admit_turn(
            &fence,
            &LeaseClaim {
                lease_id: lease.lease_id.as_str().to_owned(),
                session: session.clone(),
            },
            &attachment,
            record.managed_turn_id.as_str(),
            &snapshot,
            0,
        );
        let ticket = match admitted {
            Ok(t) => t,
            Err(e) => {
                self.fail_turn(&record.managed_turn_id)?;
                return Err(e);
            }
        };
        let frame =
            serde_json::to_vec(&json!({"method":"turn/start","turn":record.managed_turn_id,
            "input":p.prompt}))
            .map_err(|_| E::InternalError)?;
        if let Err(e) = self.engine.write_frame(&ticket, &frame, &snapshot, 0) {
            let _ = self.engine.end_turn(&ticket);
            self.fail_turn(&record.managed_turn_id)?;
            return Err(e);
        }
        self.tickets
            .insert(record.managed_turn_id.as_str().to_owned(), ticket);
        Ok(json!({"kind":"turn","value":record}))
    }
    /// A turn which never reached the provider ends failed, with no effect to be unknown.
    fn fail_turn(&mut self, turn: &ManagedTurnId) -> ControlResult<()> {
        let authority = self.authority();
        Writer::new(self.engine.connection_mut(), authority)?
            .transition_turn(
                turn,
                None,
                TurnState::Failed,
                Some(TurnReason::ProviderFailed),
            )
            .map(|_| ())
    }

    fn turn_cancel(
        &mut self,
        request: &Request,
        p: &crate::contracts::turn::TurnCancel,
    ) -> ControlResult<Value> {
        let Target::Fenced(fence) = request.target() else {
            return Err(E::InvalidRequest);
        };
        // The fence must be the very launch the turn was admitted on (immutable sidecar).
        let sidecar: Option<(String, i64, String)> = self
            .engine
            .connection()
            .query_row(
                "SELECT runtime_instance_id,generation,managed_session_id FROM managed_turn_launches WHERE managed_turn_id=?1",
                [p.managed_turn_id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|_| E::StorageUnavailable)?;
        let (instance, generation, session) = sidecar.ok_or(E::TurnNotFound)?;
        if fence.managed_session_id.as_str() != session
            || fence.runtime_instance_id.as_ref().map(|i| i.as_str()) != Some(instance.as_str())
            || fence.generation != generation as u64
        {
            return Err(E::StaleRevision);
        }
        let authority = self.authority();
        let (record, dispatch) =
            Writer::new(self.engine.connection_mut(), authority)?.cancel_turn(p)?;
        if dispatch {
            self.engine.observe_fence(fence)?;
            if let Some(ticket) = self.tickets.get(p.managed_turn_id.as_str()).cloned() {
                let snapshot = lease_snapshot(self.engine.connection(), &session, &ticket.lease_id);
                // A refused interrupt is reported by the next poll/timeout; never retried blindly.
                let _ = self.engine.write_frame(
                    &ticket,
                    b"{\"method\":\"turn/interrupt\"}",
                    &snapshot,
                    0,
                );
            }
        }
        Ok(json!({"kind":"turn","value":record}))
    }

    fn approval_respond(
        &mut self,
        request: &Request,
        p: &crate::contracts::approval::ApprovalRespond,
        now: Instant,
    ) -> ControlResult<Value> {
        let _ = now;
        let fence = self.fenced(request)?;
        let session = fence.managed_session_id.as_str().to_owned();
        let row: Option<(String, String, String, String)> = self
            .engine
            .connection()
            .query_row(
                "SELECT managed_turn_id,provider_item_id,managed_session_id,action_fingerprint FROM managed_approvals WHERE managed_approval_id=?1",
                [p.managed_approval_id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(|_| E::StorageUnavailable)?;
        let (turn, item, owner, durable_fingerprint) = row.ok_or(E::ApprovalNotFound)?;
        if owner != session {
            return Err(E::GateDenied);
        }
        // The fingerprint compared at decision time is the provider's own live request, never the
        // client's claim. After a restart none exists, so a late answer cannot be decided at all.
        let live = self.engine.pending_fingerprint(&session, &item);
        // A durable fingerprint is history, not a live admission ticket. Only an
        // exact replay may be read after the pending provider request disappeared.
        // The writer still validates the canonical replay payload itself.
        if live.is_none() {
            let existing: Option<i64> = self
                .engine
                .connection()
                .query_row(
                    "SELECT 1 FROM processed_intents WHERE intent_id=?1",
                    [p.intent_id.as_str()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|_| E::StorageUnavailable)?;
            if existing.is_none() {
                return Err(E::StaleRevision);
            }
        }
        let authority = self.authority();
        let decided = Writer::new(self.engine.connection_mut(), authority.clone())?
            .decide_approval(
                &p.managed_approval_id,
                &p.intent_id,
                p.expected_revision,
                p.decision,
                live.as_deref().unwrap_or(&durable_fingerprint),
            );
        match decided? {
            Decided::Replayed(receipt) => Ok(json!({"kind":"approval_receipt","value":receipt})),
            Decided::Fresh(record, receipt) => {
                let ticket = self.tickets.get(&turn).cloned();
                let word = match p.decision {
                    ApprovalDecision::Accept => "accept",
                    ApprovalDecision::Decline => "decline",
                    ApprovalDecision::Cancel => "cancel",
                };
                let sent = match (&ticket, &live) {
                    (Some(t), Some(fp)) => {
                        let snapshot =
                            lease_snapshot(self.engine.connection(), &session, &t.lease_id);
                        self.engine
                            .answer_approval(t, &snapshot, 0, &item, fp, word)
                    }
                    _ => Err(E::StaleRevision),
                };
                if sent.is_err() {
                    // All decisions are already durable. A declined or cancelled request
                    // that was not delivered must NOT be reported as successful dispatch.
                    // Fence the affected runtime; never retry an ambiguous provider write.
                    if let Some(t) = ticket.as_ref() {
                        self.engine.transport_failed(&t.reference);
                    }
                    if p.decision == ApprovalDecision::Accept {
                        // A persisted accept may have reached the provider before the
                        // transport failed. Keep that effect indeterminate, not retried.
                        let _ = Writer::new(self.engine.connection_mut(), authority)?
                            .settle_approvals(
                                Some(&record.managed_turn_id),
                                ApprovalReason::EffectUnknown,
                            );
                        return Err(E::ExternalEffectUnknown);
                    }
                    return Err(E::TransportError);
                }
                Ok(json!({"kind":"approval_receipt","value":receipt}))
            }
        }
    }

    fn audit(
        &self,
        stream: &WireUuid,
        after: Option<crate::contracts::Sequence>,
        limit: u16,
    ) -> ControlResult<Value> {
        let c = self.engine.connection();
        let (current, head): (String, String) = c
            .query_row(
                "SELECT stream_id,last_sequence FROM authority_metadata WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| E::StorageUnavailable)?;
        let after = after.map_or(0, |s| s.get());
        if limit == 0
            || stream.as_str() != current
            || after > head.parse::<u64>().map_err(|_| E::StorageIncompatible)?
        {
            return Err(E::InvalidRequest);
        }
        let mut q = c
            .prepare(
                "SELECT envelope FROM control_events WHERE sequence>?1 ORDER BY sequence LIMIT ?2",
            )
            .map_err(|_| E::StorageUnavailable)?;
        let mut rows = q
            .query(rusqlite::params![
                format!("{after:020}"),
                i64::from(limit) + 1
            ])
            .map_err(|_| E::StorageUnavailable)?;
        let (mut events, mut bytes, mut more) = (Vec::new(), 0usize, false);
        while let Some(r) = rows.next().map_err(|_| E::StorageUnavailable)? {
            let raw: String = r.get(0).map_err(|_| E::StorageUnavailable)?;
            if events.len() == usize::from(limit)
                || (!events.is_empty() && bytes + raw.len() + 1 > PAGE_BYTES)
            {
                more = true;
                break;
            }
            bytes += raw.len() + 1;
            events.push(unique_json(raw.as_bytes()).map_err(|_| E::StorageIncompatible)?);
        }
        let next = if more {
            events
                .last()
                .and_then(|e| e["sequence"].as_str().map(str::to_owned))
        } else {
            None
        };
        Ok(json!({"kind":"audit","value":{"events":events,"next":next}}))
    }

    // ---------------------------------------------------------- provider side

    /// The provider asked for an approval on the running turn of `from`. Durable first (under the
    /// turn's original lease), then the live correlation; either failing leaves nothing half-made.
    pub fn provider_approval_request(
        &mut self,
        from: &RuntimeRef,
        kind: ApprovalKind,
        provider_item_id: &str,
        action_fingerprint: &str,
        structure: ApprovalStructure,
    ) -> ControlResult<crate::contracts::approval::ManagedApprovalRecord> {
        // Verify the independently observed provider connection before creating durable work.
        // A caller-supplied record ID cannot select a different session or launch.
        self.engine.observe_fence(&from.fence())?;
        let session = from.managed_session_id.as_str().to_owned();
        let turn = self
            .engine
            .active_turn(&session)
            .ok_or(E::GateDenied)?
            .to_owned();
        let turn = ManagedTurnId::parse(&turn).map_err(|_| E::InternalError)?;
        self.engine.can_request_approval(from, provider_item_id)?;
        let authority = self.authority();
        let record = Writer::new(self.engine.connection_mut(), authority.clone())?.open_approval(
            &turn,
            kind,
            provider_item_id,
            action_fingerprint,
            structure,
        )?;
        if let Err(e) = self
            .engine
            .approval_requested(from, provider_item_id, action_fingerprint)
        {
            // Only an unexpected post-write admission failure reaches here.
            // This runtime is inconsistent; fail the launch closed rather than
            // silently destroying other approvals while continuing to execute.
            self.engine.transport_failed(from);
            let _ = Writer::new(self.engine.connection_mut(), authority)?
                .settle_approvals(Some(&turn), ApprovalReason::Unsupported);
            return Err(e);
        }
        Ok(record)
    }
    /// The provider reported its turn running / finished. Terminal states end the live turn.
    pub fn provider_turn_event(
        &mut self,
        from: &RuntimeRef,
        turn: &ManagedTurnId,
        provider_turn_id: Option<&str>,
        state: TurnState,
        reason: Option<TurnReason>,
    ) -> ControlResult<()> {
        // The provider connection, not the event's record ID, determines its launch.
        // Durable ownership by *this authority* is insufficient when multiple sessions
        // belong to the same authority.
        self.engine.observe_fence(&from.fence())?;
        let row: Option<(String, String, i64)> = self
            .engine
            .connection()
            .query_row(
                "SELECT managed_session_id,runtime_instance_id,generation FROM managed_turn_launches WHERE managed_turn_id=?1",
                [turn.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|_| E::StorageUnavailable)?;
        let (session, instance, generation) = row.ok_or(E::TurnNotFound)?;
        if session != from.managed_session_id.as_str()
            || instance != from.runtime_instance_id.as_str()
            || generation != i64::try_from(from.generation).map_err(|_| E::StaleRevision)?
            || self.engine.active_turn(&session) != Some(turn.as_str())
        {
            return Err(E::StaleRevision);
        }
        let authority = self.authority();
        Writer::new(self.engine.connection_mut(), authority)?.transition_turn(
            turn,
            provider_turn_id,
            state,
            reason,
        )?;
        if state.terminal()
            && let Some(ticket) = self.tickets.remove(turn.as_str())
        {
            let _ = self.engine.end_turn(&ticket);
        }
        Ok(())
    }
    /// A provider acknowledgement is attributed to its actual connection and item.
    /// The durable approval ID alone is never a dispatch or confirmation capability.
    pub fn provider_approval_confirmed(
        &mut self,
        from: &RuntimeRef,
        approval: &crate::contracts::identities::ManagedApprovalId,
        provider_item_id: &str,
    ) -> ControlResult<()> {
        self.engine.observe_fence(&from.fence())?;
        let row: Option<(String, String, String, String, i64)> = self
            .engine
            .connection()
            .query_row(
                "SELECT a.managed_session_id,a.managed_turn_id,a.provider_item_id,l.runtime_instance_id,l.generation                  FROM managed_approvals a JOIN managed_approval_launches l                  ON a.managed_approval_id=l.managed_approval_id WHERE a.managed_approval_id=?1",
                [approval.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()
            .map_err(|_| E::StorageUnavailable)?;
        let (session, turn, item, instance, generation) = row.ok_or(E::ApprovalNotFound)?;
        if session != from.managed_session_id.as_str()
            || instance != from.runtime_instance_id.as_str()
            || generation != i64::try_from(from.generation).map_err(|_| E::StaleRevision)?
            || item != provider_item_id
            || self.engine.active_turn(&session) != Some(turn.as_str())
        {
            return Err(E::StaleRevision);
        }
        let authority = self.authority();
        Writer::new(self.engine.connection_mut(), authority)?
            .confirm_approval(approval)
            .map(|_| ())
    }

    /// Periodic duty, outside any request: expire leases and approvals (and tell the engine),
    /// drive process lifecycles, and record work stranded by launches that ended.
    pub fn tick(&mut self, now: Instant) -> ControlResult<()> {
        let authority = self.authority();
        let expired: Vec<(String, String)> = {
            let mut q = self
                .engine
                .connection()
                .prepare("SELECT session_id,lease_id FROM execution_leases WHERE state='active' AND expires_at<=?1")
                .map_err(|_| E::StorageUnavailable)?;
            let at = time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|_| E::InternalError)?;
            q.query_map([at], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(|_| E::StorageUnavailable)?
                .collect::<Result<_, _>>()
                .map_err(|_| E::StorageUnavailable)?
        };
        Writer::new(self.engine.connection_mut(), authority.clone())?.expire_leases()?;
        for (session, lease) in expired {
            self.lease_lost(&session, &lease);
        }
        Writer::new(self.engine.connection_mut(), authority.clone())?.expire_approvals()?;
        self.engine.poll(now);
        if self.engine.take_ended() > 0 {
            Writer::new(self.engine.connection_mut(), authority)?.recover()?;
            let live: Vec<String> = self.tickets.keys().cloned().collect();
            for turn in live {
                let state: Option<String> = self
                    .engine
                    .connection()
                    .query_row(
                        "SELECT state FROM managed_turns WHERE managed_turn_id=?1",
                        [&turn],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(|_| E::StorageUnavailable)?;
                if state
                    .as_deref()
                    .is_none_or(|s| !matches!(s, "starting" | "running"))
                {
                    self.tickets.remove(&turn);
                }
            }
        }
        Ok(())
    }
}

fn runtime_control(o: &super::allocator::Outcome) -> Value {
    json!({"kind":"runtime_control","value":{"adapter_id":o.record.adapter_id,"runtime_kind":o.record.runtime_kind,
        "desired_state":o.record.desired_state,"revision":o.record.revision,"created_at":o.record.created_at,
        "updated_at":o.record.updated_at,"managed_session_id":o.managed_session_id,"generation":o.generation}})
}
fn lease_value(l: &crate::contracts::execution::ExecutionLease) -> ControlResult<Value> {
    Ok(json!({"kind":"lease","value":l}))
}

#[cfg(test)]
#[path = "authority_tests.rs"]
mod tests;
