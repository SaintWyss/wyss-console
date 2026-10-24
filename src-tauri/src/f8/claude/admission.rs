//! The permission to start a Claude runtime, held by the thing that starts it.
//!
//! `m3::gate` records the decisions that are still the owner's. Showing them in a window is not
//! enforcing them, so the launcher and the worker each refuse to exist without an `Admission`, and
//! the only way to get one in a production build is for every gate of the profile to be open,
//! which none is. The fixture and the tests have their own constructor, compiled only into test
//! builds and the `m3-fixture` feature: no packaged build contains it.
use crate::{
    adapters::claude::Profile,
    m3::gate::{self, Blocker},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Production(Profile),
    #[cfg(any(test, feature = "m3-fixture"))]
    Fixture,
}

/// Proof that this runtime may start. Not constructible from outside this module except through
/// `production` (every gate open) or, in test and fixture builds only, `fixture`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Admission(Kind);

impl Admission {
    /// Every decision the profile needs has been taken; otherwise the list of what is missing.
    pub fn production(profile: Profile) -> Result<Self, Vec<Blocker>> {
        let blockers = match profile {
            Profile::Inference => gate::claude_inference_blockers(),
            Profile::LocalTools => gate::claude_local_tools_blockers(),
        };
        if blockers.is_empty() {
            Ok(Self(Kind::Production(profile)))
        } else {
            Err(blockers)
        }
    }

    /// An isolated fixture or a test: nothing real is reached.
    #[cfg(any(test, feature = "m3-fixture"))]
    pub fn fixture() -> Self {
        Self(Kind::Fixture)
    }

    /// A production-kind admission without the gates, to test what the kind itself allows.
    #[cfg(test)]
    pub(crate) fn production_unchecked(profile: Profile) -> Self {
        Self(Kind::Production(profile))
    }

    pub fn permits(&self, profile: Profile) -> bool {
        match self.0 {
            Kind::Production(p) => p == profile,
            #[cfg(any(test, feature = "m3-fixture"))]
            Kind::Fixture => true,
        }
    }

    /// Whether this is the fixture kind (which can never reach the real provider).
    pub fn is_fixture(&self) -> bool {
        match self.0 {
            Kind::Production(_) => false,
            #[cfg(any(test, feature = "m3-fixture"))]
            Kind::Fixture => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_admission_is_refused_for_both_profiles_and_names_every_missing_decision() {
        for profile in [Profile::Inference, Profile::LocalTools] {
            let missing = Admission::production(profile).unwrap_err();
            for blocker in [
                Blocker::Adr0053NotAccepted,
                Blocker::StoreCannotHoldClaude,
                Blocker::RealProviderEvidenceMissing,
                Blocker::CredentialDeliveryNotAccepted,
                Blocker::IndependentReviewMissing,
            ] {
                assert!(missing.contains(&blocker), "{profile:?} {blocker:?}");
            }
        }
    }

    #[test]
    fn the_fixture_admission_exists_only_for_tests_and_the_fixture_feature() {
        // Compiled in here because this is a test build; the source says where else.
        let source = include_str!("admission.rs");
        let before_tests = source.split("#[cfg(test)]\nmod tests").next().unwrap();
        let gated = before_tests
            .matches("#[cfg(any(test, feature = \"m3-fixture\"))]")
            .count();
        assert_eq!(gated, 4, "each fixture-only item carries the gate");
        assert!(Admission::fixture().is_fixture());
        assert!(Admission::fixture().permits(Profile::LocalTools));
    }
}
