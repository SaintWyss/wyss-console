//! The window's only door to the authority: one read of the desktop's own projection.
//! No control operation, identity or raw protocol value crosses it in either direction.
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tauri::State;

#[cfg(target_os = "linux")]
pub use crate::authority::{AuthorityService, AuthorityView};

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
pub struct AuthorityStateRequest {
    /// The revision the caller already has. Null answers at once.
    after: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeErrorCode {
    Busy,
    Internal,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BridgeError {
    code: BridgeErrorCode,
}

/// The current view, or the next one once it differs from revision `after`. Waiting has
/// its own admission and never touches the terminal service or its blocking permits.
#[tauri::command]
pub async fn authority_state(
    service: State<'_, AuthorityService>,
    request: AuthorityStateRequest,
) -> Result<AuthorityView, BridgeError> {
    wait(service.inner().clone(), request.after, WAIT).await
}

async fn wait(
    service: AuthorityService,
    after: Option<u64>,
    timeout: Duration,
) -> Result<AuthorityView, BridgeError> {
    WAITING
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < MAX_WAITING).then_some(count + 1)
        })
        .map_err(|_| BridgeError {
            code: BridgeErrorCode::Busy,
        })?;
    let permit = Permit;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        service.wait_changed(after, timeout)
    })
    .await
    .map_err(|_| BridgeError {
        code: BridgeErrorCode::Internal,
    })
}

/// Where there is no authority to follow, the view says so and never changes.
#[cfg(not(target_os = "linux"))]
mod unsupported {
    use serde::Serialize;
    use std::time::Duration;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    pub struct AuthorityView {
        schema_version: u8,
        revision: u64,
        connection: &'static str,
        reason: Option<()>,
        authority: Option<()>,
        agent: Option<()>,
    }
    #[derive(Clone, Default)]
    pub struct AuthorityService;
    impl AuthorityService {
        pub fn from_environment() -> Self {
            Self
        }
        pub fn wait_changed(&self, _after: Option<u64>, _timeout: Duration) -> AuthorityView {
            AuthorityView {
                schema_version: 1,
                revision: 0,
                connection: "stopped",
                reason: None,
                authority: None,
                agent: None,
            }
        }
        pub fn shutdown(&self) {}
    }
}
#[cfg(not(target_os = "linux"))]
pub use unsupported::{AuthorityService, AuthorityView};

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::authority::{Connection, Fault};
    use std::time::Instant;

    fn block<T>(future: impl std::future::Future<Output = T>) -> T {
        tauri::async_runtime::block_on(future)
    }

    #[test]
    fn request_accepts_only_the_wait_cursor() {
        let parse = serde_json::from_value::<AuthorityStateRequest>;
        for (accepted, after) in [
            (serde_json::json!({"after": null}), None),
            (serde_json::json!({}), None),
            (serde_json::json!({"after": 7}), Some(7)),
        ] {
            assert_eq!(parse(accepted).unwrap().after, after);
        }
        for rejected in [
            serde_json::json!({"after": -1}),
            serde_json::json!({"after": "7"}),
            serde_json::json!({"after": null, "method": "authority/status"}),
            serde_json::json!({"method": "runtime/start", "params": {}}),
        ] {
            assert!(parse(rejected.clone()).is_err(), "{rejected}");
        }
    }

    #[test]
    fn waiting_is_bounded_and_admission_recovers() {
        // No runtime directory: the link settles in `waiting` and stays there.
        let service = AuthorityService::start(None);
        let view = loop {
            let view = block(wait(service.clone(), None, Duration::ZERO)).unwrap();
            if view.connection == Connection::Waiting {
                break view;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(view.reason, Some(Fault::UnsafePath));
        assert_eq!(
            serde_json::to_value(&view).unwrap()["reason"],
            "unsafe_path"
        );
        let held: Vec<_> = (0..MAX_WAITING).map(|_| Permit).collect();
        WAITING.fetch_add(MAX_WAITING, Ordering::AcqRel);
        let refused = block(wait(service.clone(), None, Duration::ZERO));
        assert_eq!(
            serde_json::to_value(refused.unwrap_err()).unwrap(),
            serde_json::json!({"code": "busy"})
        );
        drop(held);
        // A stopped service answers a waiting call at once instead of holding it.
        service.shutdown();
        let started = Instant::now();
        let last = block(wait(service, Some(u64::MAX), Duration::from_secs(30))).unwrap();
        assert_eq!(last.connection, Connection::Stopped);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(WAITING.load(Ordering::Acquire), 0);
    }
}
