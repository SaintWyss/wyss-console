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
//! **Gated operations.** Resuming and binding are admitted by the authority's execution
//! gate under an active lease that grants their scope. The desktop asks first. Only when
//! the authority says a lease is what is missing does it ask the authority to grant one,
//! for that single scope and a few seconds, ask again, and ask the authority to revoke
//! what it was granted. A lease somebody else holds is used as it is and never touched.
//! Every one of those steps is a request the authority may refuse.
//!
//! **A lease that is kept.** A managed turn runs under the lease it was admitted under
//! for as long as it runs, so that one is not given back when the turn is admitted: it
//! is held until the turn has ended, and it is the turn's time budget if nobody is
//! there to give it back.
//!
//! **A lease that could not be given back.** When the answer to a grant or a revoke is
//! lost, the lease is remembered until the moment it would have expired by itself. The
//! next call for that session settles it first, with the identities it already used.
use super::view::{
    PAGE_SCHEMA_VERSION, PAGE_SIZE, RuntimeFacts, SessionError, SessionFailure, SessionPage,
    SessionReceipt, SessionSummary,
};
use crate::contracts::identities::{ExecutionLeaseId, ManagedTurnId};
use crate::{
    contracts::{
        Sequence, WireUuid,
        control::{
            AuthorityStatus, BindingResume, Close, ControlError, ControlResult, ExecutionLease,
            ExecutionScope, GateQuery, GateReason, LeaseGrant, LeaseRevoke, List, MAX_PAGE,
            ManagedSessionRecord, Prepare, ResultValue, RuntimeTarget,
        },
        identities::ManagedSessionId,
    },
    control::client::ControlClient,
};
use std::{
    collections::{HashMap, VecDeque},
    env,
    path::PathBuf,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Protocol pages one read walks at most: two thousand sessions.
const WALK_PAGES: usize = 40;
/// How long a lease the desktop asked for may live if it cannot be revoked afterwards.
/// The authority's own deadline for a resume is ten seconds.
pub(super) const RESUME_LEASE_SECONDS: u32 = 30;
/// Past a lease's own lifetime, how long its record is kept before it is dropped unasked.
const LEASE_MARGIN: Duration = Duration::from_secs(5);

/// A lease this desktop asked for and has not seen given back. It is kept only while
/// that is so, and never past the moment the lease has expired by itself.
struct LeaseSlot {
    /// The grant exactly as it was sent: sent again, it is the same intent and payload.
    grant: LeaseGrant,
    /// What the authority granted, once it answered.
    lease: Option<(ExecutionLeaseId, u64)>,
    /// Names the revoke, however many times it has to be asked.
    revoke: WireUuid,
    until: Instant,
    /// Kept on purpose, for a turn that runs under it. Not settled until it is released.
    held: Option<Held>,
}
/// What a lease is being kept for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Held {
    /// The turn admitted under it, once the authority named it.
    pub turn: Option<ManagedTurnId>,
}

