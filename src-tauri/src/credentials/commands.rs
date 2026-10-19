//! The window's commands for API credentials (F14). The page names a credential by its id and
//! says what to call it; the key itself is typed in a native window the page cannot reach and
//! goes straight to the sealed store. No command returns a key.
use super::{CredentialError, CredentialStore, Meta, Provider};
use crate::agentconfig::{SecretToolStore, Vault};
use crate::native_dialog::{confirm_text, secret_entry};
use serde::Deserialize;
use std::sync::Arc;
use tauri::{AppHandle, State};

pub struct Credentials {
    store: Option<Arc<CredentialStore>>,
}

impl Credentials {
    pub fn start() -> Self {
        let dir = std::env::var_os("XDG_STATE_HOME")
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state"))
            })
            .map(|b| b.join("wyss-console").join("credentials"));
        let store = dir.and_then(|dir| {
            CredentialStore::open(dir, Vault::new(Box::new(SecretToolStore::new()))).ok()
        });
        Self {
            store: store.map(Arc::new),
        }
    }
    fn store(&self) -> Result<Arc<CredentialStore>, CredentialError> {
        self.store.clone().ok_or(CredentialError::StoreUnavailable)
    }
}

fn id_ok(id: &str) -> Result<(), CredentialError> {
    (id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    .then_some(())
    .ok_or(CredentialError::Unknown)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, CredentialError> + Send + 'static,
) -> Result<T, CredentialError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| CredentialError::StoreUnavailable)?
}

#[tauri::command]
pub async fn credentials_list(state: State<'_, Credentials>) -> Result<Vec<Meta>, CredentialError> {
    let store = state.store()?;
    blocking(move || Ok(store.list())).await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddRequest {
    name: String,
    provider: Provider,
}

/// Asks for the key in a native window and keeps it sealed. `None` when it was dismissed.
#[tauri::command]
pub async fn credential_add(
    app: AppHandle,
    state: State<'_, Credentials>,
    request: AddRequest,
) -> Result<Option<Meta>, CredentialError> {
    let store = state.store()?;
    blocking(move || {
        let Some(secret) = secret_entry(
            &app,
            "New API key",
            "Paste the key. It is never shown again.",
        ) else {
            return Ok(None);
        };
        store
            .add(&request.name, request.provider, &secret)
            .map(Some)
    })
    .await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdRequest {
    id: String,
}

#[tauri::command]
pub async fn credential_replace(
    app: AppHandle,
    state: State<'_, Credentials>,
    request: IdRequest,
) -> Result<Option<Meta>, CredentialError> {
    id_ok(&request.id)?;
    let store = state.store()?;
    blocking(move || {
        let Some(secret) = secret_entry(
            &app,
            "Replace API key",
            "Paste the new key. The old one is discarded.",
        ) else {
            return Ok(None);
        };
        store.replace(&request.id, &secret).map(Some)
    })
    .await
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameRequest {
    id: String,
    name: String,
}

#[tauri::command]
pub async fn credential_rename(
    state: State<'_, Credentials>,
    request: RenameRequest,
) -> Result<Meta, CredentialError> {
    id_ok(&request.id)?;
    let store = state.store()?;
    blocking(move || store.rename(&request.id, &request.name)).await
}

/// Deleting asks again in a native window: a key that is gone cannot be brought back.
#[tauri::command]
pub async fn credential_delete(
    app: AppHandle,
    state: State<'_, Credentials>,
    request: IdRequest,
) -> Result<bool, CredentialError> {
    id_ok(&request.id)?;
    let store = state.store()?;
    blocking(move || {
        let name = store
            .list()
            .into_iter()
            .find(|m| m.id == request.id)
            .map(|m| m.name)
            .ok_or(CredentialError::Unknown)?;
        if !confirm_text(
            &app,
            "Delete this API key?",
            &format!("“{name}” is removed from this computer. It cannot be brought back."),
        ) {
            return Ok(false);
        }
        store.delete(&request.id).map(|()| true)
    })
    .await
}

#[tauri::command]
pub async fn credential_verify(
    state: State<'_, Credentials>,
    request: IdRequest,
) -> Result<bool, CredentialError> {
    id_ok(&request.id)?;
    let store = state.store()?;
    blocking(move || store.verify(&request.id)).await
}

const COMMANDS: &[&str] = &[
    "credentials_list",
    "credential_add",
    "credential_replace",
    "credential_rename",
    "credential_delete",
    "credential_verify",
];

pub fn extension() -> wyss_console::Extension {
    wyss_console::Extension {
        commands: COMMANDS,
        handler: Box::new(tauri::generate_handler![
            credentials_list,
            credential_add,
            credential_replace,
            credential_rename,
            credential_delete,
            credential_verify
        ]),
        install: Box::new(|builder| builder.manage(Credentials::start())),
        on_exit: None,
    }
}
