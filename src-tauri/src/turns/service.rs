//! Managed turns over short-lived connections of the shared typed client.
//!
//! **Start and cancel** are intents. They go through `SessionService`, one mutation at a
//! time, and a start is admitted by the authority's execution gate like a binding or a
//! resume. The difference is the lease: a turn runs under the lease it was admitted
//! under for as long as it runs. So the lease the desktop asks for, exactly `turn_start`,
//! is *kept* after the turn was admitted, and given back when a read finds the turn
//! ended. If nobody is there to give it back it expires, which ends the turn: the lease
//! is the turn's time budget. A lease somebody else holds is used as it is and never
//! touched.
//!
//! **Reads** have connections of their own, so a session call never waits behind one.
//! The authority keeps a turn's output as a bounded stream in memory and offers no way
//! to wait on it, so following a turn is a paced read: one bounded page every
//! `FOLLOW_PACE` until something is there or `FOLLOW_WAIT` has passed. Nothing is read
//! while no turn is being followed.
//!
//! **Which turns exist.** The protocol has no list of turns. What it has is the durable
//! event stream, where every admitted turn is recorded with its session. The service
//! reads that stream once, from a bounded distance behind its head, and then only what
//! was appended since. What a turn *is* always comes from `managedSession/turn/status`.
//!
//! **Older turns.** That first read stops a bounded distance behind the head, and the index
//! keeps a few turns per session, so a long history is not all in it. Turns before the
//! newest ones are found by reading the same stream, with the same `audit/read`, in bounded
//! windows going back from a position a list or an earlier page handed out. The index is
//! not consulted and not held: a page costs a bounded number of reads and remembers
//! nothing, so history is never loaded as a whole and following a running turn never waits
//! for it. Nothing is invented for an old turn: what it is comes from the authority, and
//! its output is there only if the authority still holds it.
use super::view::{
    ApprovalNotice, HISTORY_SCHEMA_VERSION, HistoryCursor, LIST_SCHEMA_VERSION, SessionFacts,
    TURN_PAGE, TurnEvent, TurnHistory, TurnList, TurnReceipt, TurnSummary, TurnView,
    VIEW_SCHEMA_VERSION,
};
use crate::{
    contracts::{
        Sequence, WireUuid,
        control::{
            AttachmentProfile, AuditRead, BindingState, ControlError, ControlResult, ControlState,
            ExecutionScope, MAX_PAGE, ResultValue, TurnCancel, TurnEvents, TurnStart,
            TurnStreamPage, validate_prompt,
        },
        identities::{ManagedApprovalId, ManagedSessionId, ManagedTurnId},
    },
    control::client::ControlClient,
    sessions::{
        Grant, Held, SessionError, SessionFailure, SessionMode, SessionService, session_revision,
    },
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, PoisonError},
    thread,
    time::{Duration, Instant},
};

/// How long a turn started from the desktop may run: the lifetime of the lease it is
/// admitted under. The authority ends the turn when that lease expires.
pub(super) const TURN_LEASE_SECONDS: u32 = 1800;
/// Longest one read waits for something new before it answers with what there is.
const FOLLOW_WAIT: Duration = Duration::from_millis(1500);
/// Pause between two looks at a stream that had nothing new.
const FOLLOW_PACE: Duration = Duration::from_millis(200);
/// Stream events asked for at once. The protocol's own bound.
const EVENT_PAGE: u16 = 50;
/// Audit pages one read of the turn index takes at most.
const SCAN_PAGES: usize = 40;
/// How far behind the head the first read of the turn index starts.
const SCAN_EVENTS: u64 = SCAN_PAGES as u64 * MAX_PAGE as u64;
/// Positions of the stream one step back in history looks at, and finishes before it
/// counts what it found: a step that could not be finished is thrown away, never half used.
const HISTORY_STEP: u64 = MAX_PAGE as u64;
/// Audit pages one page of older turns reads at most.
const HISTORY_PAGES: usize = SCAN_PAGES;
/// Turns remembered for one session, and sessions remembered.
const KEEP_TURNS: usize = 32;
const KEEP_SESSIONS: usize = 64;
/// Approvals remembered, newest last. The authority holds one pending at a time.
pub(super) const KEEP_APPROVALS: usize = 16;

