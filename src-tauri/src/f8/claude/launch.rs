//! Verifying and starting the supervised bridge. Everything the process is made of is named by
//! the authority's own operator, never by a client, and every name is checked before a process
//! exists:
//!
//! - the SDK installation tree (a Python virtual environment the operator built from the pinned
//!   distribution) is checked against pip's own `RECORD` files: every listed file must hash as
//!   recorded and **no file may exist that is not listed**, which is what a planted `.pth` file
//!   or module would be. The bundled CLI is also pinned by SHA-256 (`adapters::claude::pins`);
//! - the bridge sources are compiled into this binary and written to a private directory,
//!   so what runs is exactly what was reviewed;
//! - an administrator's managed policy for Claude Code (`/etc/claude-code`) is never hidden or
//!   overridden: its mere presence makes the runtime unavailable, because it can add hooks and
//!   MCP servers that no option of the SDK can turn off (probe matrix, `managed` cases);
//! - the credential, if any, is a file of this user, mode 0600, read once and handed to the
//!   bridge in memory. It is never in an environment, a command line or a log.
use super::{
    exec::{ActionExecutor, SandboxExecutor},
    sandbox::{Sandbox, path_is_plain},
    worker::{ClaudeConfig, ClaudeWorker, Identity},
};
use crate::{
    adapters::claude::{Profile, frames::ApiKey, pins},
    contracts::control::{ControlError as E, ControlResult},
    f8::{
        engine::StopState,
        ownership::{ContainmentPolicy, Grace},
        process_worker::DeferredProcessWorker,
    },
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::Instant,
};

pub const BRIDGE_FILES: [(&str, &str); 3] = [
    (
        "api_tap.py",
        include_str!("../../../claude-bridge/api_tap.py"),
    ),
    (
        "bridge.py",
        include_str!("../../../claude-bridge/bridge.py"),
    ),
    (
        "profiles.py",
        include_str!("../../../claude-bridge/profiles.py"),
    ),
];
pub const POLICY_DIR: &str = "/etc/claude-code";
/// Where the provider process sees its own directories.
pub const SANDBOX_HOME: &str = "/home/wyss";
pub const SANDBOX_CWD: &str = "/run/wyss/cwd";

/// Why a launch is not possible: stable text for the GUI's "Unverified" explanation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Unverified {
    Platform,
    NotConfigured,
    UnsafePath,
    NotAVirtualEnvironment,
    SdkVersion,
    CliVersion,
    CliDigest,
    RecordMismatch,
    UnlistedFile,
    ManagedPolicy,
    Credential,
}
impl Unverified {
    pub const fn code(self) -> &'static str {
        match self {
            Self::Platform => "platform_unsupported",
            Self::NotConfigured => "sdk_not_configured",
            Self::UnsafePath => "unsafe_path",
            Self::NotAVirtualEnvironment => "not_a_virtual_environment",
            Self::SdkVersion => "sdk_version_unsupported",
            Self::CliVersion => "cli_version_unsupported",
            Self::CliDigest => "cli_digest_mismatch",
            Self::RecordMismatch => "record_mismatch",
            Self::UnlistedFile => "unlisted_file",
            Self::ManagedPolicy => "managed_policy_present",
            Self::Credential => "credential_unusable",
        }
    }
}

/// What the operator named, all of it explicit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeInstall {
    /// The virtual environment holding `claude-agent-sdk`.
    pub sdk_root: PathBuf,
    /// A credential file: one line, mode 0600, owned by this user. Absent means none.
    pub api_key_file: Option<PathBuf>,
    /// The provider API base URL; `None` is the provider's own.
    pub upstream: Option<String>,
    /// Where an administrator's Claude Code policy would live (tests point it elsewhere).
    pub policy_dir: PathBuf,
}
impl ClaudeInstall {
    pub fn new(sdk_root: PathBuf) -> Self {
        Self {
            sdk_root,
            api_key_file: None,
            upstream: None,
            policy_dir: PathBuf::from(POLICY_DIR),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    pub sdk_root: PathBuf,
    pub python: PathBuf,
    pub sdk_version: String,
    pub cli_version: String,
    pub cli_sha256: String,
}

fn me() -> u32 {
    nix::unistd::geteuid().as_raw()
}
/// Owned by this user or root, and nobody else can write it.
fn trusted(m: &fs::Metadata) -> bool {
    (m.uid() == me() || m.uid() == 0) && m.mode() & 0o022 == 0
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
/// pip's RECORD encoding of a SHA-256: URL-safe base64 without padding.
fn b64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = c
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=c.len() {
            out.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}
fn sha256_file(path: &Path) -> std::io::Result<[u8; 32]> {
    let mut f = fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().into())
}

/// A one-line `key = value` or `name = "value"` assignment from a small Python/INI file.
fn assignment(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
    })
}

