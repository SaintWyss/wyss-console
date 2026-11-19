use wyss_console::credentials::{CredentialError, CredentialStore, Meta, Provider};
use zeroize::Zeroizing;

#[derive(Default)]
pub(super) struct Model {
    pub items: Vec<Meta>,
}

impl std::fmt::Debug for Model {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output
            .debug_struct("CredentialViewModel")
            .finish_non_exhaustive()
    }
}

pub(super) fn matches_name(expected: &str, entered: &str) -> bool {
    expected == entered
}

impl Model {
    pub fn list(store: &CredentialStore) -> Self {
        Self {
            items: store.list(),
        }
    }

    pub fn add(
        store: &CredentialStore,
        name: &str,
        provider: Provider,
        secret: Zeroizing<String>,
    ) -> Result<(), CredentialError> {
        store.add(name, provider, &secret).map(drop)
    }

    pub fn replace(
        store: &CredentialStore,
        id: &str,
        secret: Zeroizing<String>,
    ) -> Result<(), CredentialError> {
        store.replace(id, &secret).map(drop)
    }

    pub fn rename(store: &CredentialStore, id: &str, name: &str) -> Result<(), CredentialError> {
        store.rename(id, name).map(drop)
    }

    pub fn delete(
        store: &CredentialStore,
        id: &str,
        entered: &str,
    ) -> Result<bool, CredentialError> {
        let item = Self::list(store)
            .items
            .into_iter()
            .find(|item| item.id == id)
            .ok_or(CredentialError::Unknown)?;
        if !matches_name(&item.name, entered) {
            return Ok(false);
        }
        store.delete(id).map(|()| true)
    }

    pub fn verify(store: &CredentialStore, id: &str) -> Result<bool, CredentialError> {
        store.verify(id)
    }
}

#[cfg(all(test, feature = "test-fixtures"))]
mod tests {
    use super::*;
    use wyss_console::agentconfig::{Vault, vault::MemoryStore};

    #[test]
    fn manages_credentials_with_the_core_memory_store() {
        let path = crate::personalization_scratch()
            .join(format!("native-credentials-{}", uuid::Uuid::new_v4()));
        let fixture = crate::PersonalizationFixture(path);
        let memory = MemoryStore::new();
        let store = CredentialStore::open(
            fixture.0.join("credentials"),
            Vault::new(Box::new(memory.clone())),
        )
        .unwrap();
        let password = || Zeroizing::new(String::from("1"));

        assert!(Model::list(&store).items.is_empty());
        Model::add(&store, "Fixture", Provider::Anthropic, password()).unwrap();
        let model = Model::list(&store);
        let [initial] = model.items.as_slice() else {
            panic!("expected one credential")
        };
        assert_eq!(initial.name, "Fixture");
        assert_eq!(initial.provider, Provider::Anthropic);
        assert_eq!(initial.fingerprint, "6b86b273ff34");
        assert!(Model::verify(&store, &initial.id).unwrap());

        // Removing the fixture sealing key makes the old record unreadable. Replacing
        // it with the same test password must seal it again and restore verification.
        memory.forget_for_test();
        assert!(matches!(
            Model::verify(&store, &initial.id),
            Err(CredentialError::KeyLost)
        ));
        Model::replace(&store, &initial.id, password()).unwrap();
        assert!(Model::verify(&store, &initial.id).unwrap());
        let replaced = Model::list(&store);
        assert_eq!(replaced.items[0].fingerprint, initial.fingerprint);

        Model::rename(&store, &initial.id, "Exact Name").unwrap();
        assert_eq!(Model::list(&store).items[0].name, "Exact Name");
        assert!(!Model::delete(&store, &initial.id, "exact name").unwrap());
        assert!(!Model::delete(&store, &initial.id, "Exact Name ").unwrap());
        assert!(Model::delete(&store, &initial.id, "Exact Name").unwrap());
        assert!(Model::list(&store).items.is_empty());
    }

    #[test]
    fn model_debug_omits_credential_values() {
        let password = "1";
        let model = Model {
            items: vec![Meta {
                id: "example".into(),
                name: format!("Fixture {password}"),
                provider: Provider::Anthropic,
                fingerprint: "6b86b273ff34".into(),
                created_at: 0,
                updated_at: 0,
            }],
        };
        let debug = format!("{model:?}");
        assert_eq!(debug, "CredentialViewModel { .. }");
        assert!(!debug.contains(password));
    }
}