/// What a turn's lease is asked with. In inference a turn needs nothing but its own
/// admission. In local tools the model may propose a command or a file change, and the
/// authority accepts one only under the lease the turn was admitted under, carrying that
/// action's scope: so the scopes are asked for with the turn or not at all. Which of the
/// two applies is read from the authority's own record of the attachment, never chosen
/// by the window.
const INFERENCE_SCOPES: &[ExecutionScope] = &[ExecutionScope::TurnStart];
const LOCAL_TOOLS_SCOPES: &[ExecutionScope] = &[
    ExecutionScope::TurnStart,
    ExecutionScope::CommandExecute,
    ExecutionScope::FileChange,
];

/// Which turns the durable stream says each session has. It is an index, not state: a
/// turn listed here is read from the authority before anything is said about it.
#[derive(Default)]
pub(super) struct Index {
    stream: Option<String>,
    /// Audit sequence the stream was read up to.
    scanned: u64,
    /// A read began after where the one before it ended, or after the beginning.
    pub(super) partial: bool,
    /// A turn or a whole session was forgotten to stay within the bounds below.
    lossy: bool,
    /// Each turn with the position of the event that admitted it, oldest first.
    turns: HashMap<String, VecDeque<(ManagedTurnId, u64)>>,
    /// Sessions in the order they were first seen: the oldest is forgotten first.
    order: VecDeque<String>,
    /// The turn admitted last, whichever session it belongs to. The authority runs one
    /// turn at a time, so if any turn is running it is this one.
    last: Option<ManagedTurnId>,
    /// Approvals the stream says were asked for, oldest first.
    pub(super) approvals: VecDeque<ManagedApprovalId>,
}
impl Index {
    fn request(&mut self, approval: ManagedApprovalId) {
        if self.approvals.contains(&approval) {
            return;
        }
        if self.approvals.len() == KEEP_APPROVALS {
            self.approvals.pop_front();
        }
        self.approvals.push_back(approval);
    }
    fn admit(&mut self, session: &str, turn: ManagedTurnId, sequence: u64) {
        self.last = Some(turn.clone());
        let key = session.to_ascii_lowercase();
        if !self.turns.contains_key(&key) {
            if self.order.len() == KEEP_SESSIONS
                && let Some(oldest) = self.order.pop_front()
            {
                self.turns.remove(&oldest);
                self.lossy = true;
            }
            self.order.push_back(key.clone());
        }
        let turns = self.turns.entry(key).or_default();
        if turns.iter().any(|(known, _)| known == &turn) {
            return;
        }
        if turns.len() == KEEP_TURNS {
            turns.pop_front();
            self.lossy = true;
        }
        turns.push_back((turn, sequence));
    }
    /// Where older turns of a session are looked for, when they may exist: the index is
    /// not the whole history if it started late, jumped ahead, or forgot something, or if
    /// it knows more turns than a list shows.
    fn older(&self, more_known: bool, oldest_shown: Option<u64>) -> Option<HistoryCursor> {
        if !(more_known || self.partial || self.lossy) {
            return None;
        }
        // With nothing shown, everything the index looked at is still to be looked through.
        let before = oldest_shown.unwrap_or(self.scanned + 1);
        cursor(self.stream.as_deref()?, before)
    }
}

/// A position of `stream` as the window is handed it. Nothing is before position one.
pub(super) fn cursor(stream: &str, before: u64) -> Option<HistoryCursor> {
    (before > 1).then_some(())?;
    Some(HistoryCursor {
        stream_id: WireUuid::try_from(stream.to_owned()).ok()?,
        before: before.to_string().parse::<Sequence>().ok()?,
    })
}

pub(super) struct Inner {
    pub(super) sessions: SessionService,
    /// One follower's read at a time.
    reading: Mutex<()>,
    /// One list at a time, and the index they share.
    pub(super) index: Mutex<Index>,
}

/// Application-lifetime door to managed turns. Cloning shares it. It stops with the
/// `SessionService` it was made from.
#[derive(Clone)]
pub struct TurnService {
    pub(super) inner: Arc<Inner>,
}
impl TurnService {
    pub fn new(sessions: SessionService) -> Self {
        Self {
            inner: Arc::new(Inner {
                sessions,
                reading: Mutex::new(()),
                index: Mutex::new(Index::default()),
            }),
        }
    }

