//! M9-only Native session records. Older M1-M6 binaries deserialize `AgentId` as a closed
//! two-variant enum and rewrite an unreadable registry as empty, so new provider IDs must live
//! outside `registry.json`. This sidecar is ignored by those binaries and never stores output.
use super::{
    registry::{Session, State},
    store,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
};

const MAX_FILE: u64 = 2 * 1024 * 1024;
const MAX_SESSIONS: usize = 200;
const MAX_RECOVERY_FILES: usize = 16;
const MAX_RECOVERY_ENTRIES: usize = MAX_RECOVERY_FILES;
const MAX_DEFAULT_OVERRIDE_ENTRIES: usize =
    MAX_RECOVERY_FILES * super::registry::MAX_PROJECTS + 2 * super::registry::MAX_PROJECTS;
const FILE_NAME: &str = "m9-native-sessions-v1.json";
const RECOVERY_DIR: &str = "m9-native-recovery-v1";
const DEFAULT_OVERRIDES_FILE: &str = "m9-native-default-overrides-v2.json";
const LEGACY_DEFAULT_OVERRIDES_FILE: &str = "m9-native-default-overrides-v1.json";

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    schema: u32,
    sessions: Vec<Session>,
    #[serde(default)]
    project_defaults: BTreeMap<String, super::catalogue::AgentId>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DefaultOverrides {
    schema: u32,
    project_hashes: BTreeSet<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyDefaultOverrides {
    schema: u32,
    projects: BTreeSet<String>,
}

pub struct ExtendedRegistry {
    path: PathBuf,
    data: Data,
}

#[derive(Debug, Clone, Default)]
pub struct RecoverySnapshot {
    pub sessions: Vec<Session>,
    pub project_defaults: BTreeMap<String, super::catalogue::AgentId>,
}

#[derive(Debug, Default)]
pub struct RecoveryRead {
    pub snapshots: Vec<RecoverySnapshot>,
    pub incomplete: bool,
}

fn overrides_valid(data: &DefaultOverrides) -> bool {
    data.schema == 2
        && data.project_hashes.len() <= MAX_DEFAULT_OVERRIDE_ENTRIES
        && data.project_hashes.iter().all(|key| {
            key.len() == 64
                && key
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

fn legacy_overrides_valid(data: &LegacyDefaultOverrides) -> bool {
    (data.schema == 0 || data.schema == 1)
        && data.projects.len() <= super::registry::MAX_PROJECTS
        && data.projects.iter().all(|path| path.len() <= 4096)
}

fn entry_exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn override_key(project: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut digest = Sha256::new();
    digest.update(b"wyss-console:m9-default-override:v2\0");
    digest.update(project.as_bytes());
    let mut key = String::with_capacity(64);
    for byte in digest.finalize() {
        key.push(HEX[(byte >> 4) as usize] as char);
        key.push(HEX[(byte & 0x0f) as usize] as char);
    }
    key
}

fn data_valid(data: &Data) -> bool {
    (data.schema == 0 || data.schema == 1)
        && data.sessions.len() <= MAX_SESSIONS
        && data
            .sessions
            .iter()
            .all(|session| !session.agent_id.is_legacy())
        && data.project_defaults.len() <= super::registry::MAX_PROJECTS
        && data
            .project_defaults
            .iter()
            .all(|(path, agent)| path.len() <= 4096 && !agent.is_legacy())
}

fn recovery_data(dir: &Path, suffix: &str) -> io::Result<(PathBuf, Data)> {
    let recovery_dir = dir.join(RECOVERY_DIR);
    store::private_dir(&recovery_dir)?;
    let path = recovery_dir.join(format!("{suffix}.json"));
    let data = if path.exists() {
        let bytes = store::read_bounded(&path, MAX_FILE)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "m9_recovery_unreadable"))?;
        serde_json::from_slice::<Data>(&bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "m9_recovery_invalid"))?
    } else {
        Data {
            schema: 1,
            ..Data::default()
        }
    };
    if !data_valid(&data) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "m9_recovery_schema",
        ));
    }
    Ok((path, data))
}

