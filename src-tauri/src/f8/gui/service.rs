//! The desktop's door to a protocol-2 authority: reads and intents over three links.
//!
//! **Inactive by default.** `F8Gui::inactive()` holds no connector: every call answers
//! `inactive` and nothing is sent or opened. Only Rust code that owns a `Connector` can make
//! one active; no environment variable, command argument, window request or file selects one.
//! The installed desktop is built with the inactive one.
//!
//! **Three links, three jobs.** `Control` carries reads and intents, one exchange at a time.
//! `Follow` carries the paced read of a running turn, so following never waits behind an
//! intent and an intent never waits behind a follow. `Watch` carries the authority's own
//! bounded wait for the next durable commit, on a watcher slot of its own. Each link greets
//! the authority in protocol 2 on its own connection, is dropped on any transport or
//! correlation failure, and reconnects only after a bounded, doubling delay; a refusal by
//! the authority never drops a link.
//!
//! **Intents.** The window names each user action with one intent identity and sends it
//! again, unchanged, when it could not learn the outcome. The authority's receipt makes that
//! the same action. This service never retries on its own and never invents an identity.
//!
//! **Fences.** A mutation against a runtime carries the fence the window was shown: the
//! generation and instance it observed. The fence grants nothing; the authority compares it
//! with the launch it holds and refuses a stale one. This service neither fills a fence in
//! nor repairs a refused one: the window reads again.
//!
//! **Authority instance.** Every payload names the instance it was read from. A greeting that
//! finds another instance than the one before bumps `epoch`; nothing kept for the old one
//! (turn index) survives it.
use super::{
    client::{Answer, CallError, Connector, F8Client},
    view::*,
};
use crate::{
    authority::{Connection, Fault},
    contracts::{
        Sequence, WireUuid,
        control::{
            ApprovalDecision, ApprovalGet, ApprovalPending, ApprovalRespond, AuditRead, AuditWatch,
            BindingAbandon, BindingCreate, BindingResume, BindingState, Close, ControlError as E,
            Empty, ExecutionScope, GateQuery, Get, LeaseActive, LeaseGrant, LeaseRenew,
            LeaseRevoke, List, MAX_PAGE, ManagedSessionRecord, ModelCursor, Operation, Prepare,
            ResultValue, RuntimeModels, RuntimeMutation, RuntimeSafetyProfile, RuntimeStatus,
            RuntimeTarget, TurnCancel, TurnEvents, TurnGet, TurnStart, validate_prompt,
            validate_workspace_text,
        },
        identities::{ExecutionLeaseId, ManagedApprovalId, ManagedSessionId, ManagedTurnId},
    },
    events::{EventRow, project, window},
    f8::protocol::{Projection, Target},
    sessions::{AgentMode, entry_of},
    turns::{ApprovalDetail, ApprovalNotice, TurnEvent, TurnSummary},
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Protocol pages one listing walks at most: the authority holds 128 sessions a lifetime.
const WALK_PAGES: usize = 8;
/// Longest one turn read waits for something new before it answers with what there is.
const FOLLOW_WAIT: Duration = Duration::from_millis(1500);
/// Pause between two looks at a stream that had nothing new.
const FOLLOW_PACE: Duration = Duration::from_millis(100);
/// Stream events asked for at once. The protocol's own bound.
const EVENT_PAGE: u16 = 50;
/// Longest one audit wait holds a watcher slot before it answers empty.
pub const WATCH_WAIT_MS: u32 = 20_000;
/// Audit events the first read of the turn index looks back over, at most.
const SCAN_EVENTS: u64 = 2_000;
/// Turns remembered for one session, and sessions remembered.
const KEEP_TURNS: usize = 32;
const KEEP_SESSIONS: usize = 64;
/// Model pages one catalogue reads at most (twenty entries each; the authority holds fifty).
const MODEL_PAGES: usize = 4;
/// Workspaces remembered by intent, so a retry names the folder the first attempt did.
const KEEP_WORKSPACES: usize = 16;
/// How long a connection may rest and still be used. The authority drops one that is silent
/// for five seconds (its read timeout); this is well inside that.
const REUSE_LIMIT: Duration = Duration::from_secs(2);
/// The largest integer a JavaScript number holds exactly.
const JS_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const FIRST_DELAY: Duration = Duration::from_millis(250);
const DELAY_LIMIT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Control,
    Follow,
    Watch,
}

struct Link {
    client: Option<F8Client>,
    /// When the connection last carried an exchange, answered or refused.
    last_used: Option<Instant>,
    state: Connection,
    fault: Option<Fault>,
    failures: u32,
    retry_at: Option<Instant>,
}
impl Link {
    fn new() -> Self {
        Self {
            client: None,
            last_used: None,
            state: Connection::Waiting,
            fault: None,
            failures: 0,
            retry_at: None,
        }
    }
    fn delay(&mut self) -> Duration {
        let delay = FIRST_DELAY
            .checked_mul(1u32.checked_shl(self.failures).unwrap_or(u32::MAX))
            .map_or(DELAY_LIMIT, |d| d.min(DELAY_LIMIT));
        self.failures = self.failures.saturating_add(1);
        delay
    }
    fn fail(&mut self, fault: Fault) {
        self.client = None;
        self.state = Connection::Waiting;
        self.fault = Some(fault);
        self.retry_at = Some(Instant::now() + self.delay());
    }
    fn retry_in(&self) -> Option<u64> {
        self.retry_at.map(|at| {
            at.saturating_duration_since(Instant::now())
                .as_millis()
                .min(u128::from(u64::MAX)) as u64
        })
    }
}