struct Tree {
    /// Every regular file under the root, relative, with its digest when it was hashed.
    seen: BTreeMap<PathBuf, Option<[u8; 32]>>,
}
fn walk(root: &Path, dir: &Path, out: &mut Tree, budget: &mut usize) -> Result<(), Unverified> {
    let entries = fs::read_dir(dir).map_err(|_| Unverified::UnsafePath)?;
    for e in entries {
        let e = e.map_err(|_| Unverified::UnsafePath)?;
        *budget = budget.checked_sub(1).ok_or(Unverified::UnsafePath)?;
        let path = e.path();
        let m = fs::symlink_metadata(&path).map_err(|_| Unverified::UnsafePath)?;
        if m.file_type().is_symlink() || !trusted(&m) {
            return Err(Unverified::UnsafePath);
        }
        if m.is_dir() {
            if path.file_name().is_some_and(|n| n == "__pycache__") {
                continue;
            }
            walk(root, &path, out, budget)?;
        } else if m.is_file() {
            let rel = path
                .strip_prefix(root)
                .map_err(|_| Unverified::UnsafePath)?
                .to_owned();
            out.seen.insert(rel, None);
        } else {
            return Err(Unverified::UnsafePath);
        }
    }
    Ok(())
}

/// The check of one `site-packages` against the `RECORD` files in it.
fn verify_site_packages(site: &Path) -> Result<BTreeMap<PathBuf, [u8; 32]>, Unverified> {
    let mut tree = Tree {
        seen: BTreeMap::new(),
    };
    let mut budget = 200_000usize;
    walk(site, site, &mut tree, &mut budget)?;
    let mut listed: BTreeSet<PathBuf> = BTreeSet::new();
    let mut digests: BTreeMap<PathBuf, [u8; 32]> = BTreeMap::new();
    let mut records = 0;
    for rel in tree.seen.keys().cloned().collect::<Vec<_>>() {
        let is_record = rel.file_name().is_some_and(|n| n == "RECORD")
            && rel
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|d| d.to_string_lossy().ends_with(".dist-info"));
        if !is_record {
            continue;
        }
        records += 1;
        let text = fs::read_to_string(site.join(&rel)).map_err(|_| Unverified::RecordMismatch)?;
        for line in text.lines() {
            // path,sha256=<digest>,size  (the path may be quoted when it contains a comma)
            let mut parts = line.rsplitn(3, ',');
            let (_size, hash, path) = (parts.next(), parts.next(), parts.next());
            let (Some(hash), Some(path)) = (hash, path) else {
                return Err(Unverified::RecordMismatch);
            };
            let path = path.trim_matches('"');
            if path.starts_with("..") || path.starts_with('/') || path.is_empty() {
                continue; // an installed script outside site-packages: not part of this tree
            }
            let rel = PathBuf::from(path);
            listed.insert(rel.clone());
            let Some(want) = hash.strip_prefix("sha256=") else {
                continue; // RECORD itself and compiled files carry no hash
            };
            let full = site.join(&rel);
            let got = sha256_file(&full).map_err(|_| Unverified::RecordMismatch)?;
            if b64(&got) != want {
                return Err(Unverified::RecordMismatch);
            }
            digests.insert(rel, got);
        }
    }
    if records == 0 {
        return Err(Unverified::RecordMismatch);
    }
    // A file that no package claims is code or configuration nobody vouched for.
    if tree.seen.keys().any(|rel| !listed.contains(rel)) {
        return Err(Unverified::UnlistedFile);
    }
    Ok(digests)
}

impl ClaudeInstall {
    /// Everything that can be refused without starting anything. Hashes the installation tree
    /// (about 300 MB, dominated by the bundled CLI), so it runs once per launch preparation.
    pub fn verify(&self) -> Result<Verified, Unverified> {
        self.verify_against(&pins::BUILDS)
    }