    /// Asks the authority to start a turn of the session with `prompt` as its one input.
    ///
    /// The prompt goes to the authority as it is and is kept nowhere on this side.
    pub fn start(
        &self,
        intent: WireUuid,
        session: ManagedSessionId,
        expected_revision: u64,
        prompt: String,
    ) -> Result<TurnReceipt, SessionError> {
        session_revision(expected_revision)?;
        if validate_prompt(&prompt).is_err() {
            return Err(SessionError::new(SessionFailure::Invalid));
        }
        let request = TurnStart {
            intent_id: intent,
            managed_session_id: session.clone(),
            expected_session_revision: expected_revision,
            prompt,
        };
        let sessions = &self.inner.sessions;
        sessions.call(|client| {
            sessions.settle(client, &session);
            // The profile the session is attached under decides what the turn can lead
            // to, and with it what its lease has to carry.
            let scopes = match client
                .binding_status(session.clone())
                .map_err(SessionError::of)?
                .attachment_profile
            {
                Some(AttachmentProfile::LocalToolsV1) => LOCAL_TOOLS_SCOPES,
                _ => INFERENCE_SCOPES,
            };
            // A lease kept for an earlier turn is given back once that turn has ended,
            // so this turn is admitted under a lease, and a time budget, of its own.
            if let Some(Held {
                turn: Some(earlier),
            }) = sessions.held(&session)
                && client
                    .turn_status(earlier)
                    .is_ok_and(|turn| turn.state.terminal())
            {
                sessions.release_held(client, &session);
            }
            let turn = sessions.gated(
                client,
                &session,
                expected_revision,
                Grant {
                    scopes,
                    ttl_seconds: TURN_LEASE_SECONDS,
                    hold: true,
                },
                |client| client.turn_start(request.clone()),
            )?;
            sessions.hold_for(&session, &turn.managed_turn_id);
            Ok(TurnReceipt {
                id: turn.managed_turn_id,
            })
        })
    }

    /// Asks the authority to stop a turn at the revision the caller saw. The answer is a
    /// receipt of the request: the turn has stopped when a read says so.
    pub fn cancel(
        &self,
        intent: WireUuid,
        turn: ManagedTurnId,
        expected_revision: u64,
    ) -> Result<TurnReceipt, SessionError> {
        if expected_revision == 0 || expected_revision > i64::MAX as u64 {
            return Err(SessionError::new(SessionFailure::Invalid));
        }
        self.inner.sessions.call(|client| {
            client
                .turn_cancel(TurnCancel {
                    intent_id: intent,
                    managed_turn_id: turn,
                    expected_revision,
                })
                .map(|record| TurnReceipt {
                    id: record.managed_turn_id,
                })
                .map_err(SessionError::of)
        })
    }

    /// One reading of a turn: the stream events after `after`, then its canonical record.
    ///
    /// With `wait`, and while the turn has not ended, the read looks again every
    /// `FOLLOW_PACE` until an event is there or `FOLLOW_WAIT` has passed.
    pub fn read(
        &self,
        turn: ManagedTurnId,
        after: Option<Sequence>,
        wait: bool,
    ) -> Result<TurnView, SessionError> {
        let sessions = &self.inner.sessions;
        let view = {
            let _reading = self
                .inner
                .reading
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let mut client = sessions.connect()?;
            let deadline = Instant::now() + FOLLOW_WAIT;
            let mut page = loop {
                let page = stream(&mut client, &turn, after)?;
                if !wait || !page.events.is_empty() || page.terminal || Instant::now() >= deadline {
                    break page;
                }
                thread::sleep(FOLLOW_PACE);
                if sessions.stopping() {
                    return Err(SessionError::new(SessionFailure::Unavailable));
                }
            };
            // The record is read after the events: what it says is never older than them.
            let status = client
                .turn_status_projection(turn.clone())
                .map_err(SessionError::of)?;
            // It ended between the two: the events are read again, so that a turn shown
            // as ended always comes with a stream that was read after it ended.
            if status.turn.state.terminal() && !page.terminal {
                page = stream(&mut client, &turn, after)?;
            }
            let approval = if status.turn.state.terminal() {
                None
            } else {
                client
                    .approval_pending(turn.clone())
                    .map_err(SessionError::of)?
                    .map(ApprovalNotice::of)
            };
            let next = page.next_sequence.or(after);
            TurnView {
                schema_version: VIEW_SCHEMA_VERSION,
                more: page
                    .latest_sequence
                    .zip(next)
                    .is_some_and(|(latest, next)| next < latest),
                gap: page.gap,
                events: page.events.into_iter().map(TurnEvent::of).collect(),
                next,
                turn: TurnSummary::of(status),
                approval,
            }
        };
        if !view.turn.cancellable {
            self.ended(&view.turn.session, &view.turn.id);
        }
        Ok(view)
    }

