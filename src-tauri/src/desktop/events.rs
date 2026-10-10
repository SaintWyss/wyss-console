//! The window's door to the authority's durable history: two reads.
//!
//! Neither command names a control method, takes a payload to pass on or returns an
//! event as the authority stored it. One reads the newest window of events; the other
//! reads the window before a position the window was shown, in the stream it was shown
//! it for. What comes back is the projection of `crate::events`: a closed vocabulary and
//! Wyss's own identities. There is no command here that changes anything, and none that
//! performs again what an event describes.
use serde::Deserialize;
use std::sync::atomic::{AtomicUsize, Ordering};
use tauri::State;

use super::sessions::{SessionError, SessionFailure};
use crate::contracts::{Sequence, WireUuid};
#[cfg(target_os = "linux")]
pub use crate::events::{EventService, EventsPage};

/// Reads admitted at once: the one the view makes, and room for the one that replaces it.
const MAX_READS: usize = 2;
static READS: AtomicUsize = AtomicUsize::new(0);

struct Permit(&'static AtomicUsize);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventsRequest {}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OlderEventsRequest {
    /// The stream the window read `before` from.
    stream_id: WireUuid,
    /// The position the last window said to hand back. Opaque to the window.
    before: Sequence,
}

async fn blocking<T: Send + 'static>(
    service: EventService,
    operation: impl FnOnce(EventService) -> Result<T, SessionError> + Send + 'static,
) -> Result<T, SessionError> {
    READS
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < MAX_READS).then_some(count + 1)
        })
        .map_err(|_| SessionError::new(SessionFailure::Busy))?;
    let permit = Permit(&READS);
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        operation(service)
    })
    .await
    .map_err(|_| SessionError::new(SessionFailure::Internal))?
}

/// The newest window of durable events.
#[tauri::command]
pub async fn managed_events(
    service: State<'_, EventService>,
    request: EventsRequest,
) -> Result<EventsPage, SessionError> {
    let EventsRequest {} = request;
    blocking(service.inner().clone(), |service| service.latest()).await
}
/// The window of durable events before a position the window was shown.
#[tauri::command]
pub async fn managed_events_older(
    service: State<'_, EventService>,
    request: OlderEventsRequest,
) -> Result<EventsPage, SessionError> {
    blocking(service.inner().clone(), move |service| {
        service.older(request.stream_id, request.before)
    })
    .await
}

#[cfg(not(target_os = "linux"))]
mod unsupported {
    use super::{SessionError, SessionFailure};
    use crate::contracts::{Sequence, WireUuid};
    use serde::Serialize;

    #[derive(Debug, Clone, Serialize)]
    pub struct EventsPage {}
    #[derive(Clone, Default)]
    pub struct EventService;
    impl EventService {
        pub fn new(_: super::super::sessions::SessionService) -> Self {
            Self
        }
        pub fn latest(&self) -> Result<EventsPage, SessionError> {
            Err(SessionError::new(SessionFailure::Unavailable))
        }
        pub fn older(&self, _: WireUuid, _: Sequence) -> Result<EventsPage, SessionError> {
            Err(SessionError::new(SessionFailure::Unavailable))
        }
    }
}
#[cfg(not(target_os = "linux"))]
pub use unsupported::{EventService, EventsPage};

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_history_read_names_a_position_it_was_given_and_nothing_else() {
        assert!(serde_json::from_value::<EventsRequest>(json!({})).is_ok());
        let older = json!({
            "stream_id": "11111111-1111-4111-8111-111111111111",
            "before": "51",
        });
        assert!(serde_json::from_value::<OlderEventsRequest>(older.clone()).is_ok());
        // No method, no limit, no filter of the authority's, no payload to pass on.
        for extra in [
            "method",
            "params",
            "limit",
            "after_sequence",
            "wait_ms",
            "event_type",
        ] {
            let mut request = older.clone();
            request[extra] = json!("x");
            assert!(
                serde_json::from_value::<OlderEventsRequest>(request).is_err(),
                "{extra}"
            );
            assert!(
                serde_json::from_value::<EventsRequest>(json!({ extra: 1 })).is_err(),
                "{extra}"
            );
        }
        // A position is the stream's own text: not a number, not zero, not a guess at one.
        for before in [
            json!(51),
            json!("0"),
            json!("051"),
            json!("-1"),
            json!(""),
            json!(null),
        ] {
            let request = json!({
                "stream_id": "11111111-1111-4111-8111-111111111111",
                "before": before,
            });
            assert!(
                serde_json::from_value::<OlderEventsRequest>(request).is_err(),
                "{before}"
            );
        }
        assert!(
            serde_json::from_value::<OlderEventsRequest>(json!({"stream_id": "x", "before": "5"}))
                .is_err()
        );
    }

    #[test]
    fn history_reads_are_admitted_up_to_a_bound_and_refused_beyond_it() {
        // No runtime directory: nothing can be reached, so a read that is admitted
        // answers that at once.
        let service = EventService::new(crate::sessions::SessionService::new(None));
        let read = || {
            tauri::async_runtime::block_on(blocking(service.clone(), |s| s.latest()))
                .map(|_| ())
                .map_err(|error| (error.code, error.retry))
        };
        assert_eq!(read(), Err((SessionFailure::Unavailable, true)));
        // Every place taken: the next read is refused before anything is asked.
        let held: Vec<_> = (0..MAX_READS).map(|_| Permit(&READS)).collect();
        READS.fetch_add(MAX_READS, Ordering::AcqRel);
        assert_eq!(read(), Err((SessionFailure::Busy, true)));
        assert_eq!(READS.load(Ordering::Acquire), MAX_READS);
        drop(held);
        assert_eq!(READS.load(Ordering::Acquire), 0);
        assert_eq!(read(), Err((SessionFailure::Unavailable, true)));
        assert_eq!(READS.load(Ordering::Acquire), 0);
    }
}
