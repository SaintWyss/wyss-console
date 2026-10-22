//! The production runtime spawner: Codex itself, in the isolated local-tools namespace.
//!
//! Nothing here is new machinery. It joins what protocol 2 already has and tests: the host-only
//! `NativeLocalToolsPlan` (sealed executable, catalogue and policy images; owned grants; the
//! bubblewrap namespace; a version probe of the very image that is run), the attested native
//! worker and the engine's per-launch staging. What it decides is only where each input of
//! `NativeLocalToolsPlan::prepare` comes from, and it decides it **from the authority's own
//! command line, never from a client**:
//!
//! - the executable is the one path the operator named (`--codex`); it must report exactly
//!   `codex-cli 0.159.0` when the plan is prepared;
//! - the user's Codex home is the one directory the operator named (`--codex-home`). Its
//!   `models_cache.json` is the only thing read from it besides its credential. **A credential
//!   in that directory is a capability that spends the user's provider quota, so a start that
//!   finds one is refused unless the operator said `--allow-credential` as well**;
//! - the workspace is the canonical folder the start asked for, which the engine validated;
//! - the catalogue is the user's own model cache, clamped by `safety::local_tools_catalog`
//!   (every tool selector pinned off except the patch tool) and written to this launch's
//!   private staging directory: a launch never sees another's catalogue.
//!
//! A start without a workspace (the inference profile) is refused: only the local-tools
//! profile has a containment this authority attests, and an inference runtime would have no
//! folder to bind. The plan is prepared synchronously, once per start, on the authority's
//! thread: it seals the executable (a copy of a few tens of MB from the page cache) and runs a
//! bounded `--version` probe, a few hundred milliseconds at most per runtime start. With one
//! runtime at a time that delays only the start it belongs to.
use super::{
    engine::{Spawned, Spawner},
    launcher::{NativeLocalToolsPlan, NativeLocalToolsSpawner},
    protocol::RuntimeRef,
    staging::{LaunchStaging, StagingRoot},
};
use crate::{
    adapters::codex::{probe::isolation::Isolation, safety},
    contracts::control::{ControlError as E, ControlResult, RuntimeSafetyProfile},
};
use std::{
    fs,
    io::Read,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

/// The Codex build this authority is written against. A plan refuses any other.
pub const CODEX_VERSION: &str = "0.159.0";
const CATALOGUE: &str = "catalog.json";

/// What the operator named, all of it explicit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexConfig {
    pub executable: PathBuf,
    pub home: PathBuf,
    /// The operator accepts that a credential in `home` is bound into the namespace.
    pub allow_credential: bool,
}

impl CodexConfig {
    /// What can be refused without creating anything: the choices are absolute, the home is a
    /// directory of this user, and a credential in it is allowed by name.
    pub fn check(&self) -> ControlResult<PathBuf> {
        if !self.executable.is_absolute() || !self.home.is_absolute() {
            return Err(E::UnsafePath);
        }
        let home = fs::canonicalize(&self.home).map_err(|_| E::UnsafePath)?;
        let meta = fs::metadata(&home).map_err(|_| E::UnsafePath)?;
        if !meta.is_dir() || meta.uid() != nix::unistd::geteuid().as_raw() {
            return Err(E::UnsafePath);
        }
        // A credential is the one thing that can spend the user's provider quota.
        if fs::symlink_metadata(home.join("auth.json")).is_ok() && !self.allow_credential {
            return Err(E::GateDenied);
        }
        Ok(home)
    }
}