/// Which turns the durable stream says each session has. An index, not state: a turn
/// listed here is read from the authority before anything is said about it.
#[derive(Default)]
struct Index {
    authority: Option<WireUuid>,
    stream: Option<WireUuid>,
    scanned: u64,
    /// A read began after where the one before it ended, or after the beginning.
    partial: bool,
    turns: HashMap<String, VecDeque<ManagedTurnId>>,
    order: VecDeque<String>,
}
impl Index {
    fn admit(&mut self, session: &str, turn: ManagedTurnId) {
        if !self.turns.contains_key(session) {
            if self.order.len() == KEEP_SESSIONS
                && let Some(oldest) = self.order.pop_front()
            {
                self.turns.remove(&oldest);
                self.partial = true;
            }
            self.order.push_back(session.to_owned());
        }
        let turns = self.turns.entry(session.to_owned()).or_default();
        if turns.contains(&turn) {
            return;
        }
        if turns.len() == KEEP_TURNS {
            turns.pop_front();
            self.partial = true;
        }
        turns.push_back(turn);
    }
}

/// Counters of what the service did. For tests and diagnostics: none of it is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct F8Stats {
    pub connections: u64,
    pub epoch: u64,
}

struct Inner {
    connector: Option<Arc<dyn Connector>>,
    stop: Arc<AtomicBool>,
    links: [Mutex<Link>; 3],
    index: Mutex<Index>,
    workspaces: Mutex<VecDeque<(WireUuid, String)>>,
    last_authority: Mutex<Option<WireUuid>>,
    connections: AtomicU64,
    epoch: AtomicU64,
}

/// Application-lifetime door to a protocol-2 authority. Cloning shares it.
#[derive(Clone)]
pub struct F8Gui {
    inner: Arc<Inner>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
fn fault_of(error: E) -> Fault {
    match error {
        E::ProtocolVersionUnsupported | E::StorageIncompatible => Fault::Incompatible,
        E::UnsafePath | E::PeerRejected => Fault::UnsafePath,
        E::TransportError | E::Timeout | E::TruncatedFrame => Fault::Lost,
        _ => Fault::Malformed,
    }
}
impl F8Gui {
    /// The installed configuration: nothing is sent, opened or offered.
    pub fn inactive() -> Self {
        Self::build(None)
    }
    /// An explicit local activation. Whoever calls this owns the connector.
    pub fn local(connector: impl Connector) -> Self {
        Self::build(Some(Arc::new(connector)))
    }
    fn build(connector: Option<Arc<dyn Connector>>) -> Self {
        Self {
            inner: Arc::new(Inner {
                connector,
                stop: Arc::new(AtomicBool::new(false)),
                links: [
                    Mutex::new(Link::new()),
                    Mutex::new(Link::new()),
                    Mutex::new(Link::new()),
                ],
                index: Mutex::new(Index::default()),
                workspaces: Mutex::new(VecDeque::new()),
                last_authority: Mutex::new(None),
                connections: AtomicU64::new(0),
                epoch: AtomicU64::new(0),
            }),
        }
    }
    pub fn is_active(&self) -> bool {
        self.inner.connector.is_some()
    }
    pub fn stats(&self) -> F8Stats {
        F8Stats {
            connections: self.inner.connections.load(Ordering::Acquire),
            epoch: self.inner.epoch.load(Ordering::Acquire),
        }
    }
    /// Cancels the exchange in flight and refuses every later call. Idempotent. Nothing is
    /// sent to the authority.
    pub fn shutdown(&self) {
        self.inner.stop.store(true, Ordering::Release);
        for link in &self.inner.links {
            // A link mid-exchange is cancelled by `stop`; one idle is released here.
            if let Ok(mut link) = link.try_lock() {
                link.client = None;
                link.state = Connection::Stopped;
            }
        }
    }
    fn stopping(&self) -> bool {
        self.inner.stop.load(Ordering::Acquire)
    }
    fn link(&self, role: Role) -> &Mutex<Link> {
        &self.inner.links[role as usize]
    }

    /// Runs `work` on a greeted connection of `role`, opening one when the delay allows.
    fn with<T>(
        &self,
        role: Role,
        work: impl FnOnce(&mut F8Client) -> Result<T, CallError>,
    ) -> Result<T, F8Error> {
        let Some(connector) = &self.inner.connector else {
            return Err(F8Error::new(F8Failure::Inactive));
        };
        if self.stopping() {
            return Err(F8Error::new(F8Failure::Unavailable));
        }
        let mut link = lock(self.link(role));
        // The authority closes a connection that has asked for nothing for a while, and a
        // window is idle most of the time. A connection that rested longer than that is let
        // go and made again, instead of failing the next thing the user does.
        if link.client.is_some()
            && link
                .last_used
                .is_none_or(|used| used.elapsed() > REUSE_LIMIT)
        {
            link.client = None;
        }
        if link.client.is_none() {
            if link.retry_at.is_some_and(|at| Instant::now() < at) {
                return Err(F8Error {
                    retry_in_ms: link.retry_in(),
                    ..F8Error::new(F8Failure::Unavailable)
                });
            }
            link.state = Connection::Connecting;
            let greeted = connector
                .connect()
                .and_then(|stream| F8Client::greet(stream, self.inner.stop.clone()));
            match greeted {
                Ok(client) => {
                    self.inner.connections.fetch_add(1, Ordering::AcqRel);
                    self.note_authority(client.authority());
                    link.state = Connection::Connected;
                    link.fault = None;
                    link.failures = 0;
                    link.retry_at = None;
                    link.client = Some(client);
                }
                Err(error) => {
                    link.fail(fault_of(error));
                    return Err(F8Error {
                        retry_in_ms: link.retry_in(),
                        ..F8Error::of(error)
                    });
                }
            }
        }
        let client = link
            .client
            .as_mut()
            .ok_or(F8Error::new(F8Failure::Internal))?;
        let outcome = work(client);
        link.last_used = Some(Instant::now());
        match outcome {
            Ok(value) => Ok(value),
            Err(error) => {
                if error.broken() {
                    link.fail(fault_of(error.code()));
                }
                Err(F8Error {
                    retry_in_ms: if error.broken() {
                        link.retry_in()
                    } else {
                        None
                    },
                    ..F8Error::of(error.code())
                })
            }
        }
    }
    fn note_authority(&self, authority: &WireUuid) {
        let mut last = lock(&self.inner.last_authority);
        if last.as_ref().is_some_and(|known| known != authority) {
            self.inner.epoch.fetch_add(1, Ordering::AcqRel);
            *lock(&self.inner.index) = Index::default();
            lock(&self.inner.workspaces).clear();
        }
        *last = Some(authority.clone());
    }

