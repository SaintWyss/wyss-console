//! The window's door to managed turns and their approvals: for turns two reads and two
//! product intents, for approvals two reads and two decisions.
//!
//! Each command takes one narrow request and names no control method. The window cannot
//! choose a scope, a lease, a lifetime, a runtime, a provider thread, a model, a tool or a
//! safety profile: it names a session or a turn it was shown, the revision it saw, the
//! identity of its own action and, for a start, the text the user wrote. That text goes to
//! the authority as one typed field and is never part of a command line, a path or a log.
//!
//! An approval is answered with one of two commands, Approve or Deny, for one approval at
//! the revision the window was shown. Neither takes a decision as text, a command to run
//! instead, a scope, a rule or anything that outlives that one approval. The approval's
//! identity is a handle the window was given; whether it is still pending, still inside
//! its workspace and still covered by the turn's lease is the authority's to check.
use serde::Deserialize;
use std::sync::atomic::{AtomicUsize, Ordering};
use tauri::State;

use super::sessions::{SessionError, SessionFailure};
use crate::contracts::{
    Sequence, WireUuid,
    identities::{ManagedApprovalId, ManagedSessionId, ManagedTurnId},
};
#[cfg(target_os = "linux")]
pub use crate::turns::{
    ApprovalDetail, ApprovalList, Decision, DecisionReceipt, TurnList, TurnReceipt, TurnService,
    TurnView,
};

/// Intents admitted at once. They run one at a time, with the session intents.
const MAX_INTENTS: usize = 2;
static INTENTS: AtomicUsize = AtomicUsize::new(0);
/// Reads admitted at once: one follower, one list, and room for the page they replace.
const MAX_READS: usize = 4;
static READS: AtomicUsize = AtomicUsize::new(0);

struct Permit(&'static AtomicUsize);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartTurnRequest {
    /// Names this user action. The same value sent again is the same turn.
    intent_id: WireUuid,
    session_id: ManagedSessionId,
    /// The session revision the window was shown.
    expected_revision: u64,
    /// What the user wrote, whole. The one input of the turn.
    prompt: String,
}
// Deliberately not derived: what the user wrote must not enter diagnostics.
impl std::fmt::Debug for StartTurnRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StartTurnRequest(<sensitive>)")
    }
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelTurnRequest {
    intent_id: WireUuid,
    turn_id: ManagedTurnId,
    /// The turn revision the window was shown.
    expected_revision: u64,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadTurnRequest {
    turn_id: ManagedTurnId,
    /// The cursor the last reading returned. Null reads from what the authority still holds.
    after: Option<Sequence>,
    /// Wait a moment for something new, instead of answering at once with nothing.
    wait: bool,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnsRequest {
    session_id: ManagedSessionId,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalsRequest {}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadApprovalRequest {
    /// The handle of an approval the window was shown.
    approval_id: ManagedApprovalId,
    /// Which file of a change set, and where in its diff. Null is the first of each.
    file: Option<u16>,
    offset: Option<u32>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecideApprovalRequest {
    /// Names this user action. The same value sent again is the same decision.
    intent_id: WireUuid,
    approval_id: ManagedApprovalId,
    /// The approval revision the window was shown.
    expected_revision: u64,
}

/// Admission occurs before `spawn_blocking`, with counters of its own: a turn read, which
/// may wait, never takes the place of a session call, a terminal or the authority view.
async fn blocking<T: Send + 'static>(
    (pool, limit): (&'static AtomicUsize, usize),
    service: TurnService,
    operation: impl FnOnce(TurnService) -> Result<T, SessionError> + Send + 'static,
) -> Result<T, SessionError> {
    pool.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
        (count < limit).then_some(count + 1)
    })
    .map_err(|_| SessionError::new(SessionFailure::Busy))?;
    let permit = Permit(pool);
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        operation(service)
    })
    .await
    .map_err(|_| SessionError::new(SessionFailure::Internal))?
}

