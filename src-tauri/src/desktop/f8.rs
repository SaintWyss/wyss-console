//! The window's door to a protocol-2 authority: product commands, none of which names a
//! control method.
//!
//! **Inactive by default.** The desktop manages an inactive `F8Gui`. Every command answers
//! `inactive` and sends nothing until Rust code that owns a connector has made the managed
//! service active; no request from the window can.
//!
//! Each command takes one narrow request with unknown fields refused. The window cannot
//! choose an adapter, a runtime kind, a model, an effort, a tool or a safety profile by name:
//! it names a session, turn or approval it was shown, the fence and revision it saw, the
//! identity of its own action and, where the product has the choice, one of a closed set.
//! A decision on an approval is never data: accept, decline and cancel are three commands.
//! The one path it can name is a folder the native dialog returned in this run.
use serde::Deserialize;
use std::sync::atomic::{AtomicUsize, Ordering};
use tauri::State;

use super::workspace::WorkspaceSelections;
mod confirm;
pub use crate::{
    contracts::control::{ApprovalDecision, ExecutionScope},
    f8::gui::{
        AbandonReceipt, AuditView, BindingReceipt, DecisionReceipt, F8Error, F8Failure, F8Gui,
        FenceView, LeaseReceipt, ModelsView, RuntimeReceipt, SessionDetail, SessionReceipt,
        SessionsView, StateView, TurnReading, TurnReceipt, WatchView,
    },
    turns::ApprovalDetail,
};
use crate::{
    contracts::{
        Sequence, WireUuid,
        identities::{ExecutionLeaseId, ManagedApprovalId, ManagedSessionId, ManagedTurnId},
    },
    sessions::AgentMode,
};
pub use confirm::{F8Confirm, GtkConfirmer};

/// Intents admitted at once. They run one at a time on the control link.
const MAX_INTENTS: usize = 2;
static INTENTS: AtomicUsize = AtomicUsize::new(0);
/// Reads admitted at once on the control link.
const MAX_READS: usize = 4;
static READS: AtomicUsize = AtomicUsize::new(0);
/// A turn follower, and an audit watcher: one of each, on links of their own.
const MAX_FOLLOW: usize = 1;
static FOLLOW: AtomicUsize = AtomicUsize::new(0);
const MAX_WATCH: usize = 1;
static WATCH: AtomicUsize = AtomicUsize::new(0);

