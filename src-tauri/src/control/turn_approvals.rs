//! Approval state machine of the active turn. A provider request becomes a durable
//! `ManagedApproval` only for a complete, bounded action confined to the workspace; a
//! client decision is an intent the authority revalidates in full; and exactly one answer
//! per request is ever handed to the supervisor.
use super::{Turns, Verdict};
use crate::{
    adapters::codex::{
        CodexRequestId,
        safety::{ActionKind, ApprovalRequest, FileProposal, ItemEnd, NoticeKind},
    },
    contracts::{control::*, identities::ManagedTurnId},
    control::{
        conversations::Conversations,
        mediation::{Action, Built, Held, Mediation, PROPOSAL_WAIT_SECONDS},
        runtime::{ProviderDecision, Runtime, RuntimeCommand},
        store::{Store, approvals::Decided},
    },
};
use std::{
    sync::{atomic::Ordering, mpsc::SyncSender},
    time::{Duration, Instant},
};
use time::OffsetDateTime;

/// An Accept waits this long for the profile attestation before it is abandoned undecided.
const ACCEPT_WAIT: Duration = Duration::from_secs(12);
/// Containment deadline of a turn the authority has just refused to continue.
const STOP_WAIT: Duration = Duration::from_secs(5);

pub(super) struct Accepting {
    respond: ApprovalRespond,
    request: CodexRequestId,
    reply: SyncSender<ControlResult<ResultValue>>,
    pub deadline: Instant,
}
enum Step {
    Done(ApprovalReceipt),
    Attesting(CodexRequestId),
}
/// Why the approvals of a turn end when the turn ends for `reason`.
pub(super) fn approval_reason(reason: TurnReason) -> ApprovalReason {
    match reason {
        TurnReason::Completed | TurnReason::ProviderFailed => ApprovalReason::TurnEnded,
        TurnReason::UserCancel => ApprovalReason::TurnCancelled,
        TurnReason::LeaseExpired => ApprovalReason::LeaseExpired,
        TurnReason::LeaseRevoked => ApprovalReason::LeaseRevoked,
        TurnReason::SessionClosed => ApprovalReason::SessionClosed,
        TurnReason::AuthorityShutdown => ApprovalReason::AuthorityShutdown,
        TurnReason::AuthorityRestarted => ApprovalReason::AuthorityRestarted,
        TurnReason::InferenceIsolationViolation => ApprovalReason::IsolationViolation,
        TurnReason::ApprovalExpired => ApprovalReason::ApprovalTimeout,
        TurnReason::ActionRejected => ApprovalReason::Unsupported,
        TurnReason::RuntimeStopped
        | TurnReason::RuntimeFailed
        | TurnReason::DispatchUnknown
        | TurnReason::InterruptTimeout => ApprovalReason::RuntimeLost,
    }
}
/// Terminalizes whatever approval the turn still has open. A mere deadline leaves an
/// accepted approval alone while its turn runs; once the turn is over, an accepted answer
/// the provider never reported on is indeterminate whatever ended the turn.
pub(super) fn settle_turn(
    store: &mut Store,
    turn: &ManagedTurnId,
    reason: TurnReason,
) -> ControlResult<Vec<ManagedApprovalRecord>> {
    let primary = approval_reason(reason);
    let mut settled = store.settle_approvals(Some(turn), primary)?;
    if matches!(
        primary,
        ApprovalReason::ApprovalTimeout | ApprovalReason::LeaseExpired
    ) {
        settled.extend(store.settle_approvals(Some(turn), ApprovalReason::TurnEnded)?);
    }
    Ok(settled)
}
pub(super) fn state_text(state: ApprovalState) -> &'static str {
    match state {
        ApprovalState::Pending => "pending",
        ApprovalState::Accepted => "accepted",
        ApprovalState::Declined => "declined",
        ApprovalState::Cancelled => "cancelled",
        ApprovalState::Expired => "expired",
        ApprovalState::Invalidated => "invalidated",
        ApprovalState::Indeterminate => "indeterminate",
    }
}
fn verdict(mediated: bool) -> Verdict {
    if mediated {
        Verdict::Continue
    } else {
        Verdict::Violation
    }
}
impl Turns {
    /// Tells the transient stream that these approvals are terminal. The stream names the
    /// approval and its state; the action itself is never part of an event.
    pub(super) fn announce(&mut self, settled: &[ManagedApprovalRecord]) {
        let Some(a) = &self.active else {
            return;
        };
        for record in settled {
            self.streams.push_approval(
                &a.turn,
                StreamKind::ApprovalResolved,
                &record.managed_approval_id,
                Some(state_text(record.state)),
            );
        }
    }
    /// Every notice of the mediated surface. Whatever does not fit the one legal order
    /// (announced, asked, accepted, then run) is a violation.
    pub(super) fn mediate(
        &mut self,
        store: &mut Store,
        runtime: &mut Runtime,
        kind: NoticeKind,
    ) -> ControlResult<Verdict> {
        // Only an isolated local-tools generation has a mediated surface at all.
        if runtime.status().runtime_safety_profile != Some(RuntimeSafetyProfile::CodexLocalToolsV1)
        {
            return Ok(Verdict::Violation);
        }
        let Some(a) = self.active.as_mut() else {
            return Ok(Verdict::Continue);
        };
        match kind {
            NoticeKind::CommandProposed(p) => {
                if !a.mediation.command_proposed(p) {
                    return Ok(Verdict::Violation);
                }
                self.streams
                    .push(&a.turn, StreamKind::CommandProposed, None);
                Ok(Verdict::Continue)
            }
            NoticeKind::FileProposed(p) => self.file_proposed(store, runtime, p),
            NoticeKind::ApprovalRequested(r) => self.requested(store, runtime, r),
            // Output and applied changes exist only after the authority accepted the item.
            NoticeKind::Output { item, kind, delta } => {
                if a.mediation.accepted(item.as_str()).is_none() {
                    return Ok(Verdict::Violation);
                }
                // Transient and bounded by the stream itself: the oldest events give way
                // and the gap is reported.
                if kind == ActionKind::Command {
                    a.mediation.streamed(item.as_str());
                    self.streams
                        .push(&a.turn, StreamKind::CommandOutputDelta, Some(&delta));
                }
                Ok(Verdict::Continue)
            }
            NoticeKind::DiffUpdated => Ok(verdict(a.mediation.file_accepted)),
            NoticeKind::ItemEnded {
                item,
                kind,
                end,
                output,
            } => self.item_ended(store, item.as_str(), kind, end, output),
            NoticeKind::RequestResolved(request) => self.request_resolved(store, &request),
            _ => Ok(Verdict::Continue),
        }
    }
    fn requested(
        &mut self,
        store: &mut Store,
        runtime: &mut Runtime,
        r: ApprovalRequest,
    ) -> ControlResult<Verdict> {
        let a = self.active.as_mut().expect("active turn");
        // One request at a time: there is no approval queue.
        if a.mediation.held.as_ref().is_some_and(|h| !h.answered) {
            return Ok(Verdict::Violation);
        }
        let built = a.mediation.build(&r);
        if matches!(built, Built::Violation) {
            return Ok(Verdict::Violation);
        }
        let mut held = Held {
            request: r.request,
            item: r.item.as_str().to_owned(),
            kind: r.kind,
            offered: r.offered,
            approval: None,
            action: None,
            expires: None,
            wait_until: None,
            answered: false,
        };
        // A turn that is already being stopped opens nothing: the provider withdraws the
        // request when the turn ends.
        if a.cancel.is_some() {
            a.mediation.held = Some(held);
            return Ok(Verdict::Continue);
        }
        match built {
            Built::Incomplete => {
                held.wait_until =
                    Some(store.lease_now() + time::Duration::seconds(PROPOSAL_WAIT_SECONDS));
                a.mediation.held = Some(held);
                Ok(Verdict::Continue)
            }
            Built::Action(action) => {
                a.mediation.held = Some(held);
                self.open(store, runtime, action)
            }
            _ => {
                a.mediation.held = Some(held);
                self.refuse(store, runtime)
            }
        }
    }
    /// Opens the durable approval of the held request for one complete action.
    fn open(
        &mut self,
        store: &mut Store,
        runtime: &mut Runtime,
        action: Action,
    ) -> ControlResult<Verdict> {
        let a = self.active.as_mut().expect("active turn");
        let item = match &a.mediation.held {
            Some(held) => held.item.clone(),
            None => return Ok(Verdict::Continue),
        };
        if !action.confined(&a.workspace) {
            return self.refuse(store, runtime);
        }
        let opened = store.open_approval(
            &a.turn.managed_turn_id,
            &a.turn.runtime_instance_id,
            action.kind(),
            &item,
            &action.fingerprint(),
            action.structure(),
        );
        match opened {
            Ok(record) => {
                let expires = timestamp(&record.expires_at)?;
                self.streams.push_approval(
                    &a.turn,
                    StreamKind::ApprovalRequired,
                    &record.managed_approval_id,
                    None,
                );
                let held = a.mediation.held.as_mut().expect("held request");
                held.approval = Some(record.managed_approval_id);
                held.expires = Some(expires);
                held.wait_until = None;
                held.action = Some(action);
                Ok(Verdict::Continue)
            }
            // A second pending approval, or a second request for one item.
            Err(ControlError::CapacityExceeded | ControlError::CorrelationError) => {
                Ok(Verdict::Violation)
            }
            Err(
                ControlError::GateDenied
                | ControlError::InvalidTransition
                | ControlError::InvalidRequest,
            ) => self.refuse(store, runtime),
            Err(e) => Err(e),
        }
    }
    /// The held request can never become an acceptable approval: the turn stops and the
    /// provider gets the reductive answer, if it offers one.
    fn refuse(&mut self, store: &mut Store, runtime: &mut Runtime) -> ControlResult<Verdict> {
        self.request_cancel(store, runtime, TurnReason::ActionRejected)?;
        Ok(Verdict::Continue)
    }
    fn file_proposed(
        &mut self,
        store: &mut Store,
        runtime: &mut Runtime,
        p: FileProposal,
    ) -> ControlResult<Verdict> {
        let a = self.active.as_mut().expect("active turn");
        let item = p.item.as_str();
        // Once the approval of an item is open its change set is frozen: only an identical
        // restatement is tolerated.
        if let Some(held) = a
            .mediation
            .held
            .as_ref()
            .filter(|h| h.item == item && h.approval.is_some())
        {
            let restated = Mediation::file_action(item, &p.changes).map(|f| f.fingerprint());
            let frozen = held.action.as_ref().map(Action::fingerprint);
            return Ok(verdict(restated.is_some() && restated == frozen));
        }
        let first = !a.mediation.announced(item);
        if !a.mediation.file_proposed(item, p.changes) {
            return Ok(Verdict::Violation);
        }
        if first {
            self.streams
                .push(&a.turn, StreamKind::FileChangeProposed, None);
        }
        // A request that was waiting for exactly this change set can now be decided.
        let waiting = a.cancel.is_none()
            && a.mediation.held.as_ref().is_some_and(|h| {
                h.item == item
                    && h.kind == ActionKind::File
                    && h.wait_until.is_some()
                    && !h.answered
            });
        if !waiting {
            return Ok(Verdict::Continue);
        }
        match a.mediation.build_file(item) {
            Built::Action(action) => self.open(store, runtime, action),
            Built::Incomplete => Ok(Verdict::Continue),
            Built::Refused => self.refuse(store, runtime),
            Built::Violation => Ok(Verdict::Violation),
        }
    }
    fn item_ended(
        &mut self,
        store: &mut Store,
        item: &str,
        kind: ActionKind,
        end: ItemEnd,
        output: Option<String>,
    ) -> ControlResult<Verdict> {
        let a = self.active.as_mut().expect("active turn");
        let undecided = a
            .mediation
            .held
            .as_ref()
            .is_some_and(|h| h.item == item && !h.answered && h.approval.is_some());
        let accepted = a.mediation.ended(item);
        match (end, accepted) {
            (ItemEnd::Completed | ItemEnd::Failed, Some((approval, accepted_kind))) => {
                // The provider reports the item as the kind that was accepted, or not at all.
                let command = accepted_kind == ApprovalKind::CommandExecution;
                if command != (kind == ActionKind::Command) {
                    return Ok(Verdict::Violation);
                }
                // A one-shot command reports its output only when it finishes. It is shown
                // once: not again when it was already streamed while the command ran.
                if a.mediation.unshown(item)
                    && let Some(output) = &output
                {
                    self.streams
                        .push(&a.turn, StreamKind::CommandOutputDelta, Some(output));
                }
                self.streams.push(
                    &a.turn,
                    match (command, end == ItemEnd::Completed) {
                        (true, true) => StreamKind::CommandCompleted,
                        (true, false) => StreamKind::CommandFailed,
                        (false, true) => StreamKind::FileChangeCompleted,
                        (false, false) => StreamKind::FileChangeFailed,
                    },
                    None,
                );
                match store.confirm_approval(
                    &approval,
                    &a.turn.managed_turn_id,
                    &a.turn.runtime_instance_id,
                    item,
                ) {
                    // Already indeterminate: the report changes nothing.
                    Ok(_) | Err(ControlError::InvalidTransition) => Ok(Verdict::Continue),
                    Err(e) => Err(e),
                }
            }
            // It ran, and nothing was accepted for it.
            (ItemEnd::Completed | ItemEnd::Failed, None) => Ok(Verdict::Violation),
            (ItemEnd::Declined, _) => {
                // The provider dropped the item on its own: nothing remains to decide.
                if undecided {
                    let settled = store.settle_approvals(
                        Some(&a.turn.managed_turn_id),
                        ApprovalReason::TurnEnded,
                    )?;
                    self.announce(&settled);
                }
                Ok(Verdict::Continue)
            }
        }
    }
    fn request_resolved(
        &mut self,
        store: &mut Store,
        request: &CodexRequestId,
    ) -> ControlResult<Verdict> {
        let a = self.active.as_mut().expect("active turn");
        let Some(held) = a.mediation.held.take_if(|h| h.request == *request) else {
            return Ok(Verdict::Continue);
        };
        // The provider stopped waiting before the authority answered: accepting it is no
        // longer possible, and neither is refusing it.
        if !held.answered && held.approval.is_some() {
            let settled =
                store.settle_approvals(Some(&a.turn.managed_turn_id), ApprovalReason::TurnEnded)?;
            self.announce(&settled);
        }
        Ok(Verdict::Continue)
    }
    /// Deadline of what the turn is waiting on, on the authority clock. A turn that is
    /// already being stopped waits on nothing here: its own stop deadline bounds it, and a
    /// deadline that has passed must never be handed to the core as its next wake.
    pub fn approval_deadline(&self) -> Option<OffsetDateTime> {
        let held = self
            .active
            .as_ref()
            .filter(|a| a.cancel.is_none())
            .and_then(|a| a.mediation.held.as_ref())
            .filter(|h| !h.answered)?;
        match (held.expires, held.wait_until) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
    /// The deadline passed: the approval expires durably and the turn has to stop.
    pub(super) fn approval_overdue(
        &mut self,
        store: &mut Store,
    ) -> ControlResult<Option<TurnReason>> {
        let Some(held) = self
            .active
            .as_ref()
            .and_then(|a| a.mediation.held.as_ref())
            .filter(|h| !h.answered)
        else {
            return Ok(None);
        };
        let now = store.lease_now();
        if held.wait_until.is_some_and(|t| now >= t) {
            return Ok(Some(TurnReason::ActionRejected));
        }
        if held.expires.is_none_or(|t| now < t) {
            return Ok(None);
        }
        let expired = store.expire_approvals()?;
        self.announce(&expired);
        Ok(Some(
            if expired
                .iter()
                .any(|a| a.terminal_reason == Some(ApprovalReason::LeaseExpired))
            {
                TurnReason::LeaseExpired
            } else {
                TurnReason::ApprovalExpired
            },
        ))
    }
    pub(super) fn expire_accepting(&mut self) {
        if let Some(p) = self.accepting.take_if(|p| Instant::now() >= p.deadline) {
            let _ = p.reply.try_send(Err(ControlError::Timeout));
        }
    }
    pub(super) fn reject_accepting(&mut self, error: ControlError) {
        if let Some(p) = self.accepting.take() {
            let _ = p.reply.try_send(Err(error));
        }
    }
    /// A client decision. It is an intent: nothing in it is trusted, and the reply is the
    /// canonical receipt or the reason nothing changed. An Accept is answered only after
    /// the runtime re-proved its profile; until then nothing is decided.
    pub fn respond(
        &mut self,
        store: &mut Store,
        runtime: &mut Runtime,
        conversations: &Conversations,
        p: &ApprovalRespond,
        reply: SyncSender<ControlResult<ResultValue>>,
    ) {
        match self.decide(store, runtime, conversations, p) {
            Ok(Step::Done(receipt)) => {
                let _ = reply.try_send(Ok(ResultValue::ApprovalReceipt(receipt)));
            }
            Ok(Step::Attesting(request)) => {
                self.accepting = Some(Accepting {
                    respond: p.clone(),
                    request,
                    reply,
                    deadline: Instant::now() + ACCEPT_WAIT,
                });
            }
            Err(e) => {
                if matches!(
                    e,
                    ControlError::StorageUnavailable | ControlError::StorageIncompatible
                ) {
                    store.execution_available = false;
                }
                let _ = reply.try_send(Err(e));
            }
        }
    }
    fn decide(
        &mut self,
        store: &mut Store,
        runtime: &mut Runtime,
        conversations: &Conversations,
        p: &ApprovalRespond,
    ) -> ControlResult<Step> {
        // The same intent always returns its original receipt and never a second answer.
        if let Some(receipt) = store.approval_replay(p)? {
            return Ok(Step::Done(receipt));
        }
        let record = store.approval(&p.managed_approval_id)?;
        if record.state != ApprovalState::Pending {
            return Err(ControlError::InvalidTransition);
        }
        if record.revision != p.expected_revision {
            return Err(ControlError::StaleRevision);
        }
        if p.decision != ApprovalDecision::Accept {
            return self.reduce(store, runtime, p, &record).map(Step::Done);
        }
        if self.accepting.is_some() {
            return Err(ControlError::RuntimeOperationInProgress);
        }
        let (request, workspace) = self.accept_gate(store, runtime, conversations, &record)?;
        if !runtime.turn_command(RuntimeCommand::Attest {
            request: request.clone(),
            workspace,
        }) {
            return Err(ControlError::GateDenied);
        }
        Ok(Step::Attesting(request))
    }
    /// Decline and cancel reduce authority, so they need none: only the exact pending
    /// approval and its still-held provider request. A declined file change keeps its turn
    /// when the provider offers `decline`; every other refusal ends the turn with `cancel`.
    fn reduce(
        &mut self,
        store: &mut Store,
        runtime: &mut Runtime,
        p: &ApprovalRespond,
        record: &ManagedApprovalRecord,
    ) -> ControlResult<ApprovalReceipt> {
        let a = self
            .active
            .as_mut()
            .filter(|a| a.turn.managed_turn_id == record.managed_turn_id)
            .ok_or(ControlError::GateDenied)?;
        let held = a
            .mediation
            .held
            .as_mut()
            .filter(|h| h.approval.as_ref() == Some(&record.managed_approval_id) && !h.answered)
            .ok_or(ControlError::GateDenied)?;
        let fingerprint = held
            .action
            .as_ref()
            .map(Action::fingerprint)
            .ok_or(ControlError::GateDenied)?;
        let receipt = match store.decide_approval(p, &fingerprint)? {
            Decided::Fresh(decided, receipt) => {
                self.streams.push_approval(
                    &a.turn,
                    StreamKind::ApprovalResolved,
                    &decided.managed_approval_id,
                    Some(state_text(decided.state)),
                );
                receipt
            }
            Decided::Replayed(receipt) => return Ok(receipt),
        };
        held.answered = true;
        let request = held.request.clone();
        a.mediation.dispatched = Some(request.clone());
        let held = a.mediation.held.as_ref().expect("held request");
        let keeps_turn = record.kind == ApprovalKind::FileChange
            && p.decision == ApprovalDecision::Decline
            && held.offered.decline;
        let cancel_offered = held.offered.cancel;
        if keeps_turn {
            if !runtime.turn_command(RuntimeCommand::Resolve {
                request,
                decision: ProviderDecision::Decline,
            }) {
                a.deadline = Some(Instant::now());
            }
            return Ok(receipt);
        }
        a.admission.store(false, Ordering::Release);
        a.cancel = Some(TurnReason::UserCancel);
        a.deadline = Some(Instant::now() + STOP_WAIT);
        if cancel_offered {
            // The reductive answer itself ends the turn: no interrupt follows it.
            a.interrupt_sent = true;
            if !runtime.turn_command(RuntimeCommand::Resolve {
                request,
                decision: ProviderDecision::Cancel,
            }) {
                a.deadline = Some(Instant::now());
            }
        } else {
            // No refusal is offered for this request: the turn is interrupted instead.
            self.request_cancel(store, runtime, TurnReason::UserCancel)?;
        }
        Ok(receipt)
    }
    /// Every in-memory fact an Accept depends on, checked against the exact approval. The
    /// durable facts (pending state, revision, deadline, session, turn and the original
    /// lease with its scope) are revalidated inside the decision transaction.
    fn accept_gate(
        &self,
        store: &Store,
        runtime: &Runtime,
        conversations: &Conversations,
        record: &ManagedApprovalRecord,
    ) -> ControlResult<(CodexRequestId, String)> {
        let denied = ControlError::GateDenied;
        if !store.execution_available {
            return Err(denied);
        }
        let a = self.active.as_ref().ok_or(denied)?;
        let held = a.mediation.held.as_ref().ok_or(denied)?;
        let action = held.action.as_ref().ok_or(denied)?;
        let status = runtime.status();
        let attachment = conversations
            .projection(store, runtime, &record.managed_session_id)
            .map_err(|_| denied)?;
        let proven = a.turn.managed_turn_id == record.managed_turn_id
            && a.turn.managed_session_id == record.managed_session_id
            && a.turn.state == TurnState::Running
            && a.cancel.is_none()
            // The exact generation that asked is the one that is live and ready.
            && a.turn.runtime_instance_id == record.runtime_instance_id
            && status.actual_state == ActualState::Ready
            && status.runtime_instance_id.as_ref() == Some(&record.runtime_instance_id)
            && status.runtime_safety_profile == Some(RuntimeSafetyProfile::CodexLocalToolsV1)
            && status.workspace_root.as_deref() == Some(a.workspace.as_str())
            // The session is attached to that generation under the local-tools profile.
            && attachment.state == BindingState::BoundAttached
            && attachment.attachment_profile == Some(AttachmentProfile::LocalToolsV1)
            && attachment.runtime_instance_id == status.runtime_instance_id
            && attachment.binding.as_ref().is_some_and(|b| {
                b.workspace_root == a.workspace && b.runtime_session_id == a.turn.codex_thread_id
            })
            // The exact provider request is still held, unanswered, for that exact item.
            && held.approval.as_ref() == Some(&record.managed_approval_id)
            && !held.answered
            && held.offered.accept
            && held.item == record.provider_item_id
            // The complete action is the one that was fingerprinted, and is still confined.
            && action.item() == record.provider_item_id
            && action.kind() == record.kind
            && action.fingerprint() == record.action_fingerprint
            && action.confined(&a.workspace);
        if !proven {
            return Err(denied);
        }
        Ok((held.request.clone(), a.workspace.clone()))
    }
    /// The supervisor finished re-proving the profile for the Accept in flight. On success
    /// the whole gate runs again and the decision commits; only then is the one answer
    /// handed over, after the receipt.
    pub fn attested(
        &mut self,
        store: &mut Store,
        runtime: &mut Runtime,
        conversations: &Conversations,
        request: &CodexRequestId,
        valid: bool,
    ) -> ControlResult<Verdict> {
        let Some(pending) = self.accepting.take_if(|p| p.request == *request) else {
            return Ok(Verdict::Continue);
        };
        let outcome = if valid {
            self.commit(store, runtime, conversations, &pending.respond)
        } else {
            Err(ControlError::GateDenied)
        };
        match outcome {
            Ok((receipt, dispatch)) => {
                let _ = pending
                    .reply
                    .try_send(Ok(ResultValue::ApprovalReceipt(receipt)));
                if let Some(request) = dispatch
                    && !runtime.turn_command(RuntimeCommand::Resolve {
                        request,
                        decision: ProviderDecision::Accept,
                    })
                {
                    // Durably dispatched, yet never handed over: the outcome is unprovable.
                    return self.uncertain(store);
                }
                Ok(Verdict::Continue)
            }
            Err(e) => {
                if matches!(
                    e,
                    ControlError::StorageUnavailable | ControlError::StorageIncompatible
                ) {
                    store.execution_available = false;
                }
                let _ = pending.reply.try_send(Err(e));
                Ok(Verdict::Continue)
            }
        }
    }
    fn commit(
        &mut self,
        store: &mut Store,
        runtime: &Runtime,
        conversations: &Conversations,
        p: &ApprovalRespond,
    ) -> ControlResult<(ApprovalReceipt, Option<CodexRequestId>)> {
        if let Some(receipt) = store.approval_replay(p)? {
            return Ok((receipt, None));
        }
        let record = store.approval(&p.managed_approval_id)?;
        if record.state != ApprovalState::Pending {
            return Err(ControlError::InvalidTransition);
        }
        if record.revision != p.expected_revision {
            return Err(ControlError::StaleRevision);
        }
        let (request, _) = self.accept_gate(store, runtime, conversations, &record)?;
        let a = self.active.as_mut().expect("gated turn");
        let held = a.mediation.held.as_mut().expect("gated request");
        let fingerprint = held
            .action
            .as_ref()
            .map(Action::fingerprint)
            .ok_or(ControlError::GateDenied)?;
        // Decision, dispatch marker, audit and receipt commit in one transaction.
        match store.decide_approval(p, &fingerprint)? {
            Decided::Replayed(receipt) => Ok((receipt, None)),
            Decided::Fresh(record, receipt) => {
                held.answered = true;
                let item = held.item.clone();
                self.streams.push_approval(
                    &a.turn,
                    StreamKind::ApprovalResolved,
                    &record.managed_approval_id,
                    Some(state_text(record.state)),
                );
                a.mediation
                    .accept(&item, record.managed_approval_id, record.kind);
                a.mediation.dispatched = Some(request.clone());
                Ok((receipt, Some(request)))
            }
        }
    }
    /// An accepted answer whose delivery cannot be proven: the approval is indeterminate
    /// and nothing is ever sent again for it.
    fn uncertain(&mut self, store: &mut Store) -> ControlResult<Verdict> {
        if let Some(a) = &self.active {
            let settled = store
                .settle_approvals(Some(&a.turn.managed_turn_id), ApprovalReason::EffectUnknown)?;
            self.announce(&settled);
        }
        Ok(Verdict::Uncertain)
    }
    /// What became of the one answer the supervisor was handed.
    pub fn delivered(
        &mut self,
        store: &mut Store,
        request: &CodexRequestId,
        decision: ProviderDecision,
        written: bool,
    ) -> ControlResult<Verdict> {
        // Only the answer this turn is still waiting to hear about counts.
        let Some(a) = self.active.as_mut() else {
            return Ok(Verdict::Continue);
        };
        if a.mediation.dispatched.take_if(|d| d == request).is_none() {
            return Ok(Verdict::Continue);
        }
        if written {
            // The Accept reached the provider: from here the item may run.
            let started = a
                .mediation
                .held
                .as_ref()
                .filter(|h| h.request == *request)
                .and_then(|h| a.mediation.accepted(&h.item))
                .map(|(_, kind)| *kind);
            if let (ProviderDecision::Accept, Some(kind)) = (decision, started) {
                self.streams.push(
                    &a.turn,
                    if kind == ApprovalKind::CommandExecution {
                        StreamKind::CommandStarted
                    } else {
                        StreamKind::FileChangeStarted
                    },
                    None,
                );
            }
            return Ok(Verdict::Continue);
        }
        if decision == ProviderDecision::Accept {
            return self.uncertain(store);
        }
        // A refusal that was not written leaves the turn to immediate containment.
        if let Some(a) = self.active.as_mut() {
            a.deadline = Some(Instant::now());
        }
        Ok(Verdict::Continue)
    }
    /// Canonical metadata of an approval and, only while the authority still holds its
    /// request, the action to decide on. The action is never read from storage.
    fn present(
        &self,
        approval: ManagedApprovalRecord,
        file_index: Option<u16>,
        offset: Option<u32>,
    ) -> ControlResult<ApprovalStatus> {
        let held = self
            .active
            .as_ref()
            .filter(|a| a.turn.managed_turn_id == approval.managed_turn_id)
            .and_then(|a| {
                let held = a.mediation.held.as_ref()?;
                (held.approval.as_ref() == Some(&approval.managed_approval_id)
                    && approval.state == ApprovalState::Pending)
                    .then_some((a, held))
            });
        let mut status = ApprovalStatus {
            approval,
            acceptable: false,
            command: None,
            file_count: None,
            file: None,
        };
        let Some((a, held)) = held else {
            return Ok(status);
        };
        match &held.action {
            Some(Action::Command(command)) => {
                if file_index.is_some() || offset.is_some() {
                    return Err(ControlError::InvalidRequest);
                }
                status.command = Some(command.presentation());
            }
            Some(Action::File(file)) => {
                status.file_count = Some(file.files());
                status.file = Some(file.page(file_index.unwrap_or(0), offset.unwrap_or(0))?);
            }
            None => return Ok(status),
        }
        status.acceptable = !held.answered && held.offered.accept && a.cancel.is_none();
        Ok(status)
    }
    pub fn approval_status(&self, store: &Store, p: &ApprovalGet) -> ControlResult<ResultValue> {
        let record = store.approval(&p.managed_approval_id)?;
        Ok(ResultValue::Approval(Box::new(self.present(
            record,
            p.file_index,
            p.offset,
        )?)))
    }
    /// The approval one turn is waiting on. A turn that waits on nothing has none.
    pub fn approval_pending(
        &self,
        store: &Store,
        p: &ApprovalPending,
    ) -> ControlResult<ResultValue> {
        let turn = store.turn(&p.managed_turn_id)?;
        let pending = store
            .pending_approval()?
            .filter(|a| a.managed_turn_id == turn.managed_turn_id)
            .map(|a| self.present(a, None, None))
            .transpose()?;
        Ok(ResultValue::PendingApproval(pending.map(Box::new)))
    }
}
