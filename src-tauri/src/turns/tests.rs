//! Turns against a scripted authority: the exact requests a start, a cancel and a read
//! send, what crosses to the window and what never does, and what a lost answer leaves
//! behind. How the real authority behaves is covered by `tests/turns_process.rs`; this
//! peer models only what the service can observe.
use super::{service::TURN_LEASE_SECONDS, *};
use crate::{
    contracts::{
        Sequence, WireUuid,
        control::*,
        event_envelope::EventEnvelopeV1,
        identities::{ManagedApprovalId, ManagedSessionId, ManagedTurnId},
    },
    sessions::{
        SessionFailure, SessionService,
        tests::{Peer, Reply, answer, binding, lease, record, session_id, uuid},
    },
};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

const START: &str = "managedSession/turn/start";
/// Read before every start: the profile the session is attached under.
const BINDING: &str = "managedSession/runtime/status";
const STATUS: &str = "managedSession/turn/status";
const EVENTS: &str = "managedSession/turn/events";
const CANCEL: &str = "managedSession/turn/cancel";
const PENDING: &str = "managedTurn/approval/pending";
const GATE: &str = "executionGate/status";
const GRANT: &str = "executionLease/grant";
const REVOKE: &str = "executionLease/revoke";
const AUDIT: &str = "audit/read";

/// Provider identities and text that must never reach the window.
const THREAD: &str = "0199a213-81c0-7800-8aa1-bbab2a035a53";
const PROVIDER_TURN: &str = "provider-turn-PRIVATE";
const REASONING: &str = "REASONING_SUMMARY_PRIVATE";
const COMMAND_OUTPUT: &str = "COMMAND_OUTPUT_PRIVATE";
const ACTION_DETAIL: &str = "ACTION_DETAIL_PRIVATE";

#[derive(Clone, Copy)]
enum Fault {
    Refuse(ControlError),
    /// The request takes effect and its answer never leaves.
    AnswerLost,
}

