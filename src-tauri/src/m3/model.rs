//! Managed model and reasoning-effort selection: the engineering of Proposed ADR0054, behind a
//! gate that stays closed until that ADR and the authority extension are accepted.
//!
//! It is an independently versioned capability (`CAPABILITY`), not a field of the accepted P2
//! schema. What it models:
//!
//! - **four facts about a model, never merged**: *offered* (the provider's own catalogue, not an
//!   entitlement), *configured* (pinned when the runtime started), *requested* (asked for one
//!   turn) and *reported in use* (what the provider named as serving it);
//! - **validation by provider capability**, not by string shape: the same request is valid for one
//!   provider and refused for another (Claude's SDK changes the model per turn but fixes the
//!   effort when its CLI starts);
//! - **binding to a runtime generation and a catalogue revision**, checked when the request is
//!   made and again immediately before dispatch: a restart, another build or a refreshed
//!   catalogue invalidates it, and a stale catalogue refuses;
//! - **no substitution**: a request that cannot be honored is refused, never replaced by another
//!   model, and a provider that answers with a different model is *shown* as rerouted;
//! - **effort is never "effective" because it was accepted**: the evidence is kept as what it is
//!   (requested, observed in the provider request, or reported by the provider).
use super::{gate, usage::Provider};
use crate::contracts::control::ModelOffer;
use serde::Serialize;
use std::collections::BTreeSet;

pub const CAPABILITY: &str = "m3.model_selection";
pub const CAPABILITY_VERSION: u32 = 1;
/// A catalogue older than this is not used to admit a request.
pub const CATALOGUE_MAX_AGE_SECS: u64 = 15 * 60;
pub const MAX_ENTRIES: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    GateClosed,
    InvalidWord,
    EmptyRequest,
    ProviderMismatch,
    BuildMismatch,
    SessionMismatch,
    RuntimeMismatch,
    GenerationMismatch,
    CatalogueIncomplete,
    CatalogueStale,
    CatalogueChanged,
    UnknownModel,
    UnsupportedEffort,
    EffortNotPerTurn,
    ModelNotPerTurn,
}
impl Refusal {
    pub const fn code(self) -> &'static str {
        match self {
            Self::GateClosed => "gate_closed",
            Self::InvalidWord => "invalid_word",
            Self::EmptyRequest => "empty_request",
            Self::ProviderMismatch => "provider_mismatch",
            Self::BuildMismatch => "build_mismatch",
            Self::SessionMismatch => "session_mismatch",
            Self::RuntimeMismatch => "runtime_mismatch",
            Self::GenerationMismatch => "generation_mismatch",
            Self::CatalogueIncomplete => "catalogue_incomplete",
            Self::CatalogueStale => "catalogue_stale",
            Self::CatalogueChanged => "catalogue_changed",
            Self::UnknownModel => "unknown_model",
            Self::UnsupportedEffort => "unsupported_effort",
            Self::EffortNotPerTurn => "effort_not_per_turn",
            Self::ModelNotPerTurn => "model_not_per_turn",
        }
    }
}

