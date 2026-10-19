//! The window's door to managed sessions: one read and six product intents.
//!
//! Each command takes one narrow request and names no control method. The window cannot
//! choose an adapter, a scope, a lease, a lifetime or a runtime: it names a session it was
//! shown, the revision it saw and the identity of its own action. The one path it can name
//! is a folder the user selected in the native dialog. What it gets back is the desktop's
//! projection or a product failure code.
//!
//! A mode is one of the product's two, by name: `inference` or `local_tools`. Which
//! runtime profile that means is decided on this side, and the workspace a mode is
//! entered for is the one the authority holds for the session, never one the window
//! sends with the request.
use serde::Deserialize;
use std::sync::atomic::{AtomicUsize, Ordering};
use tauri::State;

use super::turns::TurnService;
use super::workspace::WorkspaceSelections;
use crate::contracts::{Sequence, WireUuid, identities::ManagedSessionId};
#[cfg(target_os = "linux")]
pub use crate::sessions::{
    ActivationProgress, AgentMode, SessionError, SessionFailure, SessionPage, SessionReceipt,
    SessionService,
};

/// Calls that may be admitted at once. They run one at a time; the rest wait their turn.
const MAX_CALLS: usize = 4;
static CALLS: AtomicUsize = AtomicUsize::new(0);
struct Permit;
impl Drop for Permit {
    fn drop(&mut self) {
        CALLS.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionsPageRequest {
    /// Sessions created before this position. Null is the newest page.
    before: Option<Sequence>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareSessionRequest {
    /// Names this user action. The same value sent again is the same action.
    intent_id: WireUuid,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionIntentRequest {
    intent_id: WireUuid,
    session_id: ManagedSessionId,
    /// The session revision the window was shown.
    expected_revision: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivateSessionRequest {
    /// Names the whole user action. The same value sent again continues the same action.
    operation_id: WireUuid,
    session_id: ManagedSessionId,
    expected_revision: u64,
    /// The folder the user selected in the native dialog, exactly as it was returned.
    workspace: String,
    /// The mode the session is created in. A session keeps it.
    mode: AgentMode,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionModeRequest {
    /// Names the whole user action. The same value sent again continues the same action.
    operation_id: WireUuid,
    session_id: ManagedSessionId,
    expected_revision: u64,
    /// The mode to run the session in. No workspace, profile or path comes with it.
    mode: AgentMode,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbandonSessionRequest {
    intent_id: WireUuid,
    session_id: ManagedSessionId,
}

/// Admission occurs before `spawn_blocking`, with its own counter: session calls never
/// take a terminal permit or one of the authority view's waiting slots.
async fn blocking<S: Send + 'static, T: Send + 'static>(
    service: S,
    operation: impl FnOnce(S) -> Result<T, SessionError> + Send + 'static,
) -> Result<T, SessionError> {
    CALLS
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < MAX_CALLS).then_some(count + 1)
        })
        .map_err(|_| SessionError::new(SessionFailure::Busy))?;
    let permit = Permit;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        operation(service)
    })
    .await
    .map_err(|_| SessionError::new(SessionFailure::Internal))?
}

#[tauri::command]
pub async fn managed_sessions_page(
    service: State<'_, SessionService>,
    request: SessionsPageRequest,
) -> Result<SessionPage, SessionError> {
    blocking(service.inner().clone(), move |service| {
        service.page(request.before)
    })
    .await
}
#[tauri::command]
pub async fn managed_session_prepare(
    service: State<'_, SessionService>,
    request: PrepareSessionRequest,
) -> Result<SessionReceipt, SessionError> {
    blocking(service.inner().clone(), move |service| {
        service.prepare(request.intent_id)
    })
    .await
}
#[tauri::command]
pub async fn managed_session_resume(
    service: State<'_, SessionService>,
    request: SessionIntentRequest,
) -> Result<SessionReceipt, SessionError> {
    blocking(service.inner().clone(), move |service| {
        service.resume(
            request.intent_id,
            request.session_id,
            request.expected_revision,
        )
    })
    .await
}
#[tauri::command]
pub async fn managed_session_close(
    service: State<'_, SessionService>,
    request: SessionIntentRequest,
) -> Result<SessionReceipt, SessionError> {
    blocking(service.inner().clone(), move |service| {
        service.close(
            request.intent_id,
            request.session_id,
            request.expected_revision,
        )
    })
    .await
}

/// The window names no path of its own: an activation is admitted only for a folder the
/// native dialog returned. Anything else is refused here, before anything is asked.
fn selected(
    selections: &WorkspaceSelections,
    request: &ActivateSessionRequest,
) -> Result<(), SessionError> {
    if selections.chose(&request.workspace) {
        Ok(())
    } else {
        Err(SessionError::new(SessionFailure::Invalid))
    }
}

/// One step of an activation. The window sends the same request until the answer is
/// `ready`; which authority operation a step takes is decided here, from what the
/// authority reports, never by the window.
#[tauri::command]
pub async fn managed_session_activate(
    service: State<'_, TurnService>,
    selections: State<'_, WorkspaceSelections>,
    request: ActivateSessionRequest,
) -> Result<ActivationProgress, SessionError> {
    selected(&selections, &request)?;
    blocking(service.inner().clone(), move |service| {
        service.activate(
            request.operation_id,
            request.session_id,
            request.expected_revision,
            request.workspace,
            request.mode,
        )
    })
    .await
}
/// One step of entering a mode with a session that has a workspace: the one managed
/// runtime in that mode for that workspace, and the session attached to it. The window
/// sends the same request until the answer is `ready`.
#[tauri::command]
pub async fn managed_session_mode(
    service: State<'_, TurnService>,
    request: SessionModeRequest,
) -> Result<ActivationProgress, SessionError> {
    blocking(service.inner().clone(), move |service| {
        service.enter(
            request.operation_id,
            request.session_id,
            request.expected_revision,
            request.mode,
        )
    })
    .await
}
#[tauri::command]
pub async fn managed_session_abandon(
    service: State<'_, SessionService>,
    request: AbandonSessionRequest,
) -> Result<SessionReceipt, SessionError> {
    blocking(service.inner().clone(), move |service| {
        service.abandon(request.intent_id, request.session_id)
    })
    .await
}

/// Where there is no authority to ask, every call says so.
#[cfg(not(target_os = "linux"))]
mod unsupported {
    use crate::contracts::{Sequence, WireUuid, identities::ManagedSessionId};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum AgentMode {
        Inference,
        LocalTools,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
    #[serde(rename_all = "snake_case")]
    pub enum SessionFailure {
        Unavailable,
        Invalid,
        WorkspaceInvalid,
        Busy,
        Internal,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
    pub struct SessionError {
        pub code: SessionFailure,
        pub retry: bool,
    }
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    pub struct SessionPage {}
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    pub struct SessionReceipt {}
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    pub struct ActivationProgress {}
    #[derive(Clone, Default)]
    pub struct SessionService;
    impl SessionError {
        pub const fn new(code: SessionFailure) -> Self {
            Self { code, retry: false }
        }
    }
    type Refused<T> = Result<T, SessionError>;
    impl SessionService {
        pub fn from_environment() -> Self {
            Self
        }
        fn absent<T>() -> Refused<T> {
            Err(SessionError::new(SessionFailure::Unavailable))
        }
        pub fn page(&self, _: Option<Sequence>) -> Refused<SessionPage> {
            Self::absent()
        }
        pub fn prepare(&self, _: WireUuid) -> Refused<SessionReceipt> {
            Self::absent()
        }
        pub fn resume(&self, _: WireUuid, _: ManagedSessionId, _: u64) -> Refused<SessionReceipt> {
            Self::absent()
        }
        pub fn close(&self, _: WireUuid, _: ManagedSessionId, _: u64) -> Refused<SessionReceipt> {
            Self::absent()
        }
        pub fn abandon(&self, _: WireUuid, _: ManagedSessionId) -> Refused<SessionReceipt> {
            Self::absent()
        }
        pub fn shutdown(&self) {}
    }
}
#[cfg(not(target_os = "linux"))]
pub use unsupported::{
    ActivationProgress, AgentMode, SessionError, SessionFailure, SessionPage, SessionReceipt,
    SessionService,
};

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const INTENT: &str = "11111111-1111-4111-8111-111111111111";
    const SESSION: &str = "22222222-2222-4222-8222-222222222222";

    fn block<T>(future: impl std::future::Future<Output = T>) -> T {
        tauri::async_runtime::block_on(future)
    }
    fn code(error: SessionError) -> Value {
        serde_json::to_value(error).unwrap()
    }

    #[test]
    fn requests_accept_exactly_their_narrow_fields() {
        let page = serde_json::from_value::<SessionsPageRequest>;
        assert!(page(json!({})).unwrap().before.is_none());
        assert!(page(json!({"before": null})).unwrap().before.is_none());
        assert_eq!(
            page(json!({"before": "7"})).unwrap().before.unwrap().get(),
            7
        );
        for rejected in [
            json!({"before": 7}),
            json!({"before": "0"}),
            json!({"before": "07"}),
            json!({"before": "-1"}),
            json!({"before": "1", "limit": 5000}),
            json!({"after": "1"}),
            json!({"method": "managedSession/list", "params": {"limit": 50}}),
        ] {
            assert!(page(rejected.clone()).is_err(), "{rejected}");
        }

        let prepare = serde_json::from_value::<PrepareSessionRequest>;
        assert_eq!(
            prepare(json!({"intent_id": INTENT}))
                .unwrap()
                .intent_id
                .as_str(),
            INTENT
        );
        for rejected in [
            json!({}),
            json!({"intent_id": null}),
            json!({"intent_id": "not-a-uuid"}),
            json!({"intent_id": INTENT.replace('-', "")}),
            // The window cannot choose what is prepared, or for whom.
            json!({"intent_id": INTENT, "adapter_id": "claude"}),
            json!({"intent_id": INTENT, "runtime_kind": "codex"}),
            json!({"intent_id": INTENT, "workspace_root": "/"}),
            json!({"method": "managedSession/prepare", "params": {"intent_id": INTENT}}),
        ] {
            assert!(prepare(rejected.clone()).is_err(), "{rejected}");
        }

        let intent = serde_json::from_value::<SessionIntentRequest>;
        let accepted = intent(json!({
            "intent_id": INTENT, "session_id": SESSION, "expected_revision": 1
        }))
        .unwrap();
        assert_eq!(accepted.session_id.as_str(), SESSION);
        assert_eq!(accepted.expected_revision, 1);
        let full = json!({"intent_id": INTENT, "session_id": SESSION, "expected_revision": 1});
        for (field, value) in [
            ("intent_id", json!("x")),
            ("session_id", json!("x")),
            ("session_id", json!(7)),
            ("expected_revision", json!(-1)),
            ("expected_revision", json!("1")),
            ("expected_revision", json!(1.5)),
        ] {
            let mut tampered = full.clone();
            tampered[field] = value;
            assert!(intent(tampered.clone()).is_err(), "{tampered}");
        }
        for extra in [
            // Nothing that belongs to the authority can ride along.
            "scopes",
            "ttl_seconds",
            "lease_id",
            "workspace_root",
            "expected_session_revision",
            "managed_session_id",
            "method",
        ] {
            let mut tampered = full.clone();
            tampered[extra] = json!("x");
            assert!(intent(tampered.clone()).is_err(), "{tampered}");
        }
        for missing in ["intent_id", "session_id", "expected_revision"] {
            let mut tampered = full.clone();
            tampered.as_object_mut().unwrap().remove(missing);
            assert!(intent(tampered.clone()).is_err(), "{tampered}");
        }
    }

    #[test]
    fn an_activation_names_one_action_one_session_and_a_folder_the_user_selected() {
        let activate = serde_json::from_value::<ActivateSessionRequest>;
        let full = json!({
            "operation_id": INTENT, "session_id": SESSION, "expected_revision": 1,
            "workspace": "/home/user/project", "mode": "inference",
        });
        let accepted = activate(full.clone()).unwrap();
        assert_eq!(accepted.mode, AgentMode::Inference);
        assert_eq!(accepted.operation_id.as_str(), INTENT);
        assert_eq!(accepted.session_id.as_str(), SESSION);
        assert_eq!(
            (accepted.expected_revision, accepted.workspace.as_str()),
            (1, "/home/user/project")
        );
        for (field, value) in [
            ("operation_id", json!("x")),
            ("session_id", json!(7)),
            ("expected_revision", json!("1")),
            ("workspace", json!(null)),
            ("workspace", json!(["/home/user/project"])),
            ("workspace", json!({"path": "/home/user/project"})),
        ] {
            let mut tampered = full.clone();
            tampered[field] = value;
            assert!(activate(tampered.clone()).is_err(), "{tampered}");
        }
        for extra in [
            // Nothing the authority or the bridge decides can ride along.
            "intent_id",
            "scopes",
            "ttl_seconds",
            "lease_id",
            "adapter_id",
            "runtime_kind",
            "safety_profile",
            "workspace_root",
            "runtime_instance_id",
            "thread_id",
            "start_runtime",
            "method",
        ] {
            let mut tampered = full.clone();
            tampered[extra] = json!("x");
            assert!(activate(tampered.clone()).is_err(), "{tampered}");
        }
        for missing in [
            "operation_id",
            "session_id",
            "expected_revision",
            "workspace",
        ] {
            let mut tampered = full.clone();
            tampered.as_object_mut().unwrap().remove(missing);
            assert!(activate(tampered.clone()).is_err(), "{tampered}");
        }

        // The path is admitted only if the native dialog returned exactly that text.
        let selections = WorkspaceSelections::default();
        let refused = Err(SessionError::new(SessionFailure::Invalid));
        assert_eq!(selected(&selections, &accepted), refused);
        selections.remember_for_test("/home/user/project");
        assert_eq!(selected(&selections, &accepted), Ok(()));
        for other in [
            "/home/user",
            "/home/user/project/",
            "/home/user/project/sub",
            "/home/user/project/../project",
            "/",
            "/etc",
            "",
        ] {
            let mut tampered = full.clone();
            tampered["workspace"] = json!(other);
            assert_eq!(
                selected(&selections, &activate(tampered).unwrap()),
                refused,
                "{other}"
            );
        }

        let abandon = serde_json::from_value::<AbandonSessionRequest>;
        let full = json!({"intent_id": INTENT, "session_id": SESSION});
        let accepted = abandon(full.clone()).unwrap();
        assert_eq!(
            (accepted.intent_id.as_str(), accepted.session_id.as_str()),
            (INTENT, SESSION)
        );
        // The attempt given up on is the one the authority names, never the window's.
        for extra in ["operation_id", "expected_revision", "workspace", "method"] {
            let mut tampered = full.clone();
            tampered[extra] = json!("x");
            assert!(abandon(tampered.clone()).is_err(), "{tampered}");
        }
        for missing in ["intent_id", "session_id"] {
            let mut tampered = full.clone();
            tampered.as_object_mut().unwrap().remove(missing);
            assert!(abandon(tampered.clone()).is_err(), "{tampered}");
        }
    }

    #[test]
    fn the_window_is_granted_exactly_the_known_commands() {
        // The capability and the build manifest are the whole surface. Neither has a
        // command that takes a method name.
        let capability: Value =
            serde_json::from_str(include_str!("../../capabilities/main.json")).unwrap();
        let granted: Vec<&str> = capability["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_str().unwrap())
            .collect();
        let expected = [
            "allow-get-app-info",
            "allow-authority-state",
            // The lifecycle: one read and Retry. Nothing that names a service or a file.
            "allow-authority-lifecycle",
            "allow-authority-retry",
            "allow-managed-sessions-page",
            "allow-managed-session-prepare",
            "allow-managed-session-resume",
            "allow-managed-session-close",
            "allow-managed-session-activate",
            "allow-managed-session-abandon",
            "allow-managed-session-mode",
            "allow-choose-workspace",
            "allow-managed-turns",
            "allow-managed-turns-older",
            "allow-managed-turn-read",
            "allow-managed-turn-start",
            "allow-managed-turn-cancel",
            "allow-managed-approvals",
            "allow-managed-approval-read",
            "allow-managed-approval-approve",
            "allow-managed-approval-deny",
            // History: two reads, and nothing that passes a method or a payload on.
            "allow-managed-events",
            "allow-managed-events-older",
            // The desktop's own preferences: one read, one replacement of the whole typed
            // set, one reset. No key, no path and nothing that is run.
            "allow-settings-read",
            "allow-settings-update",
            "allow-settings-reset",
            "allow-create-terminal",
            "allow-terminal-write",
            "allow-terminal-read-output",
            "allow-terminal-resize",
            "allow-close-terminal",
            "allow-terminal-status",
            // The agents' configuration: two reads of what the backend already holds. No
            // path goes in, no file is read by the call, nothing is written (F1).
            "allow-configuration-inventory",
            "allow-configuration-changes",
            "allow-configuration-plan",
            "allow-configuration-apply",
            "allow-configuration-history",
            "allow-configuration-undo",
            "allow-configuration-records",
            "allow-configuration-record-delete",
            "allow-configuration-export",
            "allow-configuration-import",
            // Authoring (F5): the page names a kind and a place; the text that will run is
            // typed in a window of the desktop's own.
            "allow-configuration-author-hook",
            "allow-configuration-edit-hook",
            "allow-configuration-author-mcp",
            "allow-configuration-edit-mcp",
            // Native agent sessions and session profiles (F3, ADR 0035): an agent of the
            // closed catalogue, a project the native dialog returned, a session or a profile
            // of the desktop's own registry. None takes a command, an argument list, an
            // environment or a path.
            "allow-native-launcher",
            "allow-native-preview",
            "allow-native-start",
            "allow-native-sessions",
            "allow-native-stop",
            "allow-native-restart",
            "allow-native-forget",
            "allow-native-choose-project",
            "allow-native-project-default",
            "allow-native-profiles",
            "allow-native-profile-create",
            "allow-native-profile-save",
            "allow-native-profile-delete",
            "allow-native-profile-duplicate",
            "allow-native-profile-restore",
            "allow-native-profile-export",
            "allow-native-profile-import",
            // The host that holds the terminals past the window (F9).
            "allow-host-terminals",
            "allow-host-info",
            "allow-terminal-redraw",
            // The layout of tabs and panes (T2).
            "allow-layout-read",
            "allow-layout-write",
            // API credentials (F14): a key goes in through a native window and never comes out.
            "allow-credentials-list",
            "allow-credential-add",
            "allow-credential-replace",
            "allow-credential-rename",
            "allow-credential-delete",
            "allow-credential-verify",
            // A handoff brief (F11a): prepared for review, and saved to a chosen file.
            "allow-native-handoff-prepare",
            "allow-native-handoff-save",
        ];
        assert_eq!(granted, expected);
        let manifest = include_str!("../../build.rs");
        let (_, list) = manifest.split_once("commands(&[").unwrap();
        let (list, _) = list.split_once(']').unwrap();
        let commands: Vec<String> = list
            .split(',')
            .map(|c| c.trim().trim_matches('"').to_owned())
            .filter(|c| !c.is_empty())
            .collect();
        let permitted: Vec<String> = expected
            .iter()
            .map(|p| p.trim_start_matches("allow-").replace('-', "_"))
            .collect();
        assert_eq!(commands, permitted);
    }

    #[test]
    fn a_mode_is_one_of_the_product_s_two_and_comes_with_no_profile_path_or_scope() {
        let mode = serde_json::from_value::<SessionModeRequest>;
        let full = json!({
            "operation_id": INTENT, "session_id": SESSION, "expected_revision": 1,
            "mode": "local_tools",
        });
        let accepted = mode(full.clone()).unwrap();
        assert_eq!(accepted.operation_id.as_str(), INTENT);
        assert_eq!(accepted.session_id.as_str(), SESSION);
        assert_eq!(
            (accepted.expected_revision, accepted.mode),
            (1, AgentMode::LocalTools)
        );
        let mut inference = full.clone();
        inference["mode"] = json!("inference");
        assert_eq!(mode(inference).unwrap().mode, AgentMode::Inference);
        // Not a profile of the authority, not a mode nobody defined, not a structure.
        for other in [
            json!("codex_local_tools_v1"),
            json!("codex_inference_v1"),
            json!("full_access"),
            json!("danger-full-access"),
            json!("LocalTools"),
            json!(""),
            json!(null),
            json!(1),
            json!(["local_tools"]),
            json!({"profile": "codex_local_tools_v1"}),
        ] {
            let mut tampered = full.clone();
            tampered["mode"] = other;
            assert!(mode(tampered.clone()).is_err(), "{tampered}");
            // The same holds where a session is created.
            let mut activation = json!({
                "operation_id": INTENT, "session_id": SESSION, "expected_revision": 1,
                "workspace": "/home/user/project",
            });
            activation["mode"] = tampered["mode"].clone();
            assert!(
                serde_json::from_value::<ActivateSessionRequest>(activation.clone()).is_err(),
                "{activation}"
            );
        }
        // Entering a mode names no folder: the workspace is the one the authority holds
        // for the session. Nor a profile, a scope, a lease or anything of the runtime.
        for extra in [
            "workspace",
            "workspace_root",
            "cwd",
            "path",
            "safety_profile",
            "profile",
            "sandbox",
            "approval_policy",
            "scopes",
            "ttl_seconds",
            "lease_id",
            "runtime_instance_id",
            "force",
            "method",
        ] {
            let mut tampered = full.clone();
            tampered[extra] = json!("/home/user/elsewhere");
            assert!(mode(tampered.clone()).is_err(), "{tampered}");
        }
        for missing in ["operation_id", "session_id", "expected_revision", "mode"] {
            let mut tampered = full.clone();
            tampered.as_object_mut().unwrap().remove(missing);
            assert!(mode(tampered.clone()).is_err(), "{tampered}");
        }
    }

    #[test]
    fn a_call_without_an_authority_fails_safely_and_admission_recovers() {
        // No runtime directory: nothing can be reached and nothing is sent.
        let service = SessionService::new(None);
        let intent = || WireUuid::try_from(INTENT.to_owned()).unwrap();
        let session = || ManagedSessionId::parse(SESSION).unwrap();
        let unavailable = json!({"code": "unavailable", "retry": true});
        assert_eq!(
            code(block(blocking(service.clone(), |s| s.page(None))).unwrap_err()),
            unavailable
        );
        assert_eq!(
            code(block(blocking(service.clone(), move |s| s.prepare(intent()))).unwrap_err()),
            unavailable
        );
        // A revision no session can have is refused here, before anything is asked.
        for revision in [0, 3, u64::MAX] {
            for call in [SessionService::close, SessionService::resume] {
                let (intent, session) = (intent(), session());
                let refused = block(blocking(service.clone(), move |s| {
                    call(&s, intent, session, revision)
                }));
                assert_eq!(
                    code(refused.unwrap_err()),
                    json!({"code": "invalid", "retry": false})
                );
            }
        }
        assert_eq!(service.stats().connections, 0);

        let held: Vec<_> = (0..MAX_CALLS).map(|_| Permit).collect();
        CALLS.fetch_add(MAX_CALLS, Ordering::AcqRel);
        let refused = block(blocking(service.clone(), |s| s.page(None)));
        assert_eq!(
            code(refused.unwrap_err()),
            json!({"code": "busy", "retry": true})
        );
        drop(held);
        assert_eq!(CALLS.load(Ordering::Acquire), 0);
        // A stopped service refuses at once.
        service.shutdown();
        assert_eq!(
            code(block(blocking(service, |s| s.page(None))).unwrap_err()),
            unavailable
        );
    }
}
