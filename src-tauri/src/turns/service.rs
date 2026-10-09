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
use super::view::{
    ApprovalNotice, SessionFacts, TURN_PAGE, TurnEvent, TurnList, TurnReceipt, TurnSummary,
    TurnView, VIEW_SCHEMA_VERSION,
};
use crate::{
    contracts::{
        Sequence, WireUuid,
        control::{
            AuditRead, BindingState, ControlError, ControlResult, ControlState, ExecutionScope,
            MAX_PAGE, ResultValue, TurnCancel, TurnEvents, TurnStart, TurnStreamPage,
            validate_prompt,
        },
        identities::{ManagedSessionId, ManagedTurnId},
    },
    control::client::ControlClient,
    sessions::{Grant, Held, SessionError, SessionFailure, SessionService, session_revision},
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
/// Turns remembered for one session, and sessions remembered.
const KEEP_TURNS: usize = 32;
const KEEP_SESSIONS: usize = 64;

/// Which turns the durable stream says each session has. It is an index, not state: a
/// turn listed here is read from the authority before anything is said about it.
#[derive(Default)]
struct Index {
    stream: Option<String>,
    /// Audit sequence the stream was read up to.
    scanned: u64,
    /// A read began after where the one before it ended, or after the beginning.
    partial: bool,
    turns: HashMap<String, VecDeque<ManagedTurnId>>,
    /// Sessions in the order they were first seen: the oldest is forgotten first.
    order: VecDeque<String>,
}
impl Index {
    fn admit(&mut self, session: &str, turn: ManagedTurnId) {
        let key = session.to_ascii_lowercase();
        if !self.turns.contains_key(&key) {
            if self.order.len() == KEEP_SESSIONS
                && let Some(oldest) = self.order.pop_front()
            {
                self.turns.remove(&oldest);
            }
            self.order.push_back(key.clone());
        }
        let turns = self.turns.entry(key).or_default();
        if turns.contains(&turn) {
            return;
        }
        if turns.len() == KEEP_TURNS {
            turns.pop_front();
        }
        turns.push_back(turn);
    }
}

struct Inner {
    sessions: SessionService,
    /// One follower's read at a time.
    reading: Mutex<()>,
    /// One list at a time, and the index they share.
    index: Mutex<Index>,
}

/// Application-lifetime door to managed turns. Cloning shares it. It stops with the
/// `SessionService` it was made from.
#[derive(Clone)]
pub struct TurnService {
    inner: Arc<Inner>,
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
                    scope: ExecutionScope::TurnStart,
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
        let known: Vec<ManagedTurnId> = index
            .turns
            .get(&session.as_str().to_ascii_lowercase())
            .map(|turns| turns.iter().rev().take(TURN_PAGE).cloned().collect())
            .unwrap_or_default();
        let mut turns = Vec::with_capacity(known.len());
        for id in known {
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
            schema_version: VIEW_SCHEMA_VERSION,
            head,
            runnable: record.control_state == ControlState::Prepared
                && binding.state == BindingState::BoundAttached
                && !running,
            session: SessionFacts {
                id: record.managed_session_id,
                state: record.control_state,
                revision: record.revision,
                binding: binding.state,
                workspace: binding.binding.map(|b| b.workspace_root),
            },
            turns,
            partial: index.partial,
        })
    }
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
fn scan(client: &mut ControlClient, index: &mut Index) -> ControlResult<()> {
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
            index.admit(session, turn);
        }
        if next.is_none() {
            break;
        }
    }
    Ok(())
}