/// What one provider's integration can change and when.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Caps {
    pub model_per_turn: bool,
    pub effort_per_turn: bool,
    pub effort_per_runtime: bool,
}
impl Caps {
    /// The pinned Claude Agent SDK (0.2.173): `set_model` is a control request that applies to the
    /// next turn; `--effort` is read once when the CLI starts.
    pub const CLAUDE: Self = Self {
        model_per_turn: true,
        effort_per_turn: false,
        effort_per_runtime: true,
    };
    /// Codex's turn start carries both, but the accepted P2 protocol does not expose them: this
    /// is the capability ADR0054 would add, and nothing here claims Codex honors it today.
    pub const CODEX: Self = Self {
        model_per_turn: true,
        effort_per_turn: true,
        effort_per_runtime: false,
    };
    pub const fn of(p: Provider) -> Self {
        match p {
            Provider::Claude => Self::CLAUDE,
            Provider::Codex => Self::CODEX,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelRequest {
    pub model: String,
    pub effort: Option<String>,
}
impl ModelRequest {
    /// A model and an optional effort, each one provider identifier (printable, no space).
    pub fn new(model: &str, effort: Option<&str>) -> Result<Self, Refusal> {
        if model.is_empty() {
            return Err(Refusal::EmptyRequest);
        }
        if !ModelOffer::word_ok(model, 128) || effort.is_some_and(|e| !ModelOffer::word_ok(e, 32)) {
            return Err(Refusal::InvalidWord);
        }
        Ok(Self {
            model: model.to_owned(),
            effort: effort.map(str::to_owned),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub model_id: String,
    pub alias: Option<String>,
    pub efforts: Vec<String>,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The Claude SDK's own `get_server_info().models`.
    ClaudeServerInfo,
    /// Codex's `model/list`.
    CodexModelList,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Catalogue {
    pub provider: Provider,
    pub build: String,
    /// Raised each time the provider's listing is taken again.
    pub revision: u64,
    pub observed_at: u64,
    pub complete: bool,
    pub source: Source,
    pub entries: Vec<Entry>,
}
impl Catalogue {
    pub fn age(&self, now: u64) -> u64 {
        now.saturating_sub(self.observed_at)
    }
    pub fn freshness(&self, now: u64) -> Freshness {
        if self.age(now) <= CATALOGUE_MAX_AGE_SECS {
            Freshness::Fresh
        } else {
            Freshness::Stale
        }
    }
    /// The entry a name (id or alias) denotes. No fuzzy matching and no default substitution.
    pub fn resolve(&self, name: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.model_id == name || e.alias.as_deref() == Some(name))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    Fresh,
    Stale,
}

/// The runtime a request is for, as the caller knows it from its own owned handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scope<'a> {
    pub provider: Provider,
    pub build: &'a str,
    pub session: &'a str,
    pub runtime_instance: &'a str,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Bound {
    pub provider: Provider,
    pub build: String,
    pub session: String,
    pub runtime_instance: String,
    pub generation: u64,
    pub catalogue_revision: u64,
}

/// A request that passed validation and what it was validated against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Validated {
    pub request: ModelRequest,
    pub resolved_model: String,
    pub bound: Bound,
    pub validated_at: u64,
}

fn check_scope(scope: &Scope, bound: &Bound) -> Result<(), Refusal> {
    if scope.provider != bound.provider {
        return Err(Refusal::ProviderMismatch);
    }
    if scope.build != bound.build {
        return Err(Refusal::BuildMismatch);
    }
    if scope.session != bound.session {
        return Err(Refusal::SessionMismatch);
    }
    if scope.runtime_instance != bound.runtime_instance {
        return Err(Refusal::RuntimeMismatch);
    }
    if scope.generation != bound.generation {
        return Err(Refusal::GenerationMismatch);
    }
    Ok(())
}

/// Admission of a request for the next turn of the runtime `scope` names. `now` is Unix seconds.
pub fn validate(
    scope: &Scope,
    cat: &Catalogue,
    now: u64,
    request: ModelRequest,
) -> Result<Validated, Refusal> {
    if !gate::model_selection_blockers().is_empty() {
        return Err(Refusal::GateClosed);
    }
    validate_with_gate(scope, cat, now, request)
}

/// The same checks with the gate decision already taken by the caller (tests, and the future
/// authority handler once ADR0054 is accepted).
pub fn validate_with_gate(
    scope: &Scope,
    cat: &Catalogue,
    now: u64,
    request: ModelRequest,
) -> Result<Validated, Refusal> {
    let caps = Caps::of(scope.provider);
    if scope.provider != cat.provider {
        return Err(Refusal::ProviderMismatch);
    }
    if scope.build != cat.build {
        return Err(Refusal::BuildMismatch);
    }
    if !cat.complete {
        return Err(Refusal::CatalogueIncomplete);
    }
    if cat.freshness(now) == Freshness::Stale {
        return Err(Refusal::CatalogueStale);
    }
    if !caps.model_per_turn {
        return Err(Refusal::ModelNotPerTurn);
    }
    // Absent entirely is not "the default": a request names a model.
    let entry = cat.resolve(&request.model).ok_or(Refusal::UnknownModel)?;
    if let Some(effort) = &request.effort {
        if !caps.effort_per_turn {
            return Err(Refusal::EffortNotPerTurn);
        }
        if !entry.efforts.iter().any(|e| e == effort) {
            return Err(Refusal::UnsupportedEffort);
        }
    }
    Ok(Validated {
        resolved_model: entry.model_id.clone(),
        bound: Bound {
            provider: scope.provider,
            build: scope.build.to_owned(),
            session: scope.session.to_owned(),
            runtime_instance: scope.runtime_instance.to_owned(),
            generation: scope.generation,
            catalogue_revision: cat.revision,
        },
        request,
        validated_at: now,
    })
}

/// Run immediately before the request is dispatched: nothing it was validated for may have
/// changed since, and the catalogue must still be fresh.
pub fn revalidate(v: &Validated, scope: &Scope, cat: &Catalogue, now: u64) -> Result<(), Refusal> {
    check_scope(scope, &v.bound)?;
    if cat.revision != v.bound.catalogue_revision {
        return Err(Refusal::CatalogueChanged);
    }
    if cat.freshness(now) == Freshness::Stale {
        return Err(Refusal::CatalogueStale);
    }
    if cat.resolve(&v.request.model).map(|e| &e.model_id) != Some(&v.resolved_model) {
        return Err(Refusal::UnknownModel);
    }
    Ok(())
}

/// A runtime-level pin (the model or effort a runtime starts with). For Claude the effort can only
/// be set here; it is validated against the effort levels the pinned build lists, because the
/// catalogue does not exist yet when the runtime is started.
pub fn validate_runtime_effort(provider: Provider, effort: &str) -> Result<(), Refusal> {
    if !gate::model_selection_blockers().is_empty() {
        return Err(Refusal::GateClosed);
    }
    validate_runtime_effort_with_gate(provider, effort)
}
pub fn validate_runtime_effort_with_gate(provider: Provider, effort: &str) -> Result<(), Refusal> {
    if !Caps::of(provider).effort_per_runtime {
        return Err(Refusal::UnsupportedEffort);
    }
    // The levels the pinned build's `models` listing names for every model.
    const CLAUDE_LEVELS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
    if provider == Provider::Claude && CLAUDE_LEVELS.contains(&effort) {
        Ok(())
    } else {
        Err(Refusal::UnsupportedEffort)
    }
}

// ---- the four facts -----------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Entitlement {
    /// A listed model is not proof that this account may use it, and nothing here asks.
    NotChecked,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Offered {
    pub provider: Provider,
    pub source: Source,
    pub revision: u64,
    pub observed_at: u64,
    pub freshness: Freshness,
    pub complete: bool,
    pub entitlement: Entitlement,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Requested {
    pub turn_id: String,
    pub request: ModelRequest,
    pub resolved_model: String,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum Reroute {
    /// No model was requested for the turn: there is nothing to compare.
    NotRequested,
    /// A model was requested and the provider has not named one yet.
    NotReported,
    Honored,
    /// The provider named a model other than the one requested (or besides it).
    Rerouted {
        requested: String,
        reported: Vec<String>,
    },
}

/// What is known of the reasoning effort. Acceptance of a request is not evidence of effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "value")]
pub enum EffortEvidence {
    NotRequested,
    /// Asked for; nothing says the provider saw it.
    Requested(String),
    /// The provider's request carried it (seen by the API tap). The CLI may still lower it for a
    /// model that does not support that level.
    ObservedInProviderRequest(String),
    /// The provider itself said which effort applied.
    ProviderReported(String),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelFacts {
    pub offered: Option<Offered>,
    pub configured: Option<ModelRequest>,
    pub requested: Option<Requested>,
    pub reported_in_use: Vec<String>,
    pub reroute: Reroute,
    pub effort: EffortEvidence,
}

/// Compares the model requested for a turn with the models the provider named for it. Names are
/// resolved through the catalogue, so an alias and its model id are the same model.
pub fn reroute(
    cat: Option<&Catalogue>,
    requested: Option<&Requested>,
    reported: &BTreeSet<String>,
) -> Reroute {
    let Some(req) = requested else {
        return Reroute::NotRequested;
    };
    if reported.is_empty() {
        return Reroute::NotReported;
    }
    let canon = |n: &str| -> String {
        cat.and_then(|c| c.resolve(n))
            .map_or_else(|| n.to_owned(), |e| e.model_id.clone())
    };
    let want = canon(&req.resolved_model);
    let got: Vec<String> = reported
        .iter()
        .map(|n| canon(n))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if got.len() == 1 && got[0] == want {
        Reroute::Honored
    } else {
        Reroute::Rerouted {
            requested: want,
            reported: got,
        }
    }
}

pub fn effort(
    requested: Option<&str>,
    observed_in_request: Option<&str>,
    provider_reported: Option<&str>,
) -> EffortEvidence {
    match (requested, observed_in_request, provider_reported) {
        (_, _, Some(r)) => EffortEvidence::ProviderReported(r.to_owned()),
        (Some(want), Some(seen), None) if want == seen => {
            EffortEvidence::ObservedInProviderRequest(seen.to_owned())
        }
        (Some(want), _, None) => EffortEvidence::Requested(want.to_owned()),
        (None, _, None) => EffortEvidence::NotRequested,
    }
}

pub fn facts(
    cat: Option<&Catalogue>,
    now: u64,
    configured: Option<ModelRequest>,
    requested: Option<Requested>,
    reported: &BTreeSet<String>,
    effort_observed: Option<&str>,
    effort_reported: Option<&str>,
) -> ModelFacts {
    let want_effort = requested
        .as_ref()
        .and_then(|r| r.request.effort.clone())
        .or_else(|| configured.as_ref().and_then(|c| c.effort.clone()));
    ModelFacts {
        offered: cat.map(|c| Offered {
            provider: c.provider,
            source: c.source,
            revision: c.revision,
            observed_at: c.observed_at,
            freshness: c.freshness(now),
            complete: c.complete,
            entitlement: Entitlement::NotChecked,
            entries: c.entries.clone(),
        }),
        reroute: reroute(cat, requested.as_ref(), reported),
        configured,
        requested,
        reported_in_use: reported.iter().cloned().collect(),
        effort: effort(want_effort.as_deref(), effort_observed, effort_reported),
    }
}

#[cfg(test)]
mod tests;
