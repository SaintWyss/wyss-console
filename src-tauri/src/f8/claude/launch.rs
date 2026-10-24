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
//! - the bridge and the CLI run in a network namespace with a loopback and nothing else. The only
//!   route out is the unix socket of a credential broker, a second owned process which holds the
//!   credential (if there is one), applies the request policy again outside the sandbox the model's
//!   process lives in, and connects only to a closed set of origins (`Origin`). The credential is
//!   handed to the broker's stdin once and is never in an environment, a command line, a log or a
//!   frame of the bridge. Where a credential would come from is not decided (`credential`): this
//!   build has no source of a real one;
//! - starting is refused unless an `Admission` can be made: in a production build, only when every
//!   gate of `m3::gate` is open.
use super::{
    admission::Admission,
    credential::{ApiKey, CredentialSource, NoCredential},
    exec::{ActionExecutor, SandboxExecutor},
    sandbox::{Sandbox, path_is_plain},
    worker::{ClaudeConfig, ClaudeWorker, Identity},
};
use crate::{
    adapters::claude::{Profile, frames::BrokerToken, pins},
    contracts::control::{ControlError as E, ControlResult},
    f8::{
        engine::{Attachment, Containment, StopState, Worker},
        ownership::{ContainmentPolicy, Grace},
        process_worker::DeferredProcessWorker,
    },
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::Instant,
};

pub const BRIDGE_FILES: [(&str, &str); 5] = [
    (
        "api_tap.py",
        include_str!("../../../claude-bridge/api_tap.py"),
    ),
    (
        "broker.py",
        include_str!("../../../claude-bridge/broker.py"),
    ),
    (
        "bridge.py",
        include_str!("../../../claude-bridge/bridge.py"),
    ),
    (
        "httpguard.py",
        include_str!("../../../claude-bridge/httpguard.py"),
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
/// Where the broker's directory is seen from the broker (writable: it makes its socket there) and
/// from the bridge (read-only: connecting to a socket needs no write to the filesystem).
pub const BROKER_DIR: &str = "/run/wyss/broker";

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
    FixtureNeedsLoopback,
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
            Self::FixtureNeedsLoopback => "fixture_needs_loopback",
        }
    }
}

/// Where the provider is: a closed set, never a string a client or a file can supply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// `https://api.anthropic.com`, the only origin a credential may ever be sent to.
    Anthropic,
    /// A plain-HTTP port on this machine, for a mock provider. It cannot carry a credential (the
    /// broker refuses the combination) and exists only in test and fixture builds.
    #[cfg(any(test, feature = "m3-fixture"))]
    LoopbackPlain(u16),
}
impl Origin {
    pub fn url(self) -> String {
        match self {
            Self::Anthropic => "https://api.anthropic.com".to_owned(),
            #[cfg(any(test, feature = "m3-fixture"))]
            Self::LoopbackPlain(port) => format!("http://127.0.0.1:{port}"),
        }
    }
}

