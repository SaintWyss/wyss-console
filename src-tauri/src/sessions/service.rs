//! Session calls over short-lived connections of the shared typed client.
//!
//! **Reads.** A page is a canonical reading: `managedSession/list`, then
//! `managedSession/runtime/status` for each session shown. The protocol lists in creation
//! order only, so the newest page is found by walking forward and keeping the last
//! `PAGE_SIZE` records. The walk is bounded; nothing but the page is kept.
//!
//! **Intents.** The caller names each user action with one intent identity and sends it
//! again, unchanged, when it could not learn the outcome. The authority's processed-intent
//! record makes that the same action: a second prepare with the same intent returns the
//! first session. This service never retries on its own and never invents an identity for
//! the action itself.
//!
//! **Resume.** `managedSession/runtime/resume` is admitted by the authority's execution
//! gate under an active lease that grants `session_resume`. The desktop asks first. Only
//! when the authority says a lease is what is missing does it ask the authority to grant
//! one, for that single scope and `RESUME_LEASE_SECONDS`, ask again, and ask the authority
//! to revoke what it was granted. A lease somebody else holds is used as it is and never
//! touched. Every one of those steps is a request the authority may refuse.
use super::view::{
    PAGE_SCHEMA_VERSION, PAGE_SIZE, SessionError, SessionFailure, SessionPage, SessionReceipt,
    SessionSummary,
};
use crate::{
    contracts::{
        Sequence, WireUuid,
        control::{
            BindingResume, Close, ControlError, ControlResult, ExecutionScope, GateQuery,
            GateReason, LeaseGrant, LeaseRevoke, List, MAX_PAGE, ManagedSessionRecord, Prepare,
            ResultValue, RuntimeTarget,
        },
        identities::ManagedSessionId,
    },
    control::client::ControlClient,
};
use std::{
    collections::VecDeque,
    env,
    path::PathBuf,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

/// Protocol pages one read walks at most: two thousand sessions.
const WALK_PAGES: usize = 40;
/// How long a lease the desktop asked for may live if it cannot be revoked afterwards.
/// The authority's own deadline for a resume is ten seconds.
const RESUME_LEASE_SECONDS: u32 = 30;

/// Counters of what the service did. For tests and diagnostics: none of it is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionStats {
    /// Connections an authority greeted. Each was closed when its call returned.
    pub connections: u64,
    /// Leases the authority granted because this desktop asked.
    pub leases: u64,
}

struct Inner {
    root: Option<PathBuf>,
    /// Ends the service and cancels the exchange in flight.
    stop: Arc<AtomicBool>,
    /// One call at a time: at most one connection besides the observer's.
    turn: Mutex<()>,
    connections: AtomicU64,
    leases: AtomicU64,
}