#[tauri::command]
pub async fn managed_turns(
    service: State<'_, TurnService>,
    request: TurnsRequest,
) -> Result<TurnList, SessionError> {
    blocking(
        (&READS, MAX_READS),
        service.inner().clone(),
        move |service| service.list(request.session_id),
    )
    .await
}
#[tauri::command]
pub async fn managed_turn_read(
    service: State<'_, TurnService>,
    request: ReadTurnRequest,
) -> Result<TurnView, SessionError> {
    blocking(
        (&READS, MAX_READS),
        service.inner().clone(),
        move |service| service.read(request.turn_id, request.after, request.wait),
    )
    .await
}
#[tauri::command]
pub async fn managed_turn_start(
    service: State<'_, TurnService>,
    request: StartTurnRequest,
) -> Result<TurnReceipt, SessionError> {
    blocking(
        (&INTENTS, MAX_INTENTS),
        service.inner().clone(),
        move |service| {
            service.start(
                request.intent_id,
                request.session_id,
                request.expected_revision,
                request.prompt,
            )
        },
    )
    .await
}
#[tauri::command]
pub async fn managed_turn_cancel(
    service: State<'_, TurnService>,
    request: CancelTurnRequest,
) -> Result<TurnReceipt, SessionError> {
    blocking(
        (&INTENTS, MAX_INTENTS),
        service.inner().clone(),
        move |service| {
            service.cancel(
                request.intent_id,
                request.turn_id,
                request.expected_revision,
            )
        },
    )
    .await
}

/// The latest approvals: those waiting for a decision, with what they are about.
#[tauri::command]
pub async fn managed_approvals(
    service: State<'_, TurnService>,
    request: ApprovalsRequest,
) -> Result<ApprovalList, SessionError> {
    let ApprovalsRequest {} = request;
    blocking(
        (&READS, MAX_READS),
        service.inner().clone(),
        move |service| service.approvals(),
    )
    .await
}
#[tauri::command]
pub async fn managed_approval_read(
    service: State<'_, TurnService>,
    request: ReadApprovalRequest,
) -> Result<ApprovalDetail, SessionError> {
    blocking(
        (&READS, MAX_READS),
        service.inner().clone(),
        move |service| service.approval(request.approval_id, request.file, request.offset),
    )
    .await
}
async fn decide_in(
    service: TurnService,
    request: DecideApprovalRequest,
    decision: Decision,
) -> Result<DecisionReceipt, SessionError> {
    blocking((&INTENTS, MAX_INTENTS), service, move |service| {
        service.decide(
            request.intent_id,
            request.approval_id,
            request.expected_revision,
            decision,
        )
    })
    .await
}
/// Asks the authority to accept this one approval. Which decision a command means is
/// fixed here: the window cannot send one as data.
#[tauri::command]
pub async fn managed_approval_approve(
    service: State<'_, TurnService>,
    request: DecideApprovalRequest,
) -> Result<DecisionReceipt, SessionError> {
    decide_in(service.inner().clone(), request, Decision::Approve).await
}
#[tauri::command]
pub async fn managed_approval_deny(
    service: State<'_, TurnService>,
    request: DecideApprovalRequest,
) -> Result<DecisionReceipt, SessionError> {
    decide_in(service.inner().clone(), request, Decision::Deny).await
}

