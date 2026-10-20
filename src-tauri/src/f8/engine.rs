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
    ownership::{OwnedChild, Progress},
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

/// Sends already-authorized bytes to one runtime's provider connection. It performs no policy:
/// the engine decides, immediately beforehand, whether anything may be written at all.
pub trait Worker: Send {
    fn send(&mut self, frame: &[u8]) -> ControlResult<()>;
}

pub struct Spawned {
    pub worker: Box<dyn Worker>,
    /// The owned child, when the worker is a process. Absent only for in-memory test workers.
    pub child: Option<OwnedChild>,
}
/// Creates the worker for a launch the allocator already reserved. The engine never spawns
/// anything for a launch that has no committed reservation.
pub trait Spawner {
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
}
struct Live {
    reference: RuntimeRef,
    phase: Phase,
    worker: Box<dyn Worker>,
    child: Option<OwnedChild>,
    staging: Option<LaunchStaging>,
    meter: Option<Meter>,
    turn: Option<(String, String)>,
    /// Provider request id to pending approval, scoped to this very launch.
    approvals: BTreeMap<String, Pending>,
    attachment: Option<Attachment>,
    exit: Option<(String, Option<String>)>,
}

type StageHook = Box<dyn FnMut(LaunchStage) -> ControlResult<()> + Send>;

pub struct Engine {
    conn: Connection,
    authority: WireUuid,
    registry: Registry,
    live: BTreeMap<String, Live>,
    staging: Option<StagingRoot>,
    scheduler: Scheduler<Vec<u8>>,
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
    pub fn scheduler_mut(&mut self) -> &mut Scheduler<Vec<u8>> {
        &mut self.scheduler
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
        self.launch(&reference, spawner)?;
        Ok(outcome)
    }

    fn launch(&mut self, reference: &RuntimeRef, spawner: &mut dyn Spawner) -> ControlResult<()> {
        self.registry.reserve(reference.clone())?;
        let session = reference.managed_session_id.as_str().to_owned();
        let result = (|| {
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
                child: spawned.child,
                staging,
                meter: None,
                turn: None,
                approvals: BTreeMap::new(),
                attachment: None,
                exit: None,
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
                if self.live.get(&session).is_some_and(|l| l.child.is_none()) {
                    let reference = self.live[&session].reference.clone();
                    self.finish_unspawned(&session);
                    self.record_end(&reference, "spawn_failed", None);
                }
            } else {
                // Reserved in the registry but never spawned: release the slot, nothing to reap.
                let _ = self.registry.stopping(reference);
                let _ = self.registry.remove_after_reap(reference);
                self.record_end(reference, "spawn_failed", None);
            }
        }
        result
    }