    /// The turn has ended: the lease this desktop kept for it is given back.
    fn ended(&self, session: &ManagedSessionId, turn: &ManagedTurnId) {
        let sessions = &self.inner.sessions;
        if !sessions
            .held(session)
            .is_some_and(|held| held.turn.as_ref() == Some(turn))
        {
            return;
        }
        // Without an answer the lease stays on record and is settled or expires.
        let _ = sessions.call(|client| {
            sessions.release_held(client, session);
            Ok(())
        });
    }

    /// Whether a managed turn is running, in any session, read through `client`.
    ///
    /// The authority has no operation that says so. It runs one turn at a time and
    /// records every admission in its durable stream, so the turn admitted last is the
    /// only one that can be running, and its record says whether it is.
    pub fn active(&self, client: &mut ControlClient) -> Result<bool, SessionError> {
        let last = {
            let mut index = self
                .inner
                .index
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            scan(client, &mut index).map_err(SessionError::of)?;
            index.last.clone()
        };
        match last {
            None => Ok(false),
            Some(turn) => client
                .turn_status(turn)
                .map(|turn| !turn.state.terminal())
                .map_err(SessionError::of),
        }
    }

    /// The recent turns of a session, newest first, each as the authority holds it now.
    pub fn list(&self, session: ManagedSessionId) -> Result<TurnList, SessionError> {
        let mut index = self
            .inner
            .index
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut client = self.inner.sessions.connect()?;
        // The head first, then what it describes: the list is never older than it.
        let head = client.hello_status().last_sequence;
        scan(&mut client, &mut index).map_err(SessionError::of)?;
        let record = client.get(session.clone()).map_err(SessionError::of)?;
        let binding = client
            .binding_status(session.clone())
            .map_err(SessionError::of)?;
        let known: Vec<(ManagedTurnId, u64)> = index
            .turns
            .get(&session.as_str().to_ascii_lowercase())
            .map(|turns| turns.iter().rev().cloned().collect())
            .unwrap_or_default();
        let shown: Vec<(ManagedTurnId, u64)> = known.iter().take(TURN_PAGE).cloned().collect();
        let older = index.older(known.len() > shown.len(), shown.last().map(|(_, at)| *at));
        let mut turns = Vec::with_capacity(shown.len());
        for (id, _) in shown {
            let status = client
                .turn_status_projection(id)
                .map_err(SessionError::of)?;
            // The stream named this session for it; the record has to agree.
            if status.turn.managed_session_id != record.managed_session_id {
                return Err(SessionError::of(ControlError::CorrelationError));
            }
            turns.push(TurnSummary::of(status));
        }
        let running = turns.iter().any(|turn| turn.cancellable);
        Ok(TurnList {
            schema_version: LIST_SCHEMA_VERSION,
            head,
            runnable: record.control_state == ControlState::Prepared
                && binding.state == BindingState::BoundAttached
                && !running,
            session: SessionFacts {
                id: record.managed_session_id,
                state: record.control_state,
                revision: record.revision,
                mode: SessionMode::of(&binding),
                binding: binding.state,
                workspace: binding.binding.map(|b| b.workspace_root),
            },
            turns,
            partial: index.partial,
            older,
        })
    }

