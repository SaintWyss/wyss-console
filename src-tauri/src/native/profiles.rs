//! The profile store (09 §4.4, §4.5): one file per profile holding its versions, a pointer to
//! the current one, at most 20 versions kept; templates ship with the product and are read
//! only. Export and import move one profile as one file that carries no secret and grants
//! nothing.
use super::{
    profile::{Body, MAX_DEPTH, ProfileError, Resolved, merge, templates},
    store,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    sync::{Mutex, PoisonError},
};

pub const MAX_VERSIONS: usize = 20;
pub const MAX_PROFILES: usize = 100;
const MAX_FILE: u64 = 512 * 1024;
const MAX_IMPORT: u64 = 16 * 1024;
const FORMAT: &str = "wyss-session-profile";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Version {
    n: u32,
    saved_at: u64,
    hash: String,
    body: Body,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize)]
pub struct VersionInfo {
    pub n: u32,
    pub saved_at: u64,
    pub hash: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub id: String,
    pub template: bool,
    pub current: u32,
    pub versions: Vec<VersionInfo>,
    pub body: Body,
    /// Which profile each inherited value came from (empty without a parent).
    pub from: std::collections::BTreeMap<&'static str, String>,
    pub refers_to_missing: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Export {
    format: String,
    schema: u32,
    body: Body,
}

pub fn hash(body: &Body) -> String {
    let bytes = serde_json::to_vec(body).unwrap_or_default();
    let digest = Sha256::digest(bytes);
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
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

    fn write(&self, stored: &Stored) -> Result<(), ProfileError> {
        let bytes =
            serde_json::to_vec_pretty(stored).map_err(|_| ProfileError::StoreUnavailable)?;
        store::write_atomic(&self.path(&stored.id), &bytes)
            .map_err(|_| ProfileError::StoreUnavailable)
    }

    fn ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let id = name.strip_suffix(".json")?;
                id_ok(id).then(|| id.to_owned())
            })
            .collect();
        ids.sort();
        ids
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

    pub fn list(&self) -> Vec<Summary> {
        let _guard = self.lock();
        let mut out = Vec::new();
        let mut push =
            |id: String, template: bool, current: u32, versions: Vec<VersionInfo>, body: Body| {
                let resolved = self.resolve_unlocked(&id);
                let (from, missing) = match &resolved {
                    Ok((resolved, _)) => (resolved.from.clone(), false),
                    Err(_) => (Default::default(), true),
                };
                out.push(Summary {
                    id,
                    template,
                    current,
                    versions,
                    body,
                    from,
                    refers_to_missing: missing,
                });
            };
        for (id, body) in templates() {
            push(id.to_owned(), true, 0, Vec::new(), body);
        }
        for id in self.ids() {
            if let Some(stored) = self.read(&id) {
                let body = stored.body().cloned().unwrap_or_default();
                let versions = stored
                    .versions
                    .iter()
                    .map(|v| VersionInfo {
                        n: v.n,
                        saved_at: v.saved_at,
                        hash: v.hash.clone(),
                        name: v.body.name.clone(),
                    })
                    .collect();
                push(id, false, stored.current, versions, body);
            }
        }
        out
    }

    /// The parent must exist, and following it must not come back to `id` nor go deeper
    /// than `MAX_DEPTH`.
    fn check_extends(&self, id: &str, body: &Body) -> Result<(), ProfileError> {
        let mut seen = vec![id.to_owned()];
        let mut next = body.extends.clone();
        let mut depth = 1;
        while let Some(parent_id) = next {
            if seen.contains(&parent_id) {
                return Err(ProfileError::ProfileCycle);
            }
            depth += 1;
            if depth > MAX_DEPTH {
                return Err(ProfileError::TooDeep);
            }
            let (parent, _) = self
                .body_of(&parent_id)
                .ok_or(ProfileError::ExtendsUnknown)?;
            seen.push(parent_id);
            next = parent.extends;
        }
        Ok(())
    }

    pub fn create(&self, body: Body) -> Result<String, ProfileError> {
        body.validate()?;
        let _guard = self.lock();
        if self.ids().len() >= MAX_PROFILES {
            return Err(ProfileError::TooMany);
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        self.check_extends(&id, &body)?;
        let stored = Stored {
            id: id.clone(),
            current: 1,
            versions: vec![Version {
                n: 1,
                saved_at: store::now(),
                hash: hash(&body),
                body,
            }],
        };
        self.write(&stored)?;
        Ok(id)
    }

    /// A save that changes nothing makes no new version.
    pub fn save(&self, id: &str, body: Body) -> Result<u32, ProfileError> {
        if templates().iter().any(|(t, _)| *t == id) {
            return Err(ProfileError::ReadOnlyTemplate);
        }
        body.validate()?;
        let _guard = self.lock();
        let mut stored = self.read(id).ok_or(ProfileError::Unknown)?;
        self.check_extends(id, &body)?;
        let digest = hash(&body);
        if stored.body().is_some_and(|b| hash(b) == digest) {
            return Ok(stored.current);
        }
        let next = stored.versions.iter().map(|v| v.n).max().unwrap_or(0) + 1;
        stored.versions.push(Version {
            n: next,
            saved_at: store::now(),
            hash: digest,
            body,
        });
        while stored.versions.len() > MAX_VERSIONS {
            // The oldest that is not the current one.
            let at = stored
                .versions
                .iter()
                .position(|v| v.n != next)
                .unwrap_or(0);
            stored.versions.remove(at);
        }
        stored.current = next;
        self.write(&stored)?;
        Ok(next)
    }

    /// Moves the pointer to an earlier version. Nothing is deleted.
    pub fn restore(&self, id: &str, n: u32) -> Result<(), ProfileError> {
        let _guard = self.lock();
        let mut stored = self.read(id).ok_or(ProfileError::Unknown)?;
        let body = stored
            .versions
            .iter()
            .find(|v| v.n == n)
            .map(|v| v.body.clone())
            .ok_or(ProfileError::VersionUnknown)?;
        self.check_extends(id, &body)?;
        stored.current = n;
        self.write(&stored)
    }

    pub fn delete(&self, id: &str) -> Result<(), ProfileError> {
        if templates().iter().any(|(t, _)| *t == id) {
            return Err(ProfileError::ReadOnlyTemplate);
        }
        let _guard = self.lock();
        if self.read(id).is_none() {
            return Err(ProfileError::Unknown);
        }
        // A profile another one extends is not removed from under it.
        for other in self.ids() {
            if other != id
                && self
                    .read(&other)
                    .and_then(|s| s.body().and_then(|b| b.extends.clone()))
                    .as_deref()
                    == Some(id)
            {
                return Err(ProfileError::InUse);
            }
        }
        fs::remove_file(self.path(id)).map_err(|_| ProfileError::StoreUnavailable)
    }

    /// A copy under a new name; of a template too, which is how a template is made editable.
    pub fn duplicate(&self, id: &str) -> Result<String, ProfileError> {
        let (mut body, _) = self.body_of(id).ok_or(ProfileError::Unknown)?;
        body.name = format!("{} copy", body.name.chars().take(50).collect::<String>());
        self.create(body)
    }

    pub fn export(&self, id: &str) -> Result<Vec<u8>, ProfileError> {
        let _guard = self.lock();
        let (mut body, _) = self.body_of(id).ok_or(ProfileError::Unknown)?;
        // A parent is a local id: it means nothing on another computer.
        body.extends = None;
        let export = Export {
            format: FORMAT.to_owned(),
            schema: 1,
            body,
        };
        serde_json::to_vec_pretty(&export).map_err(|_| ProfileError::StoreUnavailable)
    }

    /// Validates the file, drops what it should not carry (a parent id), keeps the name
    /// unique by a suffix, and stores it as a new profile. It never overwrites, and it asks
    /// for nothing: a profile is a description.
    pub fn import(&self, bytes: &[u8]) -> Result<String, ProfileError> {
        if bytes.len() as u64 > MAX_IMPORT {
            return Err(ProfileError::ImportInvalid);
        }
        let export: Export =
            serde_json::from_slice(bytes).map_err(|_| ProfileError::ImportInvalid)?;
        if export.format != FORMAT || export.schema != 1 {
            return Err(ProfileError::ImportInvalid);
        }
        let mut body = export.body;
        body.extends = None;
        body.validate().map_err(|_| ProfileError::ImportInvalid)?;
        let names: Vec<String> = self.list().into_iter().map(|s| s.body.name).collect();
        let original = body.name.clone();
        let mut n = 1;
        while names.contains(&body.name) {
            n += 1;
            body.name = format!("{} ({n})", original.chars().take(50).collect::<String>());
        }
        self.create(body)
    }
}