    /// A launch with no process handle has nothing to reap.
    fn finish_unspawned(&mut self, session: &str) {
        if let Some(live) = self.live.remove(session) {
            let _ = self.registry.remove_after_reap(&live.reference);
            if let Some(s) = live.staging {
                let _ = s.cleanup();
            }
            self.scheduler.remove_runtime(session);
        }
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
        if self.stage(LaunchStage::AfterHandshake).is_err() {
            self.begin_stop_internal(
                reference.managed_session_id.as_str(),
                "protocol_failed",
                None,
            );
            return Err(E::InternalError);
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
        let identity = RuntimeIdentity {
            schema: 2,
            authority_instance_id: reference.authority_instance_id.clone(),
            managed_session_id: reference.managed_session_id.clone(),
            runtime_instance_id: reference.runtime_instance_id.clone(),
            generation: reference.generation,
            observed_version: observed_version.to_owned(),
        };
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

    /// The attachment the runtime is currently attached with changed (workspace or profile).
    pub fn replace_attachment(
        &mut self,
        fence: &RuntimeFence,
        next: Attachment,
    ) -> ControlResult<()> {
        let live = self.fenced_mut(fence)?;
        live.attachment = Some(next);
        Ok(())
    }
    fn fenced_mut(&mut self, fence: &RuntimeFence) -> ControlResult<&mut Live> {
        fence.validate(false)?;
        let live = self
            .live
            .get_mut(fence.managed_session_id.as_str())
            .ok_or(E::StaleRevision)?;
        if live.reference.fence() != *fence || live.phase != Phase::Ready {
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
        let meter = Meter::new(now_ms)?;
        live.meter = Some(meter);
        live.turn = Some((turn_id.to_owned(), lease.lease_id.clone()));
        Ok(TurnTicket {
            reference: live.reference.clone(),
            turn_id: turn_id.to_owned(),
            lease_id: lease.lease_id.clone(),
            context: context.clone(),
        })
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
        self.check_current(&ticket.reference)?;
        let session = ticket.reference.managed_session_id.as_str().to_owned();
        let live = self.live.get_mut(&session).ok_or(E::StaleRevision)?;
        if live.turn.as_ref().map(|t| (&t.0, &t.1)) != Some((&ticket.turn_id, &ticket.lease_id)) {
            return Err(E::StaleRevision);
        }
        if !leases.active(&session, &ticket.lease_id, now_ms) {
            return Err(E::GateDenied);
        }
        if live.attachment.as_ref() != Some(&ticket.context) {
            return Err(E::GateDenied);
        }
        live.worker.send(frame)
    }

    /// One inbound provider event. It must come from the current launch; it is charged to that
    /// launch's meter only; exceeding a budget stops that runtime alone.
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
            .and_then(|l| l.meter.as_mut())
            .and_then(|m| m.charge_event(now_ms, bytes).err());
        if let Some(Violation { key }) = violation {
            self.begin_stop_internal(&session, "budget_exceeded", Some(key.to_owned()));
            return Err(E::CapacityExceeded);
        }
        Ok(())
    }

    /// A provider approval request. Correlation is `(this launch, provider request id)`, never
    /// the request id alone, so identical ids on two runtimes can never cross.
    pub fn approval_requested(
        &mut self,
        from: &RuntimeRef,
        request_id: &str,
        fingerprint: &str,
    ) -> ControlResult<()> {
        self.check_current(from)?;
        let session = from.managed_session_id.as_str();
        let live = self.live.get_mut(session).ok_or(E::StaleRevision)?;
        if live.turn.is_none() {
            return Err(E::GateDenied);
        }
        if live.approvals.contains_key(request_id) {
            return Err(E::IntentConflict);
        }
        if let Some(m) = live.meter.as_mut()
            && m.approval_opened().is_err()
        {
            return Err(E::CapacityExceeded);
        }
        if live
            .approvals
            .insert(
                request_id.to_owned(),
                Pending {
                    fingerprint: fingerprint.to_owned(),
                },
            )
            .is_some()
        {
            return Err(E::IntentConflict);
        }
        Ok(())
    }
    /// An approval answer from a client. It resolves only through the fence's own launch, and
    /// only if the action fingerprint is the one that was requested.
    pub fn answer_approval(
        &mut self,
        fence: &RuntimeFence,
        request_id: &str,
        fingerprint: &str,
        frame: &[u8],
    ) -> ControlResult<()> {
        self.registry.observe(&self.authority, fence)?;
        let live = self.fenced_mut(fence)?;
        let pending = live.approvals.get(request_id).ok_or(E::ApprovalNotFound)?;
        if pending.fingerprint != fingerprint {
            return Err(E::GateDenied);
        }
        live.approvals.remove(request_id);
        if let Some(m) = live.meter.as_mut() {
            m.approval_closed();
        }
        live.worker.send(frame)
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
        let outcome = self.allocator()?.stop(request, &admission)?;
        if !outcome.replayed {
            self.begin_stop_internal(&session, "stopped", None);
        }
        Ok(outcome)
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
        if let Some(child) = live.child.as_mut() {
            child.begin_stop(Instant::now());
        }
        let _ = self.registry.stopping(&reference);
        self.scheduler.cancel_data(session);
    }

    /// Drives every stopping launch one non-blocking step. Called from outside the writer path.
    /// A reaped launch releases its slot, staging and queues, then its exit is recorded; if
    /// the record cannot be written it is retried later and the unknown stays visible.
    pub fn poll(&mut self, now: Instant) {
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
            let reaped = match live.child.as_mut() {
                None => true,
                Some(child) => matches!(child.poll_stop(now), Ok(Progress::Reaped(_))),
            };
            if !reaped {
                continue;
            }
            let Some(live) = self.live.remove(&session) else {
                continue;
            };
            let _ = self.registry.remove_after_reap(&live.reference);
            if let Some(s) = live.staging {
                let _ = s.cleanup();
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
        }
    }
    pub fn unrecorded_exits(&self) -> usize {
        self.unrecorded.len()
    }
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
