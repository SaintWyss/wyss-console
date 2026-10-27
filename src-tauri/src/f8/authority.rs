//! Inactive protocol-2 / schema-8 authority facade. It owns an `Engine` and routes strict
//! protocol-2 requests to the durable writers, the engine and its fences. There is no socket, no
//! server loop, no installed-store opener and no provider launcher here; `handle` is a pure
//! bytes-in/bytes-out function for tests and for the future server.
//!
//! Nothing is inferred: the target of every routed request is the explicit runtime fence in the
//! request, checked against the registry (live work) or the immutable sidecar (history) before
//! any state changes; leases, approvals and cancellation re-run their gates at the moment of the
//! effect; a provider answer is sent only after its durable decision committed and the engine
//! revalidated the ticket. All 29 protocol methods route here or through `operations.rs`. Native provider support is a
//! separate adapter boundary and may refuse unproved profiles without weakening authority checks.
use super::{
    engine::{Attachment, Engine, LeaseClaim, LeaseView, Spawner, TurnTicket},
    protocol::{Request, Response, RuntimeFence, RuntimeRef, Target, encode},
    writers::{Decided, TurnIntent, Writer},
};
use crate::contracts::{
    WireUuid,
    approval::{
        ApprovalDecision, ApprovalDelivery, ApprovalKind, ApprovalReason, ApprovalStructure,
    },
    control::{self as v1, ControlError as E, ControlResult, unique_json},
    identities::ManagedTurnId,
    turn::{TurnReason, TurnState},
};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    time::{Duration, Instant},
};

/// The closed exit word, and the closed budget key of a budget exit, of an unexpected exit.
type ExitFacts = Option<(String, Option<String>)>;

const NIL: &str = "00000000-0000-4000-8000-000000000000";
const PAGE_BYTES: usize = 65_536 - 1024;
// A page of durable events within its byte budget, plus the response envelope, always fits one frame,
// and any single admitted event fits the budget.
const _: () = assert!(super::audit::MAX_EVENT_BYTES < PAGE_BYTES);
const _: () = assert!(PAGE_BYTES + 1024 <= crate::contracts::control::MAX_FRAME);

/// Strict internal adapter-normalized provider notifications. This is NOT the
/// upstream Codex JSON-RPC protocol: a separately reviewed adapter must translate
/// genuine Codex messages into this vocabulary before live integration.
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ProviderNotice {
    RuntimeReady {
        observed_version: String,
        workspace: String,
        profile: String,
        revision: u64,
    },
    Content {
        managed_turn_id: ManagedTurnId,
        provider_turn_id: String,
        text: String,
    },
    BindingConfirmed {
        operation_id: WireUuid,
        provider_thread_id: String,
    },
    Models {
        models: Vec<crate::contracts::agent_adapter::AgentModelDescriptorV1>,
        offers: Vec<v1::ModelOffer>,
        complete: bool,
    },
    ApprovalItemConsumed {
        provider_item_id: String,
    },
    ApprovalItemConfirmed {
        provider_item_id: String,
    },
    TurnInterrupted {
        managed_turn_id: ManagedTurnId,
    },
    TurnRunning {
        managed_turn_id: ManagedTurnId,
        provider_turn_id: String,
    },
    TurnCompleted {
        managed_turn_id: ManagedTurnId,
        provider_turn_id: String,
    },
    TurnFailed {
        managed_turn_id: ManagedTurnId,
    },
    ApprovalRequired {
        approval_kind: ApprovalKind,
        provider_item_id: String,
        action_fingerprint: String,
        structure: ApprovalStructure,
        #[serde(default)]
        action: Option<super::operations::ProviderAction>,
    },
    ApprovalConfirmed {
        managed_approval_id: crate::contracts::identities::ManagedApprovalId,
        provider_item_id: String,
    },
}

/// Per-connection state: the authority instance observed in the handshake.
#[derive(Default)]
pub struct Client {
    observed: Option<WireUuid>,
}

pub struct Authority {
    engine: Engine,
    /// Present only for an explicitly migrated V10 authority. It never activates Managed
    /// execution on its own; turn admission still requires the guarded protocol path.
    #[cfg(target_os = "linux")]
    workspace_guard: Option<super::workspace_guard::WorkspaceGuard>,
    /// In-memory admission tickets by turn id: transient, launch-scoped, never durable.
    tickets: BTreeMap<String, TurnTicket>,
    /// Retry audit-backed recovery until its transaction actually commits.
    recovery_pending: bool,
    /// Production execution through the new M6 guard stays off until independent acceptance.
    managed_execution_enabled: bool,
    /// When the durable expiry of leases and approvals is next looked for.
    housekeeping_at: Option<Instant>,
    /// What the audit log last said of one generation's unexpected exit, and when it was read:
    /// a status request never scans the log more than once per `EXIT_LOOKUP_TTL` for a generation.
    exit_lookups: RefCell<BTreeMap<(String, i64), (Instant, ExitFacts)>>,
    streams: crate::control::turn_stream::Streams,
    #[cfg(test)]
    post_approval_write: Option<fn(&mut Connection)>,
}

/// Lease facts read from the database at the moment of an effect.
struct Snapshot {
    session: String,
    lease: String,
    active: bool,
}
impl LeaseView for Snapshot {
    fn active(&self, session: &str, lease: &str, _now_ms: u64) -> bool {
        self.active && self.session == session && self.lease == lease
    }
}
fn lease_snapshot(c: &Connection, session: &str, lease: &str) -> Snapshot {
    let active = super::writers::read_lease(c, lease).is_ok_and(|l| {
        l.session_id.as_str() == session
            && l.state == crate::contracts::execution::LeaseState::Active
            && crate::contracts::execution::timestamp(&l.expires_at)
                .is_ok_and(|t| t > time::OffsetDateTime::now_utc())
    });
    Snapshot {
        session: session.to_owned(),
        lease: lease.to_owned(),
        active,
    }
}

fn request_id_of(bytes: &[u8]) -> WireUuid {
    unique_json(bytes)
        .ok()
        .and_then(|v| v["request_id"].as_str().map(str::to_owned))
        .and_then(|s| WireUuid::try_from(s).ok())
        .unwrap_or_else(|| WireUuid::try_from(NIL.to_owned()).expect("nil uuid"))
}

