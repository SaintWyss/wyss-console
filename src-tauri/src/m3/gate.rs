//! What M3 is authorized to do. Every gate is a constant in source: it changes only by a
//! reviewed code change made after the owner accepts the decision named next to it, never by
//! a setting, a client message or a file on disk.
//!
//! Implementation completeness and authorization are separate states: the code behind a
//! closed gate is built and tested, and cannot be reached.

/// ADR0053 — Claude Managed security and authentication boundary. Proposed.
pub const ADR0053_ACCEPTED: bool = false;
/// ADR0054 — Managed model and reasoning-effort selection, and the authority extension that
/// carries it. Proposed.
pub const ADR0054_ACCEPTED: bool = false;
/// The privacy, schema and retention contract of the local metrics history. Proposed.
pub const METRICS_POLICY_ACCEPTED: bool = false;
/// The accepted authority store can hold a Claude runtime. It cannot today: the V8 schema
/// has `CHECK(adapter_id='codex')` on `managed_runtimes` and `managed_runtime_launches`.
pub const STORE_HOLDS_CLAUDE: bool = false;

/// ADR0056 — how a provider credential reaches a Claude runtime. ADR0038 keeps keys sealed and
/// never returns one; delivering one to a provider is a decision of its own. Until it is accepted
/// no production code path can obtain a credential (`f8::claude::credential`).
pub const CREDENTIAL_DELIVERY_ACCEPTED: bool = false;
/// A reviewer other than the author inspected the final code and the network confinement of the
/// runtime. Real-credential admission stays closed until this is recorded.
pub const INDEPENDENT_REVIEW_PASSED: bool = false;

/// Evidence that the exact provider build passed the qualification the profile needs. These
/// are set by recording real-provider evidence in the acceptance record, never by fixtures.
pub const REAL_PROVIDER_INFERENCE_PASSED: bool = false;
pub const REAL_PROVIDER_LOCAL_TOOLS_PASSED: bool = false;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Blocker {
    Adr0053NotAccepted,
    StoreCannotHoldClaude,
    RealProviderEvidenceMissing,
    Adr0054NotAccepted,
    MetricsPolicyNotAccepted,
    CredentialDeliveryNotAccepted,
    IndependentReviewMissing,
}
impl Blocker {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Adr0053NotAccepted => "adr0053_not_accepted",
            Self::StoreCannotHoldClaude => "store_cannot_hold_claude",
            Self::RealProviderEvidenceMissing => "real_provider_evidence_missing",
            Self::Adr0054NotAccepted => "adr0054_not_accepted",
            Self::MetricsPolicyNotAccepted => "metrics_policy_not_accepted",
            Self::CredentialDeliveryNotAccepted => "credential_delivery_not_accepted",
            Self::IndependentReviewMissing => "independent_review_missing",
        }
    }
}

/// Why Claude Inference cannot be started. Empty means it may be (never true in this source).
pub fn claude_inference_blockers() -> Vec<Blocker> {
    let mut b = Vec::new();
    if !ADR0053_ACCEPTED {
        b.push(Blocker::Adr0053NotAccepted);
    }
    if !STORE_HOLDS_CLAUDE {
        b.push(Blocker::StoreCannotHoldClaude);
    }
    if !REAL_PROVIDER_INFERENCE_PASSED {
        b.push(Blocker::RealProviderEvidenceMissing);
    }
    b.extend(credential_blockers());
    b
}
pub fn claude_local_tools_blockers() -> Vec<Blocker> {
    let mut b = Vec::new();
    if !ADR0053_ACCEPTED {
        b.push(Blocker::Adr0053NotAccepted);
    }
    if !STORE_HOLDS_CLAUDE {
        b.push(Blocker::StoreCannotHoldClaude);
    }
    if !REAL_PROVIDER_LOCAL_TOOLS_PASSED {
        b.push(Blocker::RealProviderEvidenceMissing);
    }
    b.extend(credential_blockers());
    b
}
/// What stands between a Claude runtime and a real provider credential.
fn credential_blockers() -> Vec<Blocker> {
    let mut b = Vec::new();
    if !CREDENTIAL_DELIVERY_ACCEPTED {
        b.push(Blocker::CredentialDeliveryNotAccepted);
    }
    if !INDEPENDENT_REVIEW_PASSED {
        b.push(Blocker::IndependentReviewMissing);
    }
    b
}
pub fn model_selection_blockers() -> Vec<Blocker> {
    if ADR0054_ACCEPTED {
        Vec::new()
    } else {
        vec![Blocker::Adr0054NotAccepted]
    }
}
pub fn metrics_history_blockers() -> Vec<Blocker> {
    if METRICS_POLICY_ACCEPTED {
        Vec::new()
    } else {
        vec![Blocker::MetricsPolicyNotAccepted]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::assertions_on_constants)] // the point: a tripwire on constants
    fn no_gate_is_open_in_this_source() {
        // A deliberate tripwire: opening a gate is a reviewed change that must update this
        // test, the acceptance record and the owner's decision together.
        assert!(!ADR0053_ACCEPTED && !ADR0054_ACCEPTED && !METRICS_POLICY_ACCEPTED);
        assert!(!STORE_HOLDS_CLAUDE);
        assert!(!CREDENTIAL_DELIVERY_ACCEPTED && !INDEPENDENT_REVIEW_PASSED);
        assert!(!REAL_PROVIDER_INFERENCE_PASSED && !REAL_PROVIDER_LOCAL_TOOLS_PASSED);
        assert!(!claude_inference_blockers().is_empty());
        assert!(!claude_local_tools_blockers().is_empty());
        assert!(!model_selection_blockers().is_empty());
        assert!(!metrics_history_blockers().is_empty());
    }

    #[test]
    fn local_tools_does_not_borrow_inferences_evidence() {
        assert!(claude_local_tools_blockers().contains(&Blocker::RealProviderEvidenceMissing));
        assert!(claude_inference_blockers().contains(&Blocker::StoreCannotHoldClaude));
    }

    #[test]
    fn blocker_codes_are_unique() {
        let all = [
            Blocker::Adr0053NotAccepted,
            Blocker::StoreCannotHoldClaude,
            Blocker::RealProviderEvidenceMissing,
            Blocker::Adr0054NotAccepted,
            Blocker::MetricsPolicyNotAccepted,
            Blocker::CredentialDeliveryNotAccepted,
            Blocker::IndependentReviewMissing,
        ];
        let codes: std::collections::BTreeSet<_> = all.iter().map(|b| b.code()).collect();
        assert_eq!(codes.len(), all.len());
    }
}
