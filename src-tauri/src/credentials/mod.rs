//! Provider API keys kept by the Console (F14). A key is sealed with the vault of ADR 0029
//! (ChaCha20-Poly1305, the key in the Secret Service, the record id and kind bound as
//! associated data) and written as one file in a directory only this user can enter. What the
//! window can see is a name, a provider word, a short fingerprint and two times: there is no
//! operation that returns a key, and no key is ever in a window command's answer, a log or a
//! diagnostic. Nothing here uses a key to start a provider (that needs its own decision).
pub mod commands;
use crate::agentconfig::vault::{RecordKind, Sealed, Vault, VaultError};
use crate::native::store;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    sync::{Mutex, PoisonError},
};
use zeroize::Zeroizing;

pub const MAX_CREDENTIALS: usize = 100;
pub const MAX_NAME: usize = 60;
pub const MAX_SECRET: usize = 4096;
const MAX_FILE: u64 = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Anthropic,
    Openai,
    Google,
    Openrouter,
    Other,
}

/// What the window may see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    pub id: String,
    pub name: String,
    pub provider: Provider,
    /// The first twelve hex digits of the key's SHA-256: enough to tell two keys apart, and not
    /// enough to find one.
    pub fingerprint: String,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialError {
    NameInvalid,
    SecretInvalid,
    Unknown,
    TooMany,
    /// No key store to seal with: nothing is kept, and nothing is kept unsealed instead.
    KeyStoreUnavailable,
    /// The key that sealed it is gone.
    KeyLost,
    Damaged,
    StoreUnavailable,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(flatten)]
    meta: Meta,
    sealed: Sealed,
}

pub struct CredentialStore {
    dir: PathBuf,
    vault: Vault,
    guard: Mutex<()>,
}

