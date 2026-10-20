//! Durable schema-8 turn and approval writers. Inactive: no server, provider or installed store.
//!
//! Every mutation is one IMMEDIATE transaction holding the canonical row, its launch sidecar
//! (inserted before the row, as the SQL contract requires), the vocabulary-2 audit event with the
//! launch generation, the replay receipt and, for the first write of an authority instance, its
//! announcement. Work may only be created or progressed by the authority which reserved the
//! launch, while that launch is the session's current, attested, unexited, running generation;
//! any other authority can only record the loss of stranded work (failed / interrupted /
//! indeterminate turns, invalidated / expired / indeterminate approvals). Nothing here replays a
//! provider response or revives work of a previous generation.
use super::{
    allocator::{Draft, Fault, append, attest, db, events_for, invalid, step},
    audit::AuditReplay,
    protocol::RuntimeRef,
};
use crate::contracts::{
    WireUuid,
    approval::{
        ApprovalDecision, ApprovalEffect, ApprovalKind, ApprovalReason, ApprovalReceipt,
        ApprovalState, ApprovalStructure, ManagedApprovalRecord, valid_fingerprint,
    },
    control::Operation,
    control::{ControlError as E, ControlResult, unique_json},
    execution::{ExecutionLease, LeaseState, RevocationReason, validate_scopes, validate_ttl},
    identities::{ManagedApprovalId, ManagedTurnId},
    turn::{ManagedTurnRecord, TurnReason, TurnState},
};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

const APPROVAL_TTL_SECONDS: i64 = crate::contracts::approval::APPROVAL_TTL_SECONDS;
const METHOD_RESPOND: &str = "managedTurn/approval/respond";
const METHOD_TURN: &str = "managedSession/turn/start";

/// What a client asked for, reduced to fingerprints: the prompt never enters durable state.
#[derive(Debug, Clone)]
pub struct TurnIntent {
    pub intent_id: WireUuid,
    pub expected_session_revision: u64,
    pub prompt_fingerprint: String,
    pub prompt_byte_length: usize,
}

#[derive(Debug)]
pub enum Decided {
    /// Only this entitles the caller to answer the provider (exactly once).
    Fresh(Box<ManagedApprovalRecord>, ApprovalReceipt),
    /// The original receipt of a committed decision; never causes a provider response.
    Replayed(ApprovalReceipt),
}

pub struct Writer<'c> {
    conn: &'c mut Connection,
    authority: WireUuid,
    stream: String,
    clock: Box<dyn Fn() -> OffsetDateTime + 'c>,
    fault: Fault<'c>,
}

fn stamp(t: OffsetDateTime) -> ControlResult<String> {
    t.format(&Rfc3339).map_err(invalid)
}
fn ts(s: &str) -> ControlResult<OffsetDateTime> {
    OffsetDateTime::parse(s, &Rfc3339).map_err(invalid)
}
fn enum_text(v: &impl serde::Serialize) -> ControlResult<String> {
    serde_json::to_value(v)
        .map_err(invalid)?
        .as_str()
        .map(str::to_owned)
        .ok_or(E::InternalError)
}

