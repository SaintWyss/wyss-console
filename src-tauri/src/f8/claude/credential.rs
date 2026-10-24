//! Broker credential types. A protected source is delivered by the separate native-consent
//! agent over authenticated inherited pipes; real provider policy gates remain closed.
use crate::contracts::control::{ControlError as E, ControlResult};

/// A credential's text. It never appears in `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);
impl ApiKey {
    pub fn new(value: String) -> ControlResult<Self> {
        if value.is_empty() || value.len() > 512 || !value.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(E::InvalidRequest);
        }
        Ok(Self(value))
    }
    /// Only the launcher's broker frame reads it.
    pub(super) fn expose(&self) -> &str {
        &self.0
    }
}
impl Drop for ApiKey {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.0);
    }
}
impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

/// Where a runtime's credential comes from, asked once per runtime start. Implementations are
/// owned by the authority's operator, never named by a client.
pub trait CredentialSource: Send + Sync {
    fn key(&self) -> ControlResult<Option<ApiKey>>;
    fn key_for(
        &self,
        _identity: &super::worker::Identity,
        _profile: crate::adapters::claude::Profile,
    ) -> ControlResult<Option<ApiKey>> {
        self.key()
    }
}

/// Default source until an operator explicitly selects the reviewed protected handoff.
pub struct NoCredential;
impl CredentialSource for NoCredential {
    fn key(&self) -> ControlResult<Option<ApiKey>> {
        Ok(None)
    }
}

/// A synthetic key held in memory, for tests and the isolated fixture.
#[cfg(any(test, feature = "m3-fixture"))]
pub struct SyntheticCredential(pub ApiKey);
#[cfg(any(test, feature = "m3-fixture"))]
impl CredentialSource for SyntheticCredential {
    fn key(&self) -> ControlResult<Option<ApiKey>> {
        Ok(Some(self.0.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_validated_and_never_shown() {
        let key = ApiKey::new("sk-ant-api03-SECRETVALUE".into()).unwrap();
        assert!(!format!("{key:?}").contains("SECRET"));
        assert!(ApiKey::new(String::new()).is_err());
        assert!(ApiKey::new("has space".into()).is_err());
        assert!(ApiKey::new("x".repeat(513)).is_err());
        assert!(ApiKey::new("tab\there".into()).is_err());
    }

    #[test]
    fn production_has_no_credential_source_and_gives_no_key() {
        assert_eq!(NoCredential.key().unwrap(), None);
    }

    #[test]
    fn this_module_reads_no_file_keyring_or_environment() {
        let source = include_str!("credential.rs");
        let code = source.split("#[cfg(test)]").next().unwrap();
        for forbidden in [
            "std::fs",
            "File::",
            "env::var",
            "keyring",
            "Secret Service",
            "/proc",
        ] {
            // The words may appear in the module's own comment; code lines may not use them.
            let used = code
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .any(|l| l.contains(forbidden));
            assert!(!used, "{forbidden}");
        }
    }
}