/// Application-lifetime door to managed sessions. Cloning shares it.
#[derive(Clone)]
pub struct SessionService {
    inner: Arc<Inner>,
}
impl SessionService {
    /// The authority of this user's session: `$XDG_RUNTIME_DIR`.
    pub fn from_environment() -> Self {
        Self::new(env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from))
    }
    /// Speaks to the authority of one runtime directory. It never starts one.
    pub fn new(runtime_root: Option<PathBuf>) -> Self {
        Self {
            inner: Arc::new(Inner {
                root: runtime_root,
                stop: Arc::new(AtomicBool::new(false)),
                turn: Mutex::new(()),
                connections: AtomicU64::new(0),
                leases: AtomicU64::new(0),
            }),
        }
    }
    pub fn stats(&self) -> SessionStats {
        SessionStats {
            connections: self.inner.connections.load(Ordering::Acquire),
            leases: self.inner.leases.load(Ordering::Acquire),
        }
    }
    /// Cancels the exchange in flight and refuses every later call. Idempotent. Nothing is
    /// sent to the authority.
    pub fn shutdown(&self) {
        self.inner.stop.store(true, Ordering::Release);
    }

    /// One connection, from its handshake to the end of `work`.
    fn call<T>(
        &self,
        work: impl FnOnce(&mut ControlClient) -> Result<T, SessionError>,
    ) -> Result<T, SessionError> {
        let _turn = self
            .inner
            .turn
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if self.inner.stop.load(Ordering::Acquire) {
            return Err(SessionError::new(SessionFailure::Unavailable));
        }
        let root = self
            .inner
            .root
            .as_deref()
            .ok_or(SessionError::of(ControlError::UnsafePath))?;
        // Whatever kept an authority from greeting, nothing was sent to one.
        let mut client = ControlClient::connect_cancellable(root, self.inner.stop.clone())
            .map_err(|error| SessionError {
                cause: Some(error),
                ..SessionError::new(match SessionError::of(error).code {
                    SessionFailure::Incompatible => SessionFailure::Incompatible,
                    _ => SessionFailure::Unavailable,
                })
            })?;
        self.inner.connections.fetch_add(1, Ordering::AcqRel);
        work(&mut client)
    }

    /// The newest sessions, or those created before `before`.
    pub fn page(&self, before: Option<Sequence>) -> Result<SessionPage, SessionError> {
        self.call(|client| {
            // The head first, then what it describes: the page is never older than it.
            let head = client.hello_status().last_sequence;
            let walk = walk(client, before).map_err(SessionError::of)?;
            let older = walk
                .older
                .then(|| walk.records.front().map(|s| s.created_sequence))
                .flatten();
            let mut sessions = Vec::with_capacity(walk.records.len());
            for record in walk.records.into_iter().rev() {
                let binding = client
                    .binding_status(record.managed_session_id.clone())
                    .map_err(SessionError::of)?;
                sessions.push(SessionSummary::of(record, binding));
            }
            Ok(SessionPage {
                schema_version: PAGE_SCHEMA_VERSION,
                head,
                sessions,
                older,
                truncated: walk.truncated,
            })
        })
    }

    /// Asks the authority to prepare a managed Codex session.
    pub fn prepare(&self, intent: WireUuid) -> Result<SessionReceipt, SessionError> {
        self.call(|client| {
            let target = RuntimeTarget::codex();
            client
                .prepare(Prepare {
                    intent_id: intent,
                    adapter_id: target.adapter_id,
                    runtime_kind: target.runtime_kind,
                })
                .map(receipt)
                .map_err(SessionError::of)
        })
    }

    /// Asks the authority to close a session at the revision the caller saw.
    pub fn close(
        &self,
        intent: WireUuid,
        id: ManagedSessionId,
        expected_revision: u64,
    ) -> Result<SessionReceipt, SessionError> {
        session_revision(expected_revision)?;
        self.call(|client| {
            client
                .close(Close {
                    intent_id: intent,
                    managed_session_id: id,
                    expected_revision,
                })
                .map(receipt)
                .map_err(SessionError::of)
        })
    }

    /// Asks the authority to attach a session's existing binding to the current runtime.
    pub fn resume(
        &self,
        intent: WireUuid,
        id: ManagedSessionId,
        expected_revision: u64,
    ) -> Result<SessionReceipt, SessionError> {
        session_revision(expected_revision)?;
        let leases = &self.inner.leases;
        self.call(|client| {
            let request = BindingResume {
                intent_id: intent,
                managed_session_id: id.clone(),
                expected_session_revision: expected_revision,
            };
            let attached = |_| SessionReceipt { id: id.clone() };
            // An intent the authority already holds, or a lease already in place, needs
            // nothing more from the desktop.
            match client.binding_resume(request.clone()) {
                Ok(binding) => return Ok(attached(binding)),
                Err(ControlError::GateDenied) => (),
                Err(error) => return Err(SessionError::of(error)),
            }
            let reason = gate(client, &id);
            if !matches!(
                reason,
                Some(
                    GateReason::LeaseMissing
                        | GateReason::LeaseNotActive
                        | GateReason::LeaseExpired
                )
            ) {
                return Err(denied(reason));
            }
            let lease = client
                .lease_grant(LeaseGrant {
                    intent_id: fresh(),
                    session_id: id.clone(),
                    expected_session_revision: expected_revision,
                    scopes: vec![ExecutionScope::SessionResume],
                    ttl_seconds: RESUME_LEASE_SECONDS,
                })
                .map_err(SessionError::of)?;
            leases.fetch_add(1, Ordering::AcqRel);
            let result = client.binding_resume(request);
            // Read while the lease still stands, so the reason is the one that denied.
            let reason = matches!(result, Err(ControlError::GateDenied))
                .then(|| gate(client, &id))
                .flatten();
            // The lease was asked for this one action. If this fails it ages out.
            let _ = client.lease_revoke(LeaseRevoke {
                intent_id: fresh(),
                lease_id: lease.lease_id,
                expected_revision: lease.revision,
            });
            match result {
                Ok(binding) => Ok(attached(binding)),
                Err(ControlError::GateDenied) => Err(denied(reason)),
                Err(error) => Err(SessionError::of(error)),
            }
        })
    }
}