    /// `verify` against an explicit set of builds (tests use a synthetic one).
    pub(crate) fn verify_against(&self, builds: &[pins::BuildPin]) -> Result<Verified, Unverified> {
        if std::env::consts::OS != "linux" || std::env::consts::ARCH != "x86_64" {
            return Err(Unverified::Platform);
        }
        let root = &self.sdk_root;
        if !path_is_plain(root) || fs::canonicalize(root).ok().as_ref() != Some(root) {
            return Err(Unverified::UnsafePath);
        }
        let rm = fs::symlink_metadata(root).map_err(|_| Unverified::NotConfigured)?;
        if !rm.is_dir() || !trusted(&rm) {
            return Err(Unverified::UnsafePath);
        }
        let cfg = fs::read_to_string(root.join("pyvenv.cfg"))
            .map_err(|_| Unverified::NotAVirtualEnvironment)?;
        // System site-packages would put unverified code on the interpreter's path.
        if assignment(&cfg, "include-system-site-packages").as_deref() != Some("false") {
            return Err(Unverified::NotAVirtualEnvironment);
        }
        let python = fs::canonicalize(root.join("bin/python"))
            .map_err(|_| Unverified::NotAVirtualEnvironment)?;
        let pm = fs::metadata(&python).map_err(|_| Unverified::NotAVirtualEnvironment)?;
        if !pm.is_file() || pm.uid() != 0 || pm.mode() & 0o022 != 0 {
            return Err(Unverified::UnsafePath);
        }
        let lib = root.join("lib");
        let mut site = None;
        for e in fs::read_dir(&lib)
            .map_err(|_| Unverified::NotAVirtualEnvironment)?
            .flatten()
        {
            let p = e.path().join("site-packages");
            if p.is_dir() {
                if site.is_some() {
                    return Err(Unverified::NotAVirtualEnvironment);
                }
                site = Some(p);
            }
        }
        let site = site.ok_or(Unverified::NotAVirtualEnvironment)?;
        // Nothing may sit beside the interpreter's own library that could be imported.
        for dir in ["lib64"] {
            if fs::symlink_metadata(root.join(dir))
                .is_ok_and(|m| !m.file_type().is_symlink() && !m.is_dir())
            {
                return Err(Unverified::UnsafePath);
            }
        }
        let digests = verify_site_packages(&site)?;
        let pkg = site.join("claude_agent_sdk");
        let read = |f: &str| fs::read_to_string(pkg.join(f)).map_err(|_| Unverified::SdkVersion);
        let sdk = assignment(&read("_version.py")?, "__version__").ok_or(Unverified::SdkVersion)?;
        let cli = assignment(&read("_cli_version.py")?, "__cli_version__")
            .ok_or(Unverified::CliVersion)?;
        let pin = builds
            .iter()
            .find(|b| b.sdk_version == sdk && b.cli_version == cli)
            .ok_or(if builds.iter().any(|b| b.sdk_version == sdk) {
                Unverified::CliVersion
            } else {
                Unverified::SdkVersion
            })?;
        let cli_rel = PathBuf::from("claude_agent_sdk/_bundled/claude");
        let digest = digests.get(&cli_rel).ok_or(Unverified::CliDigest)?;
        if hex(digest) != pin.cli_sha256 {
            return Err(Unverified::CliDigest);
        }
        // The distribution is the only one that may provide `claude_agent_sdk`.
        let dist = format!("claude_agent_sdk-{sdk}.dist-info");
        if !site.join(&dist).is_dir() {
            return Err(Unverified::SdkVersion);
        }
        if fs::read_dir(&self.policy_dir).is_ok_and(|mut d| d.next().is_some()) {
            return Err(Unverified::ManagedPolicy);
        }
        Ok(Verified {
            sdk_root: root.clone(),
            python: root.join("bin/python"),
            sdk_version: sdk,
            cli_version: cli,
            cli_sha256: hex(digest),
        })
    }

    pub fn read_key(&self) -> Result<Option<ApiKey>, Unverified> {
        let Some(path) = &self.api_key_file else {
            return Ok(None);
        };
        let mut f = fs::OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(path)
            .map_err(|_| Unverified::Credential)?;
        let m = f.metadata().map_err(|_| Unverified::Credential)?;
        if !m.is_file() || m.uid() != me() || m.mode() & 0o077 != 0 || m.len() == 0 || m.len() > 600
        {
            return Err(Unverified::Credential);
        }
        let mut text = String::new();
        f.read_to_string(&mut text)
            .map_err(|_| Unverified::Credential)?;
        ApiKey::new(text.trim_end_matches('\n').to_owned())
            .map(Some)
            .map_err(|_| Unverified::Credential)
    }
}

