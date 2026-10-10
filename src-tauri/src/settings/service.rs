//! The settings of the desktop, held for the life of the process.
//!
//! One owner of the preferences: every read, update and reset goes through one lock, so
//! an update racing a reset leaves one of the two outcomes and never a mixture. Nothing
//! here reaches the authority, the runtime, a process or a file other than the one
//! preference file: a preference is shown, it does not decide.
use super::model::Preferences;
use super::store::{Loaded, Store, StoreError};
use serde::Serialize;
use std::sync::{Arc, Mutex, MutexGuard};

const VIEW_SCHEMA_VERSION: u8 = 1;

/// How the preferences are kept, in closed words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Storage {
    /// Saved preferences were read, or none were saved yet.
    Ready,
    /// A saved file could not be understood and was set aside. Defaults apply until the
    /// next change.
    Recovered,
    /// The saved file is from a newer Wyss Console. It is left as it is and cannot be
    /// changed from here until it is reset.
    Newer,
    /// The saved file is not safely this user's alone and was not read. Defaults apply
    /// until the next change, which replaces it.
    Unsafe,
    /// The place preferences are kept in cannot be used. Defaults apply and nothing can
    /// be saved.
    Unavailable,
}

/// Why a change was not made. Nothing was changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsFailure {
    /// A value is out of bounds, or the set is of another version.
    Invalid,
    /// The saved preferences are from a newer version; reset them first.
    ReadOnly,
    /// The preferences could not be written.
    NotSaved,
    Busy,
    Internal,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SettingsError {
    code: SettingsFailure,
}
impl SettingsError {
    pub fn new(code: SettingsFailure) -> Self {
        Self { code }
    }
    pub fn code(&self) -> SettingsFailure {
        self.code
    }
}

/// What the build knows about itself. Constants: nothing here is read from the system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct About {
    pub version: &'static str,
    /// A short source revision, when the build knew one.
    pub revision: Option<&'static str>,
    /// Operating system and architecture, as Rust names them.
    pub platform: String,
}
impl About {
    pub fn of_build() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            revision: option_env!("WYSS_BUILD_REVISION").filter(|revision| {
                (7..=40).contains(&revision.len())
                    && revision
                        .bytes()
                        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            }),
            platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        }
    }
}

/// What the window is told: the preferences in force, how they are kept, and the facts
/// about this build the About section shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettingsView {
    pub schema_version: u8,
    pub storage: Storage,
    pub preferences: Preferences,
    pub about: About,
}

struct State {
    preferences: Preferences,
    storage: Storage,
}

#[derive(Clone)]
pub struct SettingsService {
    store: Option<Store>,
    state: Arc<Mutex<State>>,
}

impl SettingsService {
    /// The preferences of this user's account.
    pub fn from_environment() -> Self {
        Self::open(Store::from_environment())
    }
    /// Reads what `store` holds. Without a store, or with one that cannot be used, the
    /// defaults apply and nothing is saved.
    pub fn open(store: Option<Store>) -> Self {
        let (preferences, storage) = match store.as_ref().map(Store::load) {
            Some(Loaded::Stored(preferences)) => (preferences, Storage::Ready),
            Some(Loaded::Absent) => (Preferences::default(), Storage::Ready),
            Some(Loaded::Recovered) => (Preferences::default(), Storage::Recovered),
            Some(Loaded::Newer) => (Preferences::default(), Storage::Newer),
            Some(Loaded::Unsafe) => (Preferences::default(), Storage::Unsafe),
            Some(Loaded::Unavailable) | None => (Preferences::default(), Storage::Unavailable),
        };
        Self {
            store,
            state: Arc::new(Mutex::new(State {
                preferences,
                storage,
            })),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // Nothing under the lock can leave the state half changed: it is replaced whole
        // after the file was.
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
    fn view(state: &State) -> SettingsView {
        SettingsView {
            schema_version: VIEW_SCHEMA_VERSION,
            storage: state.storage,
            preferences: state.preferences,
            about: About::of_build(),
        }
    }

    /// The preferences in force. Reads memory, not the disk: nothing polls a file.
    pub fn read(&self) -> SettingsView {
        Self::view(&self.lock())
    }

    /// Replaces the preferences with `next`, saved first and in force after.
    pub fn update(&self, next: Preferences) -> Result<SettingsView, SettingsError> {
        next.validate()
            .map_err(|_| SettingsError::new(SettingsFailure::Invalid))?;
        let mut state = self.lock();
        if state.storage == Storage::Newer {
            return Err(SettingsError::new(SettingsFailure::ReadOnly));
        }
        // A set already in force and already saved is not written again.
        if state.storage == Storage::Ready && state.preferences == next {
            return Ok(Self::view(&state));
        }
        let store = self
            .store
            .as_ref()
            .ok_or(SettingsError::new(SettingsFailure::NotSaved))?;
        store
            .save(&next)
            .map_err(|_: StoreError| SettingsError::new(SettingsFailure::NotSaved))?;
        state.preferences = next;
        state.storage = Storage::Ready;
        Ok(Self::view(&state))
    }

    /// Back to the defaults. Only the preference file (and the one set aside) is removed:
    /// sessions, history, the authority and its state are not here to be touched.
    pub fn reset(&self) -> Result<SettingsView, SettingsError> {
        let mut state = self.lock();
        if let Some(store) = &self.store {
            store
                .reset()
                .map_err(|_| SettingsError::new(SettingsFailure::NotSaved))?;
            state.storage = Storage::Ready;
        } else {
            state.storage = Storage::Unavailable;
        }
        state.preferences = Preferences::default();
        Ok(Self::view(&state))
    }
}