    /// The turns of `session` before position `before` of `stream`, newest first, at most
    /// one page. The window got that position from a list or an earlier page; a position of
    /// another stream, or beyond the head of this one, is not read against this one: the
    /// answer is that history changed.
    ///
    /// This holds neither the index nor anything else another read could wait for: it
    /// asks the authority for the session, for a bounded stretch of the stream, and for
    /// the turns it finds there.
    pub fn older(
        &self,
        session: ManagedSessionId,
        stream: WireUuid,
        before: Sequence,
    ) -> Result<TurnHistory, SessionError> {
        let mut client = self.inner.sessions.connect()?;
        let (current, head) = {
            let hello = client.hello_status();
            (
                hello.stream_id.clone(),
                hello.last_sequence.map_or(0, Sequence::get),
            )
        };
        if !current.as_str().eq_ignore_ascii_case(stream.as_str()) || before.get() > head + 1 {
            return Err(SessionError::new(SessionFailure::Changed));
        }
        // The session has to exist, and is the one every turn found is checked against.
        let record = client.get(session).map_err(SessionError::of)?;
        let (found, next) = walk(
            &mut client,
            &current,
            &record.managed_session_id,
            before.get(),
        )
        .map_err(SessionError::of)?;
        let mut turns = Vec::with_capacity(found.len());
        for (_, id) in found {
            let status = client
                .turn_status_projection(id)
                .map_err(SessionError::of)?;
            // The stream named this session for it; the record has to agree.
            if status.turn.managed_session_id != record.managed_session_id {
                return Err(SessionError::of(ControlError::CorrelationError));
            }
            turns.push(TurnSummary::of(status));
        }
        Ok(TurnHistory {
            schema_version: HISTORY_SCHEMA_VERSION,
            session: record.managed_session_id,
            turns,
            older: next.and_then(|before| cursor(current.as_str(), before)),
        })
    }
}

/// A turn with the position of the event that admitted it.
type Admission = (u64, ManagedTurnId);
/// What a walk found, newest first, and the position to go on from, if any.
type Walked = (Vec<Admission>, Option<u64>);

/// The turns admitted for `session` at positions before `before`, newest first, at most
/// one page, and where to go on from. Nothing at or after the position it goes on from is
/// looked at again, and nothing before it was looked at: a page that stopped early because
/// it found enough, or because its reads ran out, goes on exactly from where it stopped.
///
/// Positions are read going back in steps, each step forwards from its start, because the
/// stream can only be read forwards. A step counts only once all of it was read, and every
/// event in it has to be the stream's, each one position after the one before: a hole is
/// not history that happens to be empty, it is a failed read.
fn walk(
    client: &mut ControlClient,
    stream: &WireUuid,
    session: &ManagedSessionId,
    before: u64,
) -> ControlResult<Walked> {
    let key = session.as_str().to_ascii_lowercase();
    let mut found: Vec<Admission> = Vec::new();
    // Positions at `settled` and after are done with.
    let mut settled = before;
    let mut pages = 0usize;
    'steps: while settled > 1 && found.len() < TURN_PAGE {
        let start = settled.saturating_sub(1 + HISTORY_STEP);
        let (mut at, mut step) = (start, Vec::new());
        while at < settled - 1 {
            if pages == HISTORY_PAGES {
                // Out of reads, mid-step: what this step found is not used.
                break 'steps;
            }
            pages += 1;
            let limit = u16::try_from((settled - 1 - at).min(u64::from(MAX_PAGE)))
                .map_err(|_| ControlError::InternalError)?;
            let ResultValue::Audit { events, .. } = client.audit(AuditRead {
                stream_id: stream.clone(),
                after_sequence: (at > 0)
                    .then(|| at.to_string().parse::<Sequence>())
                    .transpose()
                    .map_err(|_| ControlError::InternalError)?,
                limit,
            })?
            else {
                return Err(ControlError::InvalidRequest);
            };
            // History up to `settled - 1` exists: an empty or an oversized answer is not it.
            if events.is_empty() || events.len() > usize::from(limit) {
                return Err(ControlError::CorrelationError);
            }
            for event in events {
                let event =
                    serde_json::to_value(&event).map_err(|_| ControlError::InternalError)?;
                let sequence = event["sequence"]
                    .as_str()
                    .and_then(|text| text.parse::<Sequence>().ok())
                    .map(Sequence::get)
                    .ok_or(ControlError::CorrelationError)?;
                let ours = event["stream_id"]
                    .as_str()
                    .is_some_and(|id| id.eq_ignore_ascii_case(stream.as_str()));
                if sequence != at + 1 || !ours {
                    return Err(ControlError::CorrelationError);
                }
                at = sequence;
                if event["event_type"] != "authority.managed_turn.admitted" {
                    continue;
                }
                let (Some(owner), Some(turn)) = (
                    event["session_id"].as_str(),
                    event["payload"]["managed_turn_id"]
                        .as_str()
                        .and_then(|id| ManagedTurnId::parse(id).ok()),
                ) else {
                    return Err(ControlError::CorrelationError);
                };
                if owner.to_ascii_lowercase() == key {
                    step.push((sequence, turn));
                }
            }
        }
        found.extend(step);
        settled = start + 1;
    }
    found.sort_by_key(|(sequence, _)| std::cmp::Reverse(*sequence));
    // More than a page: the page is the newest, and goes on from its oldest turn, so the
    // ones left over are found again rather than lost.
    if found.len() > TURN_PAGE {
        found.truncate(TURN_PAGE);
        settled = found.last().map_or(settled, |(sequence, _)| *sequence);
    }
    Ok((found, (settled > 1).then_some(settled)))
}

