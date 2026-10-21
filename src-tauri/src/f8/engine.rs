//! Per-session managed runtime engine. Inactive and disabled: it has no server, socket, provider
//! launcher or installed-store entry point, and it is not reachable from the running authority.
//!
//! The engine composes the durable allocator (authority for identity and monotonicity), the
//! observational registry (capacity and live identity), owned child handles (the only way a
//! process is ever signalled), a bounded fair scheduler and per-runtime budget meters. Every
//! request is bound to an explicit `(authority, session, runtime instance, generation)` fence
//! and re-validated immediately before an effect: nothing is inferred from a GUI selection, a
//! PID, a provider request ID or a durable lease alone. Provider observations never grant
//! authority, a crashed runtime's approvals and leases are never inherited, and a budget
//! violation, stall or exit of one runtime never reaches another.
use super::{
    allocator::{Admission, Allocator, Outcome},
    budget::{Meter, Violation},
    protocol::{Request, RuntimeFence, RuntimeIdentity, RuntimeRef},
    registry::Registry,
    scheduler::Scheduler,
    staging::{LaunchStaging, StagingRoot},
};
use crate::contracts::{
    WireUuid,
    control::{self as v1, ControlError as E, ControlResult},
};
use rusqlite::Connection;
use std::{collections::BTreeMap, time::Instant};

/// Largest frame the engine will hand to a worker (the contract's client frame bound).
const MAX_OUTBOUND_FRAME: usize = 65_536;

