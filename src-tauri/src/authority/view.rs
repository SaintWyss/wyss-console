use super::link::{Connection, Fault};
use crate::contracts::{
    Sequence,
    agent_adapter::AgentAdapterId,
    control::{ActualState, AuthorityStatus, DesiredState, RuntimeStatus},
};
use serde::Serialize;

pub(super) const VIEW_SCHEMA_VERSION: u8 = 1;

/// What the authority said about itself. Counts and versions only: no identity of an
/// instance, a stream or a session crosses to the window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthoritySummary {
    pub service_version: Option<String>,
    pub protocol_version: u8,
    /// Authority instances seen after the first one, over this application's lifetime.
    pub restarts: u64,
    /// The durable head this view was read at. Null before the first committed event.
    pub last_sequence: Option<Sequence>,
    pub prepared_sessions: u64,
    pub closed_sessions: u64,
}

/// The managed agent runtime as the authority reports it. Diagnostics, the isolated
/// workspace and provider method counters stay on this side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentSummary {
    pub adapter: AgentAdapterId,
    pub desired: DesiredState,
    pub actual: ActualState,
    pub runtime_version: Option<String>,
    pub model_count: u16,
}

/// The whole frontend projection. `authority` and `agent` are present only while the
/// link is `connected`: an unavailable authority leaves nothing stale to read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthorityView {
    pub schema_version: u8,
    /// Grows by one with every change of this view. It is the caller's wait cursor.
    pub revision: u64,
    pub connection: Connection,
    /// Why the last attempt ended, until an authority answers again.
    pub reason: Option<Fault>,
    pub authority: Option<AuthoritySummary>,
    pub agent: Option<AgentSummary>,
}
impl AuthorityView {
    pub(super) fn initial() -> Self {
        Self {
            schema_version: VIEW_SCHEMA_VERSION,
            revision: 0,
            connection: Connection::Connecting,
            reason: None,
            authority: None,
            agent: None,
        }
    }
}

/// One canonical reading: the head first, then what it describes.
pub(super) struct Snapshot {
    pub status: AuthorityStatus,
    pub runtime: RuntimeStatus,
}
impl Snapshot {
    pub(super) fn head(&self) -> u64 {
        self.status.last_sequence.map_or(0, Sequence::get)
    }
    /// A runtime between two resting states can settle without a durable event.
    pub(super) fn settling(&self) -> bool {
        matches!(
            self.runtime.actual_state,
            ActualState::Starting | ActualState::Initializing | ActualState::Stopping
        )
    }
    pub(super) fn authority(&self, restarts: u64) -> AuthoritySummary {
        AuthoritySummary {
            service_version: display_text(&self.status.service_version),
            protocol_version: self.status.protocol_version,
            restarts,
            last_sequence: self.status.last_sequence,
            prepared_sessions: self.status.prepared_count,
            closed_sessions: self.status.closed_count,
        }
    }
    pub(super) fn agent(&self) -> AgentSummary {
        AgentSummary {
            adapter: self.runtime.adapter_id.clone(),
            desired: self.runtime.desired_state,
            actual: self.runtime.actual_state,
            runtime_version: self
                .runtime
                .runtime_version
                .as_deref()
                .and_then(display_text),
            model_count: self.runtime.model_count,
        }
    }
}

/// Version text is shown, so it is short printable ASCII or it is not shown at all.
fn display_text(text: &str) -> Option<String> {
    (!text.is_empty()
        && text.len() <= 64
        && text.bytes().all(|b| b.is_ascii_graphic() || b == b' '))
    .then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::display_text;

    #[test]
    fn version_text_is_shown_only_when_short_and_printable() {
        assert_eq!(display_text("0.159.0").as_deref(), Some("0.159.0"));
        assert_eq!(
            display_text("codex 1.2 (abc)").as_deref(),
            Some("codex 1.2 (abc)")
        );
        for hidden in ["", "a\nb", "a\u{1b}[31m", "versión", &"x".repeat(65)] {
            assert_eq!(display_text(hidden), None, "{hidden:?}");
        }
    }
}
