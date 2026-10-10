//! The window's door to the authority's lifecycle: one read of where it stands, and one
//! intent, Retry.
//!
//! Neither names a service, a command, a path or a process, and neither takes anything
//! but a wait cursor. What to install, where, and what to ask of the service manager is
//! decided in `crate::lifecycle`, from what this build was packaged with.
use super::authority::{BridgeError, BridgeErrorCode};
use serde::Deserialize;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tauri::State;

#[cfg(target_os = "linux")]
pub use crate::lifecycle::{AuthorityBootstrap, LifecycleView};

/// Longest one call waits for the view to change before answering it unchanged.
const WAIT: Duration = Duration::from_secs(20);
/// Calls that may wait at once: the window keeps one, a reload briefly leaves another.
const MAX_WAITING: usize = 4;
static WAITING: AtomicUsize = AtomicUsize::new(0);
struct Permit;
impl Drop for Permit {
    fn drop(&mut self) {
        WAITING.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleRequest {
    /// The revision the caller already has. Null answers at once.
    after: Option<u64>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryRequest {}

/// The desktop's bootstrap for this process: the authority packaged with this build, the
/// user's XDG directories and the user's service manager.
#[cfg(target_os = "linux")]
pub fn start(
    authority: &super::authority::AuthorityService,
    authority_digest: Option<&'static str>,
) -> AuthorityBootstrap {
    use crate::lifecycle::{Layout, Provision, Systemd};
    use std::{path::PathBuf, sync::Arc};
    let layout = Layout::from_environment();
    // With no runtime directory there is no manager to reach, and every call says so.
    let runtime = layout.as_ref().map_or_else(
        |_| PathBuf::from("/nonexistent"),
        |l| l.runtime().to_owned(),
    );
    AuthorityBootstrap::start(
        Arc::new(authority.clone()),
        Provision::of_build(authority_digest),
        layout,
        Arc::new(Systemd::new(&runtime)),
    )
}

/// Where the lifecycle stands, or the next view once it differs from revision `after`.
#[tauri::command]
pub async fn authority_lifecycle(
    service: State<'_, AuthorityBootstrap>,
    request: LifecycleRequest,
) -> Result<LifecycleView, BridgeError> {
    wait(service.inner().clone(), request.after, WAIT).await
}
async fn wait(
    service: AuthorityBootstrap,
    after: Option<u64>,
    timeout: Duration,
) -> Result<LifecycleView, BridgeError> {
    WAITING
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < MAX_WAITING).then_some(count + 1)
        })
        .map_err(|_| BridgeError::new(BridgeErrorCode::Busy))?;
    let permit = Permit;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        service.wait_changed(after, timeout)
    })
    .await
    .map_err(|_| BridgeError::new(BridgeErrorCode::Internal))
}

/// The user's Retry. It answers at once with the view; the attempt, if one was started,
/// is followed through `authority_lifecycle`.
#[tauri::command]
pub async fn authority_retry(
    service: State<'_, AuthorityBootstrap>,
    request: RetryRequest,
) -> Result<LifecycleView, BridgeError> {
    let RetryRequest {} = request;
    Ok(service.inner().retry())
}

/// Where no authority can be managed, the lifecycle says so and never changes.
#[cfg(not(target_os = "linux"))]
mod unsupported {
    use serde::Serialize;
    use std::time::Duration;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    pub struct Diagnostics {
        build: &'static str,
        installation: &'static str,
        service: &'static str,
    }
    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    pub struct LifecycleView {
        schema_version: u8,
        revision: u64,
        phase: &'static str,
        failure: Option<&'static str>,
        retry: bool,
        origin: Option<()>,
        update_pending: bool,
        diagnostics: Diagnostics,
    }
    #[derive(Clone, Default)]
    pub struct AuthorityBootstrap;
    impl AuthorityBootstrap {
        pub fn view(&self) -> LifecycleView {
            LifecycleView {
                schema_version: 1,
                revision: 0,
                phase: "failed",
                failure: Some("manager_unavailable"),
                retry: false,
                origin: None,
                update_pending: false,
                diagnostics: Diagnostics {
                    build: "development",
                    installation: "unknown",
                    service: "unavailable",
                },
            }
        }
        pub fn wait_changed(&self, _after: Option<u64>, _timeout: Duration) -> LifecycleView {
            self.view()
        }
        pub fn retry(&self) -> LifecycleView {
            self.view()
        }
        pub fn shutdown(&self) {}
    }
    pub fn start(
        _authority: &super::super::authority::AuthorityService,
        _authority_digest: Option<&'static str>,
    ) -> AuthorityBootstrap {
        AuthorityBootstrap
    }
}
#[cfg(not(target_os = "linux"))]
pub use unsupported::{AuthorityBootstrap, LifecycleView, start};

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_lifecycle_requests_name_nothing_but_a_wait_cursor() {
        assert!(serde_json::from_value::<LifecycleRequest>(json!({"after": null})).is_ok());
        assert!(serde_json::from_value::<LifecycleRequest>(json!({"after": 7})).is_ok());
        assert!(serde_json::from_value::<RetryRequest>(json!({})).is_ok());
        // No service, command, argument, path, unit or binary can be named from the window.
        for extra in [
            "service",
            "unit",
            "name",
            "command",
            "args",
            "path",
            "binary",
            "executable",
            "force",
            "restart",
            "stop",
            "environment",
        ] {
            assert!(
                serde_json::from_value::<RetryRequest>(json!({ extra: "x" })).is_err(),
                "{extra}"
            );
            assert!(
                serde_json::from_value::<LifecycleRequest>(json!({"after": null, extra: "x"}))
                    .is_err(),
                "{extra}"
            );
        }
        for after in [json!("1"), json!(-1), json!(1.5), json!({})] {
            assert!(serde_json::from_value::<LifecycleRequest>(json!({ "after": after })).is_err());
        }
    }
}