/// Enumerates at most the recovery limit plus one directory entry. Unexpected non-JSON files
/// count toward the bound too, so a damaged directory cannot force an unbounded allocation or
/// scan. Overflow is incomplete recovery, never evidence that the directory is clean.
fn recovery_paths(dir: &Path) -> io::Result<(Vec<PathBuf>, bool, usize)> {
    let mut entries = fs::read_dir(dir)?;
    let mut paths = Vec::with_capacity(MAX_RECOVERY_FILES);
    let mut entry_count = 0;
    for _ in 0..MAX_RECOVERY_ENTRIES {
        let Some(entry) = entries.next() else {
            return Ok((paths, false, entry_count));
        };
        let path = entry?.path();
        entry_count += 1;
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            paths.push(path);
        }
    }
    // Probe one more entry only to distinguish exactly-at-limit from overflow.
    if entries.next().transpose()?.is_some() {
        return Ok((paths, true, entry_count + 1));
    }
    Ok((paths, false, entry_count))
}

impl ExtendedRegistry {
    pub fn open(dir: PathBuf) -> io::Result<Self> {
        store::private_dir(&dir)?;
        let path = dir.join(FILE_NAME);
        let mut data = if path.exists() {
            let bytes = store::read_bounded(&path, MAX_FILE).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "m9_registry_unreadable")
            })?;
            serde_json::from_slice::<Data>(&bytes)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "m9_registry_invalid"))?
        } else {
            Data::default()
        };
        if !data_valid(&data) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "m9_registry_schema",
            ));
        }
        data.schema = 1;
        for session in &mut data.sessions {
            if !session.state.finished() {
                session.state = State::Lost;
                session.ended_at = None;
            }
        }
        let registry = Self { path, data };
        registry.save()?;
        Ok(registry)
    }

    pub fn sessions(&self) -> &[Session] {
        &self.data.sessions
    }

    pub fn project_default(&self, path: &str) -> Option<super::catalogue::AgentId> {
        self.data.project_defaults.get(path).copied()
    }

    pub fn recovery_snapshot(&self) -> RecoverySnapshot {
        RecoverySnapshot {
            sessions: self.data.sessions.clone(),
            project_defaults: self.data.project_defaults.clone(),
        }
    }

    pub fn default_override_key(project: &str) -> String {
        override_key(project)
    }

    fn default_overrides(dir: &Path) -> io::Result<(PathBuf, DefaultOverrides)> {
        store::private_dir(dir)?;
        let path = dir.join(DEFAULT_OVERRIDES_FILE);
        let legacy_path = dir.join(LEGACY_DEFAULT_OVERRIDES_FILE);
        let mut data = if entry_exists(&path)? {
            let bytes = store::read_bounded(&path, MAX_FILE).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "m9_default_overrides_unreadable",
                )
            })?;
            serde_json::from_slice::<DefaultOverrides>(&bytes).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "m9_default_overrides_invalid")
            })?
        } else {
            DefaultOverrides {
                schema: 2,
                ..DefaultOverrides::default()
            }
        };
        if !overrides_valid(&data) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "m9_default_overrides_schema",
            ));
        }
        if entry_exists(&legacy_path)? {
            let bytes = store::read_bounded(&legacy_path, MAX_FILE).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "m9_legacy_default_overrides_unreadable",
                )
            })?;
            let legacy =
                serde_json::from_slice::<LegacyDefaultOverrides>(&bytes).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "m9_legacy_default_overrides_invalid",
                    )
                })?;
            if !legacy_overrides_valid(&legacy) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "m9_legacy_default_overrides_schema",
                ));
            }
            for project in legacy.projects {
                data.project_hashes.insert(override_key(&project));
            }
            if data.project_hashes.len() > MAX_DEFAULT_OVERRIDE_ENTRIES {
                return Err(io::Error::new(
                    io::ErrorKind::StorageFull,
                    "m9_default_overrides_full",
                ));
            }
            data.schema = 2;
            let migrated = serde_json::to_vec_pretty(&data).map_err(io::Error::other)?;
            // The v2 image is durable before the plaintext v1 paths are unlinked. Any
            // conversion or unlink failure refuses migration and leaves a recoverable copy.
            store::write_atomic(&path, &migrated)?;
            fs::remove_file(&legacy_path)?;
            fs::File::open(dir)?.sync_all()?;
        }
        data.schema = 2;
        Ok((path, data))
    }

    /// A durable tombstone records that an explicit legacy-provider choice superseded a
    /// recovered M9 project default while the primary M9 sidecar was unavailable. The old
    /// recovery snapshot remains byte-for-byte intact until a healthy sidecar has consumed it.
    /// Returns whether this project already had a tombstone before this call.
    pub fn mark_default_overridden(dir: &Path, project: &str) -> io::Result<bool> {
        if project.is_empty() || project.len() > 4096 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "m9_default_override_project",
            ));
        }
        let (path, mut data) = Self::default_overrides(dir)?;
        let key = override_key(project);
        let existed = data.project_hashes.contains(&key);
        if !existed && data.project_hashes.len() >= MAX_DEFAULT_OVERRIDE_ENTRIES {
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                "m9_default_overrides_full",
            ));
        }
        data.project_hashes.insert(key);
        let bytes = serde_json::to_vec_pretty(&data).map_err(io::Error::other)?;
        store::write_atomic(&path, &bytes)?;
        Ok(existed)
    }

    /// Undoes a newly written tombstone if the provider failed to start. Existing tombstones
    /// remain in place. The caller supplies the prior membership observed before launch.
    pub fn restore_default_override(dir: &Path, project: &str, existed: bool) -> io::Result<()> {
        if existed {
            return Ok(());
        }
        let (path, mut data) = Self::default_overrides(dir)?;
        if !data.project_hashes.remove(&override_key(project)) {
            return Ok(());
        }
        if data.project_hashes.is_empty() {
            match fs::remove_file(path) {
                Ok(()) => fs::File::open(dir)?.sync_all(),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            }
        } else {
            let bytes = serde_json::to_vec_pretty(&data).map_err(io::Error::other)?;
            store::write_atomic(&path, &bytes)
        }
    }

    pub fn read_default_overrides(dir: &Path) -> io::Result<BTreeSet<String>> {
        let (_, data) = Self::default_overrides(dir)?;
        Ok(data.project_hashes)
    }

    /// Removes tombstones only after recovered metadata has been migrated and the primary
    /// registry is durable. A damaged override file remains untouched for manual recovery.
    pub fn clear_default_overrides(dir: &Path) -> io::Result<()> {
        let (path, _) = Self::default_overrides(dir)?;
        match fs::remove_file(path) {
            Ok(()) => fs::File::open(dir)?.sync_all(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub fn set_project_default(
        &mut self,
        path: &str,
        agent: super::catalogue::AgentId,
    ) -> io::Result<()> {
        if agent.is_legacy() || path.len() > 4096 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "m9_registry_project_default",
            ));
        }
        if !self.data.project_defaults.contains_key(path)
            && self.data.project_defaults.len() >= super::registry::MAX_PROJECTS
        {
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                "m9_registry_project_defaults_full",
            ));
        }
        self.data.project_defaults.insert(path.to_owned(), agent);
        self.save()
    }

    /// Removes an M9 default when a newer Native provider supersedes it. The old registry
    /// cannot contain an M9 AgentId, so leaving this value here would keep shadowing Claude or
    /// Codex after the user starts one of them.
    pub fn clear_project_default(
        &mut self,
        path: &str,
    ) -> io::Result<Option<super::catalogue::AgentId>> {
        let previous = self.data.project_defaults.remove(path);
        if let Err(error) = self.save() {
            if let Some(agent) = previous {
                self.data.project_defaults.insert(path.to_owned(), agent);
            }
            return Err(error);
        }
        Ok(previous)
    }

    /// Persist M9 records extracted from an earlier shared-registry build before the common
    /// registry is rewritten as an M1-M6-safe projection. Recovery snapshots are never loaded
    /// as writable state while the primary sidecar is corrupt; a later healthy start migrates
    /// them idempotently.
    pub fn preserve_recovery(
        dir: &Path,
        sessions: &[Session],
        defaults: &[(String, super::catalogue::AgentId)],
    ) -> io::Result<RecoverySnapshot> {
        let recovery_dir = dir.join(RECOVERY_DIR);
        store::private_dir(&recovery_dir)?;
        let (mut files, overflow, entry_count) = recovery_paths(&recovery_dir)?;
        files.sort();
        if overflow || entry_count >= MAX_RECOVERY_ENTRIES {
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                "m9_recovery_full",
            ));
        }
        let suffix = format!("recovery-{}", uuid::Uuid::new_v4().simple());
        let (path, mut data) = recovery_data(dir, &suffix)?;
        for session in sessions {
            if session.agent_id.is_legacy() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "m9_recovery_agent",
                ));
            }
            if data
                .sessions
                .iter()
                .all(|existing| existing.id != session.id)
            {
                data.sessions.push(session.clone());
            }
        }
        for (project, agent) in defaults {
            if agent.is_legacy() || project.len() > 4096 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "m9_recovery_default",
                ));
            }
            data.project_defaults
                .entry(project.clone())
                .or_insert(*agent);
        }
        if !data_valid(&data) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "m9_recovery_schema",
            ));
        }
        let bytes = serde_json::to_vec_pretty(&data).map_err(io::Error::other)?;
        store::write_atomic(&path, &bytes)?;
        Ok(RecoverySnapshot {
            sessions: data.sessions,
            project_defaults: data.project_defaults,
        })
    }

    pub fn read_recovery(dir: &Path) -> io::Result<RecoveryRead> {
        let recovery_dir = dir.join(RECOVERY_DIR);
        if !recovery_dir.exists() {
            return Ok(RecoveryRead::default());
        }
        store::private_dir(&recovery_dir)?;
        let (mut files, overflow, _) = recovery_paths(&recovery_dir)?;
        files.sort();
        let mut recovery = RecoveryRead {
            snapshots: Vec::new(),
            incomplete: overflow,
        };
        for path in files.into_iter().take(MAX_RECOVERY_FILES) {
            let Some(bytes) = store::read_bounded(&path, MAX_FILE) else {
                recovery.incomplete = true;
                continue;
            };
            let Ok(data) = serde_json::from_slice::<Data>(&bytes) else {
                recovery.incomplete = true;
                continue;
            };
            if !data_valid(&data) {
                recovery.incomplete = true;
                continue;
            }
            recovery.snapshots.push(RecoverySnapshot {
                sessions: data.sessions,
                project_defaults: data.project_defaults,
            });
        }
        Ok(recovery)
    }

    /// Removes valid snapshots only after their contents have been copied into and durably
    /// saved by the primary registry. Invalid files are left untouched for manual recovery.
    pub fn clear_recovery(dir: &Path) -> io::Result<()> {
        let recovery_dir = dir.join(RECOVERY_DIR);
        if !recovery_dir.exists() {
            return Ok(());
        }
        store::private_dir(&recovery_dir)?;
        let (files, overflow, _) = recovery_paths(&recovery_dir)?;
        if overflow {
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                "m9_recovery_full",
            ));
        }
        let mut removed = false;
        for path in files {
            let Some(bytes) = store::read_bounded(&path, MAX_FILE) else {
                continue;
            };
            let Ok(data) = serde_json::from_slice::<Data>(&bytes) else {
                continue;
            };
            if data_valid(&data) {
                fs::remove_file(path)?;
                removed = true;
            }
        }
        if removed {
            fs::File::open(recovery_dir)?.sync_all()?;
        }
        Ok(())
    }

    pub fn migrate_session(&mut self, session: Session) -> io::Result<()> {
        if session.agent_id.is_legacy() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "m9_registry_agent",
            ));
        }
        if self
            .data
            .sessions
            .iter()
            .any(|existing| existing.id == session.id)
        {
            return Ok(());
        }
        self.add(session)
    }

    pub fn migrate_project_default(
        &mut self,
        path: String,
        agent: super::catalogue::AgentId,
    ) -> io::Result<()> {
        if agent.is_legacy() || path.len() > 4096 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "m9_registry_project_default",
            ));
        }
        if !self.data.project_defaults.contains_key(&path)
            && self.data.project_defaults.len() >= super::registry::MAX_PROJECTS
        {
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                "m9_registry_project_defaults_full",
            ));
        }
        self.data.project_defaults.entry(path).or_insert(agent);
        self.save()
    }

    pub fn session(&self, id: &str) -> Option<&Session> {
        self.data.sessions.iter().find(|session| session.id == id)
    }

    pub fn session_mut(&mut self, id: &str) -> Option<&mut Session> {
        self.data
            .sessions
            .iter_mut()
            .find(|session| session.id == id)
    }

    pub fn add(&mut self, session: Session) -> io::Result<()> {
        if session.agent_id.is_legacy() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "m9_registry_agent",
            ));
        }
        if self
            .data
            .sessions
            .iter()
            .any(|existing| existing.id == session.id)
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "m9_registry_session_duplicate",
            ));
        }
        self.data.sessions.push(session);
        while self.data.sessions.len() > MAX_SESSIONS {
            let Some(oldest) = self.data.sessions.iter().position(|s| s.state.finished()) else {
                self.data.sessions.pop();
                return Err(io::Error::new(
                    io::ErrorKind::StorageFull,
                    "m9_registry_full",
                ));
            };
            self.data.sessions.remove(oldest);
        }
        self.save()
    }

    pub fn forget(&mut self, id: &str) -> bool {
        let before = self.data.sessions.len();
        self.data.sessions.retain(|session| session.id != id);
        before != self.data.sessions.len()
    }

    pub fn save(&self) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(&self.data).map_err(io::Error::other)?;
        store::write_atomic(&self.path, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_directory_entry_count_is_bounded_and_overflow_fails_closed() {
        let root = std::env::temp_dir().join(format!(
            "wyss-m9-recovery-bound-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let directory = root.join(RECOVERY_DIR);
        store::private_dir(&directory).unwrap();
        for index in 0..=MAX_RECOVERY_FILES {
            fs::write(directory.join(format!("unexpected-{index}")), b"x").unwrap();
        }

        let recovery = ExtendedRegistry::read_recovery(&root).unwrap();
        assert!(recovery.incomplete);
        assert!(recovery.snapshots.is_empty());
        assert_eq!(
            ExtendedRegistry::preserve_recovery(&root, &[], &[])
                .unwrap_err()
                .kind(),
            io::ErrorKind::StorageFull
        );
        assert_eq!(
            ExtendedRegistry::clear_recovery(&root).unwrap_err().kind(),
            io::ErrorKind::StorageFull
        );
        assert_eq!(
            fs::read_dir(&directory).unwrap().count(),
            MAX_RECOVERY_FILES + 1
        );
        fs::remove_dir_all(&directory).unwrap();
        store::private_dir(&directory).unwrap();
        for index in 0..MAX_RECOVERY_FILES {
            fs::write(directory.join(format!("unexpected-{index}")), b"x").unwrap();
        }
        let at_limit = ExtendedRegistry::read_recovery(&root).unwrap();
        assert!(!at_limit.incomplete);
        assert!(at_limit.snapshots.is_empty());
        assert_eq!(
            ExtendedRegistry::preserve_recovery(&root, &[], &[])
                .unwrap_err()
                .kind(),
            io::ErrorKind::StorageFull
        );
        ExtendedRegistry::clear_recovery(&root).unwrap();
        assert_eq!(
            fs::read_dir(&directory).unwrap().count(),
            MAX_RECOVERY_FILES
        );
        fs::remove_dir_all(root).unwrap();
    }
}