impl<'c> Writer<'c> {
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
            clock: Box::new(OffsetDateTime::now_utc),
            fault: None,
        })
    }
    pub fn with_clock(mut self, clock: impl Fn() -> OffsetDateTime + 'c) -> Self {
        self.clock = Box::new(clock);
        self
    }
    /// Fault injection: called after each transactional write step and before commit.
    pub fn with_fault(mut self, f: impl FnMut(usize) -> ControlResult<()> + 'c) -> Self {
        self.fault = Some(Box::new(f));
        self
    }

    // ------------------------------------------------------------------ turns

    /// Admits a turn on the exact live launch under the session's active lease. An identical
    /// intent returns the original record with zero side effects; a changed one conflicts.
    pub fn admit_turn(
        &mut self,
        launch: &RuntimeRef,
        intent: &TurnIntent,
        lease_id: &str,
    ) -> ControlResult<ManagedTurnRecord> {
        self.admit_turn_ex(launch, intent, lease_id).map(|(t, _)| t)
    }
    /// As `admit_turn`, also reporting whether this call created the turn (`true`) or returned
    /// the original record of an identical, already committed intent (`false`).
    pub fn admit_turn_ex(
        &mut self,
        launch: &RuntimeRef,
        intent: &TurnIntent,
        lease_id: &str,
    ) -> ControlResult<(ManagedTurnRecord, bool)> {
        launch.validate()?;
        let session = launch.managed_session_id.as_str().to_owned();
        let mutation = json!({"method":METHOD_TURN,"params":{"intent_id":intent.intent_id,
            "managed_session_id":session,"expected_session_revision":intent.expected_session_revision,
            "prompt_fingerprint":intent.prompt_fingerprint,"prompt_byte_length":intent.prompt_byte_length}})
        .to_string();
        if !valid_fingerprint(&intent.prompt_fingerprint)
            || !(1..=8192).contains(&intent.prompt_byte_length)
            || intent.expected_session_revision == 0
        {
            return Err(E::InvalidRequest);
        }
        let (authority, stream) = (self.authority.as_str().to_owned(), self.stream.clone());
        let (tx, now) = begin(&mut *self.conn, &*self.clock)?;
        if let Some((m, r)) = receipt(&tx, intent.intent_id.as_str())? {
            if m != mutation {
                return Err(E::IntentConflict);
            }
            return Ok((serde_json::from_str(&r).map_err(invalid)?, false));
        }
        require_live(&tx, launch, &authority)?;
        let (state, revision): (String, i64) = tx
            .query_row(
                "SELECT control_state,revision FROM managed_sessions WHERE session_id=?1",
                [&session],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| E::SessionNotFound)?;
        if state != "prepared" {
            return Err(E::InvalidTransition);
        }
        if revision as u64 != intent.expected_session_revision {
            return Err(E::StaleRevision);
        }
        let thread: String = tx
            .query_row(
                "SELECT runtime_session_id FROM managed_session_bindings WHERE managed_session_id=?1",
                [&session],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?
            .ok_or(E::BindingNotFound)?;
        let (lease_session, lease_state, lease_revision, lease_expires, scopes): (String, String, i64, String, String) = tx
            .query_row(
                "SELECT session_id,state,revision,expires_at,scopes FROM execution_leases WHERE lease_id=?1",
                [lease_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()
            .map_err(db)?
            .ok_or(E::LeaseNotFound)?;
        let scopes: Vec<String> = serde_json::from_str(&scopes).map_err(invalid)?;
        if lease_session != session
            || lease_state != "active"
            || now >= ts(&lease_expires)?
            || !scopes.iter().any(|s| s == "turn_start")
        {
            return Err(E::GateDenied);
        }
        let active: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM managed_turns WHERE managed_session_id=?1 AND state IN ('starting','running'))",
                [&session],
                |r| r.get(0),
            )
            .map_err(db)?;
        if active {
            return Err(E::ActiveTurnExists);
        }
        attest(&tx, &authority, &stream, &stamp(now)?)?;
        let turn_id = crate::control::store::uuid().as_str().to_owned();
        let at = stamp(now)?;
        let record: ManagedTurnRecord = serde_json::from_value(json!({
            "managed_turn_id":turn_id,"managed_session_id":session,"codex_thread_id":thread,
            "codex_turn_id":null,"admitted_lease_id":lease_id,"admitted_lease_revision":lease_revision,
            "runtime_instance_id":launch.runtime_instance_id,"state":"starting","revision":1,
            "created_at":at,"updated_at":at,"terminal_at":null,"terminal_reason":null}))
        .map_err(invalid)?;
        record.validate()?;
        tx.execute(
            "INSERT INTO managed_turn_launches VALUES(?1,?2,?3,?4)",
            params![
                turn_id,
                session,
                launch.runtime_instance_id.as_str(),
                launch.generation as i64
            ],
        )
        .map_err(db)?;
        tx.execute(
            "INSERT INTO managed_turns VALUES(?1,?2,?3,NULL,?4,?5,?6,'starting',1,?7,?8,?9,?10,1,?11,?11,NULL,NULL)",
            params![turn_id, session, thread, lease_id, lease_revision, launch.runtime_instance_id.as_str(),
                intent.prompt_fingerprint, intent.prompt_byte_length as i64, intent.intent_id.as_str(), mutation, at],
        )
        .map_err(db)?;
        step(&mut self.fault, 0)?;
        turn_event(
            &tx,
            (&authority, &stream),
            &record,
            launch.generation,
            Some(intent.intent_id.as_str()),
            "admitted",
        )?;
        step(&mut self.fault, 1)?;
        let result = serde_json::to_string(&record).map_err(invalid)?;
        tx.execute(
            "INSERT INTO processed_intents VALUES(?1,?2,?3)",
            params![intent.intent_id.as_str(), mutation, result],
        )
        .map_err(db)?;
        step(&mut self.fault, 2)?;
        tx.commit().map_err(db)?;
        Ok((record, true))
    }

    /// One legal transition of a turn. Progress (`running`, `completed`) belongs to the live
    /// owner; loss (`failed`, `interrupted`, `indeterminate`) can be recorded by any attested
    /// authority, including for a launch it did not start.
    pub fn transition_turn(
        &mut self,
        turn: &ManagedTurnId,
        provider_turn_id: Option<&str>,
        state: TurnState,
        reason: Option<TurnReason>,
    ) -> ControlResult<ManagedTurnRecord> {
        let (authority, stream) = (self.authority.as_str().to_owned(), self.stream.clone());
        let (tx, now) = begin(&mut *self.conn, &*self.clock)?;
        let mut t = read_turn(&tx, turn.as_str())?;
        let (instance, generation) = sidecar(
            &tx,
            "managed_turn_launches",
            "managed_turn_id",
            turn.as_str(),
        )?
        .ok_or(E::InvalidTransition)?;
        let legal = matches!(
            (t.state, state),
            (
                TurnState::Starting,
                TurnState::Running
                    | TurnState::Failed
                    | TurnState::Interrupted
                    | TurnState::Indeterminate
            ) | (
                TurnState::Running,
                TurnState::Completed
                    | TurnState::Failed
                    | TurnState::Interrupted
                    | TurnState::Indeterminate
            )
        );
        if !legal {
            return Err(E::InvalidTransition);
        }
        if matches!(state, TurnState::Running | TurnState::Completed)
            && !owner_live(
                &tx,
                t.managed_session_id.as_str(),
                &instance,
                generation,
                &authority,
            )?
        {
            return Err(E::GateDenied);
        }
        attest(&tx, &authority, &stream, &stamp(now)?)?;
        let at = stamp(now.max(ts(&enum_text(&t.updated_at)?)?))?;
        if let Some(p) = provider_turn_id {
            if t.codex_turn_id
                .as_ref()
                .is_some_and(|old| old.as_str() != p)
            {
                return Err(E::CorrelationError);
            }
            t.codex_turn_id = Some(crate::adapters::codex::CodexTurnId::parse(p).map_err(invalid)?);
        }
        t.state = state;
        t.revision = t
            .revision
            .checked_add(1)
            .filter(|r| *r <= i64::MAX as u64)
            .ok_or(E::CapacityExceeded)?;
        t.updated_at = at.clone().try_into().map_err(invalid)?;
        if state.terminal() {
            t.terminal_at = Some(at.clone().try_into().map_err(invalid)?);
            t.terminal_reason = reason;
        }
        t.validate()?;
        tx.execute(
            "UPDATE managed_turns SET codex_turn_id=?2,state=?3,revision=?4,updated_at=?5,terminal_at=?6,terminal_reason=?7 WHERE managed_turn_id=?1",
            params![turn.as_str(), t.codex_turn_id.as_ref().map(|c| c.as_str().to_owned()), enum_text(&state)?,
                t.revision as i64, at, t.terminal_at.as_ref().map(enum_text).transpose()?,
                t.terminal_reason.map(|r| enum_text(&r)).transpose()?],
        )
        .map_err(db)?;
        step(&mut self.fault, 0)?;
        turn_event(
            &tx,
            (&authority, &stream),
            &t,
            generation,
            None,
            &enum_text(&state)?,
        )?;
        step(&mut self.fault, 1)?;
        // A finished turn leaves no approval able to wait on it.
        if state.terminal() {
            settle_in(
                &tx,
                (&authority, &stream),
                Some(turn.as_str()),
                ApprovalReason::TurnEnded,
                now,
            )?;
        }
        step(&mut self.fault, 2)?;
        tx.commit().map_err(db)?;
        Ok(t)
    }

    /// A client cancel. A turn which already ended returns its terminal record with no
    /// dispatch (it need not find its launch alive). An active turn needs the live owner and the
    /// current revision; it records the request and the caller (never this writer) interrupts the
    /// provider. `true` means the caller should dispatch an interrupt exactly once.
    pub fn cancel_turn(
        &mut self,
        p: &crate::contracts::turn::TurnCancel,
    ) -> ControlResult<(ManagedTurnRecord, bool)> {
        let mutation = serde_json::to_string(&Operation::TurnCancel(p.clone())).map_err(invalid)?;
        let (authority, stream) = (self.authority.as_str().to_owned(), self.stream.clone());
        let (tx, now) = begin(&mut *self.conn, &*self.clock)?;
        if let Some((m, r)) = receipt(&tx, p.intent_id.as_str())? {
            if m != mutation {
                return Err(E::IntentConflict);
            }
            return Ok((serde_json::from_str(&r).map_err(invalid)?, false));
        }
        let t = read_turn(&tx, p.managed_turn_id.as_str())?;
        let (instance, generation) = sidecar(
            &tx,
            "managed_turn_launches",
            "managed_turn_id",
            p.managed_turn_id.as_str(),
        )?
        .ok_or(E::InvalidTransition)?;
        let dispatch = if t.state.terminal() {
            false
        } else {
            if t.revision != p.expected_revision {
                return Err(E::StaleRevision);
            }
            if !owner_live(
                &tx,
                t.managed_session_id.as_str(),
                &instance,
                generation,
                &authority,
            )? {
                return Err(E::GateDenied);
            }
            attest(&tx, &authority, &stream, &stamp(now)?)?;
            turn_event(
                &tx,
                (&authority, &stream),
                &t,
                generation,
                Some(p.intent_id.as_str()),
                "cancel_requested",
            )?;
            true
        };
        tx.execute(
            "INSERT INTO processed_intents VALUES(?1,?2,?3)",
            params![
                p.intent_id.as_str(),
                mutation,
                serde_json::to_string(&t).map_err(invalid)?
            ],
        )
        .map_err(db)?;
        step(&mut self.fault, 0)?;
        tx.commit().map_err(db)?;
        Ok((t, dispatch))
    }

    // -------------------------------------------------------------- approvals

    /// A provider approval request, admitted under the turn's ORIGINAL lease on the live launch.
    pub fn open_approval(
        &mut self,
        turn: &ManagedTurnId,
        kind: ApprovalKind,
        provider_item_id: &str,
        action_fingerprint: &str,
        structure: ApprovalStructure,
    ) -> ControlResult<ManagedApprovalRecord> {
        if !crate::contracts::approval::valid_provider_item(provider_item_id)
            || !valid_fingerprint(action_fingerprint)
            || structure.validate(kind).is_err()
        {
            return Err(E::InvalidRequest);
        }
        let (authority, stream) = (self.authority.as_str().to_owned(), self.stream.clone());
        let (tx, now) = begin(&mut *self.conn, &*self.clock)?;
        let t = read_turn(&tx, turn.as_str())?;
        let (instance, generation) = sidecar(
            &tx,
            "managed_turn_launches",
            "managed_turn_id",
            turn.as_str(),
        )?
        .ok_or(E::InvalidTransition)?;
        if t.state != TurnState::Running {
            return Err(E::InvalidTransition);
        }
        if !owner_live(
            &tx,
            t.managed_session_id.as_str(),
            &instance,
            generation,
            &authority,
        )? {
            return Err(E::GateDenied);
        }
        let session = t.managed_session_id.as_str().to_owned();
        let (pending, repeated): (bool, bool) = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM managed_approvals WHERE managed_session_id=?1 AND state='pending'),EXISTS(SELECT 1 FROM managed_approvals WHERE managed_turn_id=?2 AND provider_item_id=?3)",
                params![session, turn.as_str(), provider_item_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db)?;
        if repeated {
            return Err(E::CorrelationError);
        }
        if pending {
            return Err(E::CapacityExceeded);
        }
        let (lease_state, lease_session, lease_expires, scopes): (String, String, String, String) =
            tx.query_row(
                "SELECT state,session_id,expires_at,scopes FROM execution_leases WHERE lease_id=?1",
                [t.admitted_lease_id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .map_err(|_| E::LeaseNotFound)?;
        let scope = match kind {
            ApprovalKind::CommandExecution => "command_execute",
            ApprovalKind::FileChange => "file_change",
        };
        let scopes: Vec<String> = serde_json::from_str(&scopes).map_err(invalid)?;
        let deadline = (now + Duration::seconds(APPROVAL_TTL_SECONDS)).min(ts(&lease_expires)?);
        if lease_state != "active"
            || lease_session != session
            || deadline <= now
            || !scopes.iter().any(|s| s == scope)
        {
            return Err(E::GateDenied);
        }
        attest(&tx, &authority, &stream, &stamp(now)?)?;
        let sequence = next_sequence(&tx)?;
        let id = crate::control::store::uuid().as_str().to_owned();
        let at = stamp(now)?;
        let a: ManagedApprovalRecord = serde_json::from_value(json!({
            "managed_approval_id":id,"managed_turn_id":turn.as_str(),"managed_session_id":session,
            "runtime_instance_id":instance,"kind":kind,"provider_item_id":provider_item_id,
            "admitted_lease_id":t.admitted_lease_id,"admitted_lease_revision":t.admitted_lease_revision,
            "state":"pending","revision":1,"action_fingerprint":action_fingerprint,"structure":structure,
            "decision":null,"effect":"none","created_at":at,"updated_at":at,"expires_at":stamp(deadline)?,
            "terminal_at":null,"terminal_reason":null,"created_sequence":sequence.to_string()}))
        .map_err(invalid)?;
        a.validate().map_err(invalid)?;
        tx.execute(
            "INSERT INTO managed_approval_launches VALUES(?1,?2,?3,?4,?5)",
            params![id, turn.as_str(), session, instance, generation as i64],
        )
        .map_err(db)?;
        insert_approval(&tx, &a, sequence)?;
        step(&mut self.fault, 0)?;
        let written = approval_event(
            &tx,
            (&authority, &stream),
            &a,
            generation,
            None,
            "requested",
        )?;
        if written != sequence {
            return Err(E::InternalError);
        }
        step(&mut self.fault, 1)?;
        tx.commit().map_err(db)?;
        Ok(a)
    }

    /// One client decision. Accept requires the exact pending action, the original active
    /// lease with the action's scope, a running turn and the live launch; every decision needs the
    /// live launch. The caller may answer the provider only on `Fresh`.
    pub fn decide_approval(
        &mut self,
        approval: &ManagedApprovalId,
        intent_id: &WireUuid,
        expected_revision: u64,
        decision: ApprovalDecision,
        current_fingerprint: &str,
    ) -> ControlResult<Decided> {
        let mutation = json!({"method":METHOD_RESPOND,"params":{"intent_id":intent_id,
            "managed_approval_id":approval,"expected_revision":expected_revision,"decision":decision}})
        .to_string();
        let (authority, stream) = (self.authority.as_str().to_owned(), self.stream.clone());
        let (tx, now) = begin(&mut *self.conn, &*self.clock)?;
        if let Some((m, r)) = receipt(&tx, intent_id.as_str())? {
            if m != mutation {
                return Err(E::IntentConflict);
            }
            return Ok(Decided::Replayed(
                serde_json::from_str(&r).map_err(invalid)?,
            ));
        }
        let mut a = read_approval(&tx, approval.as_str())?;
        let (instance, generation) = sidecar(
            &tx,
            "managed_approval_launches",
            "managed_approval_id",
            approval.as_str(),
        )?
        .ok_or(E::InvalidTransition)?;
        if a.state != ApprovalState::Pending {
            return Err(E::InvalidTransition);
        }
        if a.revision != expected_revision {
            return Err(E::StaleRevision);
        }
        if a.action_fingerprint != current_fingerprint {
            return Err(E::CorrelationError);
        }
        if now >= ts(&enum_text(&a.expires_at)?)?
            || !owner_live(
                &tx,
                a.managed_session_id.as_str(),
                &instance,
                generation,
                &authority,
            )?
        {
            return Err(E::GateDenied);
        }
        if decision == ApprovalDecision::Accept {
            let t = read_turn(&tx, a.managed_turn_id.as_str())?;
            let (lease_state, lease_session, lease_expires, scopes): (String, String, String, String) = tx
                .query_row(
                    "SELECT state,session_id,expires_at,scopes FROM execution_leases WHERE lease_id=?1",
                    [a.admitted_lease_id.as_str()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .map_err(|_| E::LeaseNotFound)?;
            let scope = match a.kind {
                ApprovalKind::CommandExecution => "command_execute",
                ApprovalKind::FileChange => "file_change",
            };
            let scopes: Vec<String> = serde_json::from_str(&scopes).map_err(invalid)?;
            if t.state != TurnState::Running
                || t.admitted_lease_id != a.admitted_lease_id
                || t.admitted_lease_revision != a.admitted_lease_revision
                || lease_state != "active"
                || lease_session != a.managed_session_id.as_str()
                || now >= ts(&lease_expires)?
                || !scopes.iter().any(|s| s == scope)
            {
                return Err(E::GateDenied);
            }
        }
        attest(&tx, &authority, &stream, &stamp(now)?)?;
        a.state = decision.state();
        a.decision = Some(decision);
        a.effect = if decision == ApprovalDecision::Accept {
            ApprovalEffect::Dispatched
        } else {
            ApprovalEffect::None
        };
        a.terminal_reason = Some(ApprovalReason::ClientDecision);
        bump(&mut a, now)?;
        let r = ApprovalReceipt {
            managed_approval_id: a.managed_approval_id.clone(),
            managed_turn_id: a.managed_turn_id.clone(),
            kind: a.kind,
            decision,
            state: a.state,
            revision: a.revision,
            action_fingerprint: a.action_fingerprint.clone(),
            decided_at: a.updated_at.clone(),
        };
        r.validate()?;
        update_approval(&tx, &a)?;
        step(&mut self.fault, 0)?;
        approval_event(
            &tx,
            (&authority, &stream),
            &a,
            generation,
            Some(intent_id.as_str()),
            &enum_text(&a.state)?,
        )?;
        step(&mut self.fault, 1)?;
        tx.execute(
            "INSERT INTO processed_intents VALUES(?1,?2,?3)",
            params![
                intent_id.as_str(),
                mutation,
                serde_json::to_string(&r).map_err(invalid)?
            ],
        )
        .map_err(db)?;
        step(&mut self.fault, 2)?;
        tx.commit().map_err(db)?;
        Ok(Decided::Fresh(Box::new(a), r))
    }

    /// The provider resolved the accepted request: the dispatched effect is confirmed.
    pub fn confirm_approval(
        &mut self,
        approval: &ManagedApprovalId,
    ) -> ControlResult<ManagedApprovalRecord> {
        let (authority, stream) = (self.authority.as_str().to_owned(), self.stream.clone());
        let (tx, now) = begin(&mut *self.conn, &*self.clock)?;
        let mut a = read_approval(&tx, approval.as_str())?;
        let (instance, generation) = sidecar(
            &tx,
            "managed_approval_launches",
            "managed_approval_id",
            approval.as_str(),
        )?
        .ok_or(E::InvalidTransition)?;
        if a.state != ApprovalState::Accepted {
            return Err(E::InvalidTransition);
        }
        if a.effect == ApprovalEffect::Confirmed {
            return Ok(a);
        }
        if !owner_live(
            &tx,
            a.managed_session_id.as_str(),
            &instance,
            generation,
            &authority,
        )? {
            return Err(E::GateDenied);
        }
        attest(&tx, &authority, &stream, &stamp(now)?)?;
        a.effect = ApprovalEffect::Confirmed;
        bump(&mut a, now)?;
        update_approval(&tx, &a)?;
        approval_event(
            &tx,
            (&authority, &stream),
            &a,
            generation,
            None,
            "confirmed",
        )?;
        tx.commit().map_err(db)?;
        Ok(a)
    }

    /// Authority-side terminalisation without a client decision. Undecided approvals expire
    /// (deadline reasons) or are invalidated; accepted ones whose outcome was never confirmed
    /// become indeterminate. Final approvals are never touched and nothing is replayed.
    pub fn settle_approvals(
        &mut self,
        turn: Option<&ManagedTurnId>,
        reason: ApprovalReason,
    ) -> ControlResult<usize> {
        if reason == ApprovalReason::ClientDecision {
            return Err(E::InvalidRequest);
        }
        let (authority, stream) = (self.authority.as_str().to_owned(), self.stream.clone());
        let (tx, now) = begin(&mut *self.conn, &*self.clock)?;
        attest(&tx, &authority, &stream, &stamp(now)?)?;
        let n = settle_in(
            &tx,
            (&authority, &stream),
            turn.map(ManagedTurnId::as_str),
            reason,
            now,
        )?;
        tx.commit().map_err(db)?;
        Ok(n)
    }

    // ----------------------------------------------------------------- leases

    /// Durable lease grant, renewal and revocation (session grants; never launch capabilities).
    /// An identical intent returns the original lease with no side effect. Losing a lease
    /// (revoked or expired) interrupts the turns admitted under it and settles their approvals in
    /// the same transaction, so no work outlives the grant that admitted it.
    pub fn lease(&mut self, op: &Operation) -> ControlResult<ExecutionLease> {
        let intent = match op {
            Operation::LeaseGrant(p) => &p.intent_id,
            Operation::LeaseRenew(p) => &p.intent_id,
            Operation::LeaseRevoke(p) => &p.intent_id,
            _ => return Err(E::InvalidRequest),
        };
        op.validate()?;
        let mutation = serde_json::to_string(op).map_err(invalid)?;
        let (authority, stream) = (self.authority.as_str().to_owned(), self.stream.clone());
        let (tx, now) = begin(&mut *self.conn, &*self.clock)?;
        if let Some((m, r)) = receipt(&tx, intent.as_str())? {
            if m != mutation {
                return Err(E::IntentConflict);
            }
            return serde_json::from_str(&r).map_err(invalid);
        }
        attest(&tx, &authority, &stream, &stamp(now)?)?;
        let ids = (authority.as_str(), stream.as_str());
        let (lease, kind) = match op {
            Operation::LeaseGrant(p) => {
                let (state, revision): (String, i64) = tx
                    .query_row(
                        "SELECT control_state,revision FROM managed_sessions WHERE session_id=?1",
                        [p.session_id.as_str()],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .map_err(|_| E::SessionNotFound)?;
                if state != "prepared" {
                    return Err(E::InvalidTransition);
                }
                if revision as u64 != p.expected_session_revision {
                    return Err(E::StaleRevision);
                }
                if let Some(active) = active_lease(&tx, p.session_id.as_str())? {
                    if now >= lease_ts(&active.expires_at)? {
                        expire_in(&tx, ids, active, now)?;
                    } else {
                        return Err(E::LeaseAlreadyActive);
                    }
                }
                let mut scopes = p.scopes.clone();
                scopes.sort();
                let mut id = crate::control::store::uuid().as_str().to_owned();
                while id == p.session_id.as_str() {
                    id = crate::control::store::uuid().as_str().to_owned();
                }
                let at = stamp(now)?;
                let lease: ExecutionLease = serde_json::from_value(json!({
                    "lease_id":id,"session_id":p.session_id,"state":"active","scopes":scopes,"revision":1,
                    "issued_at":at,"expires_at":stamp(now + Duration::seconds(i64::from(p.ttl_seconds)))?,
                    "updated_at":at,"revocation_reason":null}))
                .map_err(invalid)?;
                (lease, "authority.execution_lease.granted")
            }
            Operation::LeaseRenew(p) => {
                let mut l = read_lease(&tx, p.lease_id.as_str())?;
                validate_ttl(p.ttl_seconds)?;
                if l.state != LeaseState::Active || now >= lease_ts(&l.expires_at)? {
                    return Err(E::InvalidTransition);
                }
                let open: String = tx
                    .query_row(
                        "SELECT control_state FROM managed_sessions WHERE session_id=?1",
                        [l.session_id.as_str()],
                        |r| r.get(0),
                    )
                    .map_err(db)?;
                if open != "prepared" {
                    return Err(E::InvalidTransition);
                }
                if l.revision != p.expected_revision {
                    return Err(E::StaleRevision);
                }
                l.revision += 1;
                l.updated_at = stamp(now)?.try_into().map_err(invalid)?;
                l.expires_at = stamp(now + Duration::seconds(i64::from(p.ttl_seconds)))?
                    .try_into()
                    .map_err(invalid)?;
                (l, "authority.execution_lease.renewed")
            }
            Operation::LeaseRevoke(p) => {
                let mut l = read_lease(&tx, p.lease_id.as_str())?;
                if l.state != LeaseState::Active || now >= lease_ts(&l.expires_at)? {
                    return Err(E::InvalidTransition);
                }
                if l.revision != p.expected_revision {
                    return Err(E::StaleRevision);
                }
                l.revision += 1;
                l.state = LeaseState::Revoked;
                l.revocation_reason = Some(RevocationReason::UserRequest);
                l.updated_at = stamp(now)?.try_into().map_err(invalid)?;
                (l, "authority.execution_lease.revoked")
            }
            _ => return Err(E::InvalidRequest),
        };
        lease.validate()?;
        write_lease(&tx, &lease)?;
        lease_event(&tx, ids, &lease, Some(intent.as_str()), kind)?;
        step(&mut self.fault, 0)?;
        if lease.state == LeaseState::Revoked {
            lose_lease_work(
                &tx,
                ids,
                &lease,
                TurnReason::LeaseRevoked,
                ApprovalReason::LeaseRevoked,
                now,
            )?;
        }
        tx.execute(
            "INSERT INTO processed_intents VALUES(?1,?2,?3)",
            params![
                intent.as_str(),
                mutation,
                serde_json::to_string(&lease).map_err(invalid)?
            ],
        )
        .map_err(db)?;
        step(&mut self.fault, 1)?;
        tx.commit().map_err(db)?;
        Ok(lease)
    }

    /// Expires every active lease past its deadline, interrupting the work it admitted.
    pub fn expire_leases(&mut self) -> ControlResult<usize> {
        let (authority, stream) = (self.authority.as_str().to_owned(), self.stream.clone());
        let (tx, now) = begin(&mut *self.conn, &*self.clock)?;
        let due: Vec<String> = {
            let mut q = tx
                .prepare(
                    "SELECT lease_id FROM execution_leases WHERE state='active' ORDER BY lease_id",
                )
                .map_err(db)?;
            q.query_map([], |r| r.get::<_, String>(0))
                .map_err(db)?
                .collect::<Result<_, _>>()
                .map_err(db)?
        };
        let mut n = 0;
        for id in due {
            let l = read_lease(&tx, &id)?;
            if now >= lease_ts(&l.expires_at)? {
                if n == 0 {
                    attest(&tx, &authority, &stream, &stamp(now)?)?;
                }
                expire_in(&tx, (&authority, &stream), l, now)?;
                n += 1;
            }
        }
        tx.commit().map_err(db)?;
        Ok(n)
    }

    /// Settles every pending approval whose deadline passed. The binding bound names the reason:
    /// the lease's own expiry or the approval TTL. Returns how many were settled.
    pub fn expire_approvals(&mut self) -> ControlResult<usize> {
        let (authority, stream) = (self.authority.as_str().to_owned(), self.stream.clone());
        let (tx, now) = begin(&mut *self.conn, &*self.clock)?;
        let due: Vec<(String, String, String, String)> = {
            let mut q = tx
                .prepare("SELECT a.managed_approval_id,a.managed_turn_id,a.expires_at,l.expires_at FROM managed_approvals a JOIN execution_leases l ON l.lease_id=a.admitted_lease_id WHERE a.state='pending' ORDER BY a.created_sequence")
                .map_err(db)?;
            q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .map_err(db)?
                .collect::<Result<_, _>>()
                .map_err(db)?
        };
        let mut n = 0;
        for (_, turn, expires, lease_expires) in due {
            if now < ts(&expires)? {
                continue;
            }
            let reason = if ts(&lease_expires)? <= ts(&expires)? {
                ApprovalReason::LeaseExpired
            } else {
                ApprovalReason::ApprovalTimeout
            };
            if n == 0 {
                attest(&tx, &authority, &stream, &stamp(now)?)?;
            }
            n += settle_in(&tx, (&authority, &stream), Some(&turn), reason, now)?;
        }
        tx.commit().map_err(db)?;
        Ok(n)
    }

    // --------------------------------------------------------------- recovery

    /// Startup recovery by an authority instance. Everything stranded on a launch this instance
    /// does not own and cannot progress (the predecessor's, an exited one, a superseded one) is
    /// terminalised as lost: approvals invalidated or indeterminate, turns indeterminate. No
    /// provider message is replayed, no exit is invented, no lease or grant is carried over.
    /// Returns (approvals, turns) recorded.
    pub fn recover(&mut self) -> ControlResult<(usize, usize)> {
        let (authority, stream) = (self.authority.as_str().to_owned(), self.stream.clone());
        let (tx, now) = begin(&mut *self.conn, &*self.clock)?;
        let mut replay = AuditReplay::with_boundary(super::migration::verify_boundary(&tx)?);
        {
            let mut q = tx
                .prepare("SELECT envelope FROM control_events ORDER BY sequence")
                .map_err(db)?;
            let mut rows = q.query([]).map_err(db)?;
            while let Some(r) = rows.next().map_err(db)? {
                let raw: String = r.get(0).map_err(db)?;
                replay.observe(&unique_json(raw.as_bytes()).map_err(invalid)?)?;
            }
        }
        let stranded = replay.unresolved_for(&authority);
        if stranded.turns.is_empty() && stranded.approvals.is_empty() {
            return Ok((0, 0));
        }
        attest(&tx, &authority, &stream, &stamp(now)?)?;
        let mut approvals = 0;
        for id in &stranded.approvals {
            let mut a = read_approval(&tx, id)?;
            let generation = sidecar(&tx, "managed_approval_launches", "managed_approval_id", id)?
                .ok_or(E::StorageIncompatible)?
                .1;
            a.state = if a.state == ApprovalState::Pending {
                ApprovalState::Invalidated
            } else {
                ApprovalState::Indeterminate
            };
            if a.state == ApprovalState::Indeterminate {
                a.effect = ApprovalEffect::Unknown;
            }
            a.terminal_reason = Some(ApprovalReason::AuthorityRestarted);
            bump(&mut a, now)?;
            update_approval(&tx, &a)?;
            approval_event(
                &tx,
                (&authority, &stream),
                &a,
                generation,
                None,
                &enum_text(&a.state)?,
            )?;
            approvals += 1;
        }
        let mut turns = 0;
        for id in &stranded.turns {
            let mut t = read_turn(&tx, id)?;
            let generation = sidecar(&tx, "managed_turn_launches", "managed_turn_id", id)?
                .ok_or(E::StorageIncompatible)?
                .1;
            let at = stamp(now.max(ts(&enum_text(&t.updated_at)?)?))?;
            t.state = TurnState::Indeterminate;
            t.revision += 1;
            t.updated_at = at.clone().try_into().map_err(invalid)?;
            t.terminal_at = Some(at.clone().try_into().map_err(invalid)?);
            t.terminal_reason = Some(TurnReason::AuthorityRestarted);
            t.validate()?;
            tx.execute(
                "UPDATE managed_turns SET state='indeterminate',revision=?2,updated_at=?3,terminal_at=?3,terminal_reason='authority_restarted' WHERE managed_turn_id=?1",
                params![id, t.revision as i64, at],
            )
            .map_err(db)?;
            turn_event(
                &tx,
                (&authority, &stream),
                &t,
                generation,
                None,
                "indeterminate",
            )?;
            turns += 1;
        }
        step(&mut self.fault, 0)?;
        tx.commit().map_err(db)?;
        Ok((approvals, turns))
    }
}

// ----------------------------------------------------------------- helpers

fn begin<'a>(
    conn: &'a mut Connection,
    clock: &dyn Fn() -> OffsetDateTime,
) -> ControlResult<(Transaction<'a>, OffsetDateTime)> {
    let now = clock();
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    Ok((tx, now))
}
fn receipt(tx: &Transaction<'_>, intent: &str) -> ControlResult<Option<(String, String)>> {
    tx.query_row(
        "SELECT mutation,result FROM processed_intents WHERE intent_id=?1",
        [intent],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(db)
}
fn next_sequence(tx: &Transaction<'_>) -> ControlResult<u64> {
    let head: String = tx
        .query_row(
            "SELECT last_sequence FROM authority_metadata WHERE id=1",
            [],
            |r| r.get(0),
        )
        .map_err(db)?;
    head.parse::<u64>()
        .map_err(invalid)?
        .checked_add(1)
        .ok_or(E::CapacityExceeded)
}
fn sidecar(
    tx: &Transaction<'_>,
    table: &str,
    key: &str,
    id: &str,
) -> ControlResult<Option<(String, u64)>> {
    let row: Option<(String, i64)> = tx
        .query_row(
            &format!("SELECT runtime_instance_id,generation FROM {table} WHERE {key}=?1"),
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db)?;
    row.map(|(i, g)| Ok((i, u64::try_from(g).map_err(invalid)?)))
        .transpose()
}

/// The launch is the session's current, attested, unexited, running generation which this very
/// authority reserved (and the session is open).
fn owner_live(
    tx: &Transaction<'_>,
    session: &str,
    instance: &str,
    generation: u64,
    authority: &str,
) -> ControlResult<bool> {
    let ok: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM managed_runtime_launches l JOIN managed_runtimes r ON r.managed_session_id=l.managed_session_id JOIN managed_sessions s ON s.session_id=l.managed_session_id WHERE l.managed_session_id=?1 AND l.runtime_instance_id=?2 AND l.generation=?3 AND l.authority_instance_id=?4 AND r.generation=l.generation AND r.desired_state='running' AND s.control_state='prepared')",
            params![session, instance, generation as i64, authority],
            |r| r.get(0),
        )
        .map_err(db)?;
    Ok(ok
        && events_for(tx, "authority.runtime.instance_ready", instance)? == 1
        && events_for(tx, "authority.runtime.instance_exited", instance)? == 0)
}
fn require_live(tx: &Transaction<'_>, launch: &RuntimeRef, authority: &str) -> ControlResult<()> {
    if launch.authority_instance_id.as_str() != authority
        || !owner_live(
            tx,
            launch.managed_session_id.as_str(),
            launch.runtime_instance_id.as_str(),
            launch.generation,
            authority,
        )?
    {
        return Err(E::StaleRevision);
    }
    Ok(())
}

fn read_turn(tx: &Transaction<'_>, id: &str) -> ControlResult<ManagedTurnRecord> {
    let raw: Option<String> = tx
        .query_row(
            "SELECT json_object('managed_turn_id',managed_turn_id,'managed_session_id',managed_session_id,'codex_thread_id',codex_thread_id,'codex_turn_id',codex_turn_id,'admitted_lease_id',admitted_lease_id,'admitted_lease_revision',admitted_lease_revision,'runtime_instance_id',runtime_instance_id,'state',state,'revision',revision,'created_at',created_at,'updated_at',updated_at,'terminal_at',terminal_at,'terminal_reason',terminal_reason) FROM managed_turns WHERE managed_turn_id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()
        .map_err(db)?;
    let t: ManagedTurnRecord =
        serde_json::from_str(&raw.ok_or(E::TurnNotFound)?).map_err(invalid)?;
    t.validate()?;
    Ok(t)
}
fn read_approval(tx: &Transaction<'_>, id: &str) -> ControlResult<ManagedApprovalRecord> {
    let raw: Option<String> = tx
        .query_row(
            "SELECT json_object('managed_approval_id',managed_approval_id,'managed_turn_id',managed_turn_id,'managed_session_id',managed_session_id,'runtime_instance_id',runtime_instance_id,'kind',kind,'provider_item_id',provider_item_id,'admitted_lease_id',admitted_lease_id,'admitted_lease_revision',admitted_lease_revision,'state',state,'revision',revision,'action_fingerprint',action_fingerprint,'structure',json(structure),'decision',decision,'effect',effect,'created_at',created_at,'updated_at',updated_at,'expires_at',expires_at,'terminal_at',terminal_at,'terminal_reason',terminal_reason,'created_sequence',ltrim(created_sequence,'0')) FROM managed_approvals WHERE managed_approval_id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()
        .map_err(db)?;
    let a: ManagedApprovalRecord =
        serde_json::from_str(&raw.ok_or(E::ApprovalNotFound)?).map_err(invalid)?;
    a.validate().map_err(invalid)?;
    Ok(a)
}
fn insert_approval(
    tx: &Transaction<'_>,
    a: &ManagedApprovalRecord,
    sequence: u64,
) -> ControlResult<()> {
    tx.execute(
        "INSERT INTO managed_approvals VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
        params![
            a.managed_approval_id.as_str(), a.managed_turn_id.as_str(), a.managed_session_id.as_str(),
            a.runtime_instance_id.as_str(), enum_text(&a.kind)?, a.provider_item_id, a.admitted_lease_id.as_str(),
            a.admitted_lease_revision as i64, enum_text(&a.state)?, a.revision as i64, a.action_fingerprint,
            serde_json::to_string(&a.structure).map_err(invalid)?, a.decision.map(|d| enum_text(&d)).transpose()?,
            enum_text(&a.effect)?, enum_text(&a.created_at)?, enum_text(&a.updated_at)?, enum_text(&a.expires_at)?,
            a.terminal_at.as_ref().map(enum_text).transpose()?, a.terminal_reason.map(|r| enum_text(&r)).transpose()?,
            format!("{sequence:020}"),
        ],
    )
    .map_err(db)?;
    Ok(())
}
fn update_approval(tx: &Transaction<'_>, a: &ManagedApprovalRecord) -> ControlResult<()> {
    a.validate().map_err(invalid)?;
    let changed = tx
        .execute(
            "UPDATE managed_approvals SET state=?2,revision=?3,decision=?4,effect=?5,updated_at=?6,terminal_at=?7,terminal_reason=?8 WHERE managed_approval_id=?1 AND revision=?9",
            params![
                a.managed_approval_id.as_str(), enum_text(&a.state)?, a.revision as i64,
                a.decision.map(|d| enum_text(&d)).transpose()?, enum_text(&a.effect)?, enum_text(&a.updated_at)?,
                a.terminal_at.as_ref().map(enum_text).transpose()?, a.terminal_reason.map(|r| enum_text(&r)).transpose()?,
                (a.revision - 1) as i64,
            ],
        )
        .map_err(|_| E::InvalidTransition)?;
    if changed == 1 {
        Ok(())
    } else {
        Err(E::StaleRevision)
    }
}
fn bump(a: &mut ManagedApprovalRecord, at: OffsetDateTime) -> ControlResult<()> {
    a.revision = a
        .revision
        .checked_add(1)
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or(E::CapacityExceeded)?;
    // The journal clock never runs backwards within one approval.
    a.updated_at = stamp(at.max(ts(&enum_text(&a.updated_at)?)?))?
        .try_into()
        .map_err(invalid)?;
    if a.terminal_at.is_none() && a.state.terminal() {
        a.terminal_at = Some(a.updated_at.clone());
    }
    a.validate().map_err(invalid)
}

fn turn_event(
    tx: &Transaction<'_>,
    (authority, stream): (&str, &str),
    t: &ManagedTurnRecord,
    generation: u64,
    correlation: Option<&str>,
    name: &str,
) -> ControlResult<u64> {
    append(
        tx,
        (authority, stream, &enum_text(&t.updated_at)?),
        Draft {
            session: Some(t.managed_session_id.as_str()),
            kind: &format!("authority.managed_turn.{name}"),
            correlation,
            payload: json!({"managed_turn_id":t.managed_turn_id,"codex_turn_id":t.codex_turn_id,
                "lease_id":t.admitted_lease_id,"lease_revision":t.admitted_lease_revision,
                "runtime_instance_id":t.runtime_instance_id,"state":t.state,"revision":t.revision,
                "reason":t.terminal_reason,"generation":generation}),
            version: 2,
        },
    )
}
fn approval_event(
    tx: &Transaction<'_>,
    (authority, stream): (&str, &str),
    a: &ManagedApprovalRecord,
    generation: u64,
    correlation: Option<&str>,
    name: &str,
) -> ControlResult<u64> {
    append(
        tx,
        (authority, stream, &enum_text(&a.updated_at)?),
        Draft {
            session: Some(a.managed_session_id.as_str()),
            kind: &format!("authority.managed_approval.{name}"),
            correlation,
            payload: json!({"managed_approval_id":a.managed_approval_id,"managed_turn_id":a.managed_turn_id,
                "kind":a.kind,"lease_id":a.admitted_lease_id,"lease_revision":a.admitted_lease_revision,
                "runtime_instance_id":a.runtime_instance_id,"state":a.state,"revision":a.revision,
                "decision":a.decision,"effect":a.effect,"reason":a.terminal_reason,
                "action_fingerprint":a.action_fingerprint,"generation":generation}),
            version: 2,
        },
    )
}

fn lease_ts(t: &crate::contracts::UtcTimestamp) -> ControlResult<OffsetDateTime> {
    ts(&enum_text(t)?)
}
pub(super) type LeaseRow = (
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
pub(super) fn read_lease(tx: &Connection, id: &str) -> ControlResult<ExecutionLease> {
    let row: Option<LeaseRow> = tx
        .query_row(
            "SELECT lease_id,session_id,state,scopes,revision,issued_at,expires_at,updated_at,revocation_reason FROM execution_leases WHERE lease_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)),
        )
        .optional()
        .map_err(db)?;
    let (lease_id, session_id, state, scopes, revision, issued_at, expires_at, updated_at, reason) =
        row.ok_or(E::LeaseNotFound)?;
    let scopes: Value = unique_json(scopes.as_bytes()).map_err(invalid)?;
    let l: ExecutionLease = serde_json::from_value(json!({"lease_id":lease_id,"session_id":session_id,
        "state":state,"scopes":scopes,"revision":revision,"issued_at":issued_at,"expires_at":expires_at,
        "updated_at":updated_at,"revocation_reason":reason}))
    .map_err(invalid)?;
    l.validate()?;
    validate_scopes(&l.scopes)?;
    Ok(l)
}
pub(super) fn active_lease(
    tx: &Connection,
    session: &str,
) -> ControlResult<Option<ExecutionLease>> {
    let id: Option<String> = tx
        .query_row(
            "SELECT lease_id FROM execution_leases WHERE session_id=?1 AND state='active'",
            [session],
            |r| r.get(0),
        )
        .optional()
        .map_err(db)?;
    id.map(|id| read_lease(tx, &id)).transpose()
}
fn write_lease(tx: &Transaction<'_>, l: &ExecutionLease) -> ControlResult<()> {
    tx.execute(
        "INSERT INTO execution_leases VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(lease_id) DO UPDATE SET state=excluded.state,revision=excluded.revision,expires_at=excluded.expires_at,updated_at=excluded.updated_at,revocation_reason=excluded.revocation_reason",
        params![l.lease_id.as_str(), l.session_id.as_str(), enum_text(&l.state)?,
            serde_json::to_string(&l.scopes).map_err(invalid)?, l.revision as i64, enum_text(&l.issued_at)?,
            enum_text(&l.expires_at)?, enum_text(&l.updated_at)?, l.revocation_reason.map(|r| enum_text(&r)).transpose()?],
    )
    .map_err(db)?;
    Ok(())
}
/// Lease events keep their baseline payload shape and version.
fn lease_event(
    tx: &Transaction<'_>,
    (authority, stream): (&str, &str),
    l: &ExecutionLease,
    correlation: Option<&str>,
    kind: &str,
) -> ControlResult<u64> {
    let mut payload = json!({"lease_id":l.lease_id,"scopes":l.scopes,"lease_state":l.state,
        "revision":l.revision,"expires_at":l.expires_at});
    if let Some(reason) = l.revocation_reason {
        payload["revocation_reason"] = json!(reason);
    }
    append(
        tx,
        (authority, stream, &enum_text(&l.updated_at)?),
        Draft {
            session: Some(l.session_id.as_str()),
            kind,
            correlation,
            payload,
            version: 1,
        },
    )
}
fn expire_in(
    tx: &Transaction<'_>,
    ids: (&str, &str),
    mut l: ExecutionLease,
    now: OffsetDateTime,
) -> ControlResult<()> {
    l.state = LeaseState::Expired;
    l.revision += 1;
    l.updated_at = stamp(now)?.try_into().map_err(invalid)?;
    write_lease(tx, &l)?;
    lease_event(tx, ids, &l, None, "authority.execution_lease.expired")?;
    lose_lease_work(
        tx,
        ids,
        &l,
        TurnReason::LeaseExpired,
        ApprovalReason::LeaseExpired,
        now,
    )
}
/// Work admitted under a lost lease ends with it: approvals first, then the turns interrupted.
fn lose_lease_work(
    tx: &Transaction<'_>,
    ids: (&str, &str),
    lease: &ExecutionLease,
    turn_reason: TurnReason,
    approval_reason: ApprovalReason,
    now: OffsetDateTime,
) -> ControlResult<()> {
    let turns: Vec<String> = {
        let mut q = tx
            .prepare("SELECT t.managed_turn_id FROM managed_turns t JOIN managed_turn_launches l ON l.managed_turn_id=t.managed_turn_id WHERE t.admitted_lease_id=?1 AND t.state IN ('starting','running') ORDER BY t.managed_turn_id")
            .map_err(db)?;
        q.query_map([lease.lease_id.as_str()], |r| r.get::<_, String>(0))
            .map_err(db)?
            .collect::<Result<_, _>>()
            .map_err(db)?
    };
    for id in turns {
        settle_in(tx, ids, Some(&id), approval_reason, now)?;
        let mut t = read_turn(tx, &id)?;
        let generation = sidecar(tx, "managed_turn_launches", "managed_turn_id", &id)?
            .ok_or(E::StorageIncompatible)?
            .1;
        let at = stamp(now.max(ts(&enum_text(&t.updated_at)?)?))?;
        t.state = TurnState::Interrupted;
        t.revision += 1;
        t.updated_at = at.clone().try_into().map_err(invalid)?;
        t.terminal_at = Some(at.clone().try_into().map_err(invalid)?);
        t.terminal_reason = Some(turn_reason);
        t.validate()?;
        tx.execute(
            "UPDATE managed_turns SET state='interrupted',revision=?2,updated_at=?3,terminal_at=?3,terminal_reason=?4 WHERE managed_turn_id=?1",
            params![id, t.revision as i64, at, enum_text(&turn_reason)?],
        )
        .map_err(db)?;
        turn_event(tx, ids, &t, generation, None, "interrupted")?;
    }
    Ok(())
}

/// Settles undecided and unconfirmed approvals (all, or those of one turn). Caller attested.
fn settle_in(
    tx: &Transaction<'_>,
    ids: (&str, &str),
    turn: Option<&str>,
    reason: ApprovalReason,
    now: OffsetDateTime,
) -> ControlResult<usize> {
    let deadline = matches!(
        reason,
        ApprovalReason::ApprovalTimeout | ApprovalReason::LeaseExpired
    );
    let found: Vec<String> = {
        let mut q = tx
            .prepare("SELECT managed_approval_id FROM managed_approvals WHERE (state='pending' OR (state='accepted' AND effect='dispatched')) AND (?1 IS NULL OR managed_turn_id=?1) ORDER BY created_sequence")
            .map_err(db)?;
        q.query_map([turn], |r| r.get::<_, String>(0))
            .map_err(db)?
            .collect::<Result<_, _>>()
            .map_err(db)?
    };
    let mut n = 0;
    for id in found {
        let mut a = read_approval(tx, &id)?;
        let generation = sidecar(tx, "managed_approval_launches", "managed_approval_id", &id)?
            .ok_or(E::StorageIncompatible)?
            .1;
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
        update_approval(tx, &a)?;
        approval_event(tx, ids, &a, generation, None, &enum_text(&a.state)?)?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
#[path = "writers_tests.rs"]
mod tests;