fn receipt(session: ManagedSessionRecord) -> SessionReceipt {
    SessionReceipt {
        id: session.managed_session_id,
    }
}

/// An identity for a request that is not the user's action: asked once, never resent.
fn fresh() -> WireUuid {
    WireUuid::try_from(uuid::Uuid::new_v4().to_string()).expect("generated UUID")
}

/// A managed session is at revision one (prepared) or two (closed).
fn session_revision(revision: u64) -> Result<(), SessionError> {
    if (1..=2).contains(&revision) {
        Ok(())
    } else {
        Err(SessionError::new(SessionFailure::Invalid))
    }
}

/// Why the gate denies a resume right now. Informational: it explains a refusal the
/// authority already made and authorizes nothing.
fn gate(client: &mut ControlClient, id: &ManagedSessionId) -> Option<GateReason> {
    client
        .gate_status(GateQuery {
            session_id: id.clone(),
            action: ExecutionScope::SessionResume,
        })
        .ok()
        .map(|status| status.reason)
}
fn denied(reason: Option<GateReason>) -> SessionError {
    reason.map_or(
        SessionError::of(ControlError::GateDenied),
        SessionError::denied,
    )
}

struct Walk {
    /// At most `PAGE_SIZE` records, oldest first.
    records: VecDeque<ManagedSessionRecord>,
    /// Sessions older than every record kept exist.
    older: bool,
    truncated: bool,
}

/// The last `PAGE_SIZE` sessions created before `before`, by walking the list forward.
fn walk(client: &mut ControlClient, before: Option<Sequence>) -> ControlResult<Walk> {
    let mut walk = Walk {
        records: VecDeque::with_capacity(PAGE_SIZE),
        older: false,
        truncated: false,
    };
    let mut after = None;
    for _ in 0..WALK_PAGES {
        let ResultValue::Sessions { sessions, next } = client.list(List {
            after,
            limit: MAX_PAGE,
        })?
        else {
            return Err(ControlError::InvalidRequest);
        };
        for session in sessions {
            // A page that does not move forward from its cursor is not a list.
            if after.is_some_and(|cursor| session.created_sequence <= cursor) {
                return Err(ControlError::CorrelationError);
            }
            after = Some(session.created_sequence);
            if before.is_some_and(|limit| session.created_sequence >= limit) {
                return Ok(walk);
            }
            if walk.records.len() == PAGE_SIZE {
                walk.records.pop_front();
                walk.older = true;
            }
            walk.records.push_back(session);
        }
        if next.is_none() {
            return Ok(walk);
        }
        // The next page starts where this one ended, wherever the peer says it does.
        if next != after {
            return Err(ControlError::CorrelationError);
        }
    }
    walk.truncated = true;
    Ok(walk)
}
