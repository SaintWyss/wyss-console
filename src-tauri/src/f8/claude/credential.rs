//! How a provider credential could reach a Claude runtime, and why it cannot yet.
//!
//! ADR0038 seals API keys and returns none; it says how a key reaches a provider "needs its own
//! decision". That decision is the proposed ADR0056, so this build has **no source of a real
//! credential**: `CredentialSource` has no production implementation, the launcher in production
//! mode asks none, and a runtime whose origin is the provider is refused without one. Nothing here
//! reads a file, a keyring or an environment variable.
//!
//! What exists for tests and the isolated fixture is a synthetic key held in memory. A key, when
//! one exists, is handed once to the credential broker's stdin and is never in an environment, a
//! command line, a log, a bridge frame or a window. This type does not zeroize its memory and no
//! claim of that kind is made: a process of the same user that can read another's memory can read it.
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
impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

/// Where a runtime's credential comes from, asked once per runtime start. Implementations are
/// owned by the authority's operator, never named by a client.
pub trait CredentialSource: Send + Sync {
    fn key(&self) -> ControlResult<Option<ApiKey>>;
}

/// The only source in a production build: there is none.
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