/// Sends already-authorized bytes to one runtime's provider connection. It performs no policy:
/// the engine decides, immediately beforehand, whether anything may be written at all.
/// **`send` must never block**: an implementation over a pipe must be non-blocking with a bounded
/// buffer and return an error (back pressure) instead of waiting, because it runs on the thread
/// which also drives every other runtime's cleanup.
pub trait Worker: Send {
    /// Fresh provider configuration and hooks attestation before admitting executable work.
    fn preflight(&mut self, _context: &Attachment) -> ControlResult<()> {
        Err(E::Unsupported)
    }
    fn send(&mut self, frame: &[u8]) -> ControlResult<()>;
    /// A security-sensitive approval decision must be accepted by the kernel pipe as one
    /// complete atomic frame now, never merely queued for delivery after lease revocation.
    /// This does not prove the provider processed the frame or that the effect completed.
    /// Unknown worker implementations fail closed instead of claiming that buffered sends
    /// have been dispatched.
    fn send_critical(&mut self, _frame: &[u8]) -> ControlResult<()> {
        Err(E::Unsupported)
    }
    /// Pushes buffered outbound bytes without blocking.
    fn flush(&mut self) -> ControlResult<()> {
        Ok(())
    }
    /// At most `max_frames` complete inbound frames, never blocking. Bytes not consumed stay
    /// in the (bounded) pipe, which is the back pressure on the provider.
    fn inbound(&mut self, _max_frames: usize) -> ControlResult<Vec<Vec<u8>>> {
        Ok(Vec::new())
    }
    /// The underlying process ended without being asked to.
    fn has_exited(&mut self) -> bool {
        false
    }
    /// Starts the bounded owned stop (EOF, TERM, KILL through the owned handle only).
    fn begin_stop(&mut self, _now: Instant) {}
    /// One non-blocking step of the stop; `Reaped` is the only proof the process is gone.
    fn poll_stop(&mut self, _now: Instant) -> StopState {
        StopState::Reaped
    }
    /// Read-only count of live descendants of the owned process (reported, never signalled).
    fn descendants(&self) -> usize {
        0
    }
    /// How far the process tree of this worker is contained.
    fn containment(&mut self) -> Containment {
        Containment::DirectChildOnly
    }
    /// Diagnostics only; never an input to any decision.
    fn pid_for_diagnostics(&self) -> Option<i32> {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Containment {
    /// Only the direct child is owned; descendants would be reported, never contained.
    DirectChildOnly,
    /// The payload runs in a PID namespace which dies with the owned child (observed).
    PidNamespace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopState {
    Pending,
    Reaped,
    /// Even SIGKILL could not be reaped: the slot stays consumed, the process is not abandoned.
    Stuck,
}

pub struct Spawned {
    pub worker: Box<dyn Worker>,
}
/// Creates the worker for a launch the allocator already reserved. The engine never spawns
/// anything for a launch that has no committed reservation.
pub trait Spawner {
    fn supports_profile(&self, _profile: v1::RuntimeSafetyProfile) -> bool {
        true
    }
    /// Whether this spawner launches payloads into a verified PID namespace. A profile which can
    /// execute tools is refused unless it does, before anything is reserved.
    fn descendant_containment(&self) -> bool {
        false
    }
    fn spawn(
        &mut self,
        launch: &RuntimeRef,
        staging: Option<&mut LaunchStaging>,
    ) -> ControlResult<Spawned>;
}

/// The lease facts needed at the moment of writing. The durable lease is a session grant;
/// launch-scoped admission is transient and lives only in the engine.
pub trait LeaseView {
    fn active(&self, session: &str, lease: &str, now_ms: u64) -> bool;
}

/// The workspace/profile the runtime was attached with. Revalidated before every dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub workspace: String,
    pub profile: String,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseClaim {
    pub lease_id: String,
    pub session: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnTicket {
    pub reference: RuntimeRef,
    pub turn_id: String,
    pub lease_id: String,
    pub context: Attachment,
}

/// Where a launch crashed, for fault injection of every stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchStage {
    AfterReservation,
    AfterSpawn,
    AfterHandshake,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    Spawned,
    Ready,
    Stopping,
}
struct Pending {
    fingerprint: String,
    action: Option<super::operations::HeldAction>,
}
struct Live {
    reference: RuntimeRef,
    phase: Phase,
    worker: Box<dyn Worker>,
    staging: Option<LaunchStaging>,
    meter: Meter,
    spawned_at: Instant,
    turn: Option<(String, String)>,
    /// Provider request id to pending approval, scoped to this very launch.
    approvals: BTreeMap<String, Pending>,
    attachment: Option<Attachment>,
    exit: Option<(String, Option<String>)>,
    requires_containment: bool,
    requested_workspace: Option<String>,
    observed_dead: Option<Instant>,
    drained_frames: usize,
    drain_empty: bool,
    models: Option<(
        Vec<crate::contracts::agent_adapter::AgentModelDescriptorV1>,
        Vec<v1::ModelOffer>,
        bool,
    )>,
}

type StageHook = Box<dyn FnMut(LaunchStage) -> ControlResult<()> + Send>;

pub struct Engine {
    conn: Connection,
    authority: WireUuid,
    registry: Registry,
    live: BTreeMap<String, Live>,
    staging: Option<StagingRoot>,
    scheduler: Scheduler<Vec<u8>>,
    epoch: Instant,
    /// Launch directories whose cleanup reported an anomaly (reported, never retried blindly).
    staging_anomalies: usize,
    stuck: usize,
    ended: usize,
    shutting_down: bool,
    /// Exits whose audit record could not be written yet (storage failure): retried, never lost.
    unrecorded: Vec<(RuntimeRef, String, Option<String>)>,
    hook: Option<StageHook>,
}

impl Engine {
    pub fn new(
        conn: Connection,
        authority: WireUuid,
        registry: Registry,
        staging: Option<StagingRoot>,
    ) -> ControlResult<Self> {
        Ok(Self {
            conn,
            authority,
            registry,
            live: BTreeMap::new(),
            staging,
            scheduler: Scheduler::new()?,
            epoch: Instant::now(),
            staging_anomalies: 0,
            stuck: 0,
            ended: 0,
            shutting_down: false,
            unrecorded: Vec::new(),
            hook: None,
        })
    }
    pub fn with_stage_hook(
        mut self,
        hook: impl FnMut(LaunchStage) -> ControlResult<()> + Send + 'static,
    ) -> Self {
        self.hook = Some(Box::new(hook));
        self
    }
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
    pub fn authority_id(&self) -> &WireUuid {
        &self.authority
    }
    /// The attested identity of the session's live launch, only if `fence` names it exactly.
    pub fn observe_fence(&self, fence: &RuntimeFence) -> ControlResult<RuntimeIdentity> {
        self.registry.observe(&self.authority, fence).cloned()
    }
    pub fn reference(&self, session: &str) -> Option<RuntimeRef> {
        self.live.get(session).map(|l| l.reference.clone())
    }
    pub fn attachment(&self, session: &str) -> Option<Attachment> {
        self.live.get(session).and_then(|l| l.attachment.clone())
    }
    /// The fingerprint of the provider's own pending request, independent of any client claim.
    /// Catalogue snapshots belong to one attested connection, never to a global provider.
    pub fn record_models(
        &mut self,
        origin: &RuntimeRef,
        models: Vec<crate::contracts::agent_adapter::AgentModelDescriptorV1>,
        offers: Vec<v1::ModelOffer>,
        complete: bool,
    ) -> ControlResult<()> {
        self.check_current(origin)?;
        if models.len() > 50
            || models.len() != offers.len()
            || models.iter().any(|m| m.validate().is_err())
            || offers.iter().any(|m| !m.valid())
        {
            return Err(E::InvalidRequest);
        }
        let live = self.fenced_mut(&origin.fence())?;
        live.models = Some((models, offers, complete));
        Ok(())
    }
    pub fn model_snapshot(
        &self,
        fence: &RuntimeFence,
    ) -> ControlResult<(
        &[crate::contracts::agent_adapter::AgentModelDescriptorV1],
        &[v1::ModelOffer],
        bool,
    )> {
        self.observe_fence(fence)?;
        let live = self
            .live
            .get(fence.managed_session_id.as_str())
            .ok_or(E::StaleRevision)?;
        let (models, offers, complete) =
            live.models.as_ref().ok_or(E::RuntimeOperationInProgress)?;
        Ok((models, offers, *complete))
    }
    /// Only the sole authority writer calls this after session, lease and intent checks.
    pub fn preflight(&mut self, fence: &RuntimeFence, context: &Attachment) -> ControlResult<()> {
        let live = self.fenced_mut(fence)?;
        if live.attachment.as_ref() != Some(context) {
            return Err(E::GateDenied);
        }
        live.worker.preflight(context)
    }
    pub(super) fn dispatch_binding(
        &mut self,
        fence: &RuntimeFence,
        frame: &[u8],
    ) -> ControlResult<()> {
        if frame.len() > MAX_OUTBOUND_FRAME {
            return Err(E::FrameTooLarge);
        }
        self.fenced_mut(fence)?.worker.send(frame)
    }
    /// Retains only a complete, independently normalized proposal on the owned connection.
    pub fn hold_approval_action(
        &mut self,
        origin: &RuntimeRef,
        item: &str,
        action: super::operations::ProviderAction,
    ) -> ControlResult<()> {
        self.check_current(origin)?;
        let held = action.build(item)?;
        let live = self.fenced_mut(&origin.fence())?;
        let context = live.attachment.as_ref().ok_or(E::GateDenied)?;
        if !held.confined(&context.workspace) {
            return Err(E::GateDenied);
        }
        let pending = live.approvals.get_mut(item).ok_or(E::ApprovalNotFound)?;
        if pending.fingerprint != held.fingerprint() {
            return Err(E::CorrelationError);
        }
        pending.action = Some(held);
        Ok(())
    }
    pub fn approval_action_confined(&self, fence: &RuntimeFence, item: &str) -> ControlResult<()> {
        self.observe_fence(fence)?;
        let live = self
            .live
            .get(fence.managed_session_id.as_str())
            .ok_or(E::StaleRevision)?;
        let action = live
            .approvals
            .get(item)
            .and_then(|p| p.action.as_ref())
            .ok_or(E::GateDenied)?;
        let context = live.attachment.as_ref().ok_or(E::GateDenied)?;
        if !action.confined(&context.workspace) {
            return Err(E::GateDenied);
        }
        Ok(())
    }
    pub fn approval_presentation(
        &self,
        fence: &RuntimeFence,
        item: &str,
        index: Option<u16>,
        offset: Option<u32>,
    ) -> ControlResult<serde_json::Value> {
        self.approval_action_confined(fence, item)?;
        self.live
            .get(fence.managed_session_id.as_str())
            .and_then(|l| l.approvals.get(item))
            .and_then(|p| p.action.as_ref())
            .ok_or(E::GateDenied)?
            .presentation(index, offset)
    }
    pub fn pending_fingerprint(&self, session: &str, request_id: &str) -> Option<String> {
        self.live
            .get(session)
            .and_then(|l| l.approvals.get(request_id))
            .map(|p| p.fingerprint.clone())
    }
    /// Phase of the session's live launch and, when attested, its identity.
    pub fn runtime_view(&self, session: &str) -> Option<(&'static str, Option<RuntimeIdentity>)> {
        let live = self.live.get(session)?;
        let phase = match live.phase {
            Phase::Spawned => "initializing",
            Phase::Ready => "ready",
            Phase::Stopping => "stopping",
        };
        let identity = (live.phase == Phase::Ready)
            .then(|| {
                self.registry
                    .observe(&self.authority, &live.reference.fence())
                    .ok()
                    .cloned()
            })
            .flatten();
        Some((phase, identity))
    }
    /// What the session's live launch was asked to be: the workspace of a local-tools launch
    /// (none for inference) and how many models it listed. Nothing here attests anything.
    pub fn launch_facts(&self, session: &str) -> Option<(Option<String>, usize)> {
        self.live.get(session).map(|live| {
            (
                live.requested_workspace.clone(),
                live.models
                    .as_ref()
                    .map_or(0, |(models, _, _)| models.len()),
            )
        })
    }
    /// The lease admitting the session's turn was lost: the in-memory turn and approvals end.
    pub fn lease_lost(&mut self, session: &str, lease_id: &str) {
        let now = self.ms(Instant::now());
        if let Some(live) = self.live.get_mut(session)
            && live.turn.as_ref().is_some_and(|t| t.1 == lease_id)
        {
            live.turn = None;
            live.approvals.clear();
            if let Ok(m) = Meter::new(now) {
                live.meter = m;
            }
        }
    }
    /// Launches reaped since the last call: the caller then recovers their stranded work.
    pub fn take_ended(&mut self) -> usize {
        std::mem::take(&mut self.ended)
    }
    /// Mandatory safety shutdown bypasses the explicit-stop admission guard. The owned
    /// workers and their capacity remain retained until their nonblocking reap completes.
    pub fn shutdown(&mut self) {
        self.shutting_down = true;
        let sessions: Vec<_> = self.live.keys().cloned().collect();
        for session in sessions {
            self.begin_stop_internal(&session, "transport_failed", None);
        }
    }
    /// The authority itself is ending: what is lost with its runtimes is lost to that.
    pub fn shutting_down(&self) -> bool {
        self.shutting_down
    }
    pub fn cleanup_complete(&self) -> bool {
        self.live.is_empty() && self.unrecorded.is_empty()
    }
    #[cfg(test)]
    pub(crate) fn scheduler_mut(&mut self) -> &mut Scheduler<Vec<u8>> {
        &mut self.scheduler
    }
    /// Queues work for one live launch (fence-checked). A full data lane is back pressure; an
    /// overflowing control lane faults that runtime, which `poll` then stops.
    pub fn enqueue(
        &mut self,
        to: &RuntimeRef,
        class: super::scheduler::Class,
        item: Vec<u8>,
        now: Instant,
    ) -> Result<(), super::scheduler::Refusal> {
        self.check_current(to)
            .map_err(|_| super::scheduler::Refusal::UnknownRuntime)?;
        let bytes = item.len();
        let ms = self.ms(now);
        self.scheduler
            .push(to.managed_session_id.as_str(), class, bytes, item, ms)
    }
    fn ms(&self, now: Instant) -> u64 {
        u64::try_from(now.saturating_duration_since(self.epoch).as_millis()).unwrap_or(u64::MAX)
    }
    /// Polls which found a stop that even SIGKILL could not reap (the slot stays consumed).
    pub fn stuck_polls(&self) -> usize {
        self.stuck
    }
    pub fn staging_anomalies(&self) -> usize {
        self.staging_anomalies
    }
    fn stage(&mut self, stage: LaunchStage) -> ControlResult<()> {
        match self.hook.as_mut() {
            Some(h) => h(stage),
            None => Ok(()),
        }
    }
    fn allocator(&mut self) -> ControlResult<Allocator<'_>> {
        Allocator::new(&mut self.conn, self.authority.clone())
    }

    /// Parses a protocol-2 request and routes it. The target is always the explicit session in
    /// the request; a missing, stale or version-confused request never selects another one.
    pub fn handle(
        &mut self,
        bytes: &[u8],
        spawner: &mut dyn Spawner,
        now: Instant,
    ) -> ControlResult<Outcome> {
        let request = Request::parse(bytes)?;
        request.correlate(Some(&self.authority))?;
        match request.operation() {
            v1::Operation::RuntimeStart(_) => self.start(&request, spawner),
            v1::Operation::RuntimeStop(_) => self.stop(&request, now),
            _ => Err(E::Unsupported),
        }
    }

    /// Durable reservation first, then (and only for a freshly reserved launch) the spawn.
    /// Capacity is checked after replay and before any reservation; a failed attempt keeps its
    /// generation; nothing is restarted implicitly.
    pub fn start(
        &mut self,
        request: &Request,
        spawner: &mut dyn Spawner,
    ) -> ControlResult<Outcome> {
        let (session, current) = match request.target() {
            super::protocol::Target::Runtime { session, .. } => (
                session.as_str().to_owned(),
                self.live.get(session.as_str()).map(|l| l.reference.clone()),
            ),
            _ => return Err(E::InvalidRequest),
        };
        if let v1::Operation::RuntimeStart(m) = request.operation()
            && !spawner.supports_profile(
                m.safety_profile
                    .unwrap_or(v1::RuntimeSafetyProfile::CodexInferenceV1),
            )
        {
            return Err(E::Unsupported);
        }
        // Execution paths whose required containment cannot be provided are refused outright:
        // nothing is reserved, spawned or recorded for them.
        if let v1::Operation::RuntimeStart(m) = request.operation()
            && m.safety_profile == Some(v1::RuntimeSafetyProfile::CodexLocalToolsV1)
            && !spawner.descendant_containment()
        {
            return Err(E::Unsupported);
        }
        let workspace = match request.operation() {
            v1::Operation::RuntimeStart(m) => m
                .workspace_root
                .as_ref()
                .map(|w| {
                    let p = std::fs::canonicalize(w).map_err(|_| E::WorkspaceInvalid)?;
                    if !p.is_dir() {
                        return Err(E::WorkspaceInvalid);
                    }
                    p.to_str().map(str::to_owned).ok_or(E::WorkspaceInvalid)
                })
                .transpose()?,
            _ => return Err(E::InvalidRequest),
        };
        let admission = Admission {
            live: current.filter(|_| {
                self.live
                    .get(&session)
                    .is_some_and(|l| l.phase != Phase::Stopping)
            }),
            slot_available: self.registry.has_free_slot(),
        };
        let outcome = self.allocator()?.start(request, &admission)?;
        if !outcome.reserved {
            return Ok(outcome);
        }
        let reference = RuntimeRef {
            schema: 2,
            authority_instance_id: self.authority.clone(),
            managed_session_id: outcome.managed_session_id.clone(),
            runtime_instance_id: outcome
                .runtime_instance_id
                .clone()
                .ok_or(E::InternalError)?,
            generation: outcome.generation,
        };
        // The reservation is durable and its generation stays consumed even if this fails: a
        // launch which reached the live table records its own end when it is reaped, otherwise
        // `launch` records it now (or queues the record), so no phantom slot remains.
        let tools = matches!(
            request.operation(),
            v1::Operation::RuntimeStart(m) if m.safety_profile == Some(v1::RuntimeSafetyProfile::CodexLocalToolsV1)
        );
        self.launch(&reference, spawner, tools, workspace)?;
        Ok(outcome)
    }

    fn launch(
        &mut self,
        reference: &RuntimeRef,
        spawner: &mut dyn Spawner,
        tools: bool,
        requested_workspace: Option<String>,
    ) -> ControlResult<()> {
        let session = reference.managed_session_id.as_str().to_owned();
        let result = (|| {
            self.registry.reserve(reference.clone())?;
            self.stage(LaunchStage::AfterReservation)?;
            let mut staging = match &self.staging {
                Some(root) => Some(root.create(&reference.runtime_instance_id)?),
                None => None,
            };
            let spawned = spawner.spawn(reference, staging.as_mut());
            let spawned = match spawned {
                Ok(s) => s,
                Err(e) => {
                    if let Some(s) = staging.take() {
                        let _ = s.cleanup();
                    }
                    return Err(e);
                }
            };
            let live = Live {
                reference: reference.clone(),
                phase: Phase::Spawned,
                worker: spawned.worker,
                staging,
                meter: Meter::new(self.ms(Instant::now()))?,
                spawned_at: Instant::now(),
                turn: None,
                approvals: BTreeMap::new(),
                attachment: None,
                exit: None,
                requires_containment: tools,
                requested_workspace,
                observed_dead: None,
                drained_frames: 0,
                drain_empty: false,
                models: None,
            };
            self.live.insert(session.clone(), live);
            self.stage(LaunchStage::AfterSpawn)?;
            self.registry.initializing(reference)?;
            self.scheduler
                .add_runtime(&session)
                .map_err(|_| E::CapacityExceeded)?;
            Ok(())
        })();
        if result.is_err() {
            if self.live.contains_key(&session) {
                // An owned child already spawned is stopped through its handle, not abandoned.
                self.begin_stop_internal(&session, "spawn_failed", None);
                // A worker with nothing to reap completes at once; a process finishes in `poll`.
                self.poll(Instant::now());
            } else {
                // Reserved in the registry but never spawned: release the slot, nothing to reap.
                let _ = self.registry.stopping(reference);
                let _ = self.registry.remove_after_reap(reference);
                self.record_end(reference, "spawn_failed", None);
            }
        }
        result
    }

    /// The provider attested its handshake. Publishing ready requires the durable readiness
    /// record first: if audit storage refuses it, the runtime is stopped and never reported ready.
    pub fn complete_handshake(
        &mut self,
        reference: &RuntimeRef,
        observed_version: &str,
        attachment: Attachment,
    ) -> ControlResult<()> {
        self.check_current(reference)?;
        let expected = self
            .live
            .get(reference.managed_session_id.as_str())
            .ok_or(E::StaleRevision)?;
        let profile = if expected.requires_containment {
            "codex_local_tools_v1"
        } else {
            "codex_inference_v1"
        };
        if attachment.profile != profile
            || attachment.revision == 0
            || expected
                .requested_workspace
                .as_ref()
                .is_some_and(|w| w != &attachment.workspace)
        {
            self.begin_stop_internal(
                reference.managed_session_id.as_str(),
                "safety_unproven",
                None,
            );
            return Err(E::GateDenied);
        }
        if self.stage(LaunchStage::AfterHandshake).is_err() {
            self.begin_stop_internal(
                reference.managed_session_id.as_str(),
                "protocol_failed",
                None,
            );
            return Err(E::InternalError);
        }
        let identity = RuntimeIdentity {
            schema: 2,
            authority_instance_id: reference.authority_instance_id.clone(),
            managed_session_id: reference.managed_session_id.clone(),
            runtime_instance_id: reference.runtime_instance_id.clone(),
            generation: reference.generation,
            observed_version: observed_version.to_owned(),
        };
        // No immutable 'ready' audit may precede either a proved containment boundary
        // or the registry's ability to accept this exact, initializing generation.
        let needs = self
            .live
            .get(reference.managed_session_id.as_str())
            .is_some_and(|l| l.requires_containment);
        if needs {
            let contained = self
                .live
                .get_mut(reference.managed_session_id.as_str())
                .is_some_and(|l| l.worker.containment() == Containment::PidNamespace);
            if !contained {
                self.begin_stop_internal(
                    reference.managed_session_id.as_str(),
                    "safety_unproven",
                    None,
                );
                return Err(E::Unsupported);
            }
        }
        if let Err(e) = self.registry.check_ready(&identity) {
            self.begin_stop_internal(
                reference.managed_session_id.as_str(),
                "protocol_failed",
                None,
            );
            return Err(e);
        }
        let ready = self
            .allocator()
            .and_then(|mut a| a.record_ready(reference, observed_version, "compatible"));
        if let Err(e) = ready {
            self.begin_stop_internal(
                reference.managed_session_id.as_str(),
                "audit_unavailable",
                None,
            );
            return Err(e);
        }
        self.registry.ready(identity)?;
        if let Some(live) = self.live.get_mut(reference.managed_session_id.as_str()) {
            live.phase = Phase::Ready;
            live.attachment = Some(attachment);
        }
        Ok(())
    }

    fn check_current(&self, reference: &RuntimeRef) -> ControlResult<&Live> {
        reference.validate()?;
        if reference.authority_instance_id != self.authority {
            return Err(E::CorrelationError);
        }
        self.live
            .get(reference.managed_session_id.as_str())
            .filter(|l| l.reference == *reference && l.phase != Phase::Stopping)
            .ok_or(E::StaleRevision)
    }

    /// The attachment changed (workspace or profile). The revision must strictly increase, and
    /// everything admitted under the old context dies with it: the turn, its ticket and every
    /// pending approval. Restoring an earlier value is a new, higher revision, never a revival.
    pub fn replace_attachment(
        &mut self,
        fence: &RuntimeFence,
        next: Attachment,
    ) -> ControlResult<()> {
        let now = self.ms(Instant::now());
        let live = self.fenced_mut(fence)?;
        let profile = if live.requires_containment {
            "codex_local_tools_v1"
        } else {
            "codex_inference_v1"
        };
        if next.profile != profile
            || live
                .requested_workspace
                .as_ref()
                .is_some_and(|w| w != &next.workspace)
        {
            return Err(E::GateDenied);
        }
        if live
            .attachment
            .as_ref()
            .is_some_and(|a| next.revision <= a.revision)
        {
            return Err(E::StaleRevision);
        }
        live.attachment = Some(next);
        live.turn = None;
        live.approvals.clear();
        live.meter = Meter::new(now)?;
        Ok(())
    }
    fn fenced_mut(&mut self, fence: &RuntimeFence) -> ControlResult<&mut Live> {
        fence.validate(false)?;
        let live = self
            .live
            .get_mut(fence.managed_session_id.as_str())
            .ok_or(E::StaleRevision)?;
        if live.reference.fence() != *fence
            || live.phase != Phase::Ready
            || live.observed_dead.is_some()
        {
            return Err(E::StaleRevision);
        }
        Ok(live)
    }

    /// Admission of one turn: the fence must be the live attested launch, the lease must belong
    /// to the very session of that fence and be active now, the accepted context must equal the
    /// current attachment, and the session may run one turn at a time.
    pub fn admit_turn(
        &mut self,
        fence: &RuntimeFence,
        lease: &LeaseClaim,
        context: &Attachment,
        turn_id: &str,
        leases: &dyn LeaseView,
        now_ms: u64,
    ) -> ControlResult<TurnTicket> {
        self.registry.observe(&self.authority, fence)?;
        let session = fence.managed_session_id.as_str().to_owned();
        if lease.session != session || !leases.active(&session, &lease.lease_id, now_ms) {
            return Err(E::GateDenied);
        }
        let live = self.fenced_mut(fence)?;
        if live.turn.is_some() {
            return Err(E::ActiveTurnExists);
        }
        if live.attachment.as_ref() != Some(context) {
            return Err(E::GateDenied);
        }
        live.turn = Some((turn_id.to_owned(), lease.lease_id.clone()));
        Ok(TurnTicket {
            reference: live.reference.clone(),
            turn_id: turn_id.to_owned(),
            lease_id: lease.lease_id.clone(),
            context: context.clone(),
        })
    }

    /// Ends the admitted turn: its ticket stops working and its approvals are discarded.
    pub fn end_turn(&mut self, ticket: &TurnTicket) -> ControlResult<()> {
        let now = self.ms(Instant::now());
        self.check_current(&ticket.reference)?;
        let live = self
            .live
            .get_mut(ticket.reference.managed_session_id.as_str())
            .ok_or(E::StaleRevision)?;
        if live.turn.as_ref().map(|t| &t.0) != Some(&ticket.turn_id) {
            return Err(E::StaleRevision);
        }
        live.turn = None;
        live.approvals.clear();
        live.meter = Meter::new(now)?;
        Ok(())
    }

    /// Revalidates, at this very moment, the launch, turn, lease and attachment of a ticket.
    fn authorize(
        &mut self,
        ticket: &TurnTicket,
        leases: &dyn LeaseView,
        now_ms: u64,
    ) -> ControlResult<&mut Live> {
        self.check_current(&ticket.reference)?;
        let session = ticket.reference.managed_session_id.as_str().to_owned();
        let live = self.live.get_mut(&session).ok_or(E::StaleRevision)?;
        if live.observed_dead.is_some()
            || live.phase != Phase::Ready
            || live.turn.as_ref().map(|t| (&t.0, &t.1)) != Some((&ticket.turn_id, &ticket.lease_id))
        {
            return Err(E::StaleRevision);
        }
        if !leases.active(&session, &ticket.lease_id, now_ms) {
            return Err(E::GateDenied);
        }
        if live.attachment.as_ref() != Some(&ticket.context) {
            return Err(E::GateDenied);
        }
        Ok(live)
    }

    /// Writes one frame to the provider of the ticket's launch after revalidating, at this very
    /// moment, the launch, turn, lease and attachment. A refusal writes nothing anywhere.
    pub fn write_frame(
        &mut self,
        ticket: &TurnTicket,
        frame: &[u8],
        leases: &dyn LeaseView,
        now_ms: u64,
    ) -> ControlResult<()> {
        if frame.len() > MAX_OUTBOUND_FRAME {
            return Err(E::FrameTooLarge);
        }
        self.authorize(ticket, leases, now_ms)?.worker.send(frame)
    }

    /// One inbound provider event. It must come from the current launch; it is charged to that
    /// launch's meter only (also outside a turn); exceeding a budget stops that runtime alone.
    pub fn provider_event(
        &mut self,
        from: &RuntimeRef,
        bytes: u64,
        now_ms: u64,
    ) -> ControlResult<()> {
        self.check_current(from)?;
        let session = from.managed_session_id.as_str().to_owned();
        let violation = self
            .live
            .get_mut(&session)
            .and_then(|l| l.meter.charge_event(now_ms, bytes).err());
        if let Some(Violation { key }) = violation {
            self.begin_stop_internal(&session, "budget_exceeded", Some(key.to_owned()));
            return Err(E::CapacityExceeded);
        }
        Ok(())
    }

    /// Nonmutating admission check, used before a durable approval is opened.
    /// Duplicate IDs and approval-budget exhaustion must not write an audit event
    /// or terminalize unrelated pending approvals on this turn.
    pub fn can_request_approval(&self, from: &RuntimeRef, request_id: &str) -> ControlResult<()> {
        self.check_current(from)?;
        let live = self
            .live
            .get(from.managed_session_id.as_str())
            .ok_or(E::StaleRevision)?;
        if live.observed_dead.is_some() || live.phase != Phase::Ready || live.turn.is_none() {
            return Err(E::GateDenied);
        }
        if live.approvals.contains_key(request_id) {
            return Err(E::IntentConflict);
        }
        let mut budget = live.meter.clone();
        budget.approval_opened().map_err(|_| E::CapacityExceeded)
    }

    /// A provider approval request. Correlation is `(this launch, provider request id)`, never
    /// the request id alone, so identical ids on two runtimes can never cross.
    pub fn approval_requested(
        &mut self,
        from: &RuntimeRef,
        request_id: &str,
        fingerprint: &str,
    ) -> ControlResult<()> {
        self.can_request_approval(from, request_id)?;
        let session = from.managed_session_id.as_str();
        let live = self.live.get_mut(session).ok_or(E::StaleRevision)?;
        live.meter
            .approval_opened()
            .map_err(|_| E::CapacityExceeded)?;
        live.approvals.insert(
            request_id.to_owned(),
            Pending {
                fingerprint: fingerprint.to_owned(),
                action: None,
            },
        );
        Ok(())
    }
    /// A client decision on a pending approval. The frame is built here from the request and the
    /// decision (never caller bytes), after the same revalidation as any write; the approval is
    /// consumed only once the frame was actually delivered.
    pub fn answer_approval(
        &mut self,
        ticket: &TurnTicket,
        leases: &dyn LeaseView,
        now_ms: u64,
        request_id: &str,
        fingerprint: &str,
        decision: &str,
    ) -> ControlResult<()> {
        if !matches!(decision, "accept" | "decline" | "cancel")
            || request_id.is_empty()
            || request_id.len() > 128
            || !request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err(E::InvalidRequest);
        }
        let live = self.authorize(ticket, leases, now_ms)?;
        let pending = live.approvals.get(request_id).ok_or(E::ApprovalNotFound)?;
        if pending.fingerprint != fingerprint {
            return Err(E::GateDenied);
        }
        let frame = format!("approval {request_id} {decision}");
        live.worker.send_critical(frame.as_bytes())?;
        live.approvals.remove(request_id);
        live.meter.approval_closed();
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn fixture_corrupt_fingerprint(&mut self, from: &RuntimeRef, item: &str) {
        self.live
            .get_mut(from.managed_session_id.as_str())
            .unwrap()
            .approvals
            .get_mut(item)
            .unwrap()
            .fingerprint = "0".repeat(64);
    }
    pub fn pending_approvals(&self, session: &str) -> usize {
        self.live.get(session).map_or(0, |l| l.approvals.len())
    }
    pub fn active_turn(&self, session: &str) -> Option<&str> {
        self.live
            .get(session)
            .and_then(|l| l.turn.as_ref())
            .map(|t| t.0.as_str())
    }
    /// Exit observation fences all new executable effects while final existing events drain.
    pub fn provider_exited(&self, reference: &RuntimeRef) -> bool {
        self.live
            .get(reference.managed_session_id.as_str())
            .is_none_or(|l| l.reference != *reference || l.observed_dead.is_some())
    }
    pub fn is_live(&self, session: &str) -> bool {
        self.live.contains_key(session)
    }

    /// Durable stop first, then the owned lifecycle; both are fenced to the observed launch.
    pub fn stop(&mut self, request: &Request, _now: Instant) -> ControlResult<Outcome> {
        let session = match request.target() {
            super::protocol::Target::Runtime { session, .. } => session.as_str().to_owned(),
            _ => return Err(E::InvalidRequest),
        };
        let admission = Admission {
            live: None,
            slot_available: self.registry.has_free_slot(),
        };
        // A refused stop returns here, before any in-memory teardown: tickets, approvals and queued
        // data stay as they are. Replay precedence is the allocator's: it answers before this check.
        let live_turn = self.active_turn(&session).is_some();
        let outcome = self
            .allocator()?
            .stop_guarded(request, &admission, live_turn)?;
        if !outcome.replayed {
            self.begin_stop_internal(&session, "stopped", None);
        }
        Ok(outcome)
    }

    /// Stop only a launch whose connection has failed while dispatching a durable
    /// provider decision. No unrelated session or generation is affected.
    pub fn transport_failed(&mut self, from: &RuntimeRef) {
        if self.check_current(from).is_ok() {
            self.begin_stop_internal(from.managed_session_id.as_str(), "transport_failed", None);
        }
    }

    /// Marks the launch stopping: its approvals are invalidated (never inherited), its queued
    /// work is discarded, and the owned lifecycle starts. This never blocks.
    fn begin_stop_internal(&mut self, session: &str, kind: &str, budget: Option<String>) {
        let Some(live) = self.live.get_mut(session) else {
            return;
        };
        if live.phase == Phase::Stopping {
            return;
        }
        live.phase = Phase::Stopping;
        live.turn = None;
        live.approvals.clear();
        live.exit = Some((kind.to_owned(), budget));
        let reference = live.reference.clone();
        live.worker.begin_stop(Instant::now());
        let _ = self.registry.stopping(&reference);
        self.scheduler.cancel_data(session);
    }

    /// Drives every stopping launch one non-blocking step. Called from outside the writer path.
    /// A reaped launch releases its slot, staging and queues, then its exit is recorded; if
    /// the record cannot be written it is retried later and the unknown stays visible.
    pub fn poll(&mut self, now: Instant) {
        let now_ms = self.ms(now);
        let init_timeout = super::budget::contract()
            .ok()
            .and_then(|c| c.get("provider_init_timeout_ms").copied())
            .unwrap_or(10_000);
        // Detect what no one reported: an unexpected child exit, a missed handshake deadline,
        // a turn over its wall clock and a faulted or stalled scheduler lane. Each stops only
        // the runtime concerned.
        let mut verdicts: Vec<(String, &'static str, Option<String>)> = Vec::new();
        for (session, live) in &mut self.live {
            if live.phase == Phase::Stopping {
                continue;
            }
            if live.worker.has_exited() {
                live.observed_dead.get_or_insert(now);
                // An inherited pipe in a descendant must never retain this slot forever.
                let queue_empty = self
                    .scheduler
                    .queued_bytes(session)
                    .is_none_or(|(c, d)| c == 0 && d == 0);
                let expired = live.observed_dead.is_some_and(|at| {
                    now.saturating_duration_since(at) >= std::time::Duration::from_millis(250)
                });
                if (live.drain_empty && queue_empty) || expired || live.drained_frames >= 256 {
                    verdicts.push((session.clone(), "child_exited", None));
                }
            } else if live.phase == Phase::Spawned
                && u64::try_from(now.saturating_duration_since(live.spawned_at).as_millis())
                    .unwrap_or(u64::MAX)
                    > init_timeout
            {
                verdicts.push((session.clone(), "handshake_timeout", None));
            } else if let Err(v) = live.meter.check_wall(now_ms) {
                verdicts.push((session.clone(), "budget_exceeded", Some(v.key.to_owned())));
            } else if self.scheduler.is_faulted(session) {
                verdicts.push((session.clone(), "transport_failed", None));
            }
        }
        self.pump(now_ms, &mut verdicts);
        for (session, live) in &self.live {
            if live.observed_dead.is_some()
                && live.drain_empty
                && self
                    .scheduler
                    .queued_bytes(session)
                    .is_none_or(|(c, d)| c == 0 && d == 0)
                && !verdicts.iter().any(|(s, _, _)| s == session)
            {
                verdicts.push((session.clone(), "child_exited", None));
            }
        }
        for fault in self.scheduler.check_stalls(now_ms) {
            verdicts.push((fault.runtime, "transport_failed", None));
        }
        for (session, kind, budget) in verdicts {
            self.begin_stop_internal(&session, kind, budget);
        }
        let stopping: Vec<String> = self
            .live
            .iter()
            .filter(|(_, l)| l.phase == Phase::Stopping)
            .map(|(s, _)| s.clone())
            .collect();
        for session in stopping {
            let Some(live) = self.live.get_mut(&session) else {
                continue;
            };
            match live.worker.poll_stop(now) {
                StopState::Reaped => (),
                StopState::Pending => continue,
                StopState::Stuck => {
                    self.stuck += 1;
                    continue;
                }
            }
            let Some(live) = self.live.remove(&session) else {
                continue;
            };
            let _ = self.registry.remove_after_reap(&live.reference);
            if let Some(s) = live.staging
                && s.cleanup().is_err()
            {
                self.staging_anomalies += 1;
            }
            self.scheduler.remove_runtime(&session);
            let (kind, budget) = live.exit.unwrap_or_else(|| ("child_exited".into(), None));
            self.record_end(&live.reference, &kind, budget);
        }
        let queued = std::mem::take(&mut self.unrecorded);
        for (reference, kind, budget) in queued {
            self.record_end(&reference, &kind, budget);
        }
    }
    /// Moves bytes for every running worker, bounded per call: flushes outbound buffers and
    /// reads inbound frames only while that runtime's data lane has room, so a slow consumer
    /// leaves bytes in the provider's own pipe (back pressure) instead of growing a queue.
    /// Each frame is charged to its runtime's meter before it is queued.
    fn pump(&mut self, now_ms: u64, verdicts: &mut Vec<(String, &'static str, Option<String>)>) {
        const FRAMES_PER_POLL: usize = 16;
        let sessions: Vec<String> = self
            .live
            .iter()
            .filter(|(_, l)| l.phase != Phase::Stopping)
            .map(|(s, _)| s.clone())
            .collect();
        for session in sessions {
            if verdicts.iter().any(|(s, _, _)| *s == session) {
                continue;
            }
            let room = self
                .scheduler
                .data_room(&session, 262_144)
                .min(FRAMES_PER_POLL);
            let Some(live) = self.live.get_mut(&session) else {
                continue;
            };
            if live.observed_dead.is_none() && live.worker.flush().is_err() {
                verdicts.push((session, "transport_failed", None));
                continue;
            }
            let frames = if room == 0 {
                Ok(Vec::new())
            } else {
                live.worker.inbound(room)
            };
            let frames = match frames {
                Ok(f) => f,
                Err(_) => {
                    verdicts.push((session, "protocol_failed", None));
                    continue;
                }
            };
            if live.observed_dead.is_some() && room > 0 {
                live.drain_empty = frames.is_empty();
                live.drained_frames = live.drained_frames.saturating_add(frames.len());
            }
            for frame in frames {
                if let Err(v) = live.meter.charge_event(now_ms, frame.len() as u64) {
                    verdicts.push((session.clone(), "budget_exceeded", Some(v.key.to_owned())));
                    break;
                }
                let bytes = frame.len();
                if self
                    .scheduler
                    .push(
                        &session,
                        super::scheduler::Class::Data,
                        bytes,
                        frame,
                        now_ms,
                    )
                    .is_err()
                {
                    verdicts.push((session.clone(), "transport_failed", None));
                    break;
                }
            }
        }
    }
    /// Delivers one scheduling quantum of queued provider frames (control first, then fair data).
    pub fn dispatch(&mut self, now: Instant) -> Vec<super::scheduler::Dispatch<Vec<u8>>> {
        let ms = self.ms(now);
        self.scheduler.tick(ms)
    }
    fn record_end(&mut self, reference: &RuntimeRef, kind: &str, budget: Option<String>) {
        let expected = kind == "stopped";
        let exit_kind = if kind == "stopped" {
            "child_exited"
        } else {
            kind
        };
        let budget_ref = budget.as_deref();
        let recorded = self
            .allocator()
            .and_then(|mut a| a.record_exit(reference, exit_kind, expected, budget_ref));
        if recorded.is_err() {
            self.unrecorded
                .push((reference.clone(), kind.to_owned(), budget));
        } else {
            // Recovery observes committed exits, not just a reap edge. In particular a
            // successful audit retry must wake reconciliation of the stranded turn.
            self.ended = self.ended.saturating_add(1);
        }
    }
    pub fn unrecorded_exits(&self) -> usize {
        self.unrecorded.len()
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