/// What the operator named, all of it explicit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeInstall {
    /// The virtual environment holding `claude-agent-sdk`.
    pub sdk_root: PathBuf,
    /// The provider. Defaults to the real one.
    pub origin: Origin,
    /// Where an administrator's Claude Code policy would live (tests point it elsewhere).
    pub policy_dir: PathBuf,
}
impl ClaudeInstall {
    pub fn new(sdk_root: PathBuf) -> Self {
        Self {
            sdk_root,
            origin: Origin::Anthropic,
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

/// The directory of one Managed session's credential broker socket (private, mode 0700).
pub fn broker_dir(state: &Path, managed_session: &str) -> ControlResult<PathBuf> {
    if !crate::adapters::claude::frames::uuid_ok(managed_session) {
        return Err(E::InvalidRequest);
    }
    let p = state.join("claude").join(managed_session).join("broker");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&p)
        .map_err(|_| E::UnsafePath)?;
    let m = fs::symlink_metadata(&p).map_err(|_| E::UnsafePath)?;
    if !m.is_dir() || m.uid() != me() || m.mode() & 0o077 != 0 {
        return Err(E::UnsafePath);
    }
    Ok(p)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Every gate must be open for a runtime to start.
    Production,
    /// Tests and the isolated fixture: nothing real is reached.
    #[cfg(any(test, feature = "m3-fixture"))]
    Fixture,
}

/// A verified installation, ready to start runtimes.
pub struct ClaudeLauncher {
    verified: Verified,
    bridge_dir: PathBuf,
    state: PathBuf,
    origin: Origin,
    credential: Box<dyn CredentialSource>,
    mode: Mode,
}

/// The bridge and the credential broker as one owned worker: bytes go to and from the bridge, the
/// broker is stopped, reaped and watched with it, and its own few frames are consumed here.
pub struct PairedWorker {
    bridge: DeferredProcessWorker,
    broker: DeferredProcessWorker,
    broker_ready: bool,
}
impl PairedWorker {
    fn broker_frames(&mut self) -> ControlResult<()> {
        for frame in self.broker.inbound(4)? {
            let v: serde_json::Value =
                serde_json::from_slice(&frame).map_err(|_| E::TransportError)?;
            match v["e"].as_str() {
                Some("broker_ready") if !self.broker_ready => self.broker_ready = true,
                // A refusal carries a fixed word; nothing else is believed or kept.
                _ => return Err(E::TransportError),
            }
        }
        Ok(())
    }
}
impl Worker for PairedWorker {
    fn preflight(&mut self, context: &Attachment) -> ControlResult<()> {
        self.bridge.preflight(context)
    }
    fn send(&mut self, frame: &[u8]) -> ControlResult<()> {
        self.bridge.send(frame)
    }
    fn send_critical(&mut self, frame: &[u8]) -> ControlResult<()> {
        self.bridge.send_critical(frame)
    }
    fn flush(&mut self) -> ControlResult<()> {
        self.broker.flush()?;
        self.bridge.flush()
    }
    fn inbound(&mut self, max_frames: usize) -> ControlResult<Vec<Vec<u8>>> {
        self.broker_frames()?;
        self.bridge.inbound(max_frames)
    }
    fn has_exited(&mut self) -> bool {
        // Either one ending unasked makes the runtime unusable: without the broker every request fails.
        self.bridge.has_exited() || self.broker.has_exited()
    }
    fn begin_stop(&mut self, now: Instant) {
        self.broker.begin_stop(now);
        self.bridge.begin_stop(now);
    }
    fn poll_stop(&mut self, now: Instant) -> StopState {
        match (self.bridge.poll_stop(now), self.broker.poll_stop(now)) {
            (StopState::Reaped, StopState::Reaped) => StopState::Reaped,
            (StopState::Stuck, _) | (_, StopState::Stuck) => StopState::Stuck,
            _ => StopState::Pending,
        }
    }
    fn descendants(&self) -> usize {
        self.bridge.descendants() + self.broker.descendants()
    }
    fn containment(&mut self) -> Containment {
        if self.bridge.containment() == Containment::PidNamespace
            && self.broker.containment() == Containment::PidNamespace
        {
            Containment::PidNamespace
        } else {
            Containment::DirectChildOnly
        }
    }
    fn pid_for_diagnostics(&self) -> Option<i32> {
        self.bridge.pid_for_diagnostics()
    }
}

impl ClaudeLauncher {
    /// A launcher for a production build. It has no credential source and no admission: `start`
    /// refuses until every gate of `m3::gate` is open and an accepted source exists.
    pub fn prepare(
        install: &ClaudeInstall,
        runtime: &Path,
        state: &Path,
    ) -> Result<Self, Unverified> {
        Self::build(
            install,
            runtime,
            state,
            Box::new(NoCredential),
            Mode::Production,
        )
    }
    /// Protected production delivery. The authority chooses the store/record and verified helper;
    /// neither a page nor the provider can supply a credential path or receive the key.
    pub fn prepare_protected(
        install: &ClaudeInstall,
        runtime: &Path,
        state: &Path,
        source: super::handoff::ProcessCredentialSource,
    ) -> Result<Self, Unverified> {
        Self::build(install, runtime, state, Box::new(source), Mode::Production)
    }
    /// A launcher for tests and the isolated fixture, with its own credential source.
    #[cfg(any(test, feature = "m3-fixture"))]
    pub fn prepare_fixture(
        install: &ClaudeInstall,
        runtime: &Path,
        state: &Path,
        credential: Box<dyn CredentialSource>,
    ) -> Result<Self, Unverified> {
        Self::build(install, runtime, state, credential, Mode::Fixture)
    }
    fn build(
        install: &ClaudeInstall,
        runtime: &Path,
        state: &Path,
        credential: Box<dyn CredentialSource>,
        mode: Mode,
    ) -> Result<Self, Unverified> {
        // A fixture reaches a mock on this machine and nothing else: it can never be pointed at the
        // real provider, with or without a key, whatever a caller supplies.
        #[cfg(any(test, feature = "m3-fixture"))]
        if mode == Mode::Fixture && install.origin == Origin::Anthropic {
            return Err(Unverified::FixtureNeedsLoopback);
        }
        let verified = install.verify()?;
        let bridge_dir = materialize_bridge(runtime).map_err(|_| Unverified::UnsafePath)?;
        Ok(Self {
            verified,
            bridge_dir,
            state: state.to_owned(),
            origin: install.origin,
            credential,
            mode,
        })
    }
    pub fn verified(&self) -> &Verified {
        &self.verified
    }
    fn admission(&self, profile: Profile) -> ControlResult<Admission> {
        match self.mode {
            Mode::Production => Admission::production(profile).map_err(|_| E::GateDenied),
            #[cfg(any(test, feature = "m3-fixture"))]
            Mode::Fixture => Ok(Admission::fixture()),
        }
    }

    /// The namespace the bridge and the CLI run in: a loopback and nothing else.
    fn bridge_sandbox(&self, home: &Path, cwd: &Path, broker: &Path) -> Sandbox {
        let mut sb = Sandbox::new(false);
        sb.ro_bind(&self.verified.sdk_root, &self.verified.sdk_root)
            .ro_bind(&self.bridge_dir, &self.bridge_dir)
            .ro_bind(broker, Path::new(BROKER_DIR))
            .bind(home, Path::new(SANDBOX_HOME))
            .bind(cwd, Path::new(SANDBOX_CWD))
            .chdir(Path::new(SANDBOX_CWD))
            .setenv("PATH", "/usr/bin")
            .setenv("HOME", SANDBOX_HOME)
            .setenv("LANG", "C.UTF-8")
            .setenv("TMPDIR", "/tmp");
        sb
    }

    /// The command line of the bridge's namespace for one session.
    fn command(&self, home: &Path, cwd: &Path, broker: &Path) -> std::process::Command {
        // An administrator's policy, if present, would have refused the launch already; it is
        // deliberately not hidden from the process either.
        self.bridge_sandbox(home, cwd, broker).command(
            self.verified.python.to_str().unwrap_or("/usr/bin/python3"),
            &[
                "-I".into(),
                self.bridge_dir.join("bridge.py").into_os_string(),
            ],
        )
    }

    /// The command line of the credential broker: the network, the standard library and its own
    /// directory, with nothing of the user's home or of the provider's state.
    fn broker_command(&self, broker: &Path) -> std::process::Command {
        let mut sb = Sandbox::new(true);
        sb.ro_bind(&self.bridge_dir, &self.bridge_dir)
            .bind(broker, Path::new(BROKER_DIR))
            .chdir(Path::new(BROKER_DIR))
            .setenv("PATH", "/usr/bin")
            .setenv("LANG", "C.UTF-8")
            .setenv("TMPDIR", "/tmp");
        sb.command(
            "/usr/bin/python3",
            &[
                "-I".into(),
                self.bridge_dir.join("broker.py").into_os_string(),
            ],
        )
    }

    /// The one line the broker is configured with, on its stdin.
    fn broker_frame(
        &self,
        token: &BrokerToken,
        profile: Profile,
        key: Option<&ApiKey>,
    ) -> ControlResult<Vec<u8>> {
        let mut v = json!({"v":1,"dir":BROKER_DIR,"token":token.as_str(),
            "origin":self.origin.url(),"profile":profile.bridge_name()});
        if let Some(k) = key {
            v["api_key"] = json!(k.expose());
        }
        serde_json::to_vec(&v).map_err(|_| E::InvalidRequest)
    }

    /// Starts one runtime of a Managed session (`identity` is the authority's own). `workspace` is the canonical folder a local-tools
    /// runtime acts on (the provider process never sees it); Inference has none. `conversation`
    /// is the id this runtime mints for a new binding. Refused unless an `Admission` can be made
    /// and, for the real provider, a credential source exists.
    pub fn start(
        &self,
        profile: Profile,
        workspace: Option<&str>,
        identity: &Identity,
        conversation: &str,
        pinned_model: Option<String>,
        effort: Option<String>,
    ) -> ControlResult<ClaudeWorker<PairedWorker>> {
        let admission = self.admission(profile)?;
        #[cfg(any(test, feature = "m3-fixture"))]
        if self.mode == Mode::Fixture && self.origin == Origin::Anthropic {
            return Err(E::GateDenied);
        }
        if !crate::adapters::claude::frames::uuid_ok(conversation) {
            return Err(E::InvalidRequest);
        }
        let attachment_workspace = match (profile, workspace) {
            (Profile::LocalTools, Some(w)) if path_is_plain(Path::new(w)) => w.to_owned(),
            (Profile::Inference, None) => SANDBOX_CWD.to_owned(),
            _ => return Err(E::InvalidRequest),
        };
        let key = self.credential.key_for(&identity, profile)?;
        if key.is_none() && self.origin == Origin::Anthropic {
            // The real provider without a credential would only fail later, and loudly.
            return Err(E::GateDenied);
        }
        let (home, cwd) = session_dirs(&self.state, &identity.managed_session)?;
        let broker_dir = broker_dir(&self.state, &identity.managed_session)?;
        let token = BrokerToken::generate()?;
        let frame = self.broker_frame(&token, profile, key.as_ref())?;
        drop(key);
        let mut broker = DeferredProcessWorker::spawn(
            self.broker_command(&broker_dir),
            Grace::default(),
            ContainmentPolicy::DescendantsContained,
        )?;
        if let Err(e) = broker.send(&frame) {
            reap(&mut broker);
            return Err(e);
        }
        let bridge = match DeferredProcessWorker::spawn(
            self.command(&home, &cwd, &broker_dir),
            Grace::default(),
            ContainmentPolicy::DescendantsContained,
        ) {
            Ok(b) => b,
            Err(e) => {
                reap(&mut broker);
                return Err(e);
            }
        };
        let inner = PairedWorker {
            bridge,
            broker,
            broker_ready: false,
        };
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
            broker_token: token,
            admission,
            turn_timeout_s: 600,
            #[cfg(test)]
            attachment_profile: None,
        };
        ClaudeWorker::new(inner, cfg, executor).map_err(|(e, mut inner)| {
            // The owned children are retained and reaped by the same bounded path as any other.
            inner.begin_stop(Instant::now());
            e
        })
    }
}

/// Stops a process this module started and has not handed on, within a short bound.
fn reap(w: &mut DeferredProcessWorker) {
    w.begin_stop(Instant::now());
    let end = Instant::now() + std::time::Duration::from_secs(3);
    while Instant::now() < end && w.poll_stop(Instant::now()) != StopState::Reaped {
        std::thread::sleep(std::time::Duration::from_millis(20));
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