struct Turn {
    intent: WireUuid,
    record: ManagedTurnRecord,
    events: Vec<(StreamKind, Option<&'static str>)>,
    /// Events the authority no longer holds, counted from the first.
    dropped: usize,
    retained: bool,
}

/// The authority as the turn service can observe it.
struct World {
    session: ManagedSessionId,
    gate: GateReason,
    lease: Option<ExecutionLease>,
    turns: Vec<Turn>,
    /// Durable events: (event type, session, turn).
    audit: Vec<(&'static str, ManagedSessionId, ManagedTurnId)>,
    faults: Vec<(&'static str, usize, Fault)>,
    seen: HashMap<String, usize>,
    /// What the running turn still puts out, and then completes, right after its events
    /// are read the next time: a turn that ends between two questions.
    ends_after_read: Option<Vec<(StreamKind, Option<&'static str>)>>,
    /// The profile the session is attached under.
    profile: &'static str,
    /// Approvals, each as the authority would answer a read of it.
    approvals: Vec<ApprovalStatus>,
    /// Which durable event, by its place in `audit`, names which approval.
    named: HashMap<usize, ManagedApprovalId>,
    /// Decisions the authority committed, by the intent that asked for them.
    decided: HashMap<String, ApprovalReceipt>,
    /// Places of `audit` the stream does not answer: a hole in what it holds.
    holes: Vec<usize>,
}
impl World {
    fn new(session: &ManagedSessionId) -> Self {
        Self {
            ends_after_read: None,
            profile: "inference_only_v1",
            approvals: Vec::new(),
            named: HashMap::new(),
            decided: HashMap::new(),
            holes: Vec::new(),
            session: session.clone(),
            gate: GateReason::Allowed,
            lease: None,
            turns: Vec::new(),
            audit: Vec::new(),
            faults: Vec::new(),
            seen: HashMap::new(),
        }
    }
    fn fault(mut self, method: &'static str, nth: usize, fault: Fault) -> Self {
        self.faults.push((method, nth, fault));
        self
    }
    fn reason(&self) -> GateReason {
        match &self.lease {
            None => GateReason::LeaseMissing,
            Some(l) if !l.scopes.contains(&ExecutionScope::TurnStart) => {
                GateReason::ScopeNotGranted
            }
            Some(_) if self.turns.iter().any(|t| !t.record.state.terminal()) => {
                GateReason::ActiveTurnExists
            }
            Some(_) => self.gate,
        }
    }
    fn turn(&mut self, id: &ManagedTurnId) -> ControlResult<&mut Turn> {
        self.turns
            .iter_mut()
            .find(|t| &t.record.managed_turn_id == id)
            .ok_or(ControlError::TurnNotFound)
    }
    /// Moves a turn on, as the authority would: one more revision.
    fn advance(&mut self, id: &ManagedTurnId, state: TurnState, reason: Option<TurnReason>) {
        let turn = self.turn(id).unwrap();
        let mut v = serde_json::to_value(&turn.record).unwrap();
        v["state"] = json!(state);
        v["revision"] = json!(turn.record.revision + 1);
        v["codex_turn_id"] = json!(PROVIDER_TURN);
        v["updated_at"] = json!("2026-10-08T00:00:09Z");
        if state.terminal() {
            v["terminal_at"] = v["updated_at"].clone();
            v["terminal_reason"] = json!(reason);
        }
        turn.record = serde_json::from_value(v).unwrap();
        turn.record.validate().unwrap();
    }
    fn page(&self, t: &Turn, p: &TurnEvents) -> TurnStreamPage {
        let after = p.after_sequence.map_or(0, Sequence::get) as usize;
        let seq = |n: usize| n.to_string().parse::<Sequence>().unwrap();
        let event = |n: usize, kind: StreamKind, text: Option<&str>| -> ManagedTurnStreamEventV1 {
            serde_json::from_value(json!({
                "schema_version": 1, "managed_turn_id": t.record.managed_turn_id,
                "managed_session_id": t.record.managed_session_id,
                "codex_turn_id": PROVIDER_TURN, "sequence": n.to_string(),
                "kind": kind, "text": text,
            }))
            .unwrap()
        };
        // As the authority does it: a stream exists from a turn's first event on, and a
        // turn without one is answered as a gap marked right after the cursor.
        let retained = t.retained && !t.events.is_empty();
        let gap = !retained || after < t.dropped;
        let mut events = Vec::new();
        if gap {
            let at = if retained {
                t.dropped.max(1)
            } else {
                after + 1
            };
            events.push(event(at, StreamKind::StreamGap, None));
        }
        if retained {
            for (i, (kind, text)) in t.events.iter().enumerate() {
                let n = i + 1;
                if n > after.max(t.dropped) && events.len() < usize::from(p.limit) {
                    events.push(event(n, *kind, *text));
                }
            }
        }
        TurnStreamPage {
            next_sequence: events.last().map(|e| e.sequence),
            events,
            earliest_sequence: (retained && t.events.len() > t.dropped).then(|| seq(t.dropped + 1)),
            latest_sequence: retained.then(|| seq(t.events.len())),
            dropped_events: t.dropped as u64,
            gap,
            terminal: t.record.state.terminal(),
            retained,
        }
    }
    fn apply(&mut self, op: &Operation) -> ControlResult<ResultValue> {
        match op {
            Operation::TurnStart(p) => {
                if let Some(t) = self.turns.iter().find(|t| t.intent == p.intent_id) {
                    return Ok(ResultValue::Turn(t.record.clone()));
                }
                match self.reason() {
                    GateReason::Allowed => (),
                    GateReason::ActiveTurnExists => return Err(ControlError::ActiveTurnExists),
                    _ => return Err(ControlError::GateDenied),
                }
                let granted = self.lease.as_ref().unwrap();
                let record: ManagedTurnRecord = serde_json::from_value(json!({
                    "managed_turn_id": uuid(), "managed_session_id": p.managed_session_id,
                    "codex_thread_id": THREAD, "codex_turn_id": null,
                    "admitted_lease_id": granted.lease_id, "admitted_lease_revision": 1,
                    "runtime_instance_id": uuid(), "state": "starting", "revision": 1,
                    "created_at": "2026-10-08T00:00:00Z", "updated_at": "2026-10-08T00:00:00Z",
                    "terminal_at": null, "terminal_reason": null,
                }))
                .unwrap();
                self.audit.push((
                    "authority.managed_turn.admitted",
                    p.managed_session_id.clone(),
                    record.managed_turn_id.clone(),
                ));
                self.turns.push(Turn {
                    intent: p.intent_id.clone(),
                    record: record.clone(),
                    events: Vec::new(),
                    dropped: 0,
                    retained: true,
                });
                Ok(ResultValue::Turn(record))
            }
            Operation::TurnStatus(p) => {
                let turn = self
                    .turns
                    .iter()
                    .find(|t| t.record.managed_turn_id == p.managed_turn_id)
                    .ok_or(ControlError::TurnNotFound)?;
                let page = self.page(
                    turn,
                    &TurnEvents {
                        managed_turn_id: p.managed_turn_id.clone(),
                        after_sequence: None,
                        limit: 1,
                    },
                );
                Ok(ResultValue::TurnStatus(ManagedTurnStatus {
                    turn: turn.record.clone(),
                    stream_earliest_sequence: page.earliest_sequence,
                    stream_latest_sequence: page.latest_sequence,
                    stream_retained: page.retained,
                }))
            }
            Operation::TurnEvents(p) => {
                let turn = self
                    .turns
                    .iter()
                    .find(|t| t.record.managed_turn_id == p.managed_turn_id)
                    .ok_or(ControlError::TurnNotFound)?;
                let page = self.page(turn, p);
                if let Some(rest) = self.ends_after_read.take() {
                    let id = p.managed_turn_id.clone();
                    self.turn(&id)?.events.extend(rest);
                    self.advance(&id, TurnState::Completed, Some(TurnReason::Completed));
                }
                Ok(ResultValue::TurnEvents(page))
            }
            Operation::TurnCancel(p) => {
                let turn = self.turn(&p.managed_turn_id)?;
                if turn.record.state.terminal() {
                    return Err(ControlError::InvalidTransition);
                }
                if turn.record.revision != p.expected_revision {
                    return Err(ControlError::StaleRevision);
                }
                let id = p.managed_turn_id.clone();
                self.advance(&id, TurnState::Interrupted, Some(TurnReason::UserCancel));
                Ok(ResultValue::Turn(self.turn(&id)?.record.clone()))
            }
            Operation::ApprovalPending(p) => Ok(ResultValue::PendingApproval(
                self.approvals
                    .iter()
                    .find(|a| {
                        a.approval.managed_turn_id == p.managed_turn_id
                            && a.approval.state == ApprovalState::Pending
                    })
                    .cloned()
                    .map(Box::new),
            )),
            Operation::ApprovalStatus(p) => {
                let status = self
                    .approvals
                    .iter()
                    .find(|a| a.approval.managed_approval_id == p.managed_approval_id)
                    .ok_or(ControlError::ApprovalNotFound)?;
                let mut status = status.clone();
                match (&mut status.file, p.file_index) {
                    // A command has no file to select.
                    (None, Some(_)) => return Err(ControlError::InvalidRequest),
                    (Some(file), index) => {
                        let index = index.unwrap_or(0);
                        if index >= status.file_count.unwrap_or(0) {
                            return Err(ControlError::InvalidRequest);
                        }
                        file.index = index;
                        file.path = format!("/work/space/file-{index}.txt");
                        file.offset = p.offset.unwrap_or(0);
                    }
                    (None, None) => (),
                }
                Ok(ResultValue::Approval(Box::new(status)))
            }
            Operation::ApprovalRespond(p) => {
                if let Some(receipt) = self.decided.get(p.intent_id.as_str()) {
                    return if receipt.decision == p.decision {
                        Ok(ResultValue::ApprovalReceipt(receipt.clone()))
                    } else {
                        Err(ControlError::IntentConflict)
                    };
                }
                let status = self
                    .approvals
                    .iter_mut()
                    .find(|a| a.approval.managed_approval_id == p.managed_approval_id)
                    .ok_or(ControlError::ApprovalNotFound)?;
                if status.approval.state != ApprovalState::Pending {
                    return Err(ControlError::InvalidTransition);
                }
                if status.approval.revision != p.expected_revision {
                    return Err(ControlError::StaleRevision);
                }
                let mut v = serde_json::to_value(&status.approval).unwrap();
                v["state"] = json!(p.decision.state());
                v["decision"] = json!(p.decision);
                v["revision"] = json!(status.approval.revision + 1);
                v["effect"] = json!(if p.decision == ApprovalDecision::Accept {
                    "dispatched"
                } else {
                    "none"
                });
                v["updated_at"] = json!("2026-10-08T00:00:09Z");
                v["terminal_at"] = v["updated_at"].clone();
                v["terminal_reason"] = json!("client_decision");
                status.approval = serde_json::from_value(v).unwrap();
                status.approval.validate().unwrap();
                // Decided: the authority no longer presents the action.
                (status.acceptable, status.command) = (false, None);
                (status.file_count, status.file) = (None, None);
                let receipt: ApprovalReceipt = serde_json::from_value(json!({
                    "managed_approval_id": status.approval.managed_approval_id,
                    "managed_turn_id": status.approval.managed_turn_id,
                    "kind": status.approval.kind, "decision": p.decision,
                    "state": status.approval.state, "revision": status.approval.revision,
                    "action_fingerprint": status.approval.action_fingerprint,
                    "decided_at": "2026-10-08T00:00:09Z",
                }))
                .unwrap();
                self.decided
                    .insert(p.intent_id.as_str().to_owned(), receipt.clone());
                Ok(ResultValue::ApprovalReceipt(receipt))
            }
            Operation::GateStatus(q) => Ok(ResultValue::GateStatus(GateStatus {
                session_id: q.session_id.clone(),
                action: q.action,
                reason: self.reason(),
            })),
            Operation::LeaseActive(_) => Ok(ResultValue::ActiveLease(self.lease.clone())),
            Operation::LeaseGrant(p) => {
                if self.lease.is_some() {
                    return Err(ControlError::LeaseAlreadyActive);
                }
                let mut granted = lease(&self.session);
                granted.scopes = p.scopes.clone();
                self.lease = Some(granted.clone());
                Ok(ResultValue::Lease(granted))
            }
            Operation::LeaseRevoke(p) => match self.lease.take() {
                Some(mut held) if held.lease_id == p.lease_id => {
                    held.state = LeaseState::Revoked;
                    held.revision = 2;
                    held.revocation_reason = Some(RevocationReason::UserRequest);
                    Ok(ResultValue::Lease(held))
                }
                other => {
                    self.lease = other;
                    Err(ControlError::InvalidTransition)
                }
            },
            Operation::Get(p) => Ok(ResultValue::Session(record(&p.managed_session_id, 7))),
            Operation::BindingStatus(p) => Ok(ResultValue::BindingStatus(
                serde_json::from_value(json!({
                    "managed_session_id": p.managed_session_id, "state": "bound_attached",
                    "binding": binding(&p.managed_session_id),
                    "runtime_instance_id": uuid(), "operation_id": null,
                    "attachment_profile": self.profile,
                }))
                .unwrap(),
            )),
            Operation::AuditRead(p) => {
                let after = p.after_sequence.map_or(0, Sequence::get) as usize;
                let events: Vec<EventEnvelopeV1> = self
                    .audit
                    .iter()
                    .enumerate()
                    .skip(after)
                    .filter(|(i, _)| !self.holes.contains(i))
                    .take(usize::from(p.limit))
                    .map(|(i, (kind, session, turn))| {
                        envelope(&p.stream_id, i + 1, kind, session, turn, self.named.get(&i))
                    })
                    .collect();
                let reached = after + events.len();
                Ok(ResultValue::Audit {
                    next: (reached < self.audit.len())
                        .then(|| reached.to_string().parse().unwrap()),
                    events,
                })
            }
            other => panic!("{other:?}"),
        }
    }
}

fn envelope(
    stream: &WireUuid,
    sequence: usize,
    kind: &str,
    session: &ManagedSessionId,
    turn: &ManagedTurnId,
    approval: Option<&ManagedApprovalId>,
) -> EventEnvelopeV1 {
    let mut payload =
        json!({"managed_turn_id": turn, "lease_id": uuid(), "runtime_instance_id": uuid()});
    if let Some(approval) = approval {
        payload["managed_approval_id"] = json!(approval);
    }
    EventEnvelopeV1::from_value(json!({
        "schema_version": 1, "event_id": uuid(), "event_type": kind,
        "payload_schema_version": 1, "timestamp": "2026-10-08T00:00:00Z",
        "stream_id": stream, "sequence": sequence.to_string(), "session_id": session,
        "source": {"component": "wyss_control", "instance_id": uuid()},
        "correlation_id": null, "causation_id": null,
        "payload": payload,
        "persistence": "durable", "sensitivity": "internal", "redaction": "none",
    }))
    .unwrap()
}

fn method(op: &Operation) -> String {
    serde_json::to_value(op).unwrap()["method"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// A peer that is `world`, the session service over it, and the world to look at.
fn authority(world: World) -> (Peer, SessionService, TurnService, Arc<Mutex<World>>) {
    let world = Arc::new(Mutex::new(world));
    let shared = world.clone();
    let peer = Peer::canonical(move |op| {
        let mut world = shared.lock().unwrap();
        let name = method(op);
        let nth = {
            let seen = world.seen.entry(name.clone()).or_insert(0);
            *seen += 1;
            *seen
        };
        let fault = world
            .faults
            .iter()
            .find(|(m, n, _)| *m == name && *n == nth)
            .map(|(_, _, fault)| *fault);
        match fault {
            Some(Fault::Refuse(error)) => answer(Err(error)),
            Some(Fault::AnswerLost) => {
                let _ = world.apply(op);
                Reply::HangUp
            }
            None => answer(world.apply(op)),
        }
    });
    let sessions = peer.service();
    let turns = TurnService::new(sessions.clone());
    (peer, sessions, turns, world)
}
fn since(peer: &Peer, from: usize) -> Vec<String> {
    peer.methods().split_off(from)
}
fn failed<T: std::fmt::Debug>(
    result: Result<T, crate::sessions::SessionError>,
) -> (SessionFailure, bool) {
    let error = result.unwrap_err();
    (error.code, error.retry)
}
const PROMPT: &str = "Say hello.\nIn one line. PROMPT_PRIVATE";

#[test]
fn a_turn_is_admitted_under_a_lease_that_is_kept_until_the_turn_has_ended() {
    let session = session_id();
    let (peer, sessions, turns, world) = authority(World::new(&session));
    let intent = uuid();
    let started = turns
        .start(intent.clone(), session.clone(), 1, PROMPT.into())
        .unwrap();
    assert_eq!(peer.methods(), [BINDING, START, GATE, GRANT, START]);
    let requests = peer.requests();
    let (Operation::TurnStart(asked), Operation::TurnStart(again)) = (&requests[1], &requests[4])
    else {
        unreachable!()
    };
    assert_eq!(asked, again);
    // The user's own identity and the text as written, whole. Nothing else is chosen.
    assert_eq!(
        (
            &asked.intent_id,
            &asked.managed_session_id,
            asked.expected_session_revision,
            asked.prompt.as_str()
        ),
        (&intent, &session, 1, PROMPT)
    );
    let Operation::LeaseGrant(grant) = &requests[3] else {
        unreachable!()
    };
    // Exactly the scope a turn needs: no session scope, no command or file scope.
    assert_eq!(grant.scopes, [ExecutionScope::TurnStart]);
    assert_eq!(
        (grant.ttl_seconds, grant.expected_session_revision),
        (TURN_LEASE_SECONDS, 1)
    );
    assert_ne!(grant.intent_id, intent);
    // The turn runs under that lease: it is not given back while the turn has not ended.
    assert!(world.lock().unwrap().lease.is_some());
    assert_eq!(sessions.stats().leases, 1);

    // Reading it while it runs gives nothing back either.
    world
        .lock()
        .unwrap()
        .advance(&started.id, TurnState::Running, None);
    let sent = peer.requests().len();
    let view = turns.read(started.id.clone(), None, false).unwrap();
    assert_eq!(view.turn.state, TurnState::Running);
    assert_eq!(since(&peer, sent), [EVENTS, STATUS, PENDING]);
    assert!(world.lock().unwrap().lease.is_some());

    // Ended: the read that finds it so gives the lease back, once.
    world.lock().unwrap().advance(
        &started.id,
        TurnState::Completed,
        Some(TurnReason::Completed),
    );
    let sent = peer.requests().len();
    let view = turns.read(started.id.clone(), view.next, true).unwrap();
    assert_eq!(view.turn.state, TurnState::Completed);
    assert_eq!(since(&peer, sent), [EVENTS, STATUS, REVOKE]);
    assert!(world.lock().unwrap().lease.is_none());
    let sent = peer.requests().len();
    turns.read(started.id.clone(), None, true).unwrap();
    assert_eq!(since(&peer, sent), [EVENTS, STATUS]);

    // The next turn is admitted under a lease of its own.
    turns
        .start(uuid(), session.clone(), 1, PROMPT.into())
        .unwrap();
    assert_eq!(sessions.stats().leases, 2);
    // Reading the first turn again, ended as it is, gives nothing back: the lease that
    // is kept now is the one the second turn runs under.
    let sent = peer.requests().len();
    turns.read(started.id.clone(), None, true).unwrap();
    assert_eq!(since(&peer, sent), [EVENTS, STATUS]);
    assert!(world.lock().unwrap().lease.is_some());
    // The same action sent again is the same turn: no lease, no new turn. (The first
    // request asks whether the turn the kept lease is for has ended.)
    let sent = peer.requests().len();
    assert_eq!(
        turns
            .start(intent, session.clone(), 1, PROMPT.into())
            .unwrap(),
        started
    );
    assert_eq!(since(&peer, sent), [BINDING, STATUS, START]);
    assert_eq!(world.lock().unwrap().turns.len(), 2);
}

#[test]
fn a_turn_whose_end_nobody_read_does_not_lend_its_lease_to_the_next_one() {
    let session = session_id();
    let (peer, sessions, turns, world) = authority(World::new(&session));
    let first = turns
        .start(uuid(), session.clone(), 1, PROMPT.into())
        .unwrap();
    // While it runs, another start is the authority's to refuse, and the lease the
    // running turn was admitted under stays where it is.
    let sent = peer.requests().len();
    assert_eq!(
        failed(turns.start(uuid(), session.clone(), 1, PROMPT.into())),
        (SessionFailure::Busy, true)
    );
    assert!(!since(&peer, sent).contains(&REVOKE.to_owned()));
    assert!(world.lock().unwrap().lease.is_some());
    // It ends, and the desktop never reads that it did.
    world
        .lock()
        .unwrap()
        .advance(&first.id, TurnState::Completed, Some(TurnReason::Completed));
    let before = world.lock().unwrap().lease.clone().unwrap().lease_id;
    let sent = peer.requests().len();
    turns
        .start(uuid(), session.clone(), 1, PROMPT.into())
        .unwrap();
    // The lease kept for the ended turn is given back first, and this turn is admitted
    // under one of its own: its time budget is its own, not what was left of another's.
    assert_eq!(
        since(&peer, sent),
        [BINDING, STATUS, REVOKE, START, GATE, GRANT, START]
    );
    assert_eq!(sessions.stats().leases, 2);
    assert_ne!(
        world.lock().unwrap().lease.clone().unwrap().lease_id,
        before
    );
}

#[test]
fn a_lost_start_answer_is_settled_by_the_same_intent_and_never_starts_a_second_turn() {
    let session = session_id();
    // The authority admits the turn and its answer never leaves.
    let (peer, sessions, turns, world) =
        authority(World::new(&session).fault(START, 2, Fault::AnswerLost));
    let intent = uuid();
    assert_eq!(
        failed(turns.start(intent.clone(), session.clone(), 1, PROMPT.into())),
        (SessionFailure::Unavailable, true)
    );
    // The turn may be running under the lease: it is kept, not revoked.
    assert_eq!(peer.methods(), [BINDING, START, GATE, GRANT, START]);
    assert_eq!(world.lock().unwrap().turns.len(), 1);
    assert!(world.lock().unwrap().lease.is_some());
    let admitted = world.lock().unwrap().turns[0]
        .record
        .managed_turn_id
        .clone();

    // The same intent: the authority's receipt. No second turn, no second lease.
    let sent = peer.requests().len();
    let receipt = turns
        .start(intent, session.clone(), 1, PROMPT.into())
        .unwrap();
    assert_eq!(receipt.id, admitted);
    assert_eq!(since(&peer, sent), [BINDING, START]);
    assert_eq!(
        (world.lock().unwrap().turns.len(), sessions.stats().leases),
        (1, 1)
    );
    // A new action while that turn runs is refused by the authority: one at a time.
    let sent = peer.requests().len();
    assert_eq!(
        failed(turns.start(uuid(), session.clone(), 1, PROMPT.into())),
        (SessionFailure::Busy, true)
    );
    assert_eq!(since(&peer, sent), [BINDING, STATUS, START]);
    assert_eq!(world.lock().unwrap().turns.len(), 1);
    // The lease learned from the receipt is given back when the turn has ended.
    world.lock().unwrap().advance(
        &admitted,
        TurnState::Interrupted,
        Some(TurnReason::UserCancel),
    );
    turns.read(admitted, None, false).unwrap();
    assert!(world.lock().unwrap().lease.is_none());
}

#[test]
fn a_refused_start_leaves_no_lease_and_a_lease_somebody_else_holds_is_never_touched() {
    let session = session_id();
    // The gate names something a lease cannot fix: nothing is granted.
    for (reason, code, retry) in [
        (
            GateReason::BindingDetached,
            SessionFailure::RuntimeUnavailable,
            false,
        ),
        (
            GateReason::AttachmentRuntimeMismatch,
            SessionFailure::RuntimeUnavailable,
            false,
        ),
        (
            GateReason::RuntimeNotReady,
            SessionFailure::RuntimeUnavailable,
            false,
        ),
        (GateReason::BindingNotFound, SessionFailure::NotBound, false),
        (
            GateReason::SessionNotPrepared,
            SessionFailure::NotAllowed,
            false,
        ),
        (GateReason::ActiveTurnExists, SessionFailure::Busy, true),
    ] {
        let mut world = World::new(&session);
        world.gate = reason;
        let mut theirs = lease(&session);
        theirs.scopes = vec![ExecutionScope::TurnStart];
        world.lease = Some(theirs.clone());
        let (peer, sessions, turns, world) = authority(world);
        assert_eq!(
            failed(turns.start(uuid(), session.clone(), 1, PROMPT.into())),
            (code, retry),
            "{reason:?}"
        );
        assert_eq!(world.lock().unwrap().lease, Some(theirs));
        assert_eq!(sessions.stats().leases, 0);
        assert!(!peer.methods().iter().any(|m| m == GRANT || m == REVOKE));
    }
    // Granted, and still refused for another reason: the lease does not outlive the refusal.
    for refusal in [ControlError::StaleRevision, ControlError::IntentConflict] {
        let (peer, _, turns, world) =
            authority(World::new(&session).fault(START, 2, Fault::Refuse(refusal)));
        assert!(
            turns
                .start(uuid(), session.clone(), 1, PROMPT.into())
                .is_err()
        );
        assert_eq!(peer.methods(), [BINDING, START, GATE, GRANT, START, REVOKE]);
        assert!(world.lock().unwrap().lease.is_none());
    }
    // A lease somebody else holds that allows a turn: used as it is, and still theirs
    // when the turn has ended.
    let mut world = World::new(&session);
    let mut theirs = lease(&session);
    theirs.scopes = vec![ExecutionScope::TurnStart, ExecutionScope::CommandExecute];
    world.lease = Some(theirs.clone());
    let (peer, sessions, turns, world) = authority(world);
    let started = turns
        .start(uuid(), session.clone(), 1, PROMPT.into())
        .unwrap();
    assert_eq!(peer.methods(), [BINDING, START]);
    world.lock().unwrap().advance(
        &started.id,
        TurnState::Completed,
        Some(TurnReason::Completed),
    );
    turns.read(started.id, None, false).unwrap();
    assert_eq!(world.lock().unwrap().lease, Some(theirs));
    assert_eq!(sessions.stats().leases, 0);
    assert!(!peer.methods().iter().any(|m| m == GRANT || m == REVOKE));
    // One that does not: the turn is denied and the lease is neither widened nor replaced.
    let mut world = World::new(&session);
    let mut narrow = lease(&session);
    narrow.scopes = vec![ExecutionScope::SessionResume];
    world.lease = Some(narrow.clone());
    let (peer, _, turns, world) = authority(world);
    assert_eq!(
        failed(turns.start(uuid(), session.clone(), 1, PROMPT.into())),
        (SessionFailure::PermissionDenied, false)
    );
    assert_eq!(peer.methods(), [BINDING, START, GATE]);
    assert_eq!(world.lock().unwrap().lease, Some(narrow));
}

#[test]
fn a_reading_carries_the_answer_and_the_lifecycle_and_nothing_private() {
    let session = session_id();
    let (_peer, _, turns, world) = authority(World::new(&session));
    let started = turns
        .start(uuid(), session.clone(), 1, PROMPT.into())
        .unwrap();
    {
        let mut world = world.lock().unwrap();
        world.advance(&started.id, TurnState::Running, None);
        world.turns[0].events = vec![
            (StreamKind::TurnStarted, None),
            (StreamKind::ReasoningSummaryDelta, Some(REASONING)),
            (StreamKind::ReasoningSummaryPart, None),
            (StreamKind::AssistantMessageDelta, Some("Hello, ")),
            (StreamKind::AssistantMessageDelta, Some("<b>world</b>")),
            (StreamKind::AssistantMessageCompleted, None),
            // The authority puts no text on these today. If it ever does, it is the
            // detail of a mediated action, and it does not cross either.
            (StreamKind::CommandProposed, Some(ACTION_DETAIL)),
            (StreamKind::ApprovalRequired, Some(ACTION_DETAIL)),
            (StreamKind::ApprovalResolved, Some("accepted")),
            (StreamKind::CommandStarted, Some(ACTION_DETAIL)),
            (StreamKind::CommandOutputDelta, Some(COMMAND_OUTPUT)),
            (StreamKind::CommandCompleted, Some(ACTION_DETAIL)),
            (StreamKind::FileChangeProposed, Some(ACTION_DETAIL)),
            (StreamKind::ApprovalResolved, Some("not a state")),
            (StreamKind::TurnCompleted, None),
        ];
    }
    let view = turns.read(started.id.clone(), None, false).unwrap();
    use TurnEventKind as K;
    let shown: Vec<(u64, K, Option<&str>)> = view
        .events
        .iter()
        .map(|e| (e.sequence.get(), e.kind, e.text.as_deref()))
        .collect();
    assert_eq!(
        shown,
        [
            (1, K::Started, None),
            // That the model reasoned is shown. What it reasoned is not.
            (2, K::Thinking, None),
            (3, K::Thinking, None),
            (4, K::Text, Some("Hello, ")),
            // Text is carried as text: making it safe to show is the window's rendering.
            (5, K::Text, Some("<b>world</b>")),
            (6, K::MessageEnd, None),
            (7, K::CommandProposed, None),
            (8, K::ApprovalRequired, None),
            (9, K::ApprovalResolved, Some("accepted")),
            (10, K::CommandStarted, None),
            // A command's output is not part of this projection.
            (11, K::CommandOutput, None),
            (12, K::CommandCompleted, None),
            (13, K::FileChangeProposed, None),
            // Only a state the contract names is passed on.
            (14, K::ApprovalResolved, None),
            (15, K::Completed, None),
        ]
    );
    assert_eq!(view.next.unwrap().get(), 15);
    assert!(!view.more && !view.gap);
    // The stream said "completed"; the record still says running, and the record is what
    // the turn is.
    assert_eq!(
        (view.turn.state, view.turn.cancellable),
        (TurnState::Running, true)
    );

    let wire = serde_json::to_value(&view).unwrap();
    assert_eq!(
        wire["turn"],
        json!({
            "id": started.id, "session": session, "state": "running", "revision": 2,
            "started_at": "2026-10-08T00:00:00Z", "updated_at": "2026-10-08T00:00:09Z",
            "ended_at": null, "reason": null, "output": true, "cancellable": true,
        })
    );
    // Cursors are decimal text, never numbers.
    assert_eq!(wire["next"], json!("15"));
    assert_eq!(
        wire["events"][3],
        json!({"sequence": "4", "kind": "text", "text": "Hello, "})
    );
    let text = wire.to_string();
    let lease_id = world.lock().unwrap().lease.clone().unwrap().lease_id;
    let record = serde_json::to_value(&world.lock().unwrap().turns[0].record).unwrap();
    for private in [
        REASONING,
        COMMAND_OUTPUT,
        ACTION_DETAIL,
        THREAD,
        PROVIDER_TURN,
        "PROMPT_PRIVATE",
        lease_id.as_str(),
        record["runtime_instance_id"].as_str().unwrap(),
        "codex",
        "lease",
        "runtime_instance",
        "approval_id",
    ] {
        assert!(!text.contains(private), "{private} crossed to the window");
    }
}

#[test]
fn a_reading_resumes_from_its_cursor_without_a_gap_or_a_repeat() {
    let session = session_id();
    let (peer, _, turns, world) = authority(World::new(&session));
    let started = turns
        .start(uuid(), session.clone(), 1, PROMPT.into())
        .unwrap();
    {
        let mut world = world.lock().unwrap();
        world.advance(&started.id, TurnState::Running, None);
        world.turns[0].events = (0..120)
            .map(|_| (StreamKind::AssistantMessageDelta, Some("x")))
            .collect();
    }
    // Bounded pages, each from where the last ended, until nothing is left.
    let (mut cursor, mut read, mut pages) = (None, Vec::new(), 0);
    loop {
        let view = turns.read(started.id.clone(), cursor, false).unwrap();
        assert!(view.events.len() <= 50);
        pages += 1;
        read.extend(view.events.iter().map(|e| e.sequence.get()));
        cursor = view.next;
        if !view.more {
            break;
        }
    }
    assert_eq!(pages, 3);
    assert_eq!(read, (1..=120).collect::<Vec<u64>>());
    let Some(Operation::TurnEvents(last)) = peer
        .requests()
        .into_iter()
        .rev()
        .find(|op| matches!(op, Operation::TurnEvents(_)))
    else {
        unreachable!()
    };
    assert_eq!((last.after_sequence.unwrap().get(), last.limit), (100, 50));
    // Nothing new: the cursor comes back unchanged and nothing is repeated.
    let idle = turns.read(started.id.clone(), cursor, false).unwrap();
    assert!(idle.events.is_empty());
    assert_eq!(idle.next, cursor);

    // The authority dropped what the reader had not read yet: said, not papered over.
    world.lock().unwrap().turns[0].dropped = 110;
    let behind = turns
        .read(started.id.clone(), "40".parse().ok(), false)
        .unwrap();
    assert!(behind.gap);
    assert_eq!(behind.events[0].kind, TurnEventKind::Gap);
    assert_eq!(
        behind.events[1..]
            .iter()
            .map(|e| e.sequence.get())
            .collect::<Vec<_>>(),
        (111..=120).collect::<Vec<u64>>()
    );
    // Nothing kept at all, as after an authority restart: the record is still read, the
    // loss is said, and the cursor stays where it was instead of following the marker
    // the authority puts right after it. Read again, it says the same and no more.
    {
        let mut world = world.lock().unwrap();
        world.turns[0].retained = false;
        world.advance(
            &started.id,
            TurnState::Indeterminate,
            Some(TurnReason::AuthorityRestarted),
        );
    }
    for _ in 0..2 {
        let gone = turns.read(started.id.clone(), cursor, true).unwrap();
        assert!(gone.gap && !gone.turn.output && !gone.more);
        assert!(gone.events.is_empty());
        assert_eq!(gone.next, cursor);
        assert_eq!(gone.turn.state, TurnState::Indeterminate);
    }
    // A turn the authority does not have.
    assert_eq!(
        failed(turns.read(ManagedTurnId::parse(uuid().as_str()).unwrap(), None, false)),
        (SessionFailure::NotFound, false)
    );
}

#[test]
fn a_turn_that_has_put_nothing_out_yet_is_not_a_gap_and_its_first_event_is_not_stepped_over() {
    let session = session_id();
    let (_peer, _, turns, world) = authority(World::new(&session));
    let started = turns
        .start(uuid(), session.clone(), 1, PROMPT.into())
        .unwrap();
    // Admitted and silent. The authority has no stream for it and marks a gap right
    // after the cursor; nothing was lost, and that mark is not a place to read from.
    let silent = turns.read(started.id.clone(), None, false).unwrap();
    assert!(silent.events.is_empty() && !silent.gap && !silent.more);
    assert_eq!(silent.next, None);
    assert_eq!(silent.turn.state, TurnState::Starting);
    {
        let mut world = world.lock().unwrap();
        world.advance(&started.id, TurnState::Running, None);
        world.turns[0].events = vec![
            (StreamKind::TurnStarted, None),
            (StreamKind::AssistantMessageDelta, Some("first")),
        ];
    }
    let first = turns.read(started.id.clone(), silent.next, false).unwrap();
    assert_eq!(
        first
            .events
            .iter()
            .map(|e| (e.sequence.get(), e.kind))
            .collect::<Vec<_>>(),
        [(1, TurnEventKind::Started), (2, TurnEventKind::Text)]
    );
    assert!(!first.gap);
}

#[test]
fn a_turn_that_ends_between_its_events_and_its_record_is_shown_ended_with_all_of_its_output() {
    let session = session_id();
    let (peer, _, turns, world) = authority(World::new(&session));
    let started = turns
        .start(uuid(), session.clone(), 1, PROMPT.into())
        .unwrap();
    {
        let mut world = world.lock().unwrap();
        world.advance(&started.id, TurnState::Running, None);
        world.turns[0].events = vec![
            (StreamKind::TurnStarted, None),
            (StreamKind::AssistantMessageDelta, Some("one ")),
        ];
        // The rest arrives, and the turn completes, right after the events are read.
        world.ends_after_read = Some(vec![
            (StreamKind::AssistantMessageDelta, Some("two")),
            (StreamKind::TurnCompleted, None),
        ]);
    }
    let from = peer.requests().len();
    let view = turns.read(started.id.clone(), None, true).unwrap();
    // Ended, and nothing more to read: so everything it put out is in this reading. A
    // follower that stops here has the whole answer.
    assert_eq!(view.turn.state, TurnState::Completed);
    assert!(!view.more);
    assert_eq!(
        view.events
            .iter()
            .filter_map(|e| e.text.as_deref())
            .collect::<String>(),
        "one two"
    );
    assert_eq!(view.events.last().unwrap().kind, TurnEventKind::Completed);
    // The events were asked for once more, from the same cursor, and only because the
    // record said the turn had ended.
    assert_eq!(
        since(&peer, from)
            .iter()
            .filter(|m| m.as_str() == EVENTS)
            .count(),
        2
    );
    let from = peer.requests().len();
    turns.read(started.id.clone(), view.next, true).unwrap();
    assert_eq!(
        since(&peer, from)
            .iter()
            .filter(|m| m.as_str() == EVENTS)
            .count(),
        1
    );
}

#[test]
fn following_waits_at_a_pace_answers_as_soon_as_something_is_there_and_stops_on_shutdown() {
    let session = session_id();
    let (peer, sessions, turns, world) = authority(World::new(&session));
    let started = turns
        .start(uuid(), session.clone(), 1, PROMPT.into())
        .unwrap();
    world
        .lock()
        .unwrap()
        .advance(&started.id, TurnState::Running, None);
    let looks = |peer: &Peer| peer.methods().iter().filter(|m| *m == EVENTS).count();

    // Nothing new: the read waits its bounded time and looks only a few times.
    let (before, began) = (looks(&peer), Instant::now());
    let view = turns.read(started.id.clone(), None, true).unwrap();
    let (elapsed, asked) = (began.elapsed(), looks(&peer) - before);
    assert!(view.events.is_empty());
    assert!(
        elapsed >= Duration::from_millis(1400) && elapsed < Duration::from_millis(2600),
        "{elapsed:?}"
    );
    assert!((4..=9).contains(&asked), "{asked} looks in {elapsed:?}");
    // Not asked to wait: one look.
    let before = looks(&peer);
    turns.read(started.id.clone(), None, false).unwrap();
    assert_eq!(looks(&peer) - before, 1);

    // Something arrives while it waits: it is answered within a pace, not at the limit.
    let writer = thread::spawn({
        let world = world.clone();
        move || {
            thread::sleep(Duration::from_millis(350));
            world.lock().unwrap().turns[0]
                .events
                .push((StreamKind::AssistantMessageDelta, Some("late")));
        }
    });
    let began = Instant::now();
    let view = turns.read(started.id.clone(), None, true).unwrap();
    writer.join().unwrap();
    assert_eq!(view.events.len(), 1);
    assert!(began.elapsed() < Duration::from_millis(1000));
    // A turn that has ended is answered at once.
    world.lock().unwrap().advance(
        &started.id,
        TurnState::Completed,
        Some(TurnReason::Completed),
    );
    let began = Instant::now();
    turns.read(started.id.clone(), view.next, true).unwrap();
    assert!(began.elapsed() < Duration::from_millis(400));

    // Shutdown ends a read that is waiting, and refuses the next.
    let again = turns
        .start(uuid(), session.clone(), 1, PROMPT.into())
        .unwrap();
    world
        .lock()
        .unwrap()
        .advance(&again.id, TurnState::Running, None);
    let waiting = thread::spawn({
        let (turns, id) = (turns.clone(), again.id.clone());
        move || turns.read(id, None, true)
    });
    thread::sleep(Duration::from_millis(300));
    let began = Instant::now();
    sessions.shutdown();
    assert_eq!(
        failed(waiting.join().unwrap()),
        (SessionFailure::Unavailable, true)
    );
    assert!(began.elapsed() < Duration::from_millis(600));
    assert_eq!(
        failed(turns.read(again.id, None, false)),
        (SessionFailure::Unavailable, true)
    );
}

#[test]
fn cancel_is_an_intent_for_one_turn_at_the_revision_that_was_shown() {
    let session = session_id();
    let (peer, _, turns, world) =
        authority(World::new(&session).fault(CANCEL, 1, Fault::AnswerLost));
    let started = turns
        .start(uuid(), session.clone(), 1, PROMPT.into())
        .unwrap();
    world
        .lock()
        .unwrap()
        .advance(&started.id, TurnState::Running, None);
    let intent = uuid();
    // The answer was lost: unknown, and nothing is claimed about the turn.
    assert_eq!(
        failed(turns.cancel(intent.clone(), started.id.clone(), 2)),
        (SessionFailure::Unavailable, true)
    );
    let Some(Operation::TurnCancel(asked)) = peer.requests().last().cloned() else {
        unreachable!()
    };
    assert_eq!(
        (
            &asked.intent_id,
            &asked.managed_turn_id,
            asked.expected_revision
        ),
        (&intent, &started.id, 2)
    );
    // What the turn is comes from reading it.
    assert_eq!(
        turns
            .read(started.id.clone(), None, false)
            .unwrap()
            .turn
            .state,
        TurnState::Interrupted
    );
    // A turn that has ended, a stale revision, an unknown turn, an impossible revision.
    assert_eq!(
        failed(turns.cancel(uuid(), started.id.clone(), 3)),
        (SessionFailure::NotAllowed, false)
    );
    let again = turns
        .start(uuid(), session.clone(), 1, PROMPT.into())
        .unwrap();
    assert_eq!(
        failed(turns.cancel(uuid(), again.id.clone(), 7)),
        (SessionFailure::Changed, false)
    );
    assert_eq!(
        failed(turns.cancel(uuid(), ManagedTurnId::parse(uuid().as_str()).unwrap(), 1)),
        (SessionFailure::NotFound, false)
    );
    let sent = peer.requests().len();
    assert_eq!(
        failed(turns.cancel(uuid(), again.id.clone(), 0)),
        (SessionFailure::Invalid, false)
    );
    assert_eq!(peer.requests().len(), sent);
    // A cancel is a request for the authority, never a lease operation of this desktop.
    assert_eq!(
        turns.cancel(uuid(), again.id.clone(), 1).unwrap().id,
        again.id
    );
    assert_eq!(peer.methods().last().map(String::as_str), Some(CANCEL));
}

#[test]
fn the_turns_of_a_session_are_found_in_the_durable_stream_and_read_from_the_authority() {
    let session = session_id();
    let other = session_id();
    let mut world = World::new(&session);
    let mut theirs = lease(&session);
    theirs.scopes = vec![ExecutionScope::TurnStart];
    world.lease = Some(theirs);
    let (peer, _, turns, world) = authority(world);
    // Nothing admitted yet.
    let empty = turns.list(session.clone()).unwrap();
    assert!(empty.turns.is_empty() && empty.runnable && !empty.partial);
    assert_eq!(empty.head.unwrap().get(), 41);
    assert_eq!(
        serde_json::to_value(&empty.session).unwrap(),
        json!({
            "id": session, "state": "prepared", "revision": 1,
            "binding": "bound_attached", "workspace": "/work/space",
            "mode": "inference",
        })
    );

    // Three turns of this session and two of another one, each ended before the next.
    let mut mine = Vec::new();
    for owner in [&session, &other, &session, &other, &session] {
        let started = turns
            .start(uuid(), owner.clone(), 1, PROMPT.into())
            .unwrap();
        world.lock().unwrap().advance(
            &started.id,
            TurnState::Completed,
            Some(TurnReason::Completed),
        );
        if owner == &session {
            mine.push(started.id);
        }
    }
    world.lock().unwrap().audit.push((
        "authority.managed_turn.completed",
        session.clone(),
        mine[0].clone(),
    ));
    let sent = peer.requests().len();
    let listed = turns.list(session.clone()).unwrap();
    // Newest first, this session's only, each as the authority holds it.
    mine.reverse();
    assert_eq!(
        listed
            .turns
            .iter()
            .map(|t| t.id.clone())
            .collect::<Vec<_>>(),
        mine
    );
    assert!(
        listed
            .turns
            .iter()
            .all(|t| t.session == session && t.state == TurnState::Completed && !t.cancellable)
    );
    assert!(listed.runnable);
    let read = since(&peer, sent);
    assert_eq!(read.iter().filter(|m| *m == AUDIT).count(), 1);
    assert_eq!(read.iter().filter(|m| *m == STATUS).count(), 3);
    // Only what was appended since is read again.
    let started = turns
        .start(uuid(), session.clone(), 1, PROMPT.into())
        .unwrap();
    let sent = peer.requests().len();
    let listed = turns.list(session.clone()).unwrap();
    let Some(Operation::AuditRead(asked)) = peer
        .requests()
        .split_off(sent)
        .into_iter()
        .find(|op| matches!(op, Operation::AuditRead(_)))
    else {
        unreachable!()
    };
    assert_eq!(
        (asked.after_sequence.unwrap().get(), asked.limit),
        (6, MAX_PAGE)
    );
    assert_eq!(listed.turns[0].id, started.id);
    // A turn of its own is running: another is not offered.
    assert!(listed.turns[0].cancellable && !listed.runnable);
    // The other session sees its own two and nothing of this one.
    let theirs = turns.list(other.clone()).unwrap();
    assert_eq!(theirs.turns.len(), 2);
    assert!(theirs.turns.iter().all(|t| t.session == other));
    // No identity but the session's and the turns' crosses.
    let wire = serde_json::to_string(&listed).unwrap();
    for private in [THREAD, PROVIDER_TURN, "lease", "codex", "runtime_instance"] {
        assert!(!wire.contains(private), "{private}");
    }
}

#[test]
fn a_long_durable_stream_is_read_from_a_bounded_distance_behind_its_head_and_no_further() {
    let session = session_id();
    let (peer, _, turns, world) = authority(World::new(&session));
    let filler = |world: &Arc<Mutex<World>>, count: usize| {
        let mut world = world.lock().unwrap();
        let turn = ManagedTurnId::parse(uuid().as_str()).unwrap();
        for _ in 0..count {
            world
                .audit
                .push(("authority.runtime.observed", session.clone(), turn.clone()));
        }
    };
    let run = || {
        let started = turns
            .start(uuid(), session.clone(), 1, PROMPT.into())
            .unwrap();
        world.lock().unwrap().advance(
            &started.id,
            TurnState::Completed,
            Some(TurnReason::Completed),
        );
        started.id
    };
    let audits = |from: usize| -> Vec<u64> {
        peer.requests()
            .split_off(from)
            .into_iter()
            .filter_map(|op| match op {
                Operation::AuditRead(p) => Some(p.after_sequence.map_or(0, Sequence::get)),
                _ => None,
            })
            .collect()
    };
    // An old turn, a long stretch of other events, a recent turn, and some more.
    let old = run();
    filler(&world, 2500);
    let recent = run();
    filler(&world, 499);
    *peer.head.lock().unwrap() = 3001;

    let from = peer.requests().len();
    let listed = turns.list(session.clone()).unwrap();
    // The last two thousand events, in forty pages, and not one read more.
    let asked = audits(from);
    assert_eq!((asked.len(), asked[0], asked[39]), (40, 1001, 2951));
    // What lies before them was not read, and that is said rather than hidden.
    assert!(listed.partial);
    assert_eq!(
        listed.turns.iter().map(|t| &t.id).collect::<Vec<_>>(),
        [&recent]
    );
    assert!(!listed.turns.iter().any(|t| t.id == old));
    // Nothing appended: nothing of the stream is read again.
    let from = peer.requests().len();
    turns.list(session.clone()).unwrap();
    assert_eq!(audits(from), [] as [u64; 0]);

    // A long silence: far more was appended than one read covers. The newest are read,
    // in the same bounded work, instead of the oldest unread ones.
    let skipped = run();
    filler(&world, 2600);
    let newest = run();
    *peer.head.lock().unwrap() = 5603;
    let from = peer.requests().len();
    let listed = turns.list(session.clone()).unwrap();
    let asked = audits(from);
    assert_eq!((asked.len(), asked[0]), (40, 3603));
    assert!(listed.partial);
    assert_eq!(
        listed.turns.iter().map(|t| &t.id).collect::<Vec<_>>(),
        [&newest, &recent]
    );
    assert!(!listed.turns.iter().any(|t| t.id == skipped));
}

#[test]
fn a_stream_or_a_record_out_of_contract_fails_the_list_instead_of_being_shown() {
    let session = session_id();
    let other = session_id();
    // The stream names a turn for this session whose record belongs to another one.
    let mut world = World::new(&session);
    let mut theirs = lease(&session);
    theirs.scopes = vec![ExecutionScope::TurnStart];
    world.lease = Some(theirs);
    let (_peer, _, turns, world) = authority(world);
    let started = turns
        .start(uuid(), other.clone(), 1, PROMPT.into())
        .unwrap();
    world.lock().unwrap().audit.push((
        "authority.managed_turn.admitted",
        session.clone(),
        started.id,
    ));
    assert_eq!(
        failed(turns.list(session.clone())),
        (SessionFailure::Internal, false)
    );

    // A stream that does not move forward from its cursor.
    let stuck = session_id();
    let turn = ManagedTurnId::parse(uuid().as_str()).unwrap();
    let peer = Peer::canonical({
        let (stuck, turn) = (stuck.clone(), turn.clone());
        move |op| match op {
            Operation::AuditRead(p) => answer(Ok(ResultValue::Audit {
                events: vec![envelope(
                    &p.stream_id,
                    1,
                    "authority.managed_turn.admitted",
                    &stuck,
                    &turn,
                    None,
                )],
                next: "1".parse().ok(),
            })),
            other => panic!("{other:?}"),
        }
    });
    let turns = TurnService::new(peer.service());
    assert_eq!(failed(turns.list(stuck)), (SessionFailure::Internal, false));
    // Bounded: it gave up on the second page instead of reading it forty times.
    assert_eq!(peer.methods().len(), 2);
}

#[test]
fn an_input_the_protocol_cannot_carry_is_refused_before_anything_is_asked() {
    let session = session_id();
    let (peer, sessions, turns, _) = authority(World::new(&session));
    for prompt in [String::new(), "x".repeat(8193), "é".repeat(4097)] {
        assert_eq!(
            failed(turns.start(uuid(), session.clone(), 1, prompt)),
            (SessionFailure::Invalid, false)
        );
    }
    for revision in [0, 3] {
        assert_eq!(
            failed(turns.start(uuid(), session.clone(), revision, PROMPT.into())),
            (SessionFailure::Invalid, false)
        );
    }
    assert!(peer.requests().is_empty());
    assert_eq!(sessions.stats().connections, 0);
    // Exactly the bound is carried, whole.
    let exact = "x".repeat(8192);
    turns.start(uuid(), session, 1, exact.clone()).unwrap();
    let Operation::TurnStart(asked) = &peer.requests()[1] else {
        unreachable!()
    };
    assert_eq!(asked.prompt, exact);
}

#[test]
fn the_window_s_fixture_has_exactly_the_shape_of_this_projection() {
    let session = session_id();
    let (_peer, _, turns, world) = authority(World::new(&session));
    let started = turns
        .start(uuid(), session.clone(), 1, PROMPT.into())
        .unwrap();
    {
        let mut world = world.lock().unwrap();
        world.advance(&started.id, TurnState::Running, None);
        world.turns[0].events = vec![(StreamKind::AssistantMessageDelta, Some("x"))];
    }
    let mut view =
        serde_json::to_value(turns.read(started.id.clone(), None, false).unwrap()).unwrap();
    view["approval"] = serde_json::to_value(ApprovalNotice {
        id: ManagedApprovalId::parse(uuid().as_str()).unwrap(),
        kind: ApprovalKind::CommandExecution,
        expires_at: serde_json::from_value(json!("2026-10-09T10:15:00Z")).unwrap(),
    })
    .unwrap();
    let list = serde_json::to_value(turns.list(session).unwrap()).unwrap();
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/turn-view-v2.json")).unwrap();
    let keys = |v: &serde_json::Value| -> Vec<String> {
        let mut keys: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    };
    for shown in [&fixture["view"], &fixture["waiting"]] {
        assert_eq!(keys(shown), keys(&view));
        assert_eq!(keys(&shown["turn"]), keys(&view["turn"]));
        for event in shown["events"].as_array().unwrap() {
            assert_eq!(keys(event), keys(&view["events"][0]));
        }
    }
    assert_eq!(
        keys(&fixture["waiting"]["approval"]),
        keys(&view["approval"])
    );
    for shown in [&fixture["list"], &fixture["empty"]] {
        assert_eq!(keys(shown), keys(&list));
        assert_eq!(keys(&shown["session"]), keys(&list["session"]));
        for turn in shown["turns"].as_array().unwrap() {
            assert_eq!(keys(turn), keys(&list["turns"][0]));
        }
        assert_eq!(shown["schema_version"], 3);
    }
    // The position older turns are looked for from, and a page of them.
    let cursor = serde_json::to_value(HistoryCursor {
        stream_id: WireUuid::try_from(
            fixture["list"]["older"]["stream_id"]
                .as_str()
                .unwrap()
                .to_owned(),
        )
        .unwrap(),
        before: "6".parse().unwrap(),
    })
    .unwrap();
    assert_eq!(keys(&fixture["list"]["older"]), keys(&cursor));
    assert!(fixture["empty"]["older"].is_null());
    let page = serde_json::to_value(TurnHistory {
        schema_version: 1,
        session: ManagedSessionId::parse(fixture["history"]["session"].as_str().unwrap()).unwrap(),
        turns: Vec::new(),
        older: None,
    })
    .unwrap();
    for shown in [&fixture["history"], &fixture["history_end"]] {
        assert_eq!(keys(shown), keys(&page));
        assert_eq!(shown["schema_version"], 1);
        for turn in shown["turns"].as_array().unwrap() {
            assert_eq!(keys(turn), keys(&list["turns"][0]));
        }
    }
    assert_eq!(keys(&fixture["history"]["older"]), keys(&cursor));
    assert!(fixture["history_end"]["older"].is_null());
    // Every name the fixture uses for a state or a reason is one the contract has.
    let named = |v: &serde_json::Value| serde_json::from_value::<TurnState>(v.clone()).is_ok();
    assert!(named(&fixture["view"]["turn"]["state"]));
    for turn in fixture["list"]["turns"].as_array().unwrap() {
        assert!(named(&turn["state"]));
        assert!(serde_json::from_value::<TurnReason>(turn["reason"].clone()).is_ok());
    }
}

#[path = "approval_tests.rs"]
mod approvals;
#[path = "history_tests.rs"]
mod history;