/// Writes the compiled-in bridge into a private directory named by its own digest and returns
/// it. An existing directory is reused only if it holds exactly these bytes.
pub fn materialize_bridge(parent: &Path) -> ControlResult<PathBuf> {
    let mut h = Sha256::new();
    for (name, body) in BRIDGE_FILES {
        h.update((name.len() as u64).to_be_bytes());
        h.update(name.as_bytes());
        h.update((body.len() as u64).to_be_bytes());
        h.update(body.as_bytes());
    }
    let dir = parent.join(format!("claude-bridge-{}", &hex(&h.finalize())[..16]));
    match fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => {
            for (name, body) in BRIDGE_FILES {
                let mut f = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o400)
                    .custom_flags(nix::libc::O_NOFOLLOW)
                    .open(dir.join(name))
                    .map_err(|_| E::UnsafePath)?;
                std::io::Write::write_all(&mut f, body.as_bytes())
                    .map_err(|_| E::StorageUnavailable)?;
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(_) => return Err(E::UnsafePath),
    }
    let m = fs::symlink_metadata(&dir).map_err(|_| E::UnsafePath)?;
    if !m.is_dir() || m.uid() != me() || m.mode() & 0o077 != 0 {
        return Err(E::UnsafePath);
    }
    let names: BTreeSet<_> = fs::read_dir(&dir)
        .map_err(|_| E::UnsafePath)?
        .flatten()
        .map(|e| e.file_name())
        .collect();
    if names.len() != BRIDGE_FILES.len() {
        return Err(E::UnsafePath);
    }
    for (name, body) in BRIDGE_FILES {
        let p = dir.join(name);
        let fm = fs::symlink_metadata(&p).map_err(|_| E::UnsafePath)?;
        if !fm.is_file()
            || fm.uid() != me()
            || fm.mode() & 0o277 != 0
            || fs::read(&p).map_err(|_| E::UnsafePath)? != body.as_bytes()
        {
            return Err(E::UnsafePath);
        }
    }
    Ok(dir)
}

/// The directories one Managed session's provider processes keep their state in. They belong to
/// the Managed session, not to a runtime or a conversation: the home holds the CLI's conversation
/// transcripts, which is what a later runtime of the same session resumes from.
pub fn session_dirs(state: &Path, managed_session: &str) -> ControlResult<(PathBuf, PathBuf)> {
    if !crate::adapters::claude::frames::uuid_ok(managed_session) {
        return Err(E::InvalidRequest);
    }
    let base = state.join("claude").join(managed_session);
    let mk = |p: &Path| match fs::DirBuilder::new().recursive(true).mode(0o700).create(p) {
        Ok(()) => Ok(()),
        Err(_) => Err(E::UnsafePath),
    };
    mk(&base)?;
    let (home, cwd) = (base.join("home"), base.join("cwd"));
    mk(&home)?;
    mk(&cwd)?;
    for p in [&home, &cwd] {
        let m = fs::symlink_metadata(p).map_err(|_| E::UnsafePath)?;
        if !m.is_dir() || m.uid() != me() || m.mode() & 0o077 != 0 {
            return Err(E::UnsafePath);
        }
    }
    if fs::read_dir(&cwd)
        .map_err(|_| E::UnsafePath)?
        .next()
        .is_some()
    {
        // The provider's working directory is empty: it has nothing to find there.
        return Err(E::UnsafePath);
    }
    Ok((home, cwd))
}

/// A verified installation, ready to start runtimes.
pub struct ClaudeLauncher {
    verified: Verified,
    bridge_dir: PathBuf,
    state: PathBuf,
    key: Option<ApiKey>,
    upstream: Option<String>,
}

impl ClaudeLauncher {
    pub fn prepare(
        install: &ClaudeInstall,
        runtime: &Path,
        state: &Path,
    ) -> Result<Self, Unverified> {
        let verified = install.verify()?;
        let key = install.read_key()?;
        let bridge_dir = materialize_bridge(runtime).map_err(|_| Unverified::UnsafePath)?;
        Ok(Self {
            verified,
            bridge_dir,
            state: state.to_owned(),
            key,
            upstream: install.upstream.clone(),
        })
    }
    pub fn verified(&self) -> &Verified {
        &self.verified
    }
    pub fn has_credential(&self) -> bool {
        self.key.is_some()
    }

