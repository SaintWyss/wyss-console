//! Bounded reads of the durable stream. Nothing here writes.
use super::view::{EVENTS_PAGE, EVENTS_SCHEMA_VERSION, EventRow, EventsPage, project};
use crate::{
    contracts::{
        Sequence, WireUuid,
        control::{AuditRead, ControlError, ResultValue},
    },
    control::client::ControlClient,
    sessions::{SessionError, SessionFailure, SessionService},
};

/// The window's reader of the authority's durable history.
///
/// It has two reads and no other method. Each opens a connection of its own, reads the
/// stream's identity and head from the authority's greeting, and then at most one window
/// of events: the newest, or the one before a position the caller was given. The
/// authority's `audit/read` only reads forward from a position, so a window is asked for
/// by where it begins; the positions are the stream's own and no window is guessed.
///
/// It keeps nothing between two reads: no cursor, no cache, no watch. When to read again
/// is the window's to decide, from the one observation of the durable head the shell
/// already has.
#[derive(Clone)]
pub struct EventService {
    sessions: SessionService,
}

pub(crate) fn position(value: u64) -> Result<Option<Sequence>, SessionError> {
    (value > 0)
        .then(|| value.to_string().parse::<Sequence>())
        .transpose()
        .map_err(|_| SessionError::new(SessionFailure::Internal))
}

impl EventService {
    pub fn new(sessions: SessionService) -> Self {
        Self { sessions }
    }

    /// The newest window of the stream the authority holds now.
    pub fn latest(&self) -> Result<EventsPage, SessionError> {
        let mut client = self.sessions.connect()?;
        let (stream, head) = greeting(&client);
        let page = window(
            &mut |read| client.audit(read),
            stream,
            head,
            head.saturating_sub(EVENTS_PAGE),
            head,
        )?;
        // The head is an event: a stream that has one and shows none is not a stream.
        if head > 0 && page.events.is_empty() {
            return Err(SessionError::of(ControlError::CorrelationError));
        }
        Ok(page)
    }

    /// The window before `before`, in the stream the caller read it from. A position of
    /// another stream, or of a history longer than the one the authority holds, is not
    /// read against this one: the answer is that history changed.
    pub fn older(&self, stream: WireUuid, before: Sequence) -> Result<EventsPage, SessionError> {
        let mut client = self.sessions.connect()?;
        let (current, head) = greeting(&client);
        if !current.as_str().eq_ignore_ascii_case(stream.as_str()) || before.get() > head + 1 {
            return Err(SessionError::new(SessionFailure::Changed));
        }
        let upper = before.get() - 1;
        window(
            &mut |read| client.audit(read),
            current,
            head,
            upper.saturating_sub(EVENTS_PAGE),
            upper,
        )
    }
}

fn greeting(client: &ControlClient) -> (WireUuid, u64) {
    let hello = client.hello_status();
    (
        hello.stream_id.clone(),
        hello.last_sequence.map_or(0, Sequence::get),
    )
}

/// Reads the events after `after` up to `upper`, both positions of `stream`, and answers
/// them newest first.
///
/// The authority may answer fewer events than asked for when they are large, so the
/// window is read in as many requests as it takes, and never more than one per event it
/// may hold. What comes back has to be the stream: every event of `stream`, each one
/// position after the one before. A hole inside what was read is not a stream and is a
/// failed read. The one hole that is said rather than refused is at the beginning: if the
/// first event is not the one asked for, history before it is unavailable. That includes
/// a window none of which is there: it is answered empty, as a gap.
pub(crate) fn window(
    audit: &mut dyn FnMut(AuditRead) -> Result<ResultValue, ControlError>,
    stream: WireUuid,
    head: u64,
    after: u64,
    upper: u64,
) -> Result<EventsPage, SessionError> {
    let mut rows: Vec<EventRow> = Vec::new();
    let (mut cursor, mut gap) = (after, false);
    'read: for _ in 0..EVENTS_PAGE {
        if cursor >= upper {
            break;
        }
        let limit = u16::try_from((upper - cursor).min(EVENTS_PAGE))
            .map_err(|_| SessionError::new(SessionFailure::Internal))?;
        let answer = audit(AuditRead {
            stream_id: stream.clone(),
            after_sequence: position(cursor)?,
            limit,
        })
        .map_err(SessionError::of)?;
        let ResultValue::Audit { events, .. } = answer else {
            return Err(SessionError::of(ControlError::CorrelationError));
        };
        // The greeting said there is history up to `upper`: an empty answer is not it.
        if events.is_empty() || events.len() > usize::from(limit) {
            return Err(SessionError::of(ControlError::CorrelationError));
        }
        for event in events {
            let envelope = serde_json::to_value(&event)
                .map_err(|_| SessionError::new(SessionFailure::Internal))?;
            let row = project(&envelope, &stream).map_err(SessionError::of)?;
            let at = row.sequence.get();
            if at <= cursor {
                return Err(SessionError::of(ControlError::CorrelationError));
            }
            if at != cursor + 1 {
                if !rows.is_empty() {
                    return Err(SessionError::of(ControlError::CorrelationError));
                }
                gap = true;
            }
            // Past the window: where its beginning is missing, the authority counts the
            // events it answers from further on. What lies beyond is not this window,
            // and nothing more of it is there to ask for.
            if at > upper {
                break 'read;
            }
            cursor = at;
            rows.push(row);
        }
    }
    // The window was read to its end, or none of it is there.
    if cursor != upper && !(gap && rows.is_empty()) {
        return Err(SessionError::of(ControlError::CorrelationError));
    }
    rows.reverse();
    let oldest = rows.last().map(|row| row.sequence);
    Ok(EventsPage {
        schema_version: EVENTS_SCHEMA_VERSION,
        stream,
        head: position(head)?,
        // Nothing lies before the first event of a stream, and nothing can be read
        // before a hole.
        older: oldest.filter(|first| first.get() > 1 && !gap),
        events: rows,
        gap,
    })
}