    /// The link and the authority as they are. It never fails: what failed is the content.
    pub fn state(&self) -> StateView {
        let active = self.is_active();
        let status = if active {
            self.with(Role::Control, |client| {
                client
                    .call(Operation::Status(Empty {}), Target::None)
                    .map(|answer| match answer.value {
                        ResultValue::Status(status) => Some(status),
                        _ => None,
                    })
            })
            .ok()
            .flatten()
        } else {
            None
        };
        let link = lock(self.link(Role::Control));
        StateView {
            schema_version: SCHEMA_VERSION,
            active,
            link: LinkView {
                state: if active {
                    link.state
                } else {
                    Connection::Stopped
                },
                fault: link.fault,
                retry_in_ms: link.client.is_none().then(|| link.retry_in()).flatten(),
            },
            authority: status.map(|s| AuthorityView {
                instance_id: s.authority_instance_id,
                service_version: s.service_version,
                protocol_version: s.protocol_version,
                storage_schema_version: s.storage_schema_version,
                stream_id: s.stream_id,
                last_sequence: s.last_sequence,
                prepared_count: s.prepared_count,
                closed_count: s.closed_count,
            }),
            epoch: self.inner.epoch.load(Ordering::Acquire),
        }
    }
    /// Forgets the delay of every link and reads the state: the user asked to try now.
    pub fn reconnect(&self) -> StateView {
        for link in &self.inner.links {
            let mut link = lock(link);
            if link.client.is_none() {
                link.retry_at = None;
                link.failures = 0;
            }
        }
        self.state()
    }

    // ------------------------------------------------------------------ reads

    /// The newest sessions, or those created before `before`, each with its runtime.
    pub fn sessions(&self, before: Option<Sequence>) -> Result<SessionsView, F8Error> {
        self.with(Role::Control, |client| {
            // The head first, then what it describes: the page is never older than it.
            let status = status(client)?;
            let mut records: Vec<ManagedSessionRecord> = Vec::new();
            let mut after: Option<Sequence> = None;
            let mut walked = false;
            for _ in 0..WALK_PAGES {
                let ResultValue::Sessions { sessions, next } = client
                    .call(
                        Operation::List(List {
                            after,
                            limit: MAX_PAGE,
                        }),
                        Target::None,
                    )?
                    .value
                else {
                    return Err(broken());
                };
                for session in sessions {
                    // A page that does not move forward from its cursor is not a list.
                    if after.is_some_and(|cursor| session.created_sequence <= cursor) {
                        return Err(broken());
                    }
                    after = Some(session.created_sequence);
                    records.push(session);
                }
                if next.is_none() {
                    walked = true;
                    break;
                }
                if next != after {
                    return Err(broken());
                }
            }
            if !walked {
                return Err(CallError::Broken(E::CorrelationError));
            }
            let total = records.len() as u32;
            records.retain(|r| before.is_none_or(|limit| r.created_sequence < limit));
            let older = records.len() > SESSION_PAGE;
            let shown: Vec<ManagedSessionRecord> =
                records.into_iter().rev().take(SESSION_PAGE).collect();
            let mut rows = Vec::with_capacity(shown.len());
            for record in shown {
                let runtime = runtime_view(client, &record.managed_session_id)?;
                rows.push(SessionRow::of(record, runtime));
            }
            Ok(SessionsView {
                schema_version: SCHEMA_VERSION,
                authority: client.authority().clone(),
                head: status.last_sequence,
                older: older.then(|| rows.last().map(|r| r.position)).flatten(),
                sessions: rows,
                capacity: total,
            })
        })
    }

    /// One session with its runtime, binding, lease, gates, turns and pending approval.
    pub fn session(&self, id: ManagedSessionId) -> Result<SessionDetail, F8Error> {
        self.with(Role::Control, |client| {
            let record = match client
                .call(
                    Operation::Get(Get {
                        managed_session_id: id.clone(),
                    }),
                    Target::None,
                )?
                .value
            {
                ResultValue::Session(record) if record.managed_session_id == id => record,
                _ => return Err(broken()),
            };
            let runtime = runtime_view(client, &id)?;
            let binding = binding_view(client, &id)?;
            let lease = match client
                .call(
                    Operation::LeaseActive(LeaseActive {
                        session_id: id.clone(),
                    }),
                    Target::None,
                )?
                .value
            {
                ResultValue::ActiveLease(lease) => lease.map(LeaseView::of),
                _ => return Err(broken()),
            };
            let mut gates = Vec::new();
            for action in [
                ExecutionScope::TurnStart,
                ExecutionScope::CommandExecute,
                ExecutionScope::FileChange,
            ] {
                let ResultValue::GateStatus(gate) = client
                    .call(
                        Operation::GateStatus(GateQuery {
                            session_id: id.clone(),
                            action,
                        }),
                        Target::None,
                    )?
                    .value
                else {
                    return Err(broken());
                };
                if gate.session_id != id || gate.action != action {
                    return Err(broken());
                }
                gates.push(GateRow {
                    action,
                    reason: gate.reason,
                });
            }
            let (known, partial) = self.turn_ids(client, &id)?;
            let mut turns = Vec::new();
            for turn in known.iter().rev().take(TURN_PAGE) {
                turns.push(turn_row(client, turn)?);
            }
            let approval = match turns.first() {
                Some(newest) if !newest.turn.state.terminal() => {
                    pending_approval(client, &newest.turn.id, &binding)?
                }
                _ => None,
            };
            Ok(SessionDetail {
                schema_version: SCHEMA_VERSION,
                authority: client.authority().clone(),
                session: SessionRow::of(record, runtime),
                binding,
                lease,
                gates,
                turns,
                turns_partial: partial,
                approval,
            })
        })
    }