pub struct CodexSpawner {
    config: CodexConfig,
    /// The authority's private product directories: hidden from the namespace.
    runtime: PathBuf,
    state: PathBuf,
    staging: PathBuf,
    cwd: PathBuf,
}
impl CodexSpawner {
    /// Checks the operator's choices and creates what the spawner owns: the staging root of
    /// the launches and the empty directory a runtime starts in. Returns the staging root for
    /// the engine. `runtime` and `state` are the authority's product directories.
    pub fn new(
        config: CodexConfig,
        runtime: &Path,
        state: &Path,
    ) -> ControlResult<(Self, StagingRoot)> {
        let home = config.check()?;
        let staging = state.join("staging");
        let cwd = runtime.join("runtime-cwd");
        for dir in [&staging, &cwd] {
            match fs::DirBuilder::new().mode(0o700).create(dir) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(_) => return Err(E::UnsafePath),
            }
        }
        // A runtime starts in an empty directory of the authority's own, or not at all.
        let empty = fs::symlink_metadata(&cwd).ok().filter(|m| {
            m.is_dir() && m.uid() == nix::unistd::geteuid().as_raw() && m.mode() & 0o777 == 0o700
        });
        if empty.is_none()
            || fs::read_dir(&cwd)
                .map_err(|_| E::UnsafePath)?
                .next()
                .is_some()
        {
            return Err(E::UnsafePath);
        }
        let root = StagingRoot::open(&staging)?;
        Ok((
            Self {
                config: CodexConfig { home, ..config },
                runtime: runtime.to_owned(),
                state: state.to_owned(),
                staging,
                cwd,
            },
            root,
        ))
    }

    /// The user's cached model listing: a regular file of this user, not a link, bounded.
    fn cache(&self) -> ControlResult<Vec<u8>> {
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(self.config.home.join("models_cache.json"))
            .map_err(|_| E::Unsupported)?;
        let meta = file.metadata().map_err(|_| E::Unsupported)?;
        if !meta.is_file()
            || meta.uid() != nix::unistd::geteuid().as_raw()
            || meta.len() > safety::MAX_CATALOG_BYTES as u64
        {
            return Err(E::Unsupported);
        }
        let mut raw = Vec::new();
        file.take(safety::MAX_CATALOG_BYTES as u64 + 1)
            .read_to_end(&mut raw)
            .map_err(|_| E::Unsupported)?;
        Ok(raw)
    }

    /// Everything one local-tools launch needs, prepared for `workspace`.
    fn plan(
        &self,
        reference: &RuntimeRef,
        staging: &mut LaunchStaging,
        workspace: &str,
    ) -> ControlResult<NativeLocalToolsPlan> {
        // The gate is not a startup check only: a credential that appeared since (`codex login`
        // after the authority started) is refused at the start that would bind it.
        self.config.check()?;
        let catalogue = safety::local_tools_catalog(&self.cache()?).map_err(|_| E::Unsupported)?;
        staging.write_file(CATALOGUE, &catalogue)?;
        let catalogue = self
            .staging
            .join(reference.runtime_instance_id.as_str())
            .join(CATALOGUE);
        if !safety::safe_catalog_path(&catalogue) {
            return Err(E::UnsafePath);
        }
        let isolation = Isolation {
            workspace: PathBuf::from(workspace),
            state: self.state.join("codex-local-tools"),
            control: vec![self.runtime.clone(), self.state.clone()],
        };
        NativeLocalToolsPlan::prepare(
            isolation,
            &self.config.executable,
            &self.config.home,
            &self.cwd,
            catalogue,
            CODEX_VERSION,
        )
    }
}
impl Spawner for CodexSpawner {
    fn supports_profile(&self, profile: RuntimeSafetyProfile) -> bool {
        profile == RuntimeSafetyProfile::CodexLocalToolsV1
    }
    fn descendant_containment(&self) -> bool {
        true
    }
    fn spawn(&mut self, _: &RuntimeRef, _: Option<&mut LaunchStaging>) -> ControlResult<Spawned> {
        // No workspace: nothing to isolate this launch for.
        Err(E::Unsupported)
    }
    fn spawn_for(
        &mut self,
        launch: &RuntimeRef,
        staging: Option<&mut LaunchStaging>,
        workspace: Option<&str>,
    ) -> ControlResult<Spawned> {
        let (Some(staging), Some(workspace)) = (staging, workspace) else {
            return Err(E::Unsupported);
        };
        let plan = self.plan(launch, staging, workspace)?;
        NativeLocalToolsSpawner::new(plan).spawn(launch, None)
    }
}

#[cfg(test)]
mod tests;