fn id_ok(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn fingerprint(secret: &[u8]) -> String {
    Sha256::digest(secret)[..6]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn from_vault(error: VaultError) -> CredentialError {
    match error {
        VaultError::KeyLost => CredentialError::KeyLost,
        VaultError::KeyStoreUnavailable => CredentialError::KeyStoreUnavailable,
        _ => CredentialError::Damaged,
    }
}

impl CredentialStore {
    pub fn open(dir: PathBuf, vault: Vault) -> Result<Self, CredentialError> {
        store::private_dir(&dir).map_err(|_| CredentialError::StoreUnavailable)?;
        Ok(Self {
            dir,
            vault,
            guard: Mutex::new(()),
        })
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.guard.lock().unwrap_or_else(PoisonError::into_inner)
    }
    fn read(&self, id: &str) -> Option<File> {
        if !id_ok(id) {
            return None;
        }
        let bytes = store::read_bounded(&self.path(id), MAX_FILE)?;
        let file: File = serde_json::from_slice(&bytes).ok()?;
        (file.meta.id == id).then_some(file)
    }
    fn write(&self, file: &File) -> Result<(), CredentialError> {
        let bytes =
            serde_json::to_vec_pretty(file).map_err(|_| CredentialError::StoreUnavailable)?;
        store::write_atomic(&self.path(&file.meta.id), &bytes)
            .map_err(|_| CredentialError::StoreUnavailable)
    }

    pub fn list(&self) -> Vec<Meta> {
        let _g = self.lock();
        let mut out: Vec<Meta> = fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                self.read(name.strip_suffix(".json")?).map(|f| f.meta)
            })
            .collect();
        out.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a.id.cmp(&b.id))
        });
        out
    }

    fn check(name: &str, secret: &str) -> Result<(), CredentialError> {
        let name_ok = !name.trim().is_empty()
            && name.chars().count() <= MAX_NAME
            && !name.chars().any(char::is_control);
        if !name_ok {
            return Err(CredentialError::NameInvalid);
        }
        if secret.is_empty()
            || secret.len() > MAX_SECRET
            || secret.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            return Err(CredentialError::SecretInvalid);
        }
        Ok(())
    }

    /// Seals and keeps a key. Nothing is kept when it cannot be sealed.
    pub fn add(
        &self,
        name: &str,
        provider: Provider,
        secret: &Zeroizing<String>,
    ) -> Result<Meta, CredentialError> {
        Self::check(name, secret)?;
        let _g = self.lock();
        let count = fs::read_dir(&self.dir).map_or(0, |d| d.flatten().count());
        if count >= MAX_CREDENTIALS {
            return Err(CredentialError::TooMany);
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let sealed = self
            .vault
            .seal(RecordKind::Credential, &id, secret.as_bytes())
            .map_err(from_vault)?;
        let now = store::now();
        let meta = Meta {
            id,
            name: name.trim().to_owned(),
            provider,
            fingerprint: fingerprint(secret.as_bytes()),
            created_at: now,
            updated_at: now,
        };
        self.write(&File {
            meta: meta.clone(),
            sealed,
        })?;
        Ok(meta)
    }

    /// A new key under the same name and identity; the old one is gone once this returns.
    pub fn replace(&self, id: &str, secret: &Zeroizing<String>) -> Result<Meta, CredentialError> {
        Self::check("x", secret)?;
        let _g = self.lock();
        let mut file = self.read(id).ok_or(CredentialError::Unknown)?;
        file.sealed = self
            .vault
            .seal(RecordKind::Credential, id, secret.as_bytes())
            .map_err(from_vault)?;
        file.meta.fingerprint = fingerprint(secret.as_bytes());
        file.meta.updated_at = store::now();
        self.write(&file)?;
        Ok(file.meta)
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<Meta, CredentialError> {
        Self::check(name, "x")?;
        let _g = self.lock();
        let mut file = self.read(id).ok_or(CredentialError::Unknown)?;
        file.meta.name = name.trim().to_owned();
        file.meta.updated_at = store::now();
        self.write(&file)?;
        Ok(file.meta)
    }

    pub fn delete(&self, id: &str) -> Result<(), CredentialError> {
        let _g = self.lock();
        self.read(id).ok_or(CredentialError::Unknown)?;
        fs::remove_file(self.path(id)).map_err(|_| CredentialError::StoreUnavailable)
    }

    /// Whether the key still opens (the key store has its key, the file is intact). The key
    /// itself is opened and dropped here, and goes nowhere.
    pub fn verify(&self, id: &str) -> Result<bool, CredentialError> {
        let _g = self.lock();
        let file = self.read(id).ok_or(CredentialError::Unknown)?;
        match self.vault.open(RecordKind::Credential, id, &file.sealed) {
            Ok(secret) => Ok(fingerprint(&secret) == file.meta.fingerprint),
            Err(VaultError::KeyLost) => Err(CredentialError::KeyLost),
            Err(VaultError::KeyStoreUnavailable) => Err(CredentialError::KeyStoreUnavailable),
            Err(_) => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agentconfig::vault::MemoryStore;
    use std::os::unix::fs::PermissionsExt;

    const KEY: &str = concat!("sk-ant-", "api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789");

    fn store() -> (CredentialStore, std::sync::Arc<MemoryStore>, PathBuf) {
        let dir = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("wyss-cred-{}", uuid::Uuid::new_v4().simple()));
        let memory = MemoryStore::new();
        let vault = Vault::new(Box::new(memory.clone()));
        (
            CredentialStore::open(dir.join("c"), vault).unwrap(),
            memory,
            dir,
        )
    }
    fn secret(text: &str) -> Zeroizing<String> {
        Zeroizing::new(text.to_owned())
    }

    #[test]
    fn a_key_is_kept_sealed_and_only_its_name_and_fingerprint_come_back() {
        let (store, _m, dir) = store();
        let meta = store
            .add("Work", Provider::Anthropic, &secret(KEY))
            .unwrap();
        assert_eq!(meta.fingerprint.len(), 12);
        let listed = store.list();
        assert_eq!(listed, vec![meta.clone()]);
        // Nothing on disk or in what the window gets holds the key.
        let file = fs::read_to_string(dir.join("c").join(format!("{}.json", meta.id))).unwrap();
        assert!(!file.contains(KEY) && !file.contains("ABCDEFGH"));
        assert!(!serde_json::to_string(&listed).unwrap().contains("ABCDEFGH"));
        let mode = fs::metadata(dir.join("c").join(format!("{}.json", meta.id)))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(
            fs::metadata(dir.join("c")).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(store.verify(&meta.id), Ok(true));
    }

    #[test]
    fn replacing_changes_the_fingerprint_and_nothing_else_about_who_it_is() {
        let (store, _m, _d) = store();
        let meta = store.add("Work", Provider::Openai, &secret(KEY)).unwrap();
        let again = store
            .replace(&meta.id, &secret(&format!("{KEY}2")))
            .unwrap();
        assert_eq!(
            (again.id.as_str(), again.name.as_str()),
            (meta.id.as_str(), "Work")
        );
        assert_ne!(again.fingerprint, meta.fingerprint);
        assert_eq!(store.verify(&meta.id), Ok(true));
        assert_eq!(store.rename(&meta.id, "Home").unwrap().name, "Home");
        store.delete(&meta.id).unwrap();
        assert!(store.list().is_empty());
        assert_eq!(store.delete(&meta.id), Err(CredentialError::Unknown));
    }

    #[test]
    fn with_no_key_store_nothing_is_kept_and_a_lost_key_is_said() {
        let (store, memory, _d) = store();
        memory.set_unavailable(true);
        assert_eq!(
            store.add("Work", Provider::Other, &secret(KEY)),
            Err(CredentialError::KeyStoreUnavailable)
        );
        assert!(store.list().is_empty(), "nothing was written unsealed");
        memory.set_unavailable(false);
        let meta = store.add("Work", Provider::Other, &secret(KEY)).unwrap();
        memory.forget_for_test();
        assert_eq!(store.verify(&meta.id), Err(CredentialError::KeyLost));
    }

    #[test]
    fn a_record_cannot_be_swapped_for_another_and_bad_input_is_refused() {
        let (store, _m, dir) = store();
        let a = store.add("A", Provider::Google, &secret(KEY)).unwrap();
        let b = store
            .add("B", Provider::Google, &secret("other-key-123456"))
            .unwrap();
        // B's sealed body under A's identity does not open (the id is bound into it).
        let bytes: serde_json::Value = serde_json::from_slice(
            &fs::read(dir.join("c").join(format!("{}.json", b.id))).unwrap(),
        )
        .unwrap();
        let mut forged: serde_json::Value = serde_json::from_slice(
            &fs::read(dir.join("c").join(format!("{}.json", a.id))).unwrap(),
        )
        .unwrap();
        forged["sealed"] = bytes["sealed"].clone();
        fs::write(
            dir.join("c").join(format!("{}.json", a.id)),
            serde_json::to_vec(&forged).unwrap(),
        )
        .unwrap();
        assert_eq!(store.verify(&a.id), Ok(false));
        for bad in ["", "has space", "new\nline", &"x".repeat(5000)] {
            assert_eq!(
                store.add("N", Provider::Other, &secret(bad)),
                Err(CredentialError::SecretInvalid),
                "{bad:.8}"
            );
        }
        assert_eq!(
            store.add("", Provider::Other, &secret(KEY)),
            Err(CredentialError::NameInvalid)
        );
        assert_eq!(
            store.add("a\u{7}b", Provider::Other, &secret(KEY)),
            Err(CredentialError::NameInvalid)
        );
        assert_eq!(store.verify("../etc"), Err(CredentialError::Unknown));
    }
}