impl Authority {
    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            #[cfg(target_os = "linux")]
            workspace_guard: None,
            tickets: BTreeMap::new(),
            // A successor must reconcile previous authority work even with no local exits.
            recovery_pending: true,
            managed_execution_enabled: false,
            housekeeping_at: None,
            exit_lookups: RefCell::new(BTreeMap::new()),
            streams: Default::default(),
            #[cfg(test)]
            post_approval_write: None,
        }
    }

    /// Open a V10 authority and fence any nonterminal claims if this is a new F8 process.
    #[cfg(target_os = "linux")]
    pub fn new_guarded(mut engine: Engine) -> ControlResult<Self> {
        let epoch = engine.authority_id().as_str().to_owned();
        let workspace_guard =
            super::workspace_guard::WorkspaceGuard::open(engine.connection_mut(), epoch)?;
        Ok(Self {
            engine,
            workspace_guard: Some(workspace_guard),
            tickets: BTreeMap::new(),
            recovery_pending: true,
            managed_execution_enabled: false,
            housekeeping_at: None,
            exit_lookups: RefCell::new(BTreeMap::new()),
            streams: Default::default(),
            #[cfg(test)]
            post_approval_write: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn fixture_enable_guarded_execution(&mut self) {
        self.managed_execution_enabled = true;
    }

    #[cfg(target_os = "linux")]
    pub fn workspace_guard_ready(&self) -> ControlResult<bool> {
        match &self.workspace_guard {
            Some(guard) => guard.capability_ready(self.engine.connection()),
            None => Ok(false),
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn workspace_guard_ready(&self) -> ControlResult<bool> {
        Ok(false)
    }
    pub fn engine(&self) -> &Engine {
        &self.engine
    }
    #[cfg(test)]
    pub(crate) fn fixture_after_approval_write(&mut self, inject: fn(&mut Connection)) {
        self.post_approval_write = Some(inject);
    }
    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    fn authority(&self) -> WireUuid {
        self.engine.authority_id().clone()
    }
    fn fail(&self, id: &WireUuid, e: E) -> Vec<u8> {
        Response::error(id.clone(), self.authority(), e)
            .and_then(|r| r.encode())
            .unwrap_or_default()
    }
    fn ok(&self, id: &WireUuid, result: Value) -> Vec<u8> {
        let built = json!({"schema_version":2,"request_id":id,
            "authority_instance_id":self.authority(),"result":result});
        encode(&built)
            .and_then(|b| Response::parse(&b))
            .and_then(|r| r.encode())
            .unwrap_or_else(|e| self.fail(id, e))
    }

    /// One strict protocol-2 exchange. Always returns a well-formed response frame.
    pub fn handle(
        &mut self,
        client: &mut Client,
        bytes: &[u8],
        spawner: &mut dyn Spawner,
        now: Instant,
    ) -> Vec<u8> {
        let id = request_id_of(bytes);
        if unique_json(bytes)
            .ok()
            .and_then(|value| {
                value
                    .get("method")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .is_some_and(|method| {
                method == "authority/capabilities" || method.starts_with("orchestration/")
            })
        {
            return self.handle_workspace_guard(client, bytes, now);
        }
        let request = match Request::parse(bytes) {
            Ok(r) => r,
            Err(e) => return self.fail(&id, e),
        };
        if let Err(e) = request.correlate(client.observed.as_ref()) {
            return self.fail(request.request_id(), e);
        }
        match self.route(client, &request, spawner, now) {
            Ok(result) => self.ok(request.request_id(), result),
            Err(e) => self.fail(request.request_id(), e),
        }
    }

    fn handle_workspace_guard(
        &mut self,
        client: &mut Client,
        bytes: &[u8],
        now: Instant,
    ) -> Vec<u8> {
        let fallback_id = request_id_of(bytes);
        let request = match super::guard_protocol::Request::parse(bytes) {
            Ok(request) => request,
            Err(error) => return self.fail(&fallback_id, error),
        };
        if request.authority_instance_id != self.authority() {
            return self.fail(&request.request_id, E::CorrelationError);
        }
        if client.observed.as_ref() != Some(&request.authority_instance_id) {
            return self.fail(&request.request_id, E::HandshakeRequired);
        }
        let result = (|| -> ControlResult<Value> {
            use super::guard_protocol::Operation as O;
            match request.operation {
                O::Capabilities => {
                    let guard = self.workspace_guard.as_ref().ok_or(E::Unsupported)?;
                    let guard_ready = guard.capability_ready(self.engine.connection())?;
                    Ok(json!({
                        "capability": "workspace_guard_v1",
                        "revision": 1,
                        "authority_store_id": guard.store_id(),
                        "authority_epoch": guard.epoch(),
                        "guard_ready": guard_ready,
                        "managed_execution_enabled": self.managed_execution_enabled,
                        "ready": guard_ready && self.managed_execution_enabled
                    }))
                }
                O::ImportClaims {
                    coordinator_store_id,
                    snapshot_revision,
                    claims,
                } => {
                    let guard = self.workspace_guard.as_ref().ok_or(E::Unsupported)?;
                    let digest = guard.import_claims(
                        self.engine.connection_mut(),
                        &coordinator_store_id,
                        snapshot_revision,
                        &claims,
                    )?;
                    let guard_ready = guard.capability_ready(self.engine.connection())?;
                    Ok(json!({
                        "coordinator_store_id": coordinator_store_id,
                        "snapshot_revision": snapshot_revision,
                        "snapshot_digest": digest,
                        "guard_ready": guard_ready,
                        "managed_execution_enabled": self.managed_execution_enabled,
                        "ready": guard_ready && self.managed_execution_enabled
                    }))
                }
                O::ObserveRoot { locator } => {
                    let guard = self.workspace_guard.as_mut().ok_or(E::Unsupported)?;
                    let root_binding_id =
                        guard.observe_root(self.engine.connection_mut(), &locator)?;
                    Ok(json!({
                        "root_binding_id": root_binding_id,
                        "root_observation_revision": 1,
                        "authority_store_id": guard.store_id(),
                        "authority_epoch": guard.epoch()
                    }))
                }
                O::AcquireClaim {
                    coordinator_store_id,
                    run_id,
                    plan_revision,
                    plan_digest,
                    root_binding_id,
                } => {
                    let guard = self.workspace_guard.as_mut().ok_or(E::Unsupported)?;
                    let claim = guard.acquire_claim(
                        self.engine.connection_mut(),
                        &coordinator_store_id,
                        &run_id,
                        plan_revision,
                        &plan_digest,
                        &root_binding_id,
                    )?;
                    Ok(json!({
                        "claim_id": claim.claim_id,
                        "root_binding_id": claim.root_binding_id,
                        "claim_generation": claim.claim_generation,
                        "claim_revision": claim.claim_revision,
                        "gate_revision": claim.gate_revision,
                        "phase": claim.phase,
                        "gate_state": claim.gate_state,
                        "settlement_id": claim.settlement_id,
                        "coordinator_revision": claim.coordinator_revision
                    }))
                }
                O::SetGate {
                    claim_id,
                    expected_claim_revision,
                    expected_gate_revision,
                    gate,
                    settlement_id,
                    coordinator_revision,
                } => {
                    let guard = self.workspace_guard.as_mut().ok_or(E::Unsupported)?;
                    let claim = guard.set_gate(
                        self.engine.connection_mut(),
                        &claim_id,
                        expected_claim_revision,
                        expected_gate_revision,
                        &gate,
                        settlement_id.as_deref(),
                        coordinator_revision,
                    )?;
                    Ok(json!({
                        "claim_id": claim.claim_id,
                        "root_binding_id": claim.root_binding_id,
                        "claim_generation": claim.claim_generation,
                        "claim_revision": claim.claim_revision,
                        "gate_revision": claim.gate_revision,
                        "phase": claim.phase,
                        "gate_state": claim.gate_state,
                        "settlement_id": claim.settlement_id,
                        "coordinator_revision": claim.coordinator_revision
                    }))
                }
                O::ClaimStatus { claim_id } => {
                    let guard = self.workspace_guard.as_ref().ok_or(E::Unsupported)?;
                    let claim = guard.claim_status(self.engine.connection(), &claim_id)?;
                    Ok(json!({
                        "claim_id": claim.claim_id,
                        "root_binding_id": claim.root_binding_id,
                        "claim_generation": claim.claim_generation,
                        "claim_revision": claim.claim_revision,
                        "gate_revision": claim.gate_revision,
                        "phase": claim.phase,
                        "gate_state": claim.gate_state,
                        "settlement_id": claim.settlement_id,
                        "coordinator_revision": claim.coordinator_revision
                    }))
                }
                O::VerificationPrepare(prepare) => {
                    let guard = self.workspace_guard.as_ref().ok_or(E::Unsupported)?;
                    let prepared =
                        guard.prepare_verification(self.engine.connection_mut(), &prepare)?;
                    Ok(json!({
                        "observation_id": prepared.observation_id,
                        "claim_id": prepared.claim_id,
                        "claim_generation": prepared.claim_generation,
                        "claim_revision": prepared.claim_revision,
                        "root_binding_id": prepared.root_binding_id,
                        "content_digest": prepared.content_digest
                    }))
                }
                O::VerificationCommit(commit) => {
                    let guard = self.workspace_guard.as_ref().ok_or(E::Unsupported)?;
                    let claim = guard.commit_verification(self.engine.connection_mut(), &commit)?;
                    Ok(json!({
                        "claim_id": claim.claim_id,
                        "root_binding_id": claim.root_binding_id,
                        "claim_generation": claim.claim_generation,
                        "claim_revision": claim.claim_revision,
                        "gate_revision": claim.gate_revision,
                        "phase": claim.phase,
                        "gate_state": claim.gate_state,
                        "settlement_id": claim.settlement_id,
                        "coordinator_revision": claim.coordinator_revision
                    }))
                }
                O::ClaimRelease(release) => {
                    let guard = self.workspace_guard.as_ref().ok_or(E::Unsupported)?;
                    let claim = guard.release_claim(self.engine.connection_mut(), &release)?;
                    Ok(json!({
                        "claim_id": claim.claim_id,
                        "root_binding_id": claim.root_binding_id,
                        "claim_generation": claim.claim_generation,
                        "claim_revision": claim.claim_revision,
                        "gate_revision": claim.gate_revision,
                        "phase": claim.phase,
                        "gate_state": claim.gate_state,
                        "settlement_id": claim.settlement_id,
                        "coordinator_revision": claim.coordinator_revision
                    }))
                }
                O::ClaimReleaseUnused(release) => {
                    let guard = self.workspace_guard.as_ref().ok_or(E::Unsupported)?;
                    let claim =
                        guard.release_unused_claim(self.engine.connection_mut(), &release)?;
                    Ok(json!({
                        "claim_id": claim.claim_id,
                        "root_binding_id": claim.root_binding_id,
                        "claim_generation": claim.claim_generation,
                        "claim_revision": claim.claim_revision,
                        "gate_revision": claim.gate_revision,
                        "phase": claim.phase,
                        "gate_state": claim.gate_state,
                        "settlement_id": claim.settlement_id,
                        "coordinator_revision": claim.coordinator_revision
                    }))
                }
                O::StartPrepare(tuple) => {
                    if !self.managed_execution_enabled {
                        return Err(E::GateDenied);
                    }
                    if tuple.authority_epoch != self.authority().as_str() {
                        return Err(E::StaleRevision);
                    }
                    let reference = self
                        .engine
                        .reference(&tuple.session_id)
                        .ok_or(E::StaleRevision)?;
                    if reference.runtime_instance_id.as_str() != tuple.runtime_instance_id
                        || reference.generation != tuple.runtime_generation
                    {
                        return Err(E::StaleRevision);
                    }
                    let fence = reference.fence();
                    let attachment = self
                        .engine
                        .attachment(&tuple.session_id)
                        .ok_or(E::GateDenied)?;
                    self.engine.observe_fence(&fence)?;
                    super::operations::require_bound_launch(&self.engine, &fence)?;
                    self.engine.preflight(&fence, &attachment)?;
                    let lease =
                        super::writers::active_lease(self.engine.connection(), &tuple.session_id)?
                            .ok_or(E::GateDenied)?;
                    if lease.lease_id.as_str() != tuple.lease_id
                        || lease.revision != tuple.lease_revision
                    {
                        return Err(E::StaleRevision);
                    }
                    let prepared = self
                        .workspace_guard
                        .as_mut()
                        .ok_or(E::Unsupported)?
                        .prepare_start(
                            self.engine.connection_mut(),
                            &tuple,
                            &reference,
                            &attachment,
                        )?;
                    Ok(json!({
                        "receipt_id": prepared.receipt_id,
                        "receipt_revision": prepared.receipt_revision,
                        "expires_at": prepared.expires_at,
                        "tuple_digest": prepared.tuple_digest
                    }))
                }
                O::StartCommit(commit) => self.guarded_start_commit(commit, now),
            }
        })();
        match result {
            Ok(value) => {
                super::guard_protocol::success(&request.request_id, &self.authority(), value)
                    .unwrap_or_else(|error| self.fail(&request.request_id, error))
            }
            Err(error) => self.fail(&request.request_id, error),
        }
    }

    fn guarded_start_commit(
        &mut self,
        commit: super::guard_protocol::StartCommit,
        _now: Instant,
    ) -> ControlResult<Value> {
        if !self.managed_execution_enabled
            || super::provider::version(self.engine.connection())? != 10
        {
            return Err(E::GateDenied);
        }
        let tuple = commit.tuple;
        let tuple_digest = tuple.digest()?;
        let prompt_fingerprint = format!("{:x}", Sha256::digest(commit.prompt.as_bytes()));
        if prompt_fingerprint != tuple.prompt_digest {
            return Err(E::InvalidRequest);
        }
        let intent = TurnIntent {
            intent_id: WireUuid::try_from(tuple.intent_id.clone())
                .map_err(|_| E::InvalidRequest)?,
            expected_session_revision: tuple.session_revision,
            prompt_fingerprint,
            prompt_byte_length: commit.prompt.len(),
        };
        let reference = self
            .engine
            .reference(&tuple.session_id)
            .ok_or(E::StaleRevision)?;
        if reference.runtime_instance_id.as_str() != tuple.runtime_instance_id
            || reference.generation != tuple.runtime_generation
        {
            return Err(E::StaleRevision);
        }
        let fence = reference.fence();
        self.engine.observe_fence(&fence)?;
        super::operations::require_bound_launch(&self.engine, &fence)?;
        let attachment = self
            .engine
            .attachment(&tuple.session_id)
            .ok_or(E::GateDenied)?;
        if attachment.revision != tuple.attachment_revision
            || attachment.profile
                != match tuple.model_profile {
                    super::guard_protocol::StartProfile::CodexInferenceV1 => "codex_inference_v1",
                    super::guard_protocol::StartProfile::CodexLocalToolsV1 => {
                        "codex_local_tools_v1"
                    }
                }
        {
            return Err(E::StaleRevision);
        }
        self.engine.preflight(&fence, &attachment)?;
        let lease = super::writers::active_lease(self.engine.connection(), &tuple.session_id)?
            .ok_or(E::GateDenied)?;
        if lease.lease_id.as_str() != tuple.lease_id || lease.revision != tuple.lease_revision {
            return Err(E::StaleRevision);
        }

        // L1: the F8 claim transition, receipt consumption, turn reservation, idempotence
        // result and audit link share the existing single writer transaction.
        let authority = self.authority();
        let (record, fresh) = {
            let guard = self.workspace_guard.as_mut().ok_or(E::Unsupported)?;
            let mut reserve = |tx: &rusqlite::Transaction<'_>, turn_id: &str| {
                guard.consume_prepared_start_in(
                    tx,
                    &tuple,
                    &commit.receipt_id,
                    commit.receipt_revision,
                    &tuple_digest,
                    turn_id,
                )
            };
            Writer::new(self.engine.connection_mut(), authority.clone())?.admit_guarded_turn_ex(
                &reference,
                &intent,
                lease.lease_id.as_str(),
                tuple.lease_revision,
                &tuple_digest,
                &mut reserve,
            )?
        };
        if !fresh {
            return Ok(json!({"kind":"turn","value":record,"replayed":true}));
        }

        let ticket = match self.engine.admit_turn(
            &fence,
            &LeaseClaim {
                lease_id: lease.lease_id.as_str().to_owned(),
                session: tuple.session_id.clone(),
            },
            &attachment,
            record.managed_turn_id.as_str(),
            &lease_snapshot(
                self.engine.connection(),
                &tuple.session_id,
                lease.lease_id.as_str(),
            ),
            0,
        ) {
            Ok(ticket) => ticket,
            Err(error) => {
                self.workspace_guard
                    .as_mut()
                    .ok_or(E::Unsupported)?
                    .mark_guarded_attempt_known_no_send(
                        self.engine.connection_mut(),
                        &tuple.attempt_id,
                    )?;
                self.fail_turn(&record.managed_turn_id)?;
                return Err(error);
            }
        };

        // L2 is persisted before the first transport operation. This transition can happen
        // only once; crashes or ambiguous writes keep the receipt consumed and claim held.
        if let Err(error) = self
            .workspace_guard
            .as_mut()
            .ok_or(E::Unsupported)?
            .mark_guarded_attempt_possibly_sent(self.engine.connection_mut(), &tuple)
        {
            let _ = self.engine.end_turn(&ticket);
            self.workspace_guard
                .as_mut()
                .ok_or(E::Unsupported)?
                .mark_guarded_attempt_known_no_send(
                    self.engine.connection_mut(),
                    &tuple.attempt_id,
                )?;
            self.fail_turn(&record.managed_turn_id)?;
            return Err(error);
        }
        let frame = serde_json::to_vec(&json!({
            "method":"turn/start",
            "turn":record.managed_turn_id,
            "input":commit.prompt
        }))
        .map_err(|_| E::InternalError)?;
        let lease_view = lease_snapshot(
            self.engine.connection(),
            &tuple.session_id,
            lease.lease_id.as_str(),
        );
        if self
            .engine
            .write_guarded_start_frame(&ticket, &frame, &lease_view, 0)
            .is_err()
        {
            self.engine.transport_failed(&ticket.reference);
            let authority = self.authority();
            Writer::new(self.engine.connection_mut(), authority)?.transition_turn(
                &record.managed_turn_id,
                None,
                TurnState::Indeterminate,
                Some(TurnReason::DispatchUnknown),
            )?;
            return Err(E::ExternalEffectUnknown);
        }
        self.tickets
            .insert(record.managed_turn_id.as_str().to_owned(), ticket);
        Ok(json!({"kind":"turn","value":record,"replayed":false}))
    }

    fn route(
        &mut self,
        client: &mut Client,
        request: &Request,
        spawner: &mut dyn Spawner,
        now: Instant,
    ) -> ControlResult<Value> {
        use v1::Operation as O;
        match request.operation() {
            O::Hello(_) => {
                client.observed = Some(self.authority());
                self.status("hello")
            }
            O::Status(_) => self.status("status"),
            O::RuntimeStart(_) => {
                let schema = super::provider::version(self.engine.connection())?;
                let pinned = if schema == 10
                    && matches!(request.operation(), O::RuntimeStart(m) if m.safety_profile.is_some_and(v1::RuntimeSafetyProfile::local_tools))
                {
                    let v1::Operation::RuntimeStart(mutation) = request.operation() else {
                        return Err(E::InvalidRequest);
                    };
                    let locator = mutation
                        .workspace_root
                        .as_deref()
                        .ok_or(E::WorkspaceInvalid)?;
                    #[cfg(target_os = "linux")]
                    {
                        let guard = self.workspace_guard.as_ref().ok_or(E::Unsupported)?;
                        Some(guard.duplicate_root_fd_for_locator(
                            self.engine.connection(),
                            std::path::Path::new(locator),
                        )?)
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        let _ = locator;
                        return Err(E::Unsupported);
                    }
                } else {
                    None
                };
                let root_binding_id = pinned.as_ref().map(|(root, _)| root.clone());
                let authority_instance_id = self.authority().as_str().to_owned();
                let o = self
                    .engine
                    .start_with_pinned_workspace(request, spawner, pinned)?;
                if let (Some(root_binding_id), Some(runtime_instance_id)) =
                    (root_binding_id, o.runtime_instance_id.as_ref())
                {
                    #[cfg(target_os = "linux")]
                    self.workspace_guard
                        .as_ref()
                        .ok_or(E::Unsupported)?
                        .bind_runtime_root(
                            self.engine.connection_mut(),
                            o.managed_session_id.as_str(),
                            runtime_instance_id.as_str(),
                            o.generation,
                            &root_binding_id,
                            &authority_instance_id,
                        )?;
                    #[cfg(not(target_os = "linux"))]
                    return Err(E::Unsupported);
                }
                Ok(runtime_control(&o))
            }
            O::RuntimeStop(_) => {
                let o = self.engine.stop(request, now)?;
                Ok(runtime_control(&o))
            }
            O::RuntimeStatus(_) => self.runtime_status(request),
            O::LeaseGrant(_) | O::LeaseRenew(_) | O::LeaseRevoke(_) => self.lease_op(request),
            // Reads project time expiry exactly as protocol 1 does; the stored row is never changed.
            O::LeaseGet(p) => {
                let l = super::writers::read_lease(self.engine.connection(), p.lease_id.as_str())?;
                lease_value(&l.effective_at(time::OffsetDateTime::now_utc())?)
            }
            O::LeaseActive(p) => {
                let l =
                    super::writers::active_lease(self.engine.connection(), p.session_id.as_str())?
                        .map(|l| l.effective_at(time::OffsetDateTime::now_utc()))
                        .transpose()?
                        .filter(|l| l.state == crate::contracts::execution::LeaseState::Active);
                Ok(json!({"kind":"active_lease","value":l}))
            }
            O::TurnStart(p) => self.turn_start(request, p),
            O::TurnCancel(p) => self.turn_cancel(request, p),
            O::ApprovalRespond(p) => self.approval_respond(request, p, now),
            O::AuditRead(p) => self.audit(&p.stream_id, p.after_sequence, p.limit),
            O::AuditWatch(p) => self.audit(&p.stream_id, p.after_sequence, p.limit),
            _ => super::operations::handle(&mut self.engine, request, &mut self.streams),
        }
    }

    fn status(&self, kind: &str) -> ControlResult<Value> {
        let c = self.engine.connection();
        let (stream, head): (String, String) = c
            .query_row(
                "SELECT stream_id,last_sequence FROM authority_metadata WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| E::StorageUnavailable)?;
        let count = |state: &str| -> ControlResult<i64> {
            c.query_row(
                "SELECT count(*) FROM managed_sessions WHERE control_state=?1",
                [state],
                |r| r.get(0),
            )
            .map_err(|_| E::StorageUnavailable)
        };
        Ok(json!({"kind":kind,"value":{"protocol_version":2,
            "authority_instance_id":self.authority(),"service_version":env!("CARGO_PKG_VERSION"),
            "storage_schema_version":super::provider::version(c)?,"stream_id":stream,
            "last_sequence":if head == "0" { Value::Null } else { json!(head) },
            "prepared_count":count("prepared")?,"closed_count":count("closed")?}}))
    }

    fn runtime_status(&self, request: &Request) -> ControlResult<Value> {
        let Target::Runtime { session, .. } = request.target() else {
            return Err(E::InvalidRequest);
        };
        let s = session.as_str();
        let provider = super::provider::session(self.engine.connection(), s)?;
        let (adapter, runtime_kind) = provider.fields();
        // A status for a session that was never prepared would invent a stopped runtime for it.
        super::operations::session(self.engine.connection(), s)?;
        let row: Option<(String, i64, i64)> = self
            .engine
            .connection()
            .query_row(
                "SELECT desired_state,revision,generation FROM managed_runtimes WHERE managed_session_id=?1",
                [s],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|_| E::StorageUnavailable)?;
        let (desired, revision, generation) = row.unwrap_or(("stopped".into(), 0, 0));
        let view = self.engine.runtime_view(s);
        let (mut actual, identity) = match &view {
            Some((phase, id)) => (*phase, id.clone()),
            None => ("stopped", None),
        };
        // What the live launch was asked to be, and what it listed once it is ready.
        let launch = self.engine.launch_facts(s);
        let (profile, workspace, models) = match &launch {
            Some((Some(root), listed)) => {
                (Some(provider.profile(true)), Some(root.clone()), *listed)
            }
            Some((None, listed)) => (Some(provider.profile(false)), None, *listed),
            None => (None, None, 0),
        };
        let models = if actual == "ready" { models } else { 0 };
        // A runtime that was meant to run and whose launch ended unexpectedly says why, from
        // the durable record of that exit. Nothing about a missing record is invented.
        let (mut diagnostic, mut budget) = (None, None);
        if launch.is_none()
            && desired == "running"
            && generation > 0
            && let Some((kind, key)) = self.unexpected_exit(s, generation)?
        {
            actual = "failed";
            diagnostic = Some(kind);
            budget = key;
        }
        let mut value = json!({"adapter_id":adapter,"runtime_kind":runtime_kind,"desired_state":desired,
            "revision":revision.max(0),"actual_state":actual,
            "runtime_instance_id":self.engine.reference(s).map(|r| r.runtime_instance_id),
            "runtime_version":identity.as_ref().map(|i| i.observed_version.clone()),
            "compatibility":if identity.is_some() { "compatible" } else { "unknown" },
            "model_count":models,"diagnostic":diagnostic,
            "outbound_methods":{"initialize":0,"initialized":0,"model_list":0},
            "managed_session_id":s,"generation":generation,
            "runtime_identity":identity,"budget_failure":budget});
        if let Some(profile) = profile {
            value["runtime_safety_profile"] = json!(profile);
        }
        if let Some(root) = workspace {
            value["workspace_root"] = json!(root);
        }
        Ok(json!({"kind":"runtime_status","value":value}))
    }

    /// The closed exit word, and the closed budget key for a budget exit, of the newest
    /// unexpected exit of one generation of a session, when this very authority recorded it.
    /// What a predecessor recorded is history: the runtime is not running, and that is all this
    /// authority can say of it.
    fn unexpected_exit(&self, session: &str, generation: i64) -> ControlResult<ExitFacts> {
        let key = (session.to_owned(), generation);
        let now = Instant::now();
        if let Some((at, facts)) = self.exit_lookups.borrow().get(&key)
            && (facts.is_some() || now.duration_since(*at) < EXIT_LOOKUP_TTL)
        {
            return Ok(facts.clone());
        }
        let facts = self.read_unexpected_exit(session, generation)?;
        let mut cache = self.exit_lookups.borrow_mut();
        if cache.len() >= EXIT_LOOKUP_LIMIT {
            cache.clear();
        }
        cache.insert(key, (now, facts.clone()));
        Ok(facts)
    }

    fn read_unexpected_exit(&self, session: &str, generation: i64) -> ControlResult<ExitFacts> {
        self.engine
            .connection()
            .query_row(
                "SELECT json_extract(envelope,'$.payload.exit_kind'), json_extract(envelope,'$.payload.budget_failure')
                 FROM control_events
                 WHERE json_extract(envelope,'$.event_type')='authority.runtime.instance_exited'
                   AND json_extract(envelope,'$.session_id')=?1
                   AND json_extract(envelope,'$.payload.generation')=?2
                   AND json_extract(envelope,'$.payload.expected')=0
                   AND json_extract(envelope,'$.source.instance_id')=?3
                 ORDER BY sequence DESC LIMIT 1",
                rusqlite::params![session, generation, self.authority().as_str()],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
            )
            .optional()
            .map_err(|_| E::StorageUnavailable)
    }

    /// The fence names the live attested launch, or the request refuses before anything changes.
    fn fenced(&self, request: &Request) -> ControlResult<RuntimeFence> {
        match request.target() {
            Target::Fenced(f) => {
                self.engine.observe_fence(f)?;
                Ok(f.clone())
            }
            _ => Err(E::InvalidRequest),
        }
    }

    fn lease_op(&mut self, request: &Request) -> ControlResult<Value> {
        // Resolve exact receipts before consulting the current runtime. A receipt never
        // regrants a lease, and a different fence under the same intent is a conflict.
        let intent = match request.operation() {
            v1::Operation::LeaseGrant(p) => p.intent_id.as_str(),
            v1::Operation::LeaseRenew(p) => p.intent_id.as_str(),
            v1::Operation::LeaseRevoke(p) => p.intent_id.as_str(),
            _ => return Err(E::InvalidRequest),
        };
        let previous: Option<(String, String)> = self
            .engine
            .connection()
            .query_row(
                "SELECT mutation,result FROM processed_intents WHERE intent_id=?1",
                [intent],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|_| E::StorageUnavailable)?;
        if let Some((mutation, value)) = previous {
            if mutation.as_bytes() != request.canonical_intent()? {
                return Err(E::IntentConflict);
            }
            let lease = serde_json::from_str(&value).map_err(|_| E::StorageIncompatible)?;
            return lease_value(&lease);
        }
        // Grant and renewal are launch-fenced (protocol 2). Revocation deliberately is not: a
        // grant must stay revocable when its runtime is gone.
        let revoke = matches!(request.operation(), v1::Operation::LeaseRevoke(_));
        let fence = if revoke {
            None
        } else {
            Some(self.fenced(request)?)
        };
        if let (Some(fence), v1::Operation::LeaseRenew(p)) = (&fence, request.operation()) {
            // A renewal targets a lease of the fence's own session, never another's.
            let l = super::writers::read_lease(self.engine.connection(), p.lease_id.as_str())?;
            if l.session_id.as_str() != fence.managed_session_id.as_str() {
                return Err(E::GateDenied);
            }
        }
        let authority = self.authority();
        let lease = Writer::new(self.engine.connection_mut(), authority)?.lease_request(request)?;
        if lease.state != crate::contracts::execution::LeaseState::Active {
            let session = lease.session_id.as_str().to_owned();
            self.lease_lost(&session, lease.lease_id.as_str());
        }
        lease_value(&lease)
    }
    fn lease_lost(&mut self, session: &str, lease_id: &str) {
        self.engine.lease_lost(session, lease_id);
        self.tickets.retain(|_, t| t.lease_id != lease_id);
    }

    fn turn_start(
        &mut self,
        request: &Request,
        p: &crate::contracts::turn::TurnStart,
    ) -> ControlResult<Value> {
        let schema_version = super::provider::version(self.engine.connection())?;
        let Target::Fenced(fence) = request.target() else {
            return Err(E::InvalidRequest);
        };
        let session = fence.managed_session_id.as_str().to_owned();
        let intent = TurnIntent {
            intent_id: p.intent_id.clone(),
            expected_session_revision: p.expected_session_revision,
            prompt_fingerprint: format!("{:x}", Sha256::digest(p.prompt.as_bytes())),
            prompt_byte_length: p.prompt.len(),
        };
        let replay: bool = self
            .engine
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM processed_intents WHERE intent_id=?1)",
                [p.intent_id.as_str()],
                |r| r.get(0),
            )
            .map_err(|_| E::StorageUnavailable)?;
        let authority = self.authority();
        if replay {
            // Receipt replay reads the allocating authority from immutable launch history;
            // succession never grants the successor permission to execute that old launch.
            let instance = fence
                .runtime_instance_id
                .as_ref()
                .ok_or(E::InvalidRequest)?;
            let owner: String = self.engine.connection().query_row(
                "SELECT authority_instance_id FROM managed_runtime_launches WHERE managed_session_id=?1 AND runtime_instance_id=?2 AND generation=?3",
                rusqlite::params![fence.managed_session_id.as_str(), instance.as_str(), fence.generation as i64],
                |r| r.get(0),
            ).map_err(|_| E::IntentConflict)?;
            let reference = RuntimeRef {
                schema: 2,
                authority_instance_id: WireUuid::try_from(owner)
                    .map_err(|_| E::StorageIncompatible)?,
                managed_session_id: fence.managed_session_id.clone(),
                runtime_instance_id: fence.runtime_instance_id.clone().ok_or(E::InvalidRequest)?,
                generation: fence.generation,
            };
            let (record, fresh) = Writer::new(self.engine.connection_mut(), authority)?
                .admit_turn_ex(&reference, &intent, "")?;
            if fresh {
                return Err(E::InternalError);
            }
            return Ok(json!({"kind":"turn","value":record}));
        }
        self.engine.observe_fence(fence)?;
        let reference = self.engine.reference(&session).ok_or(E::StaleRevision)?;
        let lease = super::writers::active_lease(self.engine.connection(), &session)?
            .ok_or(E::GateDenied)?;
        super::operations::require_bound_launch(&self.engine, fence)?;
        let attachment = self.engine.attachment(&session).ok_or(E::GateDenied)?;
        self.engine.preflight(fence, &attachment)?;
        if schema_version == 10
            && !self
                .workspace_guard
                .as_ref()
                .ok_or(E::GateDenied)?
                .ordinary_start_allowed(
                    self.engine.connection(),
                    reference.runtime_instance_id.as_str(),
                )?
        {
            return Err(E::GateDenied);
        }
        let (record, fresh) = Writer::new(self.engine.connection_mut(), authority.clone())?
            .admit_turn_ex(&reference, &intent, lease.lease_id.as_str())?;
        if !fresh {
            return Ok(json!({"kind":"turn","value":record}));
        }
        let attachment: Attachment = self.engine.attachment(&session).ok_or(E::GateDenied)?;
        let snapshot = lease_snapshot(self.engine.connection(), &session, lease.lease_id.as_str());
        let admitted = self.engine.admit_turn(
            fence,
            &LeaseClaim {
                lease_id: lease.lease_id.as_str().to_owned(),
                session: session.clone(),
            },
            &attachment,
            record.managed_turn_id.as_str(),
            &snapshot,
            0,
        );
        let ticket = match admitted {
            Ok(t) => t,
            Err(e) => {
                self.fail_turn(&record.managed_turn_id)?;
                return Err(e);
            }
        };
        let frame =
            serde_json::to_vec(&json!({"method":"turn/start","turn":record.managed_turn_id,
            "input":p.prompt}))
            .map_err(|_| E::InternalError)?;
        if self
            .engine
            .write_frame(&ticket, &frame, &snapshot, 0)
            .is_err()
        {
            // A failed write is not proof that zero bytes/effects reached the
            // provider. Never claim a definitive no-effect failure or resend.
            let authority = self.authority();
            let recorded =
                Writer::new(self.engine.connection_mut(), authority).and_then(|mut w| {
                    w.transition_turn(
                        &record.managed_turn_id,
                        None,
                        TurnState::Indeterminate,
                        Some(TurnReason::DispatchUnknown),
                    )
                });
            // Cleanup is mandatory even when SQLite refuses the indeterminate
            // journal record. A later reaped-runtime recovery reconciles it.
            self.engine.transport_failed(&ticket.reference);
            recorded?;
            return Err(E::ExternalEffectUnknown);
        }
        self.tickets
            .insert(record.managed_turn_id.as_str().to_owned(), ticket);
        Ok(json!({"kind":"turn","value":record}))
    }
    /// A turn which never reached the provider ends failed, with no effect to be unknown.
    fn fail_turn(&mut self, turn: &ManagedTurnId) -> ControlResult<()> {
        let authority = self.authority();
        Writer::new(self.engine.connection_mut(), authority)?
            .transition_turn(
                turn,
                None,
                TurnState::Failed,
                Some(TurnReason::ProviderFailed),
            )
            .map(|_| ())
    }

    /// Legacy writer receipts already bind these operations to immutable sidecars. Reconstruct
    /// that original fence before admission so exact historical replay remains readable, while
    /// a changed fence or operation cannot masquerade as the original intent.
    fn replay_record_intent(
        &self,
        request: &Request,
        intent: &str,
        table: &str,
        key: &str,
    ) -> ControlResult<Option<Value>> {
        let previous: Option<(String, String)> = self
            .engine
            .connection()
            .query_row(
                "SELECT mutation,result FROM processed_intents WHERE intent_id=?1",
                [intent],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|_| E::StorageUnavailable)?;
        let Some((mutation, result)) = previous else {
            return Ok(None);
        };
        let mut original = unique_json(mutation.as_bytes()).map_err(|_| E::StorageIncompatible)?;
        let proposed = unique_json(&request.canonical_intent()?)?;
        if original["method"] != proposed["method"] {
            return Err(E::IntentConflict);
        }
        let id = original["params"][key]
            .as_str()
            .ok_or(E::StorageIncompatible)?;
        let sql = format!(
            "SELECT managed_session_id,generation,runtime_instance_id FROM {table} WHERE {key}=?1"
        );
        let (session, generation, instance): (String, i64, String) = self
            .engine
            .connection()
            .query_row(&sql, [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|_| E::StorageIncompatible)?;
        let generation = u64::try_from(generation).map_err(|_| E::StorageIncompatible)?;
        original["params"]["runtime_fence"] = json!({"managed_session_id":session,
            "generation":generation,"runtime_instance_id":instance});
        if original != proposed {
            return Err(E::IntentConflict);
        }
        Ok(Some(
            unique_json(result.as_bytes()).map_err(|_| E::StorageIncompatible)?,
        ))
    }

    fn turn_cancel(
        &mut self,
        request: &Request,
        p: &crate::contracts::turn::TurnCancel,
    ) -> ControlResult<Value> {
        if let Some(record) = self.replay_record_intent(
            request,
            p.intent_id.as_str(),
            "managed_turn_launches",
            "managed_turn_id",
        )? {
            return Ok(json!({"kind":"turn","value":record}));
        }
        let Target::Fenced(fence) = request.target() else {
            return Err(E::InvalidRequest);
        };
        // The fence must be the very launch the turn was admitted on (immutable sidecar).
        let sidecar: Option<(String, i64, String)> = self
            .engine
            .connection()
            .query_row(
                "SELECT runtime_instance_id,generation,managed_session_id FROM managed_turn_launches WHERE managed_turn_id=?1",
                [p.managed_turn_id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|_| E::StorageUnavailable)?;
        let (instance, generation, session) = sidecar.ok_or(E::TurnNotFound)?;
        if fence.managed_session_id.as_str() != session
            || fence.runtime_instance_id.as_ref().map(|i| i.as_str()) != Some(instance.as_str())
            || fence.generation != generation as u64
        {
            return Err(E::StaleRevision);
        }
        // A new cancellation requires the observed live launch BEFORE durable mutation.
        // Exact already-committed intent replays and terminal turns need no live process.
        let replay: bool = self
            .engine
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM processed_intents WHERE intent_id=?1)",
                [p.intent_id.as_str()],
                |r| r.get(0),
            )
            .map_err(|_| E::StorageUnavailable)?;
        let active: bool = self.engine.connection().query_row(
            "SELECT state IN ('starting','running') FROM managed_turns WHERE managed_turn_id=?1",
            [p.managed_turn_id.as_str()], |r| r.get(0),
        ).map_err(|_| E::TurnNotFound)?;
        if active && !replay {
            self.engine.observe_fence(fence)?;
        }
        let authority = self.authority();
        let (record, dispatch) =
            Writer::new(self.engine.connection_mut(), authority)?.cancel_turn(p)?;
        if dispatch {
            let Some(ticket) = self.tickets.get(p.managed_turn_id.as_str()).cloned() else {
                // Durable cancel_requested is not evidence that any provider
                // received an interrupt. Fence this launch; effect is unknown.
                if let Some(reference) = self.engine.reference(&session) {
                    self.engine.transport_failed(&reference);
                }
                return Err(E::ExternalEffectUnknown);
            };
            {
                let snapshot = lease_snapshot(self.engine.connection(), &session, &ticket.lease_id);
                // A buffered write is only queued, not proof that the provider
                // processed the interrupt. Refusals must never be silently hidden.
                if self
                    .engine
                    .write_frame(&ticket, b"{\"method\":\"turn/interrupt\"}", &snapshot, 0)
                    .is_err()
                {
                    self.engine.transport_failed(&ticket.reference);
                    return Err(E::ExternalEffectUnknown);
                }
                // The write is accepted, not answered: the provider now owes the turn's end.
                self.engine
                    .interrupt_dispatched(&ticket.reference, Instant::now());
            }
        }
        Ok(json!({"kind":"turn","value":record}))
    }

    fn approval_respond(
        &mut self,
        request: &Request,
        p: &crate::contracts::approval::ApprovalRespond,
        now: Instant,
    ) -> ControlResult<Value> {
        let _ = now;
        if let Some(receipt) = self.replay_record_intent(
            request,
            p.intent_id.as_str(),
            "managed_approval_launches",
            "managed_approval_id",
        )? {
            return Ok(json!({"kind":"approval_receipt","value":receipt}));
        }
        let Target::Fenced(fence) = request.target() else {
            return Err(E::InvalidRequest);
        };
        let session = fence.managed_session_id.as_str().to_owned();
        let row: Option<(String, String, String, String)> = self
            .engine
            .connection()
            .query_row(
                "SELECT managed_turn_id,provider_item_id,managed_session_id,action_fingerprint FROM managed_approvals WHERE managed_approval_id=?1",
                [p.managed_approval_id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(|_| E::StorageUnavailable)?;
        let (turn, item, owner, durable_fingerprint) = row.ok_or(E::ApprovalNotFound)?;
        if owner != session {
            return Err(E::GateDenied);
        }
        let (instance,generation): (String,i64) = self.engine.connection().query_row(
            "SELECT runtime_instance_id,generation FROM managed_approval_launches WHERE managed_approval_id=?1",
            [p.managed_approval_id.as_str()], |r| Ok((r.get(0)?,r.get(1)?))).map_err(|_| E::ApprovalNotFound)?;
        if fence.runtime_instance_id.as_ref().map(|i| i.as_str()) != Some(instance.as_str())
            || fence.generation != generation as u64
        {
            return Err(E::StaleRevision);
        }
        let replay: bool = self
            .engine
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM processed_intents WHERE intent_id=?1)",
                [p.intent_id.as_str()],
                |r| r.get(0),
            )
            .map_err(|_| E::StorageUnavailable)?;
        if !replay {
            self.engine.observe_fence(fence)?;
        }
        // The fingerprint compared at decision time is the provider's own live request, never the
        // client's claim. After a restart none exists, so a late answer cannot be decided at all.
        let live = self.engine.pending_fingerprint(&session, &item);
        // A durable fingerprint is history, not a live admission ticket. Only an
        // exact replay may be read after the pending provider request disappeared.
        // The writer still validates the canonical replay payload itself.
        if live.is_none() {
            let existing: Option<i64> = self
                .engine
                .connection()
                .query_row(
                    "SELECT 1 FROM processed_intents WHERE intent_id=?1",
                    [p.intent_id.as_str()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|_| E::StorageUnavailable)?;
            if existing.is_none() {
                return Err(E::StaleRevision);
            }
        }
        if !replay {
            if !self
                .engine
                .approval_decisions(&session, &item)
                .contains(&p.decision)
            {
                return Err(E::Unsupported);
            }
            let attachment = self.engine.attachment(&session).ok_or(E::GateDenied)?;
            self.engine.preflight(fence, &attachment)?;
        }
        if !replay && p.decision == ApprovalDecision::Accept {
            self.engine.approval_action_confined(fence, &item)?;
        }
        let authority = self.authority();
        let decided = Writer::new(self.engine.connection_mut(), authority.clone())?
            .decide_approval(
                &p.managed_approval_id,
                &p.intent_id,
                p.expected_revision,
                p.decision,
                live.as_deref().unwrap_or(&durable_fingerprint),
            );
        match decided? {
            Decided::Replayed(receipt) => Ok(json!({"kind":"approval_receipt","value":receipt})),
            Decided::Fresh(record, receipt) => {
                let ticket = self.tickets.get(&turn).cloned();
                let word = match p.decision {
                    ApprovalDecision::Accept => "accept",
                    ApprovalDecision::Decline => "decline",
                    ApprovalDecision::Cancel => "cancel",
                };
                let sent = match (&ticket, &live) {
                    (Some(t), Some(fp)) => {
                        let snapshot =
                            lease_snapshot(self.engine.connection(), &session, &t.lease_id);
                        self.engine
                            .answer_approval(t, &snapshot, 0, &item, fp, word)
                    }
                    _ => Err(E::StaleRevision),
                };
                if sent.is_err() {
                    // All decisions are already durable. A declined or cancelled request
                    // that was not delivered must NOT be reported as successful dispatch.
                    // Fence the affected runtime; never retry an ambiguous provider write.
                    // A missing transient ticket is also a protocol failure. The
                    // already-validated request fence still identifies the single
                    // affected launch, even when no TurnTicket survived.
                    if let Some(reference) = self.engine.reference(&session) {
                        self.engine.transport_failed(&reference);
                    }
                    if p.decision == ApprovalDecision::Accept {
                        // A persisted accept may have reached the provider before the
                        // transport failed. Keep that effect indeterminate, not retried.
                        let _ = Writer::new(self.engine.connection_mut(), authority)?
                            .settle_approvals(
                                Some(&record.managed_turn_id),
                                ApprovalReason::EffectUnknown,
                            );
                        return Err(E::ExternalEffectUnknown);
                    }
                    return Err(E::TransportError);
                }
                #[cfg(test)]
                if let Some(inject) = self.post_approval_write.take() {
                    inject(self.engine.connection_mut());
                }
                let recorded =
                    Writer::new(self.engine.connection_mut(), authority).and_then(|mut writer| {
                        writer.record_delivery(
                            &p.managed_approval_id,
                            ApprovalDelivery::KernelAccepted,
                        )
                    });
                if let Err(e) = recorded {
                    if let Some(reference) = self.engine.reference(&session) {
                        self.engine.transport_failed(&reference);
                    }
                    return Err(e);
                }
                Ok(json!({"kind":"approval_receipt","value":receipt}))
            }
        }
    }

    /// A protocol-2 refusal for a request the transport declines to hold. Nothing is written.
    pub fn refusal(&self, request: &[u8], e: E) -> Vec<u8> {
        self.fail(&request_id_of(request), e)
    }

    /// Whether the durable stream holds an event after `after`. A read only: it wakes a pending
    /// audit watch cheaply, and the watch itself is then answered by an ordinary audit read.
    pub fn audit_has_after(&self, after: u64) -> ControlResult<bool> {
        let head: String = self
            .engine
            .connection()
            .query_row(
                "SELECT last_sequence FROM authority_metadata WHERE id=1",
                [],
                |r| r.get(0),
            )
            .map_err(|_| E::StorageUnavailable)?;
        Ok(head.parse::<u64>().map_err(|_| E::StorageIncompatible)? > after)
    }

    fn audit(
        &self,
        stream: &WireUuid,
        after: Option<crate::contracts::Sequence>,
        limit: u16,
    ) -> ControlResult<Value> {
        let c = self.engine.connection();
        let (current, head): (String, String) = c
            .query_row(
                "SELECT stream_id,last_sequence FROM authority_metadata WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| E::StorageUnavailable)?;
        let after = after.map_or(0, |s| s.get());
        if limit == 0
            || stream.as_str() != current
            || after > head.parse::<u64>().map_err(|_| E::StorageIncompatible)?
        {
            return Err(E::InvalidRequest);
        }
        let mut q = c
            .prepare(
                "SELECT envelope FROM control_events WHERE sequence>?1 ORDER BY sequence LIMIT ?2",
            )
            .map_err(|_| E::StorageUnavailable)?;
        let mut rows = q
            .query(rusqlite::params![
                format!("{after:020}"),
                i64::from(limit) + 1
            ])
            .map_err(|_| E::StorageUnavailable)?;
        let (mut events, mut bytes, mut more) = (Vec::new(), 0usize, false);
        while let Some(r) = rows.next().map_err(|_| E::StorageUnavailable)? {
            let raw: String = r.get(0).map_err(|_| E::StorageUnavailable)?;
            if events.len() == usize::from(limit)
                || (!events.is_empty() && bytes + raw.len() + 1 > PAGE_BYTES)
            {
                more = true;
                break;
            }
            bytes += raw.len() + 1;
            events.push(unique_json(raw.as_bytes()).map_err(|_| E::StorageIncompatible)?);
        }
        let next = if more {
            events
                .last()
                .and_then(|e| e["sequence"].as_str().map(str::to_owned))
        } else {
            None
        };
        Ok(json!({"kind":"audit","value":{"events":events,"next":next}}))
    }

    // ---------------------------------------------------------- provider side

    /// The provider asked for an approval on the running turn of `from`. Durable first (under the
    /// turn's original lease), then the live correlation; either failing leaves nothing half-made.
    pub fn provider_approval_request(
        &mut self,
        from: &RuntimeRef,
        kind: ApprovalKind,
        provider_item_id: &str,
        action_fingerprint: &str,
        structure: ApprovalStructure,
    ) -> ControlResult<crate::contracts::approval::ManagedApprovalRecord> {
        // Verify the independently observed provider connection before creating durable work.
        // A caller-supplied record ID cannot select a different session or launch.
        self.engine.observe_fence(&from.fence())?;
        let session = from.managed_session_id.as_str().to_owned();
        let turn = self
            .engine
            .active_turn(&session)
            .ok_or(E::GateDenied)?
            .to_owned();
        let turn = ManagedTurnId::parse(&turn).map_err(|_| E::InternalError)?;
        self.engine.can_request_approval(from, provider_item_id)?;
        let authority = self.authority();
        let record = Writer::new(self.engine.connection_mut(), authority.clone())?.open_approval(
            &turn,
            kind,
            provider_item_id,
            action_fingerprint,
            structure,
        )?;
        if let Err(e) = self
            .engine
            .approval_requested(from, provider_item_id, action_fingerprint)
        {
            // Only an unexpected post-write admission failure reaches here.
            // This runtime is inconsistent; fail the launch closed rather than
            // silently destroying other approvals while continuing to execute.
            self.engine.transport_failed(from);
            let _ = Writer::new(self.engine.connection_mut(), authority)?
                .settle_approvals(Some(&turn), ApprovalReason::Unsupported);
            return Err(e);
        }
        Ok(record)
    }
    /// The provider reported its turn running / finished. Terminal states end the live turn.
    pub fn provider_turn_event(
        &mut self,
        from: &RuntimeRef,
        turn: &ManagedTurnId,
        provider_turn_id: Option<&str>,
        state: TurnState,
        reason: Option<TurnReason>,
    ) -> ControlResult<()> {
        // The provider connection, not the event's record ID, determines its launch.
        // Durable ownership by *this authority* is insufficient when multiple sessions
        // belong to the same authority.
        self.engine.observe_fence(&from.fence())?;
        let row: Option<(String, String, i64)> = self
            .engine
            .connection()
            .query_row(
                "SELECT managed_session_id,runtime_instance_id,generation FROM managed_turn_launches WHERE managed_turn_id=?1",
                [turn.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|_| E::StorageUnavailable)?;
        let (session, instance, generation) = row.ok_or(E::TurnNotFound)?;
        if session != from.managed_session_id.as_str()
            || instance != from.runtime_instance_id.as_str()
            || generation != i64::try_from(from.generation).map_err(|_| E::StaleRevision)?
            || self.engine.active_turn(&session) != Some(turn.as_str())
        {
            return Err(E::StaleRevision);
        }
        let authority = self.authority();
        Writer::new(self.engine.connection_mut(), authority)?.transition_turn(
            turn,
            provider_turn_id,
            state,
            reason,
        )?;
        if state.terminal()
            && let Some(ticket) = self.tickets.remove(turn.as_str())
        {
            let _ = self.engine.end_turn(&ticket);
        }
        Ok(())
    }
    /// A provider acknowledgement is attributed to its actual connection and item.
    /// The durable approval ID alone is never a dispatch or confirmation capability.
    pub fn provider_approval_confirmed(
        &mut self,
        from: &RuntimeRef,
        approval: &crate::contracts::identities::ManagedApprovalId,
        provider_item_id: &str,
    ) -> ControlResult<()> {
        self.engine.observe_fence(&from.fence())?;
        let row: Option<(String, String, String, String, i64)> = self
            .engine
            .connection()
            .query_row(
                "SELECT a.managed_session_id,a.managed_turn_id,a.provider_item_id,l.runtime_instance_id,l.generation                  FROM managed_approvals a JOIN managed_approval_launches l                  ON a.managed_approval_id=l.managed_approval_id WHERE a.managed_approval_id=?1",
                [approval.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()
            .map_err(|_| E::StorageUnavailable)?;
        let (session, turn, item, instance, generation) = row.ok_or(E::ApprovalNotFound)?;
        if session != from.managed_session_id.as_str()
            || instance != from.runtime_instance_id.as_str()
            || generation != i64::try_from(from.generation).map_err(|_| E::StaleRevision)?
            || item != provider_item_id
            || self.engine.active_turn(&session) != Some(turn.as_str())
        {
            return Err(E::StaleRevision);
        }
        let authority = self.authority();
        Writer::new(self.engine.connection_mut(), authority)?
            .confirm_approval(approval)
            .map(|_| ())
    }

    fn approval_for_item(
        &self,
        from: &RuntimeRef,
        item: &str,
    ) -> ControlResult<crate::contracts::identities::ManagedApprovalId> {
        self.engine.observe_fence(&from.fence())?;
        let turn = self
            .engine
            .active_turn(from.managed_session_id.as_str())
            .ok_or(E::TurnNotFound)?;
        let id: String = self.engine.connection().query_row(
            "SELECT a.managed_approval_id FROM managed_approvals a JOIN managed_approval_launches l USING(managed_approval_id) WHERE a.managed_session_id=?1 AND a.managed_turn_id=?2 AND a.provider_item_id=?3 AND l.runtime_instance_id=?4 AND l.generation=?5",
            rusqlite::params![from.managed_session_id.as_str(),turn,item,from.runtime_instance_id.as_str(),from.generation as i64], |r|r.get(0)).map_err(|_|E::ApprovalNotFound)?;
        crate::contracts::identities::ManagedApprovalId::parse(&id)
            .map_err(|_| E::StorageIncompatible)
    }
    fn provider_content(
        &mut self,
        from: &RuntimeRef,
        turn: &ManagedTurnId,
        provider_turn: &str,
        text: &str,
    ) -> ControlResult<()> {
        self.engine.observe_fence(&from.fence())?;
        if text.len() > 262_144
            || self.engine.active_turn(from.managed_session_id.as_str()) != Some(turn.as_str())
        {
            return Err(E::CorrelationError);
        }
        let record = super::writers::read_turn_record(self.engine.connection(), turn.as_str())?;
        if record.state != TurnState::Running
            || record.managed_session_id != from.managed_session_id
            || record.runtime_instance_id != from.runtime_instance_id
            || record.codex_turn_id.as_ref().map(|p| p.as_str()) != Some(provider_turn)
        {
            return Err(E::CorrelationError);
        }
        self.streams.push(
            &record,
            crate::contracts::turn::StreamKind::AssistantMessageDelta,
            Some(text),
        );
        Ok(())
    }

    /// Drain the bounded scheduler and attribute every normalized frame to the
    /// independently owned worker that produced it. An unknown frame is refused,
    /// never treated as proof of execution authority or silently accumulated.
    fn route_provider_frames(&mut self, now: Instant) {
        let dispatched = self.engine.dispatch(now);
        for frame in dispatched {
            let Some(from) = self.engine.reference(&frame.runtime) else {
                continue;
            };
            if frame.class != super::scheduler::Class::Data {
                // No adapter mapping exists for generic control frames yet.
                self.engine.transport_failed(&from);
                continue;
            }
            let notice = unique_json(&frame.item).and_then(|v| {
                serde_json::from_value::<ProviderNotice>(v).map_err(|_| E::InvalidRequest)
            });
            if self.engine.provider_exited(&from)
                && matches!(
                    &notice,
                    Ok(ProviderNotice::RuntimeReady { .. }
                        | ProviderNotice::BindingConfirmed { .. }
                        | ProviderNotice::Models { .. }
                        | ProviderNotice::ApprovalRequired { .. })
                )
            {
                self.engine.transport_failed(&from);
                continue;
            }
            let result = match notice {
                Ok(ProviderNotice::RuntimeReady {
                    observed_version,
                    workspace,
                    profile,
                    revision,
                }) => self
                    .engine
                    .complete_handshake(
                        &from,
                        &observed_version,
                        Attachment {
                            workspace,
                            profile,
                            revision,
                        },
                    )
                    .map(|_| ()),
                Ok(ProviderNotice::BindingConfirmed {
                    operation_id,
                    provider_thread_id,
                }) => super::operations::confirm_binding(
                    &mut self.engine,
                    &from,
                    &operation_id,
                    &provider_thread_id,
                ),
                Ok(ProviderNotice::Models {
                    models,
                    offers,
                    complete,
                }) => self.engine.record_models(&from, models, offers, complete),
                Ok(ProviderNotice::Content {
                    managed_turn_id,
                    provider_turn_id,
                    text,
                }) => self.provider_content(&from, &managed_turn_id, &provider_turn_id, &text),
                Ok(ProviderNotice::ApprovalItemConsumed { provider_item_id }) => self
                    .approval_for_item(&from, &provider_item_id)
                    .and_then(|id| {
                        let authority = self.authority();
                        Writer::new(self.engine.connection_mut(), authority)?
                            .record_delivery(&id, ApprovalDelivery::ProviderConsumed)
                    }),
                Ok(ProviderNotice::ApprovalItemConfirmed { provider_item_id }) => self
                    .approval_for_item(&from, &provider_item_id)
                    .and_then(|id| self.provider_approval_confirmed(&from, &id, &provider_item_id)),
                Ok(ProviderNotice::TurnInterrupted { managed_turn_id }) => self
                    .provider_turn_event(
                        &from,
                        &managed_turn_id,
                        None,
                        TurnState::Interrupted,
                        Some(TurnReason::UserCancel),
                    ),
                Ok(ProviderNotice::TurnRunning {
                    managed_turn_id,
                    provider_turn_id,
                }) => self.provider_turn_event(
                    &from,
                    &managed_turn_id,
                    Some(&provider_turn_id),
                    TurnState::Running,
                    None,
                ),
                Ok(ProviderNotice::TurnCompleted {
                    managed_turn_id,
                    provider_turn_id,
                }) => self.provider_turn_event(
                    &from,
                    &managed_turn_id,
                    Some(&provider_turn_id),
                    TurnState::Completed,
                    Some(TurnReason::Completed),
                ),
                Ok(ProviderNotice::TurnFailed { managed_turn_id }) => self.provider_turn_event(
                    &from,
                    &managed_turn_id,
                    None,
                    TurnState::Failed,
                    Some(TurnReason::ProviderFailed),
                ),
                Ok(ProviderNotice::ApprovalRequired {
                    approval_kind,
                    provider_item_id,
                    action_fingerprint,
                    structure,
                    action,
                }) => self
                    .provider_approval_request(
                        &from,
                        approval_kind,
                        &provider_item_id,
                        &action_fingerprint,
                        structure,
                    )
                    .and_then(|_| match action {
                        Some(action) => {
                            self.engine
                                .hold_approval_action(&from, &provider_item_id, action)
                        }
                        None => Ok(()),
                    }),
                Ok(ProviderNotice::ApprovalConfirmed {
                    managed_approval_id,
                    provider_item_id,
                }) => {
                    self.provider_approval_confirmed(&from, &managed_approval_id, &provider_item_id)
                }
                Err(e) => Err(e),
            };
            if result.is_err() {
                // A malformed or cross-session provider message faults that runtime
                // alone. The fence is known from the owned worker, never from JSON.
                self.engine.transport_failed(&from);
            }
        }
    }

    /// Periodic duty, outside any request: expire leases and approvals (and tell the engine),
    /// drive process lifecycles, and record work stranded by launches that ended.
    pub fn tick(&mut self, now: Instant) -> ControlResult<()> {
        let authority = self.authority();
        // Expiry is projected by the clock wherever it is read, so recording it durably does not
        // need the supervisor's cadence: a quarter of a second is a tenth of the work at rest.
        let due = self.housekeeping_at.is_none_or(|at| now >= at);
        if due {
            self.housekeeping_at = Some(now + HOUSEKEEPING_INTERVAL);
        }
        let housekeeping = (|| -> ControlResult<()> {
            if !due {
                return Ok(());
            }
            let expired: Vec<(String, String)> = {
                let mut q = self
                .engine
                .connection()
                .prepare("SELECT session_id,lease_id FROM execution_leases WHERE state='active' AND expires_at<=?1")
                .map_err(|_| E::StorageUnavailable)?;
                let at = time::OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)
                    .map_err(|_| E::InternalError)?;
                q.query_map([at], |r| Ok((r.get(0)?, r.get(1)?)))
                    .map_err(|_| E::StorageUnavailable)?
                    .collect::<Result<_, _>>()
                    .map_err(|_| E::StorageUnavailable)?
            };
            Writer::new(self.engine.connection_mut(), authority.clone())?.expire_leases()?;
            for (session, lease) in expired {
                self.lease_lost(&session, &lease);
            }
            Writer::new(self.engine.connection_mut(), authority.clone())?.expire_approvals()?;
            Ok(())
        })();
        // Lifecycle progress must never depend on SQLite accepting an unrelated expiry write.
        self.engine.poll(now);
        self.route_provider_frames(now);
        if self.engine.take_ended() > 0 {
            self.recovery_pending = true;
        }
        housekeeping?;
        if self.recovery_pending {
            // Keep the recovery edge set on any SQLite/audit failure; next tick retries.
            let shutdown = self.engine.shutting_down();
            Writer::new(self.engine.connection_mut(), authority)?.recover_as(shutdown)?;
            // A binding the lost runtime was creating or resuming is uncertain, not pending: it
            // is recorded as such so it can be seen, and abandoned, instead of waiting forever.
            super::operations::recover_bindings(&mut self.engine)?;
            let live: Vec<String> = self.tickets.keys().cloned().collect();
            for turn in live {
                let state: Option<String> = self
                    .engine
                    .connection()
                    .query_row(
                        "SELECT state FROM managed_turns WHERE managed_turn_id=?1",
                        [&turn],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(|_| E::StorageUnavailable)?;
                if state
                    .as_deref()
                    .is_none_or(|s| !matches!(s, "starting" | "running"))
                {
                    self.tickets.remove(&turn);
                }
            }
            self.recovery_pending = false;
        }
        Ok(())
    }
}

fn runtime_control(o: &super::allocator::Outcome) -> Value {
    json!({"kind":"runtime_control","value":{"adapter_id":o.record.adapter_id,"runtime_kind":o.record.runtime_kind,
        "desired_state":o.record.desired_state,"revision":o.record.revision,"created_at":o.record.created_at,
        "updated_at":o.record.updated_at,"managed_session_id":o.managed_session_id,"generation":o.generation}})
}
fn lease_value(l: &crate::contracts::execution::ExecutionLease) -> ControlResult<Value> {
    Ok(json!({"kind":"lease","value":l}))
}

/// How often the durable expiry of leases and approvals is recorded.
const HOUSEKEEPING_INTERVAL: Duration = Duration::from_millis(250);
/// A found exit is final for its generation; a missing one is looked for again after this long.
const EXIT_LOOKUP_TTL: Duration = Duration::from_millis(200);
const EXIT_LOOKUP_LIMIT: usize = 256;

#[cfg(test)]
#[path = "authority_tests.rs"]
mod tests;
