//! History against a scripted authority: the exact reads a window costs, what crosses to
//! the window and what never does, and what a stream that is not one is answered with.
//! How the real authority's stream behaves is covered by `tests/events_process.rs`.
use super::*;
use crate::{
    contracts::{
        Sequence, WireUuid, control::*, event_envelope::EventEnvelopeV1,
        identities::ManagedSessionId,
    },
    sessions::{
        SessionError, SessionFailure,
        tests::{Peer, Reply, answer, session_id, uuid},
    },
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

/// Provider and authority internals that must never reach the window.
const THREAD: &str = "0199a213-81c0-7800-8aa1-bbab2a035a53";
const PROVIDER_TURN: &str = "provider-turn-PRIVATE";
const LEASE: &str = "5e5e5e5e-5e5e-4e5e-8e5e-5e5e5e5e5e5e";
const INSTANCE: &str = "1c1c1c1c-1c1c-4c1c-8c1c-1c1c1c1c1c1c";
const OPERATION: &str = "0b0b0b0b-0b0b-4b0b-8b0b-0b0b0b0b0b0b";
const FINGERPRINT: &str = "9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f";
const TURN: &str = "77777777-7777-4777-8777-777777777777";
const APPROVAL: &str = "88888888-8888-4888-8888-888888888888";
const PRIVATE: [&str; 9] = [
    THREAD,
    PROVIDER_TURN,
    LEASE,
    INSTANCE,
    OPERATION,
    FINGERPRINT,
    "SECRET_COMMAND",
    "SECRET_REASONING",
    "SECRET_FUTURE",
];

/// One recorded event: its kind, its payload and whether it names a session.
type Entry = (&'static str, Value, bool);

#[derive(Default)]
struct Log {
    entries: Vec<Entry>,
    /// Events an answer holds at most, as when they are large.
    chunk: Option<usize>,
    /// Events at or below this position are not there to read any more.
    lost: u64,
    /// A position that is left out of what is read.
    hole: Option<u64>,
    /// Envelopes are stamped with another stream than the one asked for.
    foreign: bool,
    /// Answers nothing although there is history.
    empty: bool,
}
fn envelope(
    stream: &WireUuid,
    sequence: u64,
    session: &ManagedSessionId,
    (kind, payload, named): &Entry,
) -> EventEnvelopeV1 {
    EventEnvelopeV1::from_value(json!({
        "schema_version": 1, "event_id": uuid(), "event_type": kind,
        "payload_schema_version": 1,
        "timestamp": format!("2026-10-09T10:{:02}:{:02}Z", sequence / 60 % 60, sequence % 60),
        "stream_id": stream, "sequence": sequence.to_string(),
        "session_id": named.then_some(session),
        "source": {"component": "wyss_control", "instance_id": uuid()},
        "correlation_id": uuid(), "causation_id": null,
        "payload": payload,
        "persistence": "durable", "sensitivity": "internal", "redaction": "none",
    }))
    .unwrap()
}
/// An authority whose stream is `log`, and the service that reads it.
fn authority(log: Log) -> (Peer, EventService, Arc<Mutex<Log>>, ManagedSessionId) {
    let session = session_id();
    let log = Arc::new(Mutex::new(log));
    let (shared, named) = (log.clone(), session.clone());
    let peer = Peer::canonical(move |op| {
        let Operation::AuditRead(p) = op else {
            return answer(Err(ControlError::InvalidRequest));
        };
        let log = shared.lock().unwrap();
        let after = p.after_sequence.map_or(0, Sequence::get);
        let stream = if log.foreign {
            uuid()
        } else {
            p.stream_id.clone()
        };
        let limit = log.chunk.unwrap_or(usize::MAX).min(usize::from(p.limit));
        let events: Vec<EventEnvelopeV1> = (after + 1..=log.entries.len() as u64)
            .filter(|at| *at > log.lost && Some(*at) != log.hole && !log.empty)
            .take(limit)
            .map(|at| envelope(&stream, at, &named, &log.entries[at as usize - 1]))
            .collect();
        answer(Ok(ResultValue::Audit { events, next: None }))
    });
    *peer.head.lock().unwrap() = log.lock().unwrap().entries.len() as u64;
    let service = EventService::new(peer.service());
    (peer, service, log, session)
}
fn turns(count: usize) -> Vec<Entry> {
    (0..count)
        .map(|_| {
            (
                "authority.managed_turn.running",
                json!({"managed_turn_id": TURN, "state": "running", "reason": null}),
                true,
            )
        })
        .collect()
}
fn reads(peer: &Peer) -> Vec<(Option<u64>, u16)> {
    peer.requests()
        .into_iter()
        .map(|op| match op {
            Operation::AuditRead(p) => (p.after_sequence.map(Sequence::get), p.limit),
            other => panic!("{other:?}"),
        })
        .collect()
}
fn positions(page: &EventsPage) -> Vec<u64> {
    page.events.iter().map(|row| row.sequence.get()).collect()
}
fn at(value: u64) -> Sequence {
    value.to_string().parse().unwrap()
}
fn failed(result: Result<EventsPage, SessionError>) -> SessionError {
    result.unwrap_err()
}

#[test]
fn the_newest_window_is_read_whole_and_newest_first_in_bounded_requests() {
    // More history than a window: the newest fifty, in one request for exactly them.
    let (peer, service, log, _) = authority(Log {
        entries: turns(120),
        ..Log::default()
    });
    let page = service.latest().unwrap();
    assert_eq!(positions(&page), (71..=120).rev().collect::<Vec<_>>());
    assert_eq!(reads(&peer), [(Some(70), 50)]);
    assert_eq!(
        (page.head, page.older, page.gap),
        (Some(at(120)), Some(at(71)), false)
    );
    assert_eq!(page.schema_version, 1);
    // The authority answers large events a few at a time: the same window, in as many
    // requests as it takes, each for what is still missing and never for more.
    log.lock().unwrap().chunk = Some(7);
    let again = service.latest().unwrap();
    assert_eq!(again, page);
    let asked = &reads(&peer)[1..];
    assert_eq!(asked.len(), 8);
    assert_eq!(asked[0], (Some(70), 50));
    assert_eq!(asked[1], (Some(77), 43));
    assert_eq!(asked[7], (Some(119), 1));

    // Less history than a window: all of it, from its beginning, and that is said.
    let (peer, service, _, _) = authority(Log {
        entries: turns(30),
        ..Log::default()
    });
    let page = service.latest().unwrap();
    assert_eq!(positions(&page), (1..=30).rev().collect::<Vec<_>>());
    assert_eq!(reads(&peer), [(None, 30)]);
    assert_eq!(
        (page.head, page.older, page.gap),
        (Some(at(30)), None, false)
    );

    // No history at all: nothing is asked for.
    let (peer, service, _, _) = authority(Log::default());
    let page = service.latest().unwrap();
    assert!(page.events.is_empty());
    assert_eq!((page.head, page.older, page.gap), (None, None, false));
    assert!(reads(&peer).is_empty());
}

#[test]
fn the_window_before_a_position_is_read_in_the_stream_the_position_came_from() {
    let (peer, service, _, _) = authority(Log {
        entries: turns(120),
        ..Log::default()
    });
    let newest = service.latest().unwrap();
    let stream = newest.stream.clone();
    // The position a window hands back is its oldest event. The window before it ends
    // right below, and neither overlaps the other.
    let middle = service
        .older(stream.clone(), newest.older.unwrap())
        .unwrap();
    assert_eq!(positions(&middle), (21..=70).rev().collect::<Vec<_>>());
    assert_eq!(middle.older, Some(at(21)));
    let first = service.older(stream.clone(), at(21)).unwrap();
    assert_eq!(positions(&first), (1..=20).rev().collect::<Vec<_>>());
    assert_eq!((first.older, first.gap), (None, false));
    assert_eq!(reads(&peer), [(Some(70), 50), (Some(20), 50), (None, 20)]);
    // Before the first event there is nothing, and nothing is asked for.
    let none = service.older(stream.clone(), at(1)).unwrap();
    assert!(none.events.is_empty() && none.older.is_none() && !none.gap);
    assert_eq!(reads(&peer).len(), 3);
    // The stream's identity is not case: the same stream spelled in capitals is it.
    let shouted: WireUuid =
        serde_json::from_value(json!(stream.as_str().to_ascii_uppercase())).unwrap();
    assert_eq!(positions(&service.older(shouted, at(3)).unwrap()), [2, 1]);

    // A position of another stream, or past the history this authority holds, is not
    // read against it: history changed, and no request was made.
    let asked = reads(&peer).len();
    for (stream, before) in [
        (uuid(), at(71)),
        (stream.clone(), at(122)),
        (stream, at(9_000)),
    ] {
        let error = failed(service.older(stream, before));
        assert_eq!((error.code, error.retry), (SessionFailure::Changed, false));
    }
    assert_eq!(reads(&peer).len(), asked);
}

/// One event of every kind the authority emits, with everything it really carries.
fn every_kind() -> Vec<Entry> {
    let turn = |state: &str, reason: Value| {
        json!({
            "managed_turn_id": TURN, "codex_turn_id": PROVIDER_TURN, "lease_id": LEASE,
            "lease_revision": 1, "runtime_instance_id": INSTANCE, "state": state,
            "revision": 2, "reason": reason,
        })
    };
    let approval = |state: &str, reason: Value, effect: &str| {
        json!({
            "managed_approval_id": APPROVAL, "managed_turn_id": TURN, "kind": "command_execution",
            "state": state, "revision": 2, "reason": reason, "decision": null, "effect": effect,
            "action_fingerprint": FINGERPRINT, "lease_id": LEASE, "lease_revision": 1,
            "runtime_instance_id": INSTANCE,
        })
    };
    let lease = |state: &str, reason: Value| {
        json!({
            "lease_id": LEASE, "lease_state": state, "revision": 2,
            "scopes": ["turn_start", "command_execute"],
            "expires_at": "2026-10-09T11:00:00Z", "revocation_reason": reason,
        })
    };
    vec![
        (
            "authority.managed_session.prepared",
            json!({"adapter_id": "codex", "runtime_kind": "codex", "control_state": "prepared", "revision": 1}),
            true,
        ),
        (
            "authority.runtime.desired_state_changed",
            json!({"adapter_id": "codex", "runtime_kind": "codex", "desired_state": "running", "revision": 1}),
            false,
        ),
        (
            "authority.runtime.instance_ready",
            json!({"adapter_id": "codex", "runtime_kind": "codex", "runtime_instance_id": INSTANCE,
                   "runtime_version": "0.159.0", "compatibility": "compatible"}),
            false,
        ),
        (
            "authority.managed_session.runtime_bound",
            json!({"adapter_id": "codex", "runtime_kind": "codex", "runtime_session_id": THREAD, "binding_revision": 1}),
            true,
        ),
        (
            "authority.managed_session.runtime_attached",
            json!({"runtime_session_id": THREAD, "runtime_instance_id": INSTANCE}),
            true,
        ),
        (
            "authority.execution_lease.granted",
            lease("active", Value::Null),
            true,
        ),
        (
            "authority.managed_turn.admitted",
            turn("starting", Value::Null),
            true,
        ),
        (
            "authority.managed_turn.running",
            turn("running", Value::Null),
            true,
        ),
        (
            "authority.managed_approval.requested",
            approval("pending", Value::Null, "none"),
            true,
        ),
        (
            "authority.managed_approval.accepted",
            approval("accepted", json!("client_decision"), "dispatched"),
            true,
        ),
        (
            "authority.managed_approval.confirmed",
            approval("accepted", json!("client_decision"), "confirmed"),
            true,
        ),
        (
            "authority.managed_turn.completed",
            turn("completed", json!("completed")),
            true,
        ),
        (
            "authority.execution_lease.revoked",
            lease("revoked", json!("user_request")),
            true,
        ),
        (
            "authority.managed_turn.cancel_requested",
            turn("running", json!("user_cancel")),
            true,
        ),
        (
            "authority.managed_turn.interrupted",
            turn("interrupted", json!("user_cancel")),
            true,
        ),
        (
            "authority.managed_approval.declined",
            approval("declined", json!("client_decision"), "none"),
            true,
        ),
        (
            "authority.managed_approval.expired",
            approval("expired", json!("approval_timeout"), "none"),
            true,
        ),
        (
            "authority.managed_approval.invalidated",
            approval("invalidated", json!("authority_restarted"), "none"),
            true,
        ),
        (
            "authority.managed_approval.cancelled",
            approval("cancelled", json!("client_decision"), "none"),
            true,
        ),
        (
            "authority.managed_approval.indeterminate",
            approval("indeterminate", json!("effect_unknown"), "unknown"),
            true,
        ),
        (
            "authority.managed_turn.failed",
            turn("failed", json!("provider_failed")),
            true,
        ),
        (
            "authority.managed_turn.indeterminate",
            turn("indeterminate", json!("authority_restarted")),
            true,
        ),
        (
            "authority.execution_lease.renewed",
            lease("active", Value::Null),
            true,
        ),
        (
            "authority.execution_lease.expired",
            lease("expired", Value::Null),
            true,
        ),
        (
            "authority.managed_session.runtime_operation_indeterminate",
            json!({"operation_id": OPERATION}),
            true,
        ),
        (
            "authority.managed_session.runtime_operation_abandoned",
            json!({"operation_id": OPERATION}),
            true,
        ),
        (
            "authority.runtime.instance_exited",
            json!({"adapter_id": "codex", "runtime_kind": "codex", "runtime_instance_id": INSTANCE,
                   "exit_kind": "child_exited", "expected": true, "desired_state": "stopped"}),
            false,
        ),
        (
            "authority.managed_session.closed",
            json!({"adapter_id": "codex", "runtime_kind": "codex", "control_state": "closed", "revision": 2}),
            true,
        ),
    ]
}

#[test]
fn what_crosses_is_a_closed_projection_and_never_the_payload() {
    let entries = every_kind();
    let count = entries.len() as u64;
    let (_peer, service, _, session) = authority(Log {
        entries,
        ..Log::default()
    });
    let page = service.latest().unwrap();
    assert_eq!(page.events.len() as u64, count);
    let wire = serde_json::to_value(&page).unwrap();
    let text = wire.to_string();
    for private in PRIVATE {
        assert!(!text.contains(private), "{private} crossed");
    }
    for word in [
        "payload",
        "lease_id",
        "fingerprint",
        "runtime_instance",
        "codex",
        "thread",
        "operation_id",
        "correlation",
        "causation",
        "event_id",
        "source",
        "revision",
    ] {
        assert!(!text.contains(word), "{word} crossed");
    }
    // Exactly these fields, for the page and for every row.
    let keys = |value: &Value| {
        let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    };
    assert_eq!(
        keys(&wire),
        ["events", "gap", "head", "older", "schema_version", "stream"]
    );
    for row in wire["events"].as_array().unwrap() {
        assert_eq!(
            keys(row),
            [
                "approval",
                "at",
                "category",
                "disposition",
                "facts",
                "kind",
                "security",
                "sequence",
                "session",
                "turn"
            ]
        );
        assert_eq!(
            keys(&row["facts"]),
            [
                "approval_effect",
                "approval_kind",
                "approval_reason",
                "desired",
                "exit",
                "expected",
                "expires_at",
                "revocation",
                "runtime_version",
                "scopes",
                "turn_reason"
            ]
        );
        // A position is text, as the stream wrote it.
        assert!(row["sequence"].is_string());
    }
    assert_eq!(wire["head"], json!(count.to_string()));
    // Oldest last: read them in the order they were recorded.
    let rows: Vec<&Value> = wire["events"].as_array().unwrap().iter().rev().collect();
    let kinds: Vec<&str> = rows.iter().map(|r| r["kind"].as_str().unwrap()).collect();
    assert_eq!(
        kinds,
        [
            "session_prepared",
            "runtime_desired",
            "runtime_ready",
            "session_bound",
            "session_attached",
            "lease_granted",
            "turn_admitted",
            "turn_running",
            "approval_requested",
            "approval_accepted",
            "approval_confirmed",
            "turn_completed",
            "lease_revoked",
            "turn_cancel_requested",
            "turn_interrupted",
            "approval_declined",
            "approval_expired",
            "approval_invalidated",
            "approval_cancelled",
            "approval_indeterminate",
            "turn_failed",
            "turn_indeterminate",
            "lease_renewed",
            "lease_expired",
            "binding_indeterminate",
            "binding_abandoned",
            "runtime_exited",
            "session_closed"
        ]
    );
    let by = |kind: &str| *rows.iter().find(|r| r["kind"] == kind).unwrap();
    // The identities that cross are Wyss's own, and only where the event is about them.
    assert_eq!(by("session_prepared")["session"], json!(session));
    assert_eq!(by("runtime_ready")["session"], Value::Null);
    assert_eq!(by("turn_completed")["turn"], json!(TURN));
    assert_eq!(by("turn_completed")["approval"], Value::Null);
    assert_eq!(by("approval_requested")["approval"], json!(APPROVAL));
    assert_eq!(by("approval_requested")["turn"], json!(TURN));
    assert_eq!(by("lease_granted")["turn"], Value::Null);
    // The facts are the recorded ones, in the authority's own closed words.
    assert_eq!(by("runtime_desired")["facts"]["desired"], "running");
    assert_eq!(by("runtime_ready")["facts"]["runtime_version"], "0.159.0");
    assert_eq!(by("runtime_exited")["facts"]["exit"], "child_exited");
    assert_eq!(by("runtime_exited")["facts"]["expected"], true);
    assert_eq!(
        by("turn_interrupted")["facts"]["turn_reason"],
        "user_cancel"
    );
    assert_eq!(
        by("approval_expired")["facts"]["approval_reason"],
        "approval_timeout"
    );
    assert_eq!(
        by("approval_confirmed")["facts"]["approval_effect"],
        "confirmed"
    );
    assert_eq!(
        by("approval_requested")["facts"]["approval_kind"],
        "command_execution"
    );
    assert_eq!(
        by("lease_granted")["facts"]["scopes"],
        json!(["turn_start", "command_execute"])
    );
    assert_eq!(
        by("lease_granted")["facts"]["expires_at"],
        "2026-10-09T11:00:00Z"
    );
    assert_eq!(by("lease_revoked")["facts"]["revocation"], "user_request");
    // Categories, and what reads as other than ordinary: only what the record says.
    let shown = |kind: &str| {
        let row = by(kind);
        (
            row["category"].as_str().unwrap().to_owned(),
            row["disposition"].as_str().unwrap().to_owned(),
            row["security"].as_bool().unwrap(),
        )
    };
    let is = |category: &str, disposition: &str, security: bool| {
        (category.to_owned(), disposition.to_owned(), security)
    };
    assert_eq!(shown("session_prepared"), is("session", "normal", false));
    assert_eq!(shown("runtime_exited"), is("runtime", "normal", false));
    assert_eq!(shown("turn_completed"), is("turn", "normal", false));
    assert_eq!(shown("turn_interrupted"), is("turn", "normal", false));
    assert_eq!(shown("turn_failed"), is("turn", "failure", false));
    assert_eq!(shown("turn_indeterminate"), is("turn", "attention", false));
    assert_eq!(shown("approval_declined"), is("approval", "normal", false));
    assert_eq!(
        shown("approval_expired"),
        is("approval", "attention", false)
    );
    assert_eq!(
        shown("approval_invalidated"),
        is("approval", "attention", false)
    );
    assert_eq!(
        shown("approval_indeterminate"),
        is("approval", "attention", false)
    );
    assert_eq!(
        shown("binding_indeterminate"),
        is("session", "attention", false)
    );
    assert_eq!(shown("lease_granted"), is("lease", "normal", true));
    assert_eq!(shown("lease_expired"), is("lease", "normal", true));
}

#[test]
fn what_the_record_says_about_isolation_and_failure_is_what_is_shown() {
    let turn = |kind: &'static str, reason: &str| -> Entry {
        (
            kind,
            json!({"managed_turn_id": TURN, "state": "x", "reason": reason}),
            true,
        )
    };
    let exit = |kind: &str, expected: bool| -> Entry {
        (
            "authority.runtime.instance_exited",
            json!({"exit_kind": kind, "expected": expected, "desired_state": "running"}),
            false,
        )
    };
    let (_peer, service, _, _) = authority(Log {
        entries: vec![
            turn(
                "authority.managed_turn.failed",
                "inference_isolation_violation",
            ),
            turn("authority.managed_turn.interrupted", "lease_expired"),
            turn("authority.managed_turn.interrupted", "approval_expired"),
            exit("child_exited", false),
            exit("safety_unproven", false),
            exit("inference_isolation_violation", false),
            (
                "authority.managed_approval.invalidated",
                json!({"managed_approval_id": APPROVAL, "managed_turn_id": TURN,
                       "kind": "file_change", "reason": "isolation_violation", "effect": "none"}),
                true,
            ),
        ],
        ..Log::default()
    });
    let page = service.latest().unwrap();
    let shown: Vec<(Disposition, bool)> = page
        .events
        .iter()
        .rev()
        .map(|row| (row.disposition, row.security))
        .collect();
    assert_eq!(
        shown,
        [
            (Disposition::Security, true),
            (Disposition::Attention, false),
            (Disposition::Attention, false),
            (Disposition::Failure, false),
            (Disposition::Security, true),
            (Disposition::Security, true),
            (Disposition::Security, true),
        ]
    );
}

#[test]
fn an_event_this_build_does_not_know_crosses_as_its_position_and_time_alone() {
    let (_peer, service, _, session) = authority(Log {
        entries: vec![
            // A kind from a newer authority, carrying things nobody here has vetted.
            (
                "authority.future.observation",
                json!({"managed_turn_id": TURN, "managed_approval_id": APPROVAL,
                       "note": "SECRET_FUTURE", "command": "SECRET_COMMAND",
                       "reasoning": "SECRET_REASONING", "reason": "user_cancel",
                       "scopes": ["turn_start"], "runtime_version": "9.9.9"}),
                true,
            ),
            (
                "vendor.something.else",
                json!({"anything": {"nested": ["SECRET_FUTURE"]}}),
                false,
            ),
            // A known kind whose words are newer than this build: the kind stays, the
            // fact is left out, and nothing of the unknown value is shown.
            (
                "authority.managed_turn.interrupted",
                json!({"managed_turn_id": TURN, "reason": "SECRET_FUTURE", "state": "interrupted"}),
                true,
            ),
            (
                "authority.runtime.instance_ready",
                json!({"runtime_version": "0.1 SECRET_FUTURE <script>"}),
                false,
            ),
            (
                "authority.execution_lease.granted",
                json!({"lease_id": LEASE, "scopes": ["turn_start", "SECRET_FUTURE"],
                       "expires_at": "tomorrow", "revocation_reason": 7}),
                true,
            ),
            (
                "authority.managed_turn.running",
                json!({"managed_turn_id": "not-an-identity"}),
                true,
            ),
        ],
        ..Log::default()
    });
    let page = service.latest().unwrap();
    let text = serde_json::to_string(&page).unwrap();
    for private in PRIVATE {
        assert!(!text.contains(private), "{private} crossed");
    }
    let rows: Vec<&EventRow> = page.events.iter().rev().collect();
    for unknown in &rows[..2] {
        assert_eq!(
            (
                unknown.category,
                unknown.kind,
                unknown.disposition,
                unknown.security
            ),
            (
                EventCategory::Authority,
                EventKind::Other,
                Disposition::Normal,
                false
            )
        );
        assert_eq!(unknown.facts, EventFacts::default());
        assert_eq!(
            (unknown.turn.as_ref(), unknown.approval.as_ref()),
            (None, None)
        );
    }
    // The envelope's own session is the one thing of an unknown event that is named.
    assert_eq!(rows[0].session.as_ref(), Some(&session));
    assert_eq!(rows[1].session, None);
    assert_eq!(rows[2].kind, EventKind::TurnInterrupted);
    assert_eq!(rows[2].facts, EventFacts::default());
    assert_eq!(rows[3].kind, EventKind::RuntimeReady);
    assert_eq!(rows[3].facts, EventFacts::default());
    assert_eq!(rows[4].kind, EventKind::LeaseGranted);
    assert_eq!(rows[4].facts, EventFacts::default());
    assert_eq!(
        (rows[5].kind, rows[5].turn.as_ref()),
        (EventKind::TurnRunning, None)
    );
}

#[test]
fn a_stream_that_is_not_one_is_refused_and_a_missing_beginning_is_said() {
    let stream_error = |error: SessionError| {
        assert_eq!(
            error.cause,
            Some(ControlError::CorrelationError),
            "{error:?}"
        );
    };
    // History before position 80 is not there to read: the window begins at a hole, says
    // so, and offers nothing before it.
    let (peer, service, log, _) = authority(Log {
        entries: turns(120),
        lost: 80,
        ..Log::default()
    });
    let page = service.latest().unwrap();
    assert_eq!(positions(&page), (81..=120).rev().collect::<Vec<_>>());
    assert_eq!((page.gap, page.older), (true, None));
    // The same when it is an older window that begins at the hole.
    log.lock().unwrap().lost = 25;
    let older = service.older(page.stream.clone(), at(71)).unwrap();
    assert_eq!(positions(&older), (26..=70).rev().collect::<Vec<_>>());
    assert_eq!((older.gap, older.older), (true, None));
    // A hole inside what was read is not a beginning: nothing is shown around it.
    {
        let mut log = log.lock().unwrap();
        (log.lost, log.hole) = (0, Some(100));
    }
    stream_error(failed(service.latest()));
    // Events of another stream are not this history.
    {
        let mut log = log.lock().unwrap();
        (log.hole, log.foreign) = (None, true);
    }
    stream_error(failed(service.latest()));
    // An answer with nothing in it, where the greeting said there is history.
    {
        let mut log = log.lock().unwrap();
        (log.foreign, log.empty) = (false, true);
    }
    stream_error(failed(service.latest()));
    drop(peer);

    // Whatever else the authority answers to a history read is not history.
    let peer = Peer::canonical(|_| answer(Ok(ResultValue::ActiveLease(None))));
    let error = failed(EventService::new(peer.service()).latest());
    assert_eq!(
        (error.code, error.cause),
        (SessionFailure::Internal, Some(ControlError::InvalidRequest))
    );
    // And a refusal is a refusal.
    let peer = Peer::canonical(|_| -> Reply { answer(Err(ControlError::StorageUnavailable)) });
    let error = failed(EventService::new(peer.service()).latest());
    assert_eq!(error.cause, Some(ControlError::StorageUnavailable));
}