    /// The turns the durable stream says were admitted for `session`, oldest first.
    fn turn_ids(
        &self,
        client: &mut F8Client,
        session: &ManagedSessionId,
    ) -> Result<(Vec<ManagedTurnId>, bool), CallError> {
        let mut index = lock(&self.inner.index);
        scan(client, &mut index)?;
        let turns = index
            .turns
            .get(session.as_str())
            .map(|turns| turns.iter().cloned().collect())
            .unwrap_or_default();
        Ok((turns, index.partial))
    }

    // ------------------------------------------------------------------ session intents

    pub fn prepare(&self, intent: WireUuid) -> Result<SessionReceipt, F8Error> {
        let target = RuntimeTarget::codex();
        self.with(Role::Control, |client| {
            let ResultValue::Session(record) = client
                .call(
                    Operation::Prepare(Prepare {
                        intent_id: intent,
                        adapter_id: target.adapter_id,
                        runtime_kind: target.runtime_kind,
                    }),
                    Target::None,
                )?
                .value
            else {
                return Err(broken());
            };
            Ok(SessionReceipt {
                schema_version: SCHEMA_VERSION,
                authority: client.authority().clone(),
                id: record.managed_session_id,
            })
        })
    }

    pub fn close(
        &self,
        intent: WireUuid,
        fence: FenceView,
        expected_revision: u64,
    ) -> Result<SessionReceipt, F8Error> {
        let session = fence.session.clone();
        self.with(Role::Control, |client| {
            let ResultValue::Session(record) = client
                .call(
                    Operation::Close(Close {
                        intent_id: intent,
                        managed_session_id: session.clone(),
                        expected_revision,
                    }),
                    Target::Fenced(fence.fence()),
                )?
                .value
            else {
                return Err(broken());
            };
            if record.managed_session_id != session {
                return Err(broken());
            }
            Ok(SessionReceipt {
                schema_version: SCHEMA_VERSION,
                authority: client.authority().clone(),
                id: record.managed_session_id,
            })
        })
    }

    // ------------------------------------------------------------------ runtime

    pub fn runtime_start(
        &self,
        intent: WireUuid,
        session: ManagedSessionId,
        expected_revision: u64,
        expected_generation: u64,
        mode: AgentMode,
        workspace: Option<String>,
    ) -> Result<RuntimeReceipt, F8Error> {
        let (profile, workspace_root) = match (mode, workspace) {
            (AgentMode::Inference, None) => (None, None),
            (AgentMode::LocalTools, Some(root)) => {
                validate_workspace_text(&root).map_err(F8Error::of)?;
                (Some(RuntimeSafetyProfile::CodexLocalToolsV1), Some(root))
            }
            _ => return Err(F8Error::new(F8Failure::Invalid)),
        };
        let target = RuntimeTarget::codex();
        self.runtime_mutation(
            session.clone(),
            Operation::RuntimeStart(RuntimeMutation {
                intent_id: intent,
                adapter_id: target.adapter_id,
                runtime_kind: target.runtime_kind,
                expected_revision: Some(expected_revision),
                safety_profile: profile,
                workspace_root,
            }),
            Target::Runtime {
                session,
                generation: Some(expected_generation),
                instance: None,
            },
        )
    }

    pub fn runtime_stop(
        &self,
        intent: WireUuid,
        fence: FenceView,
        expected_revision: u64,
    ) -> Result<RuntimeReceipt, F8Error> {
        let target = RuntimeTarget::codex();
        self.runtime_mutation(
            fence.session.clone(),
            Operation::RuntimeStop(RuntimeMutation {
                intent_id: intent,
                adapter_id: target.adapter_id,
                runtime_kind: target.runtime_kind,
                expected_revision: Some(expected_revision),
                safety_profile: None,
                workspace_root: None,
            }),
            Target::Runtime {
                session: fence.session,
                generation: Some(fence.generation),
                instance: fence.instance,
            },
        )
    }

    fn runtime_mutation(
        &self,
        session: ManagedSessionId,
        operation: Operation,
        target: Target,
    ) -> Result<RuntimeReceipt, F8Error> {
        self.with(Role::Control, |client| {
            let answer = client.call(operation, target)?;
            let (ResultValue::RuntimeControl(record), Projection::Runtime { generation, .. }) =
                (answer.value, answer.projection)
            else {
                return Err(broken());
            };
            Ok(RuntimeReceipt {
                schema_version: SCHEMA_VERSION,
                authority: client.authority().clone(),
                session,
                desired: record.desired_state,
                revision: record.revision,
                generation,
            })
        })
    }