    /// The command line of the bridge's namespace for one session.
    pub fn command(&self, home: &Path, cwd: &Path) -> std::process::Command {
        let mut sb = Sandbox::new(true);
        sb.ro_bind(&self.verified.sdk_root, &self.verified.sdk_root)
            .ro_bind(&self.bridge_dir, &self.bridge_dir)
            .bind(home, Path::new(SANDBOX_HOME))
            .bind(cwd, Path::new(SANDBOX_CWD))
            .chdir(Path::new(SANDBOX_CWD))
            .setenv("PATH", "/usr/bin")
            .setenv("HOME", SANDBOX_HOME)
            .setenv("LANG", "C.UTF-8")
            .setenv("TMPDIR", "/tmp");
        // An administrator's policy, if present, would have refused the launch already; it is
        // deliberately not hidden from the process either.
        sb.command(
            self.verified.python.to_str().unwrap_or("/usr/bin/python3"),
            &[
                "-I".into(),
                self.bridge_dir.join("bridge.py").into_os_string(),
            ],
        )
    }

    /// Starts one runtime of a Managed session (`identity` is the authority's own). `workspace` is the canonical folder a local-tools
    /// runtime acts on (the provider process never sees it); Inference has none. `conversation`
    /// is the id this runtime mints for a new binding.
    pub fn start(
        &self,
        profile: Profile,
        workspace: Option<&str>,
        identity: &Identity,
        conversation: &str,
        pinned_model: Option<String>,
        effort: Option<String>,
    ) -> ControlResult<ClaudeWorker<DeferredProcessWorker>> {
        if !crate::adapters::claude::frames::uuid_ok(conversation) {
            return Err(E::InvalidRequest);
        }
        let (home, cwd) = session_dirs(&self.state, &identity.managed_session)?;
        let attachment_workspace = match (profile, workspace) {
            (Profile::LocalTools, Some(w)) if path_is_plain(Path::new(w)) => w.to_owned(),
            (Profile::Inference, None) => SANDBOX_CWD.to_owned(),
            _ => return Err(E::InvalidRequest),
        };
        let inner = DeferredProcessWorker::spawn(
            self.command(&home, &cwd),
            Grace::default(),
            ContainmentPolicy::DescendantsContained,
        )?;
        let executor: Box<dyn ActionExecutor> = match profile {
            Profile::LocalTools => Box::new(SandboxExecutor::new()),
            Profile::Inference => Box::new(NoExecutor),
        };
        let cfg = ClaudeConfig {
            identity: identity.clone(),
            profile,
            workspace: attachment_workspace,
            revision: 1,
            cwd: SANDBOX_CWD.to_owned(),
            session: conversation.to_owned(),
            pinned_model,
            effort,
            upstream: self.upstream.clone(),
            api_key: self.key.clone(),
            turn_timeout_s: 600,
            #[cfg(test)]
            attachment_profile: None,
        };
        ClaudeWorker::new(inner, cfg, executor).map_err(|(e, mut inner)| {
            // The owned child is retained and reaped by the same bounded path as any other.
            use crate::f8::engine::Worker;
            inner.begin_stop(Instant::now());
            e
        })
    }
}

/// The Inference profile has no tool and therefore nothing to dispatch.
pub struct NoExecutor;
impl ActionExecutor for NoExecutor {
    fn dispatch(&mut self, _: super::exec::Approved) -> ControlResult<()> {
        Err(E::Unsupported)
    }
    fn poll(&mut self, _: Instant) -> Option<super::exec::Outcome> {
        None
    }
    fn busy(&self) -> bool {
        false
    }
    fn begin_stop(&mut self, _: Instant) {}
    fn poll_stop(&mut self, _: Instant) -> StopState {
        StopState::Reaped
    }
}

#[cfg(test)]
mod tests;

/// Test support: the SHA-256 the mock provider logs for a key it received.
#[cfg(test)]
pub(crate) fn tests_support_sha(key: &str) -> String {
    hex(&Sha256::digest(key.as_bytes()))
}