/// The lease a gated operation may ask the authority for.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Grant {
    /// Exactly the scopes the lease is asked with. The first is the one the gate is
    /// asked about: the operation's own.
    pub scopes: &'static [ExecutionScope],
    pub ttl_seconds: u32,
    /// Keep the lease after the operation was admitted, instead of giving it back.
    pub hold: bool,
}

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
    /// By session, the lease of this desktop whose return is not yet known.
    slots: Mutex<HashMap<String, LeaseSlot>>,
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
                slots: Mutex::new(HashMap::new()),
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

    /// One connection, from its handshake to the end of `work`. One at a time.
    pub(crate) fn call<T>(
        &self,
        work: impl FnOnce(&mut ControlClient) -> Result<T, SessionError>,
    ) -> Result<T, SessionError> {
        let _turn = self
            .inner
            .turn
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        work(&mut self.connect()?)
    }
    /// A greeted connection to the authority, cancelled when the service stops. The
    /// caller owns how many it holds at once.
    pub(crate) fn connect(&self) -> Result<ControlClient, SessionError> {
        if self.stopping() {
            return Err(SessionError::new(SessionFailure::Unavailable));
        }
        let root = self
            .inner
            .root
            .as_deref()
            .ok_or(SessionError::of(ControlError::UnsafePath))?;
        // Whatever kept an authority from greeting, nothing was sent to one.
        let client =
            ControlClient::connect_cancellable(root, self.inner.stop.clone()).map_err(|error| {
                SessionError {
                    cause: Some(error),
                    ..SessionError::new(match SessionError::of(error).code {
                        SessionFailure::Incompatible => SessionFailure::Incompatible,
                        _ => SessionFailure::Unavailable,
                    })
                }
            })?;
        self.inner.connections.fetch_add(1, Ordering::AcqRel);
        Ok(client)
    }
    pub(crate) fn stopping(&self) -> bool {
        self.inner.stop.load(Ordering::Acquire)
    }

    /// The identity of the authority's durable stream, as its greeting stated it. Read only:
    /// it names which authority a session belongs to, so P1 and P2 sessions never mix.
    pub fn stream_id(&self) -> Result<WireUuid, SessionError> {
        self.call(|client| Ok(client.hello_status().stream_id.clone()))
    }

    /// The authority's own status, read now: its protocol, storage schema, version and stream.
    /// Read only. It is what a caller may take as the authority's word about itself.
    pub fn status(&self) -> Result<AuthorityStatus, SessionError> {
        self.call(|client| client.status().map_err(SessionError::of))
    }

    /// The newest sessions, or those created before `before`.
    pub fn page(&self, before: Option<Sequence>) -> Result<SessionPage, SessionError> {
        self.call(|client| {
            // The head first, then what it describes: the page is never older than it.
            let head = client.hello_status().last_sequence;
            let runtime = RuntimeFacts::of(&client.runtime_status().map_err(SessionError::of)?);
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
                runtime,
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
        self.call(|client| {
            self.settle(client, &id);
            let request = BindingResume {
                intent_id: intent,
                managed_session_id: id.clone(),
                expected_session_revision: expected_revision,
            };
            self.gated(
                client,
                &id,
                expected_revision,
                Grant {
                    scopes: &[ExecutionScope::SessionResume],
                    ttl_seconds: RESUME_LEASE_SECONDS,
                    hold: false,
                },
                |client| client.binding_resume(request.clone()),
            )
            .map(|_| SessionReceipt { id: id.clone() })
        })
    }

    /// Asks the authority for `ask`, which its execution gate admits under `scope`.
    ///
    /// An intent the authority already holds, or a lease already in place, needs nothing
    /// more. Only when the gate names a lease as what is missing is one asked for:
    /// those scopes alone, for `ttl_seconds`, and given back as soon as `ask` was answered,
    /// unless the grant says to hold it and the operation was admitted.
    pub(crate) fn gated<T>(
        &self,
        client: &mut ControlClient,
        id: &ManagedSessionId,
        expected_revision: u64,
        grant: Grant,
        mut ask: impl FnMut(&mut ControlClient) -> ControlResult<T>,
    ) -> Result<T, SessionError> {
        let Grant {
            scopes,
            ttl_seconds,
            hold,
        } = grant;
        let scope = scopes[0];
        match ask(client) {
            Ok(value) => return Ok(value),
            Err(ControlError::GateDenied) => (),
            Err(error) => return Err(SessionError::of(error)),
        }
        let reason = gate(client, id, scope);
        if !matches!(
            reason,
            Some(GateReason::LeaseMissing | GateReason::LeaseNotActive | GateReason::LeaseExpired)
        ) {
            return Err(denied(reason));
        }
        let grant = LeaseGrant {
            intent_id: fresh(),
            session_id: id.clone(),
            expected_session_revision: expected_revision,
            scopes: scopes.to_vec(),
            ttl_seconds,
        };
        // Recorded before it is sent: if no answer comes back, the next call for this
        // session finds out whether it was taken.
        let now = Instant::now();
        let mut slots = self.slots();
        // A record nobody came back for describes a lease that has expired: it goes.
        slots.retain(|_, slot| slot.until > now);
        slots.insert(
            key(id),
            LeaseSlot {
                grant: grant.clone(),
                lease: None,
                revoke: fresh(),
                until: now + Duration::from_secs(ttl_seconds.into()) + LEASE_MARGIN,
                held: None,
            },
        );
        drop(slots);
        let lease = match client.lease_grant(grant) {
            Ok(lease) => lease,
            Err(error) => {
                if !SessionError::unknown(error) {
                    self.slots().remove(&key(id));
                }
                return Err(SessionError::of(error));
            }
        };
        self.inner.leases.fetch_add(1, Ordering::AcqRel);
        self.granted(id, &lease);
        let result = ask(client);
        // Read while the lease still stands, so the reason is the one that denied.
        let reason = matches!(result, Err(ControlError::GateDenied))
            .then(|| gate(client, id, scope))
            .flatten();
        // An answer that never came may be an admission: a lease to be held stays, and
        // the same intent tells later what was admitted under it.
        let admitted = match &result {
            Ok(_) => true,
            Err(error) => SessionError::unknown(*error),
        };
        if hold && admitted {
            if let Some(slot) = self.slots().get_mut(&key(id)) {
                slot.held = Some(Held { turn: None });
            }
        } else {
            // The lease was asked for this one action.
            self.release(client, id);
        }
        match result {
            Ok(value) => Ok(value),
            Err(ControlError::GateDenied) => Err(denied(reason)),
            Err(error) => Err(SessionError::of(error)),
        }
    }

    fn slots(&self) -> std::sync::MutexGuard<'_, HashMap<String, LeaseSlot>> {
        self.inner
            .slots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
    fn granted(&self, id: &ManagedSessionId, lease: &ExecutionLease) {
        if let Some(slot) = self.slots().get_mut(&key(id)) {
            slot.lease = Some((lease.lease_id.clone(), lease.revision));
        }
    }
    /// Asks the authority to revoke the lease it granted this desktop for the session.
    /// Only a lease whose identity came back from this desktop's own grant is ever named.
    /// An answer, whatever it says, ends it; without one the lease stays on record.
    fn release(&self, client: &mut ControlClient, id: &ManagedSessionId) {
        let Some((lease_id, expected_revision, intent_id)) =
            self.slots().get(&key(id)).and_then(|slot| {
                let (lease, revision) = slot.lease.clone()?;
                Some((lease, revision, slot.revoke.clone()))
            })
        else {
            return;
        };
        match client.lease_revoke(LeaseRevoke {
            intent_id,
            lease_id,
            expected_revision,
        }) {
            Err(error) if SessionError::unknown(error) => (),
            _ => {
                self.slots().remove(&key(id));
            }
        }
    }
    /// The lease this desktop keeps for a turn of the session, if it keeps one.
    pub(crate) fn held(&self, id: &ManagedSessionId) -> Option<Held> {
        self.slots()
            .get(&key(id))
            .and_then(|slot| slot.held.clone())
    }
    /// Names the turn that was admitted under the lease being kept.
    pub(crate) fn hold_for(&self, id: &ManagedSessionId, turn: &ManagedTurnId) {
        if let Some(held) = self
            .slots()
            .get_mut(&key(id))
            .and_then(|slot| slot.held.as_mut())
        {
            held.turn = Some(turn.clone());
        }
    }
    /// Gives back the lease kept for a turn: the turn has ended, or was never admitted.
    /// Without an answer it stays on record, no longer kept, and is settled like any other.
    pub(crate) fn release_held(&self, client: &mut ControlClient, id: &ManagedSessionId) {
        match self.slots().get_mut(&key(id)) {
            Some(slot) if slot.held.is_some() => slot.held = None,
            _ => return,
        }
        self.settle(client, id);
    }
    /// Settles what an earlier call for this session could not: a grant or a revoke
    /// whose answer never came. Nothing here is reported: it ends when the authority
    /// answers or when the lease has expired by itself. A lease kept for a turn is not
    /// touched.
    pub(crate) fn settle(&self, client: &mut ControlClient, id: &ManagedSessionId) {
        let Some((grant, known, until)) = self
            .slots()
            .get(&key(id))
            .filter(|slot| slot.held.is_none() || Instant::now() >= slot.until)
            .map(|slot| (slot.grant.clone(), slot.lease.is_some(), slot.until))
        else {
            return;
        };
        if Instant::now() >= until {
            self.slots().remove(&key(id));
            return;
        }
        if !known {
            // A grant that was never taken would be granted by sending it again. Only
            // while a lease stands can it be this one, and then the same intent names it.
            let standing = client.lease_active(id.clone());
            let answer = match standing {
                Ok(Some(_)) => client.lease_grant(grant),
                Ok(None) => Err(ControlError::LeaseNotFound),
                Err(error) => Err(error),
            };
            match answer {
                Ok(lease) => self.granted(id, &lease),
                Err(error) if SessionError::unknown(error) => return,
                Err(_) => {
                    self.slots().remove(&key(id));
                    return;
                }
            }
        }
        self.release(client, id);
    }
}

fn receipt(session: ManagedSessionRecord) -> SessionReceipt {
    SessionReceipt {
        id: session.managed_session_id,
    }
}

/// Sessions are told apart whatever the case of their identity.
fn key(id: &ManagedSessionId) -> String {
    id.as_str().to_ascii_lowercase()
}

/// An identity for a request that is not the user's action.
fn fresh() -> WireUuid {
    WireUuid::try_from(uuid::Uuid::new_v4().to_string()).expect("generated UUID")
}

/// A managed session is at revision one (prepared) or two (closed).
pub(crate) fn session_revision(revision: u64) -> Result<(), SessionError> {
    if (1..=2).contains(&revision) {
        Ok(())
    } else {
        Err(SessionError::new(SessionFailure::Invalid))
    }
}

/// Why the gate denies the action right now. Informational: it explains a refusal the
/// authority already made and authorizes nothing.
fn gate(
    client: &mut ControlClient,
    id: &ManagedSessionId,
    action: ExecutionScope,
) -> Option<GateReason> {
    client
        .gate_status(GateQuery {
            session_id: id.clone(),
            action,
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

#[cfg(test)]
impl SessionService {
    /// As if every lease on record had outlived its lifetime.
    pub(super) fn age_leases_for_test(&self) {
        let now = Instant::now();
        for slot in self.slots().values_mut() {
            slot.until = now;
        }
    }
    pub(super) fn leases_on_record_for_test(&self) -> usize {
        self.slots().len()
    }
}