    /// What the runtime of one launch listed, with the model the session reports using.
    pub fn models(&self, fence: FenceView) -> Result<ModelsView, F8Error> {
        let Some(instance) = fence.instance.clone() else {
            return Err(F8Error::new(F8Failure::RuntimeUnavailable));
        };
        if fence.generation == 0 {
            return Err(F8Error::new(F8Failure::RuntimeUnavailable));
        }
        let target = RuntimeTarget::codex();
        self.with(Role::Control, |client| {
            let mut entries = Vec::new();
            let mut complete = None;
            let mut cursor: Option<ModelCursor> = None;
            let mut version = None;
            for _ in 0..MODEL_PAGES {
                let answer = client.call(
                    Operation::RuntimeModels(RuntimeModels {
                        adapter_id: target.adapter_id.clone(),
                        runtime_kind: target.runtime_kind,
                        cursor: cursor.take(),
                        limit: 20,
                    }),
                    Target::Runtime {
                        session: fence.session.clone(),
                        generation: Some(fence.generation),
                        instance: Some(instance.clone()),
                    },
                )?;
                let (
                    ResultValue::RuntimeModels {
                        models,
                        offers,
                        complete: whole,
                        next,
                        ..
                    },
                    Projection::Runtime {
                        identity: Some(identity),
                        ..
                    },
                ) = (answer.value, answer.projection)
                else {
                    return Err(broken());
                };
                version = Some(identity.observed_version);
                complete = whole;
                for (index, model) in models.iter().enumerate() {
                    let value = serde_json::to_value(model)
                        .map_err(|_| CallError::Local(E::InternalError))?;
                    entries.extend(entry_of(&value, offers.get(index)));
                }
                match next {
                    Some(next) => cursor = Some(next),
                    None => break,
                }
            }
            if cursor.is_some() {
                complete = Some(false);
            }
            let observed = match binding_view(client, &fence.session) {
                Ok(binding) => binding.observed_model,
                Err(CallError::Broken(error)) => return Err(CallError::Broken(error)),
                Err(_) => None,
            };
            Ok(ModelsView {
                schema_version: SCHEMA_VERSION,
                authority: client.authority().clone(),
                fence: fence.clone(),
                version: version.ok_or_else(broken)?,
                complete,
                entries,
                observed,
            })
        })
    }

    // ------------------------------------------------------------------ leases

    pub fn lease_grant(
        &self,
        intent: WireUuid,
        fence: FenceView,
        expected_session_revision: u64,
        scopes: Vec<ExecutionScope>,
        ttl_seconds: u32,
    ) -> Result<LeaseReceipt, F8Error> {
        let mut ordered = scopes;
        ordered.sort();
        let session = fence.session.clone();
        self.lease_call(
            Operation::LeaseGrant(LeaseGrant {
                intent_id: intent,
                session_id: session.clone(),
                expected_session_revision,
                scopes: ordered,
                ttl_seconds,
            }),
            Target::Fenced(fence.fence()),
            Some(session),
        )
    }
    pub fn lease_renew(
        &self,
        intent: WireUuid,
        fence: FenceView,
        lease: ExecutionLeaseId,
        expected_revision: u64,
        ttl_seconds: u32,
    ) -> Result<LeaseReceipt, F8Error> {
        let session = fence.session.clone();
        self.lease_call(
            Operation::LeaseRenew(LeaseRenew {
                intent_id: intent,
                lease_id: lease,
                expected_revision,
                ttl_seconds,
            }),
            Target::Fenced(fence.fence()),
            Some(session),
        )
    }
    /// Revoking needs no live runtime: it is how a lease is taken back after one is gone.
    pub fn lease_revoke(
        &self,
        intent: WireUuid,
        lease: ExecutionLeaseId,
        expected_revision: u64,
    ) -> Result<LeaseReceipt, F8Error> {
        self.lease_call(
            Operation::LeaseRevoke(LeaseRevoke {
                intent_id: intent,
                lease_id: lease,
                expected_revision,
            }),
            Target::None,
            None,
        )
    }
    fn lease_call(
        &self,
        operation: Operation,
        target: Target,
        session: Option<ManagedSessionId>,
    ) -> Result<LeaseReceipt, F8Error> {
        self.with(Role::Control, |client| {
            let ResultValue::Lease(lease) = client.call(operation, target)?.value else {
                return Err(broken());
            };
            if session.as_ref().is_some_and(|s| *s != lease.session_id) {
                return Err(broken());
            }
            Ok(LeaseReceipt {
                schema_version: SCHEMA_VERSION,
                authority: client.authority().clone(),
                lease: LeaseView::of(lease),
            })
        })
    }

    // ------------------------------------------------------------------ binding

    /// Binds `workspace`. The caller has already established that the user chose it; the
    /// authority decides whether it may be a workspace.
    pub fn binding_create(
        &self,
        intent: WireUuid,
        fence: FenceView,
        expected_session_revision: u64,
        workspace: String,
    ) -> Result<BindingReceipt, F8Error> {
        validate_workspace_text(&workspace).map_err(F8Error::of)?;
        // A retry of the same intent is checked against the folder the first attempt named,
        // even if the folder has been replaced since.
        let canonical = self.canonical(&intent, &workspace)?;
        let session = fence.session.clone();
        self.with(Role::Control, |client| {
            let answer = client.call_binding_create(
                Operation::BindingCreate(BindingCreate {
                    intent_id: intent,
                    managed_session_id: session.clone(),
                    expected_session_revision,
                    workspace_root: workspace,
                }),
                Target::Fenced(fence.fence()),
                &canonical,
            )?;
            binding_receipt(client, answer, session)
        })
    }
    pub fn binding_resume(
        &self,
        intent: WireUuid,
        fence: FenceView,
        expected_session_revision: u64,
    ) -> Result<BindingReceipt, F8Error> {
        let session = fence.session.clone();
        self.with(Role::Control, |client| {
            let answer = client.call(
                Operation::BindingResume(BindingResume {
                    intent_id: intent,
                    managed_session_id: session.clone(),
                    expected_session_revision,
                }),
                Target::Fenced(fence.fence()),
            )?;
            binding_receipt(client, answer, session)
        })
    }
    pub fn binding_abandon(
        &self,
        intent: WireUuid,
        fence: FenceView,
        operation: WireUuid,
    ) -> Result<AbandonReceipt, F8Error> {
        let session = fence.session.clone();
        self.with(Role::Control, |client| {
            let ResultValue::BindingAbandoned(done) = client
                .call(
                    Operation::BindingAbandon(BindingAbandon {
                        intent_id: intent,
                        managed_session_id: session.clone(),
                        operation_id: operation.clone(),
                    }),
                    Target::Fenced(fence.fence()),
                )?
                .value
            else {
                return Err(broken());
            };
            if done.operation_id != operation {
                return Err(broken());
            }
            Ok(AbandonReceipt {
                schema_version: SCHEMA_VERSION,
                authority: client.authority().clone(),
                session,
            })
        })
    }
    /// The canonical workspace the first attempt of `intent` named, or `workspace` resolved
    /// now when this is the first. Resolution happens here and only here.
    fn canonical(&self, intent: &WireUuid, workspace: &str) -> Result<String, F8Error> {
        let mut known = lock(&self.inner.workspaces);
        if let Some((_, canonical)) = known.iter().find(|(id, _)| id == intent) {
            return Ok(canonical.clone());
        }
        let canonical = std::fs::canonicalize(workspace)
            .ok()
            .and_then(|path| path.into_os_string().into_string().ok())
            .filter(|text| validate_workspace_text(text).is_ok())
            .ok_or(F8Error::new(F8Failure::WorkspaceInvalid))?;
        if known.len() == KEEP_WORKSPACES {
            known.pop_front();
        }
        known.push_back((intent.clone(), canonical.clone()));
        Ok(canonical)
    }