struct Permit(&'static AtomicUsize);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionsRequest {
    /// Sessions created before this position. Null is the newest page.
    before: Option<Sequence>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRequest {
    session_id: ManagedSessionId,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareRequest {
    /// Names this user action. The same value sent again is the same action.
    intent_id: WireUuid,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloseRequest {
    intent_id: WireUuid,
    fence: FenceView,
    expected_revision: u64,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartRuntimeRequest {
    intent_id: WireUuid,
    session_id: ManagedSessionId,
    /// The revision and generation of the runtime the window was shown.
    expected_revision: u64,
    expected_generation: u64,
    mode: AgentMode,
    /// Local tools only: a folder the native dialog returned.
    workspace: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopRuntimeRequest {
    intent_id: WireUuid,
    fence: FenceView,
    expected_revision: u64,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelsRequest {
    fence: FenceView,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantRequest {
    intent_id: WireUuid,
    fence: FenceView,
    expected_session_revision: u64,
    scopes: Vec<ExecutionScope>,
    ttl_seconds: u32,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenewRequest {
    intent_id: WireUuid,
    fence: FenceView,
    lease_id: ExecutionLeaseId,
    expected_revision: u64,
    ttl_seconds: u32,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevokeRequest {
    intent_id: WireUuid,
    lease_id: ExecutionLeaseId,
    expected_revision: u64,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindRequest {
    intent_id: WireUuid,
    fence: FenceView,
    expected_session_revision: u64,
    /// A folder the native dialog returned, exactly as it was returned.
    workspace: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResumeRequest {
    intent_id: WireUuid,
    fence: FenceView,
    expected_session_revision: u64,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbandonRequest {
    intent_id: WireUuid,
    fence: FenceView,
    operation_id: WireUuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartTurnRequest {
    intent_id: WireUuid,
    fence: FenceView,
    expected_session_revision: u64,
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
    fence: FenceView,
    turn_id: ManagedTurnId,
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
pub struct ReadApprovalRequest {
    approval_id: ManagedApprovalId,
    /// Which file of a change set, and where in its diff. Null is the first of each.
    file: Option<u16>,
    offset: Option<u32>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecideRequest {
    intent_id: WireUuid,
    fence: FenceView,
    approval_id: ManagedApprovalId,
    /// The approval revision the window was shown.
    expected_revision: u64,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditRequest {
    /// Where older history is read: the stream a window was handed and the position before
    /// which to read. Null is the newest window.
    older: Option<OlderAudit>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OlderAudit {
    stream_id: WireUuid,
    before: Sequence,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditWaitRequest {
    stream_id: WireUuid,
    after: Option<Sequence>,
    wait_ms: u32,
}

/// Admission occurs before `spawn_blocking`, with counters of their own: a turn follower,
/// which may wait, never takes the place of an intent or of a read.
async fn blocking<T: Send + 'static>(
    (pool, limit): (&'static AtomicUsize, usize),
    service: F8Gui,
    operation: impl FnOnce(F8Gui) -> Result<T, F8Error> + Send + 'static,
) -> Result<T, F8Error> {
    if !service.is_active() {
        return Err(F8Error::new(F8Failure::Inactive));
    }
    pool.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
        (count < limit).then_some(count + 1)
    })
    .map_err(|_| F8Error::new(F8Failure::Busy))?;
    let permit = Permit(pool);
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        operation(service)
    })
    .await
    .map_err(|_| F8Error::new(F8Failure::Internal))?
}
const INTENT: (&AtomicUsize, usize) = (&INTENTS, MAX_INTENTS);
const READ: (&AtomicUsize, usize) = (&READS, MAX_READS);

/// The window offers nothing from this interface unless it is active.
#[tauri::command]
pub async fn f8_state(service: State<'_, F8Gui>) -> Result<StateView, F8Error> {
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.state())
        .await
        .map_err(|_| F8Error::new(F8Failure::Internal))
}
#[tauri::command]
pub async fn f8_reconnect(service: State<'_, F8Gui>) -> Result<StateView, F8Error> {
    let service = service.inner().clone();
    tauri::async_runtime::spawn_blocking(move || service.reconnect())
        .await
        .map_err(|_| F8Error::new(F8Failure::Internal))
}
#[tauri::command]
pub async fn f8_sessions(
    service: State<'_, F8Gui>,
    request: SessionsRequest,
) -> Result<SessionsView, F8Error> {
    blocking(READ, service.inner().clone(), move |s| {
        s.sessions(request.before)
    })
    .await
}
#[tauri::command]
pub async fn f8_session(
    service: State<'_, F8Gui>,
    request: SessionRequest,
) -> Result<SessionDetail, F8Error> {
    blocking(READ, service.inner().clone(), move |s| {
        s.session(request.session_id)
    })
    .await
}
#[tauri::command]
pub async fn f8_session_prepare(
    service: State<'_, F8Gui>,
    request: PrepareRequest,
) -> Result<SessionReceipt, F8Error> {
    blocking(INTENT, service.inner().clone(), move |s| {
        s.prepare(request.intent_id)
    })
    .await
}
#[tauri::command]
pub async fn f8_session_close(
    service: State<'_, F8Gui>,
    request: CloseRequest,
) -> Result<SessionReceipt, F8Error> {
    blocking(INTENT, service.inner().clone(), move |s| {
        s.close(request.intent_id, request.fence, request.expected_revision)
    })
    .await
}

/// The window names no path of its own: a workspace is admitted only for a folder the
/// native dialog returned. Anything else is refused here, before anything is asked.
fn chosen(selections: &WorkspaceSelections, workspace: &str) -> Result<(), F8Error> {
    if selections.chose(workspace) {
        Ok(())
    } else {
        Err(F8Error::new(F8Failure::Invalid))
    }
}

#[tauri::command]
pub async fn f8_runtime_start(
    service: State<'_, F8Gui>,
    selections: State<'_, WorkspaceSelections>,
    request: StartRuntimeRequest,
) -> Result<RuntimeReceipt, F8Error> {
    if let Some(workspace) = &request.workspace {
        chosen(&selections, workspace)?;
    }
    blocking(INTENT, service.inner().clone(), move |s| {
        s.runtime_start(
            request.intent_id,
            request.session_id,
            request.expected_revision,
            request.expected_generation,
            request.mode,
            request.workspace,
        )
    })
    .await
}
#[tauri::command]
pub async fn f8_runtime_stop(
    service: State<'_, F8Gui>,
    request: StopRuntimeRequest,
) -> Result<RuntimeReceipt, F8Error> {
    blocking(INTENT, service.inner().clone(), move |s| {
        s.runtime_stop(request.intent_id, request.fence, request.expected_revision)
    })
    .await
}
#[tauri::command]
pub async fn f8_models(
    service: State<'_, F8Gui>,
    request: ModelsRequest,
) -> Result<ModelsView, F8Error> {
    blocking(READ, service.inner().clone(), move |s| {
        s.models(request.fence)
    })
    .await
}
/// A lease that lets a runtime propose to run commands or change files is allowed by the
/// user in the desktop's own window first; one that only starts turns is not.
#[tauri::command]
pub async fn f8_lease_grant(
    service: State<'_, F8Gui>,
    confirm: State<'_, F8Confirm>,
    request: GrantRequest,
) -> Result<LeaseReceipt, F8Error> {
    let confirm = confirm.inner().clone();
    blocking(INTENT, service.inner().clone(), move |s| {
        if confirm::acts(&request.scopes) {
            let prompt = confirm::lease_prompt(
                false,
                &request.fence.session,
                confirm::workspace_of(&s, &request.fence.session).as_deref(),
                Some(&request.scopes),
                request.ttl_seconds,
            );
            confirm::allow(
                &confirm,
                &format!("{}|grant|{}", request.intent_id.as_str(), prompt.body),
                &prompt,
            )?;
        }
        s.lease_grant(
            request.intent_id,
            request.fence,
            request.expected_session_revision,
            request.scopes,
            request.ttl_seconds,
        )
    })
    .await
}
/// Extending a lease keeps what it allows alive, so it is allowed the same way.
#[tauri::command]
pub async fn f8_lease_renew(
    service: State<'_, F8Gui>,
    confirm: State<'_, F8Confirm>,
    request: RenewRequest,
) -> Result<LeaseReceipt, F8Error> {
    let confirm = confirm.inner().clone();
    blocking(INTENT, service.inner().clone(), move |s| {
        let (workspace, scopes) = confirm::lease_of(&s, &request.fence.session, &request.lease_id);
        let prompt = confirm::lease_prompt(
            true,
            &request.fence.session,
            workspace.as_deref(),
            scopes.as_deref(),
            request.ttl_seconds,
        );
        confirm::allow(
            &confirm,
            &format!(
                "{}|renew|{}|{}",
                request.intent_id.as_str(),
                request.lease_id.as_str(),
                prompt.body
            ),
            &prompt,
        )?;
        s.lease_renew(
            request.intent_id,
            request.fence,
            request.lease_id,
            request.expected_revision,
            request.ttl_seconds,
        )
    })
    .await
}
#[tauri::command]
pub async fn f8_lease_revoke(
    service: State<'_, F8Gui>,
    request: RevokeRequest,
) -> Result<LeaseReceipt, F8Error> {
    blocking(INTENT, service.inner().clone(), move |s| {
        s.lease_revoke(
            request.intent_id,
            request.lease_id,
            request.expected_revision,
        )
    })
    .await
}
#[tauri::command]
pub async fn f8_binding_create(
    service: State<'_, F8Gui>,
    selections: State<'_, WorkspaceSelections>,
    request: BindRequest,
) -> Result<BindingReceipt, F8Error> {
    chosen(&selections, &request.workspace)?;
    blocking(INTENT, service.inner().clone(), move |s| {
        s.binding_create(
            request.intent_id,
            request.fence,
            request.expected_session_revision,
            request.workspace,
        )
    })
    .await
}
#[tauri::command]
pub async fn f8_binding_resume(
    service: State<'_, F8Gui>,
    request: ResumeRequest,
) -> Result<BindingReceipt, F8Error> {
    blocking(INTENT, service.inner().clone(), move |s| {
        s.binding_resume(
            request.intent_id,
            request.fence,
            request.expected_session_revision,
        )
    })
    .await
}
#[tauri::command]
pub async fn f8_binding_abandon(
    service: State<'_, F8Gui>,
    request: AbandonRequest,
) -> Result<AbandonReceipt, F8Error> {
    blocking(INTENT, service.inner().clone(), move |s| {
        s.binding_abandon(request.intent_id, request.fence, request.operation_id)
    })
    .await
}
#[tauri::command]
pub async fn f8_turn_start(
    service: State<'_, F8Gui>,
    request: StartTurnRequest,
) -> Result<TurnReceipt, F8Error> {
    blocking(INTENT, service.inner().clone(), move |s| {
        s.turn_start(
            request.intent_id,
            request.fence,
            request.expected_session_revision,
            request.prompt,
        )
    })
    .await
}
#[tauri::command]
pub async fn f8_turn_cancel(
    service: State<'_, F8Gui>,
    request: CancelTurnRequest,
) -> Result<TurnReceipt, F8Error> {
    blocking(INTENT, service.inner().clone(), move |s| {
        s.turn_cancel(
            request.intent_id,
            request.fence,
            request.turn_id,
            request.expected_revision,
        )
    })
    .await
}
#[tauri::command]
pub async fn f8_turn_read(
    service: State<'_, F8Gui>,
    request: ReadTurnRequest,
) -> Result<TurnReading, F8Error> {
    blocking((&FOLLOW, MAX_FOLLOW), service.inner().clone(), move |s| {
        s.turn(request.turn_id, request.after, request.wait)
    })
    .await
}
#[tauri::command]
pub async fn f8_approval_read(
    service: State<'_, F8Gui>,
    request: ReadApprovalRequest,
) -> Result<ApprovalDetail, F8Error> {
    blocking(READ, service.inner().clone(), move |s| {
        s.approval(request.approval_id, request.file, request.offset)
    })
    .await
}
async fn decide_in(
    service: F8Gui,
    request: DecideRequest,
    decision: ApprovalDecision,
) -> Result<DecisionReceipt, F8Error> {
    blocking(INTENT, service, move |s| {
        s.approval_respond(
            request.intent_id,
            request.fence,
            request.approval_id,
            request.expected_revision,
            decision,
        )
    })
    .await
}
/// Asks the authority to accept this one approval, once the user has allowed, in the
/// desktop's own window, exactly what the authority holds for it. Which decision a command
/// means is fixed here: the window cannot send one as data.
#[tauri::command]
pub async fn f8_approval_accept(
    service: State<'_, F8Gui>,
    confirm: State<'_, F8Confirm>,
    request: DecideRequest,
) -> Result<DecisionReceipt, F8Error> {
    let confirm = confirm.inner().clone();
    blocking(INTENT, service.inner().clone(), move |s| {
        confirm::accept(
            &s,
            &confirm,
            request.intent_id,
            request.fence,
            request.approval_id,
            request.expected_revision,
        )
    })
    .await
}
/// A refusal of this one action. The turn goes on as the provider and the authority decide.
#[tauri::command]
pub async fn f8_approval_decline(
    service: State<'_, F8Gui>,
    request: DecideRequest,
) -> Result<DecisionReceipt, F8Error> {
    decide_in(service.inner().clone(), request, ApprovalDecision::Decline).await
}
/// A cancellation of the action and the provider's wait on it.
#[tauri::command]
pub async fn f8_approval_cancel(
    service: State<'_, F8Gui>,
    request: DecideRequest,
) -> Result<DecisionReceipt, F8Error> {
    decide_in(service.inner().clone(), request, ApprovalDecision::Cancel).await
}
#[tauri::command]
pub async fn f8_audit(
    service: State<'_, F8Gui>,
    request: AuditRequest,
) -> Result<AuditView, F8Error> {
    blocking(READ, service.inner().clone(), move |s| {
        match request.older {
            None => s.audit_latest(),
            Some(older) => s.audit_older(older.stream_id, older.before),
        }
    })
    .await
}
#[tauri::command]
pub async fn f8_audit_wait(
    service: State<'_, F8Gui>,
    request: AuditWaitRequest,
) -> Result<WatchView, F8Error> {
    blocking((&WATCH, MAX_WATCH), service.inner().clone(), move |s| {
        s.audit_wait(request.stream_id, request.after, request.wait_ms)
    })
    .await
}

#[cfg(test)]
mod tests;
