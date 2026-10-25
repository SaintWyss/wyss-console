//! What a provider worker can tell the M3 views about its runtime: identity, attestation
//! outcome, usage and the model facts it observed. A plain, serializable projection with no
//! prompt, output, reasoning or credential in it, and no authority: nothing reads it to decide
//! anything.
use super::usage::{Provider, UsageSnapshot};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// The bridge was started; nothing was attested yet.
    Starting,
    /// Attested and usable for the profile.
    Ready,
    /// Refused or failed; the reason is in `refusal`.
    Refused,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Build {
    pub sdk_version: Option<String>,
    pub cli_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OfferedModel {
    pub model_id: String,
    pub alias: Option<String>,
    pub is_default: bool,
    pub efforts: Vec<String>,
}

/// What the loopback API tap observed of provider requests (the independent leg).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TapSummary {
    pub requests: u64,
    pub violations: u64,
    pub tools_seen: Vec<String>,
    pub models_seen: Vec<String>,
    pub efforts_seen: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionState {
    /// Put to the user and waiting.
    Held,
    Declined,
    Cancelled,
    /// Accepted and handed to the authority's executor.
    Running,
    Completed,
    /// The executor refused or could not start it: the effect did not happen.
    Failed,
    /// Nobody can say whether the effect happened. Never retried.
    Indeterminate,
    /// Never put to the user (out of scope, malformed, stale): the model was told why.
    Refused,
}

/// One tool call of the model and how it ended. The action's content is not here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionRecord {
    pub item: String,
    pub tool: String,
    pub state: ActionState,
    /// For `Refused`: the stable code of why.
    pub refusal: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProviderReport {
    pub provider: Provider,
    pub profile: String,
    pub phase: Phase,
    /// Stable code of the policy refusal, if any.
    pub refusal: Option<&'static str>,
    pub build: Build,
    pub offered: Vec<OfferedModel>,
    pub tap: TapSummary,
    pub usage: UsageSnapshot,
    /// Proposals the authority refused to put to the user (out of scope, malformed, stale).
    pub refused_proposals: u64,
    /// Actions currently held for a decision.
    pub pending_approvals: u64,
    /// Actions whose effect is not known.
    pub indeterminate_actions: u64,
    /// Tool calls the CLI itself denied (it was asked for something it does not offer).
    pub denied_attempts: u64,
    /// A few bounded words saying what failed the worker; never provider content.
    pub failure: Option<String>,
    /// The provider API's error class of the latest provider error (authentication_failed,
    /// rate_limit, server_error, ...), as the provider named it.
    pub provider_error: Option<String>,
    /// The four facts about the model (offered, configured, requested, reported in use).
    pub model: Option<super::model::ModelFacts>,
    /// The latest tool calls (bounded) and their states.
    pub actions: Vec<ActionRecord>,
}