    // ------------------------------------------------------------------ turns

    pub fn turn_start(
        &self,
        intent: WireUuid,
        fence: FenceView,
        expected_session_revision: u64,
        prompt: String,
    ) -> Result<TurnReceipt, F8Error> {
        validate_prompt(&prompt).map_err(F8Error::of)?;
        let session = fence.session.clone();
        self.with(Role::Control, |client| {
            let ResultValue::Turn(turn) = client
                .call(
                    Operation::TurnStart(TurnStart {
                        intent_id: intent,
                        managed_session_id: session.clone(),
                        expected_session_revision,
                        prompt,
                    }),
                    Target::Fenced(fence.fence()),
                )?
                .value
            else {
                return Err(broken());
            };
            if turn.managed_session_id != session {
                return Err(broken());
            }
            Ok(TurnReceipt {
                schema_version: SCHEMA_VERSION,
                authority: client.authority().clone(),
                id: turn.managed_turn_id,
            })
        })
    }

    pub fn turn_cancel(
        &self,
        intent: WireUuid,
        fence: FenceView,
        turn: ManagedTurnId,
        expected_revision: u64,
    ) -> Result<TurnReceipt, F8Error> {
        let session = fence.session.clone();
        self.with(Role::Control, |client| {
            let ResultValue::Turn(record) = client
                .call(
                    Operation::TurnCancel(TurnCancel {
                        intent_id: intent,
                        managed_turn_id: turn.clone(),
                        expected_revision,
                    }),
                    Target::Fenced(fence.fence()),
                )?
                .value
            else {
                return Err(broken());
            };
            if record.managed_turn_id != turn || record.managed_session_id != session {
                return Err(broken());
            }
            Ok(TurnReceipt {
                schema_version: SCHEMA_VERSION,
                authority: client.authority().clone(),
                id: turn,
            })
        })
    }

    /// One reading of a turn: stream events after `after`, then its canonical record.
    /// With `wait`, the answer is held while there is nothing new, for at most one bound.
    pub fn turn(
        &self,
        id: ManagedTurnId,
        after: Option<Sequence>,
        wait: bool,
    ) -> Result<TurnReading, F8Error> {
        let deadline = Instant::now() + if wait { FOLLOW_WAIT } else { Duration::ZERO };
        self.with(Role::Follow, |client| {
            let cursor = after;
            loop {
                let ResultValue::TurnEvents(page) = client
                    .call(
                        Operation::TurnEvents(TurnEvents {
                            managed_turn_id: id.clone(),
                            after_sequence: cursor,
                            limit: EVENT_PAGE,
                        }),
                        Target::None,
                    )?
                    .value
                else {
                    return Err(broken());
                };
                // Events come strictly after the cursor, each one after the one before.
                let mut last = cursor.map_or(0, Sequence::get);
                for event in &page.events {
                    if event.managed_turn_id != id || event.sequence.get() <= last {
                        return Err(broken());
                    }
                    last = event.sequence.get();
                }
                if page
                    .next_sequence
                    .is_some_and(|next| next.get() < cursor.map_or(0, Sequence::get))
                {
                    return Err(broken());
                }
                // The record is read after the events it comes with.
                let row = turn_row(client, &id)?;
                let ended = row.turn.state.terminal();
                let more = page.next_sequence.is_some_and(|next| {
                    page.latest_sequence
                        .is_some_and(|latest| next.get() < latest.get())
                });
                if !page.events.is_empty()
                    || page.gap
                    || ended
                    || Instant::now() >= deadline
                    || self.stopping()
                {
                    let next = page.events.last().map(|event| event.sequence).or(cursor);
                    let approval = if ended {
                        None
                    } else {
                        match client.call(
                            Operation::ApprovalPending(ApprovalPending {
                                managed_turn_id: id.clone(),
                            }),
                            Target::None,
                        ) {
                            Ok(answer) => match answer.value {
                                ResultValue::PendingApproval(pending) => pending
                                    .filter(|status| status.approval.managed_turn_id == id)
                                    .map(|status| ApprovalNotice::of(*status)),
                                _ => return Err(broken()),
                            },
                            // The turn may have ended between the two reads.
                            Err(CallError::Refused(E::TurnNotFound)) => None,
                            Err(error) => return Err(error),
                        }
                    };
                    return Ok(TurnReading {
                        schema_version: SCHEMA_VERSION,
                        authority: client.authority().clone(),
                        turn: row,
                        events: page.events.into_iter().map(TurnEvent::of).collect(),
                        next,
                        more,
                        gap: page.gap,
                        approval,
                    });
                }
                // The cursor stays: nothing new was read.
                std::thread::sleep(FOLLOW_PACE);
            }
        })
    }

    // ------------------------------------------------------------------ approvals

