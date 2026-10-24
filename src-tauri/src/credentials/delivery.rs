//! ADR0056 candidate: an owner-selected sealed record, approved natively for one runtime.
//! There is no command/IPC to return a key. The production constructor fails before touching
//! the store until admission is authorized; the production launcher still uses NoCredential.
use crate::credentials::{CredentialStore, Meta, Provider};
use std::sync::{Arc, Mutex};
use wyss_console::adapters::claude::Profile;
use wyss_console::contracts::control::{ControlError as E, ControlResult};
use wyss_console::f8::claude::credential::{ApiKey, CredentialSource};

type Consent = dyn Fn(&Meta, Profile) -> bool + Send + Sync;

pub struct SealedCredentialSource {
    store: Arc<CredentialStore>,
    id: String,
    profile: Profile,
    consent: Box<Consent>,
    used: Mutex<bool>,
}

impl SealedCredentialSource {
    /// Only native application code may construct this source. No client can choose a path,
    /// store, keyring item or prompt implementation. It is not wired to production admission.
    pub fn prepare(
        store: Arc<CredentialStore>,
        id: String,
        profile: Profile,
        app: tauri::AppHandle,
    ) -> ControlResult<Self> {
        let blockers = match profile {
            Profile::Inference => wyss_console::m3::gate::claude_inference_blockers(),
            Profile::LocalTools => wyss_console::m3::gate::claude_local_tools_blockers(),
        };
        if !blockers.is_empty() {
            return Err(E::GateDenied);
        }
        Ok(Self::with_consent(
            store,
            id,
            profile,
            Box::new(move |meta, profile| {
                let profile = match profile {
                    Profile::Inference => "Inference",
                    Profile::LocalTools => "Local Tools",
                };
                crate::native_dialog::confirm_text(
                    &app,
                    "Claude Managed credential",
                    &format!(
                        "Deliver {} ({}) to the Anthropic broker for one {profile} runtime? Deleting the record does not stop an existing runtime.",
                        meta.name, meta.fingerprint
                    ),
                    "Deliver once",
                )
            }),
        ))
    }

    fn with_consent(
        store: Arc<CredentialStore>,
        id: String,
        profile: Profile,
        consent: Box<Consent>,
    ) -> Self {
        Self {
            store,
            id,
            profile,
            consent,
            used: Mutex::new(false),
        }
    }
}

impl CredentialSource for SealedCredentialSource {
    fn key(&self) -> ControlResult<Option<ApiKey>> {
        let mut used = self.used.lock().map_err(|_| E::GateDenied)?;
        if *used {
            return Err(E::GateDenied);
        }
        // A source is a one-shot runtime grant, including refusal or an unavailable vault.
        *used = true;
        let meta = self
            .store
            .list()
            .into_iter()
            .find(|m| m.id == self.id && m.provider == Provider::Anthropic)
            .ok_or(E::GateDenied)?;
        if !(self.consent)(&meta, self.profile) {
            return Err(E::GateDenied);
        }
        let secret = self
            .store
            .unseal_for_broker(&meta)
            .map_err(|_| E::GateDenied)?;
        ApiKey::new(secret.to_string()).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agentconfig::vault::{MemoryStore, Vault};
    use zeroize::Zeroizing;

    fn store() -> (Arc<CredentialStore>, std::path::PathBuf) {
        let path =
            std::env::temp_dir().join(format!("wyss-release-credential-{}", uuid::Uuid::new_v4()));
        (
            Arc::new(
                CredentialStore::open(path.clone(), Vault::new(Box::new(MemoryStore::new())))
                    .unwrap(),
            ),
            path,
        )
    }

    #[test]
    fn sealed_synthetic_key_is_delivered_once_and_never_persisted_plaintext() {
        let (store, path) = store();
        let secret = Zeroizing::new("release-SYNTHETIC-only".to_owned());
        let meta = store
            .add("synthetic", Provider::Anthropic, &secret)
            .unwrap();
        let source = SealedCredentialSource::with_consent(
            store,
            meta.id.clone(),
            Profile::Inference,
            Box::new(|_, _| true),
        );
        assert_eq!(
            source.key().unwrap(),
            Some(ApiKey::new(secret.to_string()).unwrap())
        );
        assert_eq!(source.key(), Err(E::GateDenied));
        let persisted = std::fs::read_to_string(path.join(format!("{}.json", meta.id))).unwrap();
        assert!(!persisted.contains(secret.as_str()));
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn native_refusal_wrong_provider_deletion_and_replacement_revoke_delivery() {
        for action in ["refuse", "wrong_provider", "delete", "replace"] {
            let (store, path) = store();
            let provider = if action == "wrong_provider" {
                Provider::Openai
            } else {
                Provider::Anthropic
            };
            let meta = store
                .add(
                    "synthetic",
                    provider,
                    &Zeroizing::new("synthetic-original".into()),
                )
                .unwrap();
            let change = store.clone();
            let source = SealedCredentialSource::with_consent(
                store,
                meta.id,
                Profile::LocalTools,
                Box::new(move |m, _| {
                    match action {
                        "refuse" => return false,
                        "delete" => change.delete(&m.id).unwrap(),
                        "replace" => {
                            change
                                .replace(&m.id, &Zeroizing::new("synthetic-replaced".into()))
                                .unwrap();
                        }
                        _ => (),
                    }
                    true
                }),
            );
            assert_eq!(source.key(), Err(E::GateDenied), "{action}");
            std::fs::remove_dir_all(path).unwrap();
        }
    }
}
