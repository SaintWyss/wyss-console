//! Resolves built-in templates and versioned profile files referenced by saved sessions.
//! Profile files are read-only.
use super::{
    profile::{Body, MAX_DEPTH, ProfileError, Resolved, merge, templates},
    store,
};
use serde::Deserialize;
use std::{
    path::PathBuf,
    sync::{Mutex, PoisonError},
};

const MAX_FILE: u64 = 512 * 1024;

/// One saved version. The file also carries when it was saved and its hash; neither is read.
#[derive(Debug, Clone, Deserialize)]
struct Version {
    n: u32,
    body: Body,
}

#[derive(Debug, Clone, Deserialize)]
struct Stored {
    id: String,
    current: u32,
    versions: Vec<Version>,
}

impl Stored {
    fn body(&self) -> Option<&Body> {
        self.versions
            .iter()
            .find(|v| v.n == self.current)
            .map(|v| &v.body)
    }
}

pub struct ProfileStore {
    dir: PathBuf,
    guard: Mutex<()>,
}

fn id_ok(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl ProfileStore {
    pub fn open(dir: PathBuf) -> Result<Self, ProfileError> {
        store::private_dir(&dir).map_err(|_| ProfileError::StoreUnavailable)?;
        Ok(Self {
            dir,
            guard: Mutex::new(()),
        })
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    fn read(&self, id: &str) -> Option<Stored> {
        if !id_ok(id) {
            return None;
        }
        let bytes = store::read_bounded(&self.path(id), MAX_FILE)?;
        let stored: Stored = serde_json::from_slice(&bytes).ok()?;
        (stored.id == id && stored.body().is_some()).then_some(stored)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.guard.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The body of a profile and the version it is at (0 for a template).
    fn body_of(&self, id: &str) -> Option<(Body, u32)> {
        if let Some((_, body)) = templates().into_iter().find(|(t, _)| *t == id) {
            return Some((body, 0));
        }
        let stored = self.read(id)?;
        let body = stored.body()?.clone();
        // Nothing saves a profile any more, so the shape rules a save applied are applied to
        // what is read: a file changed by hand passes no word a saved one could not.
        body.validate().ok()?;
        Some((body, stored.current))
    }

    /// The profile with what it extends merged in, and its version. The chain is followed up
    /// to `MAX_DEPTH`; a cycle or a missing parent is an error.
    pub fn resolve(&self, id: &str) -> Result<(Resolved, u32), ProfileError> {
        let (_guard, ()) = (self.lock(), ());
        self.resolve_unlocked(id)
    }

    fn resolve_unlocked(&self, id: &str) -> Result<(Resolved, u32), ProfileError> {
        let mut chain: Vec<(String, Body)> = Vec::new();
        let mut seen = vec![id.to_owned()];
        let (first, version) = self.body_of(id).ok_or(ProfileError::Unknown)?;
        let mut next = first.extends.clone();
        chain.push((first.name.clone(), first));
        while let Some(parent_id) = next {
            if seen.contains(&parent_id) {
                return Err(ProfileError::ProfileCycle);
            }
            if chain.len() >= MAX_DEPTH {
                return Err(ProfileError::TooDeep);
            }
            let (parent, _) = self
                .body_of(&parent_id)
                .ok_or(ProfileError::ExtendsUnknown)?;
            seen.push(parent_id);
            next = parent.extends.clone();
            chain.push((parent.name.clone(), parent));
        }
        Ok((merge(&chain), version))
    }

    /// Writes a versioned profile fixture for the read-only store.
    #[cfg(test)]
    fn put(&self, id: &str, version: u32, body: &Body) {
        let text = serde_json::json!({
            "id": id,
            "current": version,
            "versions": [{"n": version, "saved_at": 0, "hash": "", "body": body}],
        });
        std::fs::write(self.path(id), serde_json::to_vec(&text).unwrap()).unwrap();
    }

    #[cfg(test)]
    pub(crate) fn create(&self, body: Body) -> Result<String, ProfileError> {
        let id = uuid::Uuid::new_v4().simple().to_string();
        self.put(&id, 1, &body);
        Ok(id)
    }

    #[cfg(test)]
    pub(crate) fn save(&self, id: &str, body: Body) -> Result<u32, ProfileError> {
        let next = self.read(id).ok_or(ProfileError::Unknown)?.current + 1;
        self.put(id, next, &body);
        Ok(next)
    }
}