    /// One approval as the authority holds it now. `file` and `offset` select a file of a
    /// change set and a page of its diff.
    pub fn approval(
        &self,
        id: ManagedApprovalId,
        file: Option<u16>,
        offset: Option<u32>,
    ) -> Result<ApprovalDetail, F8Error> {
        self.with(Role::Control, |client| {
            let ResultValue::Approval(status) = client
                .call(
                    Operation::ApprovalStatus(ApprovalGet {
                        managed_approval_id: id.clone(),
                        file_index: file,
                        offset,
                    }),
                    Target::None,
                )?
                .value
            else {
                return Err(broken());
            };
            if status.approval.managed_approval_id != id {
                return Err(broken());
            }
            let binding = binding_view(client, &status.approval.managed_session_id)?;
            Ok(ApprovalDetail::of(*status, binding.workspace))
        })
    }

    /// One decision on one approval, at the revision the caller saw and under the fence it
    /// saw. `intent` names the user's action: sent again unchanged it is the same decision.
    pub fn approval_respond(
        &self,
        intent: WireUuid,
        fence: FenceView,
        id: ManagedApprovalId,
        expected_revision: u64,
        decision: ApprovalDecision,
    ) -> Result<DecisionReceipt, F8Error> {
        self.with(Role::Control, |client| {
            let ResultValue::ApprovalReceipt(receipt) = client
                .call(
                    Operation::ApprovalRespond(ApprovalRespond {
                        intent_id: intent,
                        managed_approval_id: id.clone(),
                        expected_revision,
                        decision,
                    }),
                    Target::Fenced(fence.fence()),
                )?
                .value
            else {
                return Err(broken());
            };
            if receipt.managed_approval_id != id {
                return Err(broken());
            }
            Ok(DecisionReceipt {
                schema_version: SCHEMA_VERSION,
                authority: client.authority().clone(),
                id: receipt.managed_approval_id,
                state: receipt.state,
            })
        })
    }

    // ------------------------------------------------------------------ audit

    /// The newest window of the durable stream.
    pub fn audit_latest(&self) -> Result<AuditView, F8Error> {
        self.audit(None)
    }
    /// The window before `before`, in the stream the caller read it from.
    pub fn audit_older(&self, stream: WireUuid, before: Sequence) -> Result<AuditView, F8Error> {
        self.audit(Some((stream, before)))
    }
    fn audit(&self, older: Option<(WireUuid, Sequence)>) -> Result<AuditView, F8Error> {
        use crate::events::EVENTS_PAGE;
        self.with(Role::Control, |client| {
            let status = status(client)?;
            let head = status.last_sequence.map_or(0, Sequence::get);
            let (upper, after) = match &older {
                None => (head, head.saturating_sub(EVENTS_PAGE)),
                Some((stream, before)) => {
                    // A position of another stream, or past this one's head, is history that
                    // changed: it is not read against this one.
                    if *stream != status.stream_id || before.get() > head + 1 {
                        return Err(CallError::Refused(E::StaleRevision));
                    }
                    let upper = before.get() - 1;
                    (upper, upper.saturating_sub(EVENTS_PAGE))
                }
            };
            let mut broken_link = None;
            let page = window(
                &mut |read| {
                    client
                        .call(Operation::AuditRead(read), Target::None)
                        .map(|a| a.value)
                        .map_err(|error| {
                            if error.broken() {
                                broken_link = Some(error.code());
                            }
                            error.code()
                        })
                },
                status.stream_id.clone(),
                head,
                after,
                upper,
            );
            if let Some(code) = broken_link {
                return Err(CallError::Broken(code));
            }
            let page = page
                .map_err(|error| CallError::Broken(error.cause.unwrap_or(E::CorrelationError)))?;
            // The head is an event: a stream that has one and shows none is not a stream.
            if older.is_none() && head > 0 && page.events.is_empty() {
                return Err(CallError::Broken(E::CorrelationError));
            }
            Ok(AuditView {
                schema_version: SCHEMA_VERSION,
                authority: client.authority().clone(),
                page,
            })
        })
    }