/// One page of a turn's stream after `after`.
///
/// The authority holds no stream for a turn that has put nothing out yet, nor for one
/// whose output it no longer keeps, and answers both as a gap with a marker at a position
/// it derives from the cursor. That position is not one in a stream: taking it as the
/// next cursor would step over the first real event. Such a page is returned empty, with
/// the cursor where it was, and is a gap only if the turn has ended.
fn stream(
    client: &mut ControlClient,
    turn: &ManagedTurnId,
    after: Option<Sequence>,
) -> Result<TurnStreamPage, SessionError> {
    let mut page = client
        .turn_events(TurnEvents {
            managed_turn_id: turn.clone(),
            after_sequence: after,
            limit: EVENT_PAGE,
        })
        .map_err(SessionError::of)?;
    if !page.retained {
        page.events.clear();
        page.next_sequence = None;
        page.gap = page.terminal;
    }
    Ok(page)
}

/// Brings the index up to the head of the durable stream: a bounded read of what was
/// appended since the last one, and never of more than the last `SCAN_EVENTS` events.
pub(super) fn scan(client: &mut ControlClient, index: &mut Index) -> ControlResult<()> {
    let (stream, head) = {
        let hello = client.hello_status();
        (
            hello.stream_id.clone(),
            hello.last_sequence.map_or(0, Sequence::get),
        )
    };
    // Another stream, or one shorter than what was read, is another history.
    if index.stream.as_deref() != Some(stream.as_str()) || head < index.scanned {
        *index = Index {
            stream: Some(stream.as_str().to_owned()),
            ..Index::default()
        };
    }
    // Further behind than one read covers, the first time or after a long silence: the
    // newest events are the ones read, and what lies before them is said to be unread.
    if head - index.scanned > SCAN_EVENTS {
        index.scanned = head - SCAN_EVENTS;
        index.partial = true;
    }
    for _ in 0..SCAN_PAGES {
        if index.scanned >= head {
            break;
        }
        let after = (index.scanned > 0)
            .then(|| index.scanned.to_string().parse::<Sequence>())
            .transpose()
            .map_err(|_| ControlError::InternalError)?;
        let ResultValue::Audit { events, next } = client.audit(AuditRead {
            stream_id: stream.clone(),
            after_sequence: after,
            limit: MAX_PAGE,
        })?
        else {
            return Err(ControlError::InvalidRequest);
        };
        if events.is_empty() {
            break;
        }
        for event in events {
            let event = serde_json::to_value(&event).map_err(|_| ControlError::InternalError)?;
            let sequence = event["sequence"]
                .as_str()
                .and_then(|text| text.parse::<Sequence>().ok())
                .map(Sequence::get)
                .ok_or(ControlError::CorrelationError)?;
            // A page that does not move forward from its cursor is not a stream.
            if sequence <= index.scanned {
                return Err(ControlError::CorrelationError);
            }
            index.scanned = sequence;
            if event["event_type"] == "authority.managed_approval.requested" {
                let approval = event["payload"]["managed_approval_id"]
                    .as_str()
                    .and_then(|id| ManagedApprovalId::parse(id).ok())
                    .ok_or(ControlError::CorrelationError)?;
                index.request(approval);
                continue;
            }
            if event["event_type"] != "authority.managed_turn.admitted" {
                continue;
            }
            let (Some(session), Some(turn)) = (
                event["session_id"].as_str(),
                event["payload"]["managed_turn_id"]
                    .as_str()
                    .and_then(|id| ManagedTurnId::parse(id).ok()),
            ) else {
                return Err(ControlError::CorrelationError);
            };
            index.admit(session, turn, sequence);
        }
        if next.is_none() {
            break;
        }
    }
    Ok(())
}