/// Where there is no authority to ask, every call says so.
#[cfg(not(target_os = "linux"))]
mod unsupported {
    use super::super::sessions::{ActivationProgress, AgentMode};
    use super::{SessionError, SessionFailure};
    use crate::contracts::{
        Sequence, WireUuid,
        identities::{ManagedApprovalId, ManagedSessionId, ManagedTurnId},
    };
    use serde::Serialize;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Decision {
        Approve,
        Deny,
    }
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    pub struct ApprovalList {}
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    pub struct ApprovalDetail {}
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    pub struct DecisionReceipt {}

    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    pub struct TurnList {}
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    pub struct TurnView {}
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    pub struct TurnReceipt {}
    #[derive(Clone, Default)]
    pub struct TurnService;
    type Refused<T> = Result<T, SessionError>;
    impl TurnService {
        pub fn new(_: super::super::sessions::SessionService) -> Self {
            Self
        }
        fn absent<T>() -> Refused<T> {
            Err(SessionError::new(SessionFailure::Unavailable))
        }
        pub fn list(&self, _: ManagedSessionId) -> Refused<TurnList> {
            Self::absent()
        }
        pub fn read(&self, _: ManagedTurnId, _: Option<Sequence>, _: bool) -> Refused<TurnView> {
            Self::absent()
        }
        pub fn start(
            &self,
            _: WireUuid,
            _: ManagedSessionId,
            _: u64,
            _: String,
        ) -> Refused<TurnReceipt> {
            Self::absent()
        }
        pub fn cancel(&self, _: WireUuid, _: ManagedTurnId, _: u64) -> Refused<TurnReceipt> {
            Self::absent()
        }
        pub fn activate(
            &self,
            _: WireUuid,
            _: ManagedSessionId,
            _: u64,
            _: String,
            _: AgentMode,
        ) -> Refused<ActivationProgress> {
            Self::absent()
        }
        pub fn enter(
            &self,
            _: WireUuid,
            _: ManagedSessionId,
            _: u64,
            _: AgentMode,
        ) -> Refused<ActivationProgress> {
            Self::absent()
        }
        pub fn approvals(&self) -> Refused<ApprovalList> {
            Self::absent()
        }
        pub fn approval(
            &self,
            _: ManagedApprovalId,
            _: Option<u16>,
            _: Option<u32>,
        ) -> Refused<ApprovalDetail> {
            Self::absent()
        }
        pub fn decide(
            &self,
            _: WireUuid,
            _: ManagedApprovalId,
            _: u64,
            _: Decision,
        ) -> Refused<DecisionReceipt> {
            Self::absent()
        }
    }
}
#[cfg(not(target_os = "linux"))]
pub use unsupported::{
    ApprovalDetail, ApprovalList, Decision, DecisionReceipt, TurnList, TurnReceipt, TurnService,
    TurnView,
};

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::sessions::SessionService;
    use serde_json::{Value, json};

    const INTENT: &str = "11111111-1111-4111-8111-111111111111";
    const SESSION: &str = "22222222-2222-4222-8222-222222222222";
    const TURN: &str = "33333333-3333-4333-8333-333333333333";

    fn block<T>(future: impl std::future::Future<Output = T>) -> T {
        tauri::async_runtime::block_on(future)
    }
    fn code(error: SessionError) -> Value {
        serde_json::to_value(error).unwrap()
    }

    #[test]
    fn turn_requests_accept_exactly_their_narrow_fields() {
        let start = serde_json::from_value::<StartTurnRequest>;
        let full = json!({
            "intent_id": INTENT, "session_id": SESSION, "expected_revision": 1,
            "prompt": "Say hello.\nIn one line.",
        });
        let accepted = start(full.clone()).unwrap();
        assert_eq!(accepted.intent_id.as_str(), INTENT);
        assert_eq!(accepted.session_id.as_str(), SESSION);
        assert_eq!(accepted.expected_revision, 1);
        // The text is carried whole, untrimmed, and is not what the request prints.
        assert_eq!(accepted.prompt, "Say hello.\nIn one line.");
        assert_eq!(format!("{accepted:?}"), "StartTurnRequest(<sensitive>)");
        for (field, value) in [
            ("intent_id", json!("x")),
            ("session_id", json!(7)),
            ("expected_revision", json!("1")),
            ("prompt", json!(null)),
            ("prompt", json!(["Say hello."])),
            ("prompt", json!({"type": "text", "text": "Say hello."})),
        ] {
            let mut tampered = full.clone();
            tampered[field] = value;
            assert!(start(tampered.clone()).is_err(), "{tampered}");
        }
        for extra in [
            // Nothing the authority, the runtime or the provider decides can ride along.
            "scopes",
            "ttl_seconds",
            "lease_id",
            "model",
            "tools",
            "sandbox",
            "approval_policy",
            "safety_profile",
            "thread_id",
            "runtime_instance_id",
            "workspace",
            "cwd",
            "input",
            "method",
        ] {
            let mut tampered = full.clone();
            tampered[extra] = json!("x");
            assert!(start(tampered.clone()).is_err(), "{tampered}");
        }
        for missing in ["intent_id", "session_id", "expected_revision", "prompt"] {
            let mut tampered = full.clone();
            tampered.as_object_mut().unwrap().remove(missing);
            assert!(start(tampered.clone()).is_err(), "{tampered}");
        }

        let cancel = serde_json::from_value::<CancelTurnRequest>;
        let full = json!({"intent_id": INTENT, "turn_id": TURN, "expected_revision": 2});
        let accepted = cancel(full.clone()).unwrap();
        assert_eq!(
            (accepted.turn_id.as_str(), accepted.expected_revision),
            (TURN, 2)
        );
        for extra in ["session_id", "reason", "decision", "force", "method"] {
            let mut tampered = full.clone();
            tampered[extra] = json!("x");
            assert!(cancel(tampered.clone()).is_err(), "{tampered}");
        }
        for missing in ["intent_id", "turn_id", "expected_revision"] {
            let mut tampered = full.clone();
            tampered.as_object_mut().unwrap().remove(missing);
            assert!(cancel(tampered.clone()).is_err(), "{tampered}");
        }

        let read = serde_json::from_value::<ReadTurnRequest>;
        let first = read(json!({"turn_id": TURN, "after": null, "wait": false})).unwrap();
        assert!(first.after.is_none() && !first.wait);
        let next = read(json!({"turn_id": TURN, "after": "17", "wait": true})).unwrap();
        assert_eq!(next.after.unwrap().get(), 17);
        for rejected in [
            // The cursor is the decimal text it was given as, never a number.
            json!({"turn_id": TURN, "after": 17, "wait": true}),
            json!({"turn_id": TURN, "after": "0", "wait": true}),
            json!({"turn_id": TURN, "after": "017", "wait": true}),
            json!({"turn_id": TURN, "after": "-1", "wait": true}),
            json!({"turn_id": TURN, "after": null}),
            json!({"turn_id": TURN, "after": null, "wait": true, "limit": 5000}),
            json!({"turn_id": TURN, "after": null, "wait": true, "wait_ms": 600000}),
            json!({"turn_id": "x", "after": null, "wait": true}),
            json!({"method": "managedSession/turn/events", "params": {}}),
        ] {
            assert!(read(rejected.clone()).is_err(), "{rejected}");
        }

        let turns = serde_json::from_value::<TurnsRequest>;
        assert_eq!(
            turns(json!({"session_id": SESSION}))
                .unwrap()
                .session_id
                .as_str(),
            SESSION
        );
        for rejected in [
            json!({}),
            json!({"session_id": SESSION, "limit": 500}),
            json!({"session_id": SESSION, "after_sequence": "1"}),
            json!({"stream_id": SESSION}),
        ] {
            assert!(turns(rejected.clone()).is_err(), "{rejected}");
        }
    }

    #[test]
    fn an_approval_is_read_by_its_handle_and_decided_with_nothing_but_its_revision() {
        const APPROVAL: &str = "44444444-4444-4444-8444-444444444444";
        let decide = serde_json::from_value::<DecideApprovalRequest>;
        let full = json!({"intent_id": INTENT, "approval_id": APPROVAL, "expected_revision": 1});
        let accepted = decide(full.clone()).unwrap();
        assert_eq!(accepted.intent_id.as_str(), INTENT);
        assert_eq!(accepted.approval_id.as_str(), APPROVAL);
        assert_eq!(accepted.expected_revision, 1);
        // Which decision a request means is the command it was sent to, never data: no
        // decision, no replacement action, nothing that outlives this one approval.
        for extra in [
            "decision",
            "accept",
            "command",
            "cwd",
            "edited",
            "amendment",
            "execpolicy_amendment",
            "for_session",
            "always",
            "remember",
            "rule",
            "scope",
            "scopes",
            "lease_id",
            "fingerprint",
            "action_fingerprint",
            "turn_id",
            "session_id",
            "method",
        ] {
            let mut tampered = full.clone();
            tampered[extra] = json!("accept");
            assert!(decide(tampered.clone()).is_err(), "{tampered}");
        }
        for (field, value) in [
            ("intent_id", json!("x")),
            ("approval_id", json!("approval-1")),
            ("approval_id", json!(null)),
            ("expected_revision", json!("1")),
            ("expected_revision", json!(-1)),
        ] {
            let mut tampered = full.clone();
            tampered[field] = value;
            assert!(decide(tampered.clone()).is_err(), "{tampered}");
        }
        for missing in ["intent_id", "approval_id", "expected_revision"] {
            let mut tampered = full.clone();
            tampered.as_object_mut().unwrap().remove(missing);
            assert!(decide(tampered.clone()).is_err(), "{tampered}");
        }

        let read = serde_json::from_value::<ReadApprovalRequest>;
        let page = read(json!({"approval_id": APPROVAL, "file": 2, "offset": 8192})).unwrap();
        assert_eq!(
            (page.approval_id.as_str(), page.file, page.offset),
            (APPROVAL, Some(2), Some(8192))
        );
        let first = read(json!({"approval_id": APPROVAL, "file": null, "offset": null})).unwrap();
        assert_eq!((first.file, first.offset), (None, None));
        for tampered in [
            json!({"approval_id": APPROVAL, "file": -1, "offset": 0}),
            json!({"approval_id": APPROVAL, "file": 70000, "offset": 0}),
            json!({"approval_id": APPROVAL, "file": 0, "offset": "0"}),
            json!({"approval_id": APPROVAL, "file": 0, "offset": 0, "path": "/etc/passwd"}),
            json!({"approval_id": APPROVAL, "file": 0, "offset": 0, "limit": 1000000}),
            json!({"approval_id": "x", "file": null, "offset": null}),
        ] {
            assert!(read(tampered.clone()).is_err(), "{tampered}");
        }
        // The list takes nothing at all.
        assert!(serde_json::from_value::<ApprovalsRequest>(json!({})).is_ok());
        assert!(
            serde_json::from_value::<ApprovalsRequest>(json!({"session_id": SESSION})).is_err()
        );

        // Without an authority a decision reaches nobody, and a revision that cannot be
        // one is refused before anything is asked.
        let sessions = crate::sessions::SessionService::new(None);
        let service = TurnService::new(sessions.clone());
        let request = |revision| DecideApprovalRequest {
            intent_id: WireUuid::try_from(INTENT.to_owned()).unwrap(),
            approval_id: ManagedApprovalId::parse(APPROVAL).unwrap(),
            expected_revision: revision,
        };
        for decision in [Decision::Approve, Decision::Deny] {
            assert_eq!(
                code(block(decide_in(service.clone(), request(0), decision)).unwrap_err()),
                json!({"code": "invalid", "retry": false})
            );
            assert_eq!(
                code(block(decide_in(service.clone(), request(1), decision)).unwrap_err()),
                json!({"code": "unavailable", "retry": true})
            );
        }
        assert_eq!(sessions.stats().connections, 0);
        assert_eq!(INTENTS.load(Ordering::Acquire), 0);
    }

    #[test]
    fn a_turn_call_without_an_authority_fails_safely_and_admission_recovers() {
        // No runtime directory: nothing can be reached and nothing is sent.
        let sessions = SessionService::new(None);
        let service = TurnService::new(sessions.clone());
        let intent = || WireUuid::try_from(INTENT.to_owned()).unwrap();
        let session = || ManagedSessionId::parse(SESSION).unwrap();
        let turn = || ManagedTurnId::parse(TURN).unwrap();
        let unavailable = json!({"code": "unavailable", "retry": true});
        let invalid = json!({"code": "invalid", "retry": false});
        let reads = (&READS, MAX_READS);
        let intents = (&INTENTS, MAX_INTENTS);
        assert_eq!(
            code(block(blocking(reads, service.clone(), move |s| s.list(session()))).unwrap_err()),
            unavailable
        );
        assert_eq!(
            code(
                block(blocking(reads, service.clone(), move |s| {
                    s.read(turn(), None, true)
                }))
                .unwrap_err()
            ),
            unavailable
        );
        // An input the protocol cannot carry, or a revision nothing can have, is refused
        // here, before anything is asked.
        for prompt in [String::new(), "x".repeat(8193)] {
            let refused = block(blocking(intents, service.clone(), move |s| {
                s.start(intent(), session(), 1, prompt)
            }));
            assert_eq!(code(refused.unwrap_err()), invalid);
        }
        for revision in [0, 3, u64::MAX] {
            let refused = block(blocking(intents, service.clone(), move |s| {
                s.start(intent(), session(), revision, "hello".into())
            }));
            assert_eq!(code(refused.unwrap_err()), invalid);
        }
        let refused = block(blocking(intents, service.clone(), move |s| {
            s.cancel(intent(), turn(), 0)
        }));
        assert_eq!(code(refused.unwrap_err()), invalid);
        assert_eq!(sessions.stats().connections, 0);

        // Each pool refuses beyond its bound and recovers; one never takes from the other.
        let held: Vec<_> = (0..MAX_READS).map(|_| Permit(&READS)).collect();
        READS.fetch_add(MAX_READS, Ordering::AcqRel);
        let refused = block(blocking(reads, service.clone(), move |s| s.list(session())));
        assert_eq!(
            code(refused.unwrap_err()),
            json!({"code": "busy", "retry": true})
        );
        let other = block(blocking(intents, service.clone(), move |s| {
            s.cancel(intent(), turn(), 1)
        }));
        assert_eq!(code(other.unwrap_err()), unavailable);
        drop(held);
        assert_eq!(READS.load(Ordering::Acquire), 0);
        assert_eq!(INTENTS.load(Ordering::Acquire), 0);
        // Stopped with the session service: every call refuses at once.
        sessions.shutdown();
        assert_eq!(
            code(block(blocking(reads, service, move |s| s.list(session()))).unwrap_err()),
            unavailable
        );
    }
}