    /// Holds a watcher slot until the next durable commit after `after`, or for `wait_ms`.
    pub fn audit_wait(
        &self,
        stream: WireUuid,
        after: Option<Sequence>,
        wait_ms: u32,
    ) -> Result<WatchView, F8Error> {
        let wait_ms = wait_ms.min(WATCH_WAIT_MS);
        self.with(Role::Watch, |client| {
            let current = status(client)?;
            if current.stream_id != stream
                || after.map_or(0, Sequence::get) > current.last_sequence.map_or(0, Sequence::get)
            {
                return Err(CallError::Refused(E::StaleRevision));
            }
            let ResultValue::Audit { events, .. } = client
                .call(
                    Operation::AuditWatch(AuditWatch {
                        stream_id: stream.clone(),
                        after_sequence: after,
                        limit: 1,
                        wait_ms,
                    }),
                    Target::None,
                )?
                .value
            else {
                return Err(broken());
            };
            let changed = !events.is_empty();
            let head = if changed {
                status(client)?.last_sequence
            } else {
                current.last_sequence
            };
            Ok(WatchView {
                schema_version: SCHEMA_VERSION,
                authority: client.authority().clone(),
                stream,
                head,
                changed,
            })
        })
    }
}

fn broken() -> CallError {
    CallError::Broken(E::CorrelationError)
}

fn status(client: &mut F8Client) -> Result<crate::contracts::control::AuthorityStatus, CallError> {
    match client
        .call(Operation::Status(Empty {}), Target::None)?
        .value
    {
        ResultValue::Status(status) if status.authority_instance_id == *client.authority() => {
            Ok(status)
        }
        _ => Err(broken()),
    }
}

fn runtime_view(
    client: &mut F8Client,
    session: &ManagedSessionId,
) -> Result<RuntimeView, CallError> {
    let target = RuntimeTarget::codex();
    let answer = client.call(
        Operation::RuntimeStatus(target),
        Target::Runtime {
            session: session.clone(),
            generation: None,
            instance: None,
        },
    )?;
    let (
        ResultValue::RuntimeStatus(RuntimeStatus {
            desired_state,
            revision,
            actual_state,
            runtime_instance_id,
            runtime_version,
            model_count,
            diagnostic,
            runtime_safety_profile,
            workspace_root,
            ..
        }),
        Projection::Runtime {
            session: observed,
            generation,
            budget_failure,
            ..
        },
    ) = (answer.value, answer.projection)
    else {
        return Err(broken());
    };
    if observed != *session {
        return Err(broken());
    }
    // A counter at its end is larger than a number the window holds exactly. It is shown at
    // the largest one that is, and said to be exhausted: no start will be asked against it.
    let exhausted = generation >= i64::MAX as u64;
    let generation = generation.min(JS_SAFE_INTEGER);
    Ok(RuntimeView {
        exhausted,
        fence: FenceView {
            session: session.clone(),
            generation,
            instance: runtime_instance_id.clone(),
        },
        desired: desired_state,
        actual: actual_state,
        revision,
        generation,
        instance: runtime_instance_id,
        version: runtime_version,
        profile: runtime_safety_profile,
        workspace: workspace_root,
        model_count,
        diagnostic,
        budget_failure,
    })
}

fn binding_view(
    client: &mut F8Client,
    session: &ManagedSessionId,
) -> Result<BindingView, CallError> {
    let answer = client.call(
        Operation::BindingStatus(Get {
            managed_session_id: session.clone(),
        }),
        Target::None,
    )?;
    let ResultValue::BindingStatus(binding) = answer.value else {
        return Err(broken());
    };
    if binding.managed_session_id != *session {
        return Err(broken());
    }
    let attached = binding.state == BindingState::BoundAttached;
    Ok(BindingView {
        state: binding.state,
        workspace: binding.binding.map(|b| b.workspace_root),
        mode: binding.attachment_profile.filter(|_| attached),
        operation: binding.operation_id,
        observed_model: binding.observed_model.filter(|_| attached),
        attached_instance: binding.runtime_instance_id,
    })
}

fn binding_receipt(
    client: &F8Client,
    answer: Answer,
    session: ManagedSessionId,
) -> Result<BindingReceipt, CallError> {
    let ResultValue::Binding(binding) = answer.value else {
        return Err(broken());
    };
    if binding.managed_session_id != session {
        return Err(broken());
    }
    Ok(BindingReceipt {
        schema_version: SCHEMA_VERSION,
        authority: client.authority().clone(),
        session,
        workspace: binding.workspace_root,
    })
}

/// A turn as the authority holds it, with the launch it was admitted on.
fn turn_row(client: &mut F8Client, id: &ManagedTurnId) -> Result<TurnRow, CallError> {
    let answer = client.call(
        Operation::TurnStatus(TurnGet {
            managed_turn_id: id.clone(),
        }),
        Target::None,
    )?;
    let (
        ResultValue::TurnStatus(status),
        Projection::History {
            reference,
            attestation,
        },
    ) = (answer.value, answer.projection)
    else {
        return Err(broken());
    };
    if status.turn.managed_turn_id != *id {
        return Err(broken());
    }
    Ok(TurnRow {
        generation: reference.map(|r| r.generation),
        live: attestation.is_some(),
        turn: TurnSummary::of(status),
    })
}

/// The approval a running turn waits on, with its action, read against the session's workspace.
fn pending_approval(
    client: &mut F8Client,
    turn: &ManagedTurnId,
    binding: &BindingView,
) -> Result<Option<ApprovalDetail>, CallError> {
    let answer = match client.call(
        Operation::ApprovalPending(ApprovalPending {
            managed_turn_id: turn.clone(),
        }),
        Target::None,
    ) {
        Ok(answer) => answer,
        Err(CallError::Refused(E::TurnNotFound)) => return Ok(None),
        Err(error) => return Err(error),
    };
    let ResultValue::PendingApproval(pending) = answer.value else {
        return Err(broken());
    };
    Ok(pending
        .filter(|status| status.approval.managed_turn_id == *turn)
        .map(|status| ApprovalDetail::of(*status, binding.workspace.clone())))
}

/// Reads the durable stream forward from where the index stopped, remembering which turn
/// was admitted for which session. Evidence of admission, not state.
fn scan(client: &mut F8Client, index: &mut Index) -> Result<(), CallError> {
    let current = status(client)?;
    let head = current.last_sequence.map_or(0, Sequence::get);
    if index.authority.as_ref() != Some(client.authority())
        || index.stream.as_ref() != Some(&current.stream_id)
        || index.scanned > head
    {
        *index = Index {
            authority: Some(client.authority().clone()),
            stream: Some(current.stream_id.clone()),
            scanned: head.saturating_sub(SCAN_EVENTS),
            partial: head > SCAN_EVENTS,
            ..Index::default()
        };
    }
    let mut guard = 0;
    while index.scanned < head && guard < (SCAN_EVENTS / u64::from(MAX_PAGE)) as usize + 2 {
        guard += 1;
        let limit = u16::try_from((head - index.scanned).min(u64::from(MAX_PAGE)))
            .map_err(|_| CallError::Local(E::InternalError))?;
        let ResultValue::Audit { events, .. } = client
            .call(
                Operation::AuditRead(AuditRead {
                    stream_id: current.stream_id.clone(),
                    after_sequence: crate::events::position(index.scanned)
                        .map_err(|_| CallError::Local(E::InternalError))?,
                    limit,
                }),
                Target::None,
            )?
            .value
        else {
            return Err(broken());
        };
        if events.is_empty() || events.len() > usize::from(limit) {
            return Err(broken());
        }
        for event in events {
            let envelope =
                serde_json::to_value(&event).map_err(|_| CallError::Local(E::InternalError))?;
            let row: EventRow = project(&envelope, &current.stream_id).map_err(|_| broken())?;
            let at = row.sequence.get();
            if at <= index.scanned {
                return Err(broken());
            }
            index.scanned = at;
            if envelope["event_type"] == "authority.managed_turn.admitted"
                && let (Some(session), Some(turn)) = (row.session, row.turn)
            {
                index.admit(session.as_str(), turn);
            }
        }
    }
    Ok(())
}
