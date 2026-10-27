//! The native side of the Console: starts an agent in a terminal through the user's shell,
//! keeps the registry, resolves profiles. It takes the terminal service as an argument and
//! holds no thread: state is refreshed from the terminal when the overview asks, and process
//! resources are sampled only then (nothing runs while the view is hidden).
use super::{
    capabilities,
    catalogue::{self, AGENTS, AgentId},
    extended_registry::{ExtendedRegistry, RecoveryRead},
    profile::{self, LaunchPlan, Word},
    profiles::ProfileStore,
    recipe::{self, family},
    registry::{M9Open, ProfileRef, Registry, Session, State},
    resources::{self, Usage},
    store,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use wyss_console::{
    contracts::identities::TerminalId,
    terminal::{
        TerminalService,
        contracts::{CreateTerminal, ErrorCode, Launch, TerminalSize, TerminalState},
    },
};

/// Most bytes of a handoff brief.
pub const MAX_BRIEF: usize = 16 * 1024;
const FOUND_FOR: Duration = Duration::from_secs(30);

/// Variables that belong to *one running agent session*, not to the user: if the Console was
/// itself started from inside Claude Code or Codex it has them, and an agent started from it
/// must not take them for its own (it would think it is a child of that session, and would
/// hold its messaging token). The user's own configuration variables (`CLAUDE_CONFIG_DIR`,
/// `ANTHROPIC_API_KEY`, `CODEX_HOME`, `CLAUDE_CODE_USE_*`…) are not in this list.
pub const SESSION_MARKERS: [&str; 17] = [
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_VERSION",
    "CLAUDE_EFFORT",
    "CLAUDE_PID",
    "CLAUDE_PLUGIN_DATA",
    "CLAUDE_PLUGIN_ROOT",
    "CODEX_COMPANION_SESSION_ID",
    "CODEX_THREAD_ID",
    "CODEX_SESSION_ID",
    "WYSS_NATIVE_SESSION_ID",
];
/// Words of an error the window can show; never a path or a command.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NativeError {
    pub code: &'static str,
    pub reasons: Vec<String>,
}

impl NativeError {
    pub fn new(code: &'static str) -> Self {
        Self {
            code,
            reasons: Vec::new(),
        }
    }
}

impl From<profile::ProfileError> for NativeError {
    fn from(error: profile::ProfileError) -> Self {
        let value = serde_json::to_value(error).unwrap_or_default();
        let word = value.as_str().unwrap_or("profile_invalid");
        // Leaked once per distinct word: there are a dozen, and they live for the process.
        let code: &'static str = Box::leak(format!("profile_{word}").into_boxed_str());
        Self::new(code)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalogueEntry {
    pub agent_id: AgentId,
    pub name: &'static str,
    pub executable: &'static str,
    pub installed: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShellView {
    pub path: String,
    pub family: Option<&'static str>,
    /// Whether the agent will start through the user's shell (its environment is the
    /// user's) or directly (the Console's own environment).
    pub through_shell: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectView {
    pub path: String,
    pub home: bool,
    pub default_agent: Option<AgentId>,
    pub default_profile: Option<String>,
    pub last_used: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LauncherView {
    pub agents: Vec<CatalogueEntry>,
    pub shell: Option<ShellView>,
    pub projects: Vec<ProjectView>,
    /// Legacy M1-M6 clients do not know this field. M9 windows use it to distinguish an older
    /// host from a damaged/unavailable M9 sidecar without hiding the legacy providers.
    pub m9_native_status: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviewView {
    pub applied: Vec<String>,
    pub with_cost: Vec<String>,
    pub not_applicable: Vec<String>,
    pub can_start: bool,
    pub through_shell: bool,
    /// Opaque binding to this exact resolved profile plan. Start rejects it after any edit,
    /// restore or inherited change, and uses the plan whose fingerprint matched.
    pub plan_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartRequest {
    pub agent_id: AgentId,
    /// A folder the native dialog returned in this run, the home folder, or a project the
    /// registry knows. Anything else is refused.
    pub project: String,
    pub profile_id: Option<String>,
    pub size: TerminalSize,
    /// A reviewed handoff brief (F11a): the agent's first message. At most 16 KiB, starts with
    /// `# `, no NUL: it travels as one word of the command line, never as text a shell reads.
    #[serde(default)]
    pub brief: Option<String>,
    /// The token returned by the exact profile preview shown before this start.
    #[serde(default)]
    pub plan_token: Option<String>,
    /// Exact CLI version observed when that preview was shown; Native binds it again inside
    /// the user's shell before executing the shell-resolved binary.
    #[serde(default)]
    pub expected_cli_version: Option<String>,
}

/// A session as the window sees it: the registry's record plus what is observed now.
#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    #[serde(flatten)]
    pub session: Session,
    pub plane: &'static str,
    pub terminal_id: Option<String>,
    pub pid: Option<u32>,
    pub usage: Option<Usage>,
}

/// What marks a terminal as an agent's: enough for a window that attaches later to label it.
#[derive(Debug, Clone, Serialize)]
pub struct NativeTag {
    pub session_id: String,
    pub agent_name: String,
    pub project: String,
    pub profile: Option<String>,
    pub through_shell: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StartReply {
    pub session: SessionView,
    pub terminal: wyss_console::terminal::contracts::TerminalInfo,
}

struct Live {
    terminal: TerminalId,
    pid: Option<u32>,
    last: Option<(resources::Sample, Instant)>,
    usage: Option<Usage>,
}

struct Inner {
    base: PathBuf,
    registry: Mutex<Registry>,
    extended_registry: Mutex<Option<ExtendedRegistry>>,
    /// Sticky for this service lifetime after an M9 sidecar write reports failure. It keeps
    /// optional providers unavailable until reopen while legacy launches remain independent.
    m9_storage_degraded: AtomicBool,
    /// A failed durable removal of an actual M9 default must not be bypassed by a later
    /// in-memory no-op; the process must reopen and re-read the authoritative sidecar.
    m9_default_clear_blocked: AtomicBool,
    /// M9 sessions whose process started but whose sidecar record could not be committed.
    /// This volatile entry keeps the live process manageable without claiming durability.
    volatile_sessions: Mutex<HashMap<String, Session>>,
    recovered_sessions: Vec<Session>,
    recovered_defaults: Mutex<BTreeMap<String, AgentId>>,
    profiles: ProfileStore,
    home: PathBuf,
    path_var: Option<OsString>,
    chosen: Mutex<Vec<String>>,
    live: Mutex<HashMap<String, Live>>,
    found: Mutex<Option<(Instant, Vec<CatalogueEntry>)>>,
    starting: AtomicBool,
    /// Variables added to every launch, besides the session's id. Empty outside tests.
    extra_env: Vec<(String, String)>,
    /// A shell standing in for the user's. Never set outside tests.
    shell: Option<PathBuf>,
    /// Names removed from the environment an agent inherits.
    scrub: Vec<String>,
}

#[derive(Clone)]
pub struct NativeService {
    inner: Arc<Inner>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn id_ok(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

struct Starting<'a>(&'a AtomicBool);
impl Drop for Starting<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl NativeService {
    /// Opens the stores under `base` (private, created when missing). What a crash left is
    /// finished here: sessions that were running are `lost`, their generated files removed.
    pub fn open(base: PathBuf, home: PathBuf, path_var: Option<OsString>) -> Option<Self> {
        store::private_dir(&base).ok()?;
        let registry_dir = base.join("registry");
        let M9Open {
            registry,
            lost: _lost,
            extended_sessions: migrated_sessions,
            extended_defaults: migrated_defaults,
        } = Registry::open_for_m9(registry_dir.clone()).ok()?;
        let (default_overrides, overrides_complete) =
            match ExtendedRegistry::read_default_overrides(&registry_dir) {
                Ok(overrides) => (overrides, true),
                Err(_) => (BTreeSet::new(), false),
            };
        let recovery =
            ExtendedRegistry::read_recovery(&registry_dir).unwrap_or_else(|_| RecoveryRead {
                snapshots: Vec::new(),
                incomplete: true,
            });
        let mut recovered_sessions = Vec::new();
        let mut recovered_defaults = BTreeMap::new();
        let mut migration_sessions = migrated_sessions.clone();
        let migration_defaults: BTreeMap<String, AgentId> =
            migrated_defaults.iter().cloned().collect();
        let mut extended_registry = ExtendedRegistry::open(registry_dir.clone()).ok();
        if let Some(extended) = extended_registry.as_mut() {
            let mut migration_complete = overrides_complete && !recovery.incomplete;
            if migration_complete {
                let current_paths = extended
                    .recovery_snapshot()
                    .project_defaults
                    .into_keys()
                    .collect::<Vec<_>>();
                for path in current_paths {
                    if default_overrides.contains(&ExtendedRegistry::default_override_key(&path))
                        && extended.clear_project_default(&path).is_err()
                    {
                        migration_complete = false;
                        break;
                    }
                }
            }
            if migration_complete {
                'migrate: for snapshot in &recovery.snapshots {
                    for session in snapshot.sessions.iter().cloned() {
                        if extended.migrate_session(session).is_err() {
                            migration_complete = false;
                            break 'migrate;
                        }
                    }
                    for (path, agent) in &snapshot.project_defaults {
                        if !default_overrides
                            .contains(&ExtendedRegistry::default_override_key(path))
                            && extended
                                .migrate_project_default(path.clone(), *agent)
                                .is_err()
                        {
                            migration_complete = false;
                            break 'migrate;
                        }
                    }
                }
            }
            if migration_complete {
                for session in migrated_sessions {
                    if extended.migrate_session(session).is_err() {
                        migration_complete = false;
                        break;
                    }
                }
            }
            if migration_complete {
                for (path, agent) in migrated_defaults {
                    if !default_overrides.contains(&ExtendedRegistry::default_override_key(&path))
                        && extended.migrate_project_default(path, agent).is_err()
                    {
                        migration_complete = false;
                        break;
                    }
                }
            }
            if !migration_complete
                && (!migration_sessions.is_empty() || !migration_defaults.is_empty())
            {
                let pending_defaults = migration_defaults
                    .iter()
                    .map(|(path, agent)| (path.clone(), *agent))
                    .collect::<Vec<_>>();
                // The shared registry still owns these interim records until the safe M1-M6
                // projection is durable. Copy every pending M9 record first; if bounded
                // recovery cannot accept them, leave the source registry untouched and refuse
                // startup instead of losing history on the next restart.
                ExtendedRegistry::preserve_recovery(
                    &registry_dir,
                    &migration_sessions,
                    &pending_defaults,
                )
                .ok()?;
            }
            // Sidecar writes sync their parent directory before this source registry is
            // atomically replaced with the M1-M6-safe projection.
            registry.save().ok()?;
            if migration_complete
                && ExtendedRegistry::clear_recovery(&registry_dir).is_ok()
                && ExtendedRegistry::clear_default_overrides(&registry_dir).is_ok()
            {
                // All M9 metadata is now represented by the durable sidecar.
            } else {
                // A valid but over-cap or otherwise unmergeable M9 extension must not make
                // Claude/Codex unavailable. Preserve the current sidecar view plus every
                // immutable recovery/migration source and explicitly block M9 admission.
                let current = extended.recovery_snapshot();
                recovered_sessions.extend(current.sessions);
                recovered_defaults.extend(current.project_defaults);
                for snapshot in recovery.snapshots {
                    recovered_sessions.extend(snapshot.sessions);
                    for (path, agent) in snapshot.project_defaults {
                        recovered_defaults.entry(path).or_insert(agent);
                    }
                }
                recovered_sessions.append(&mut migration_sessions);
                for (path, agent) in migration_defaults {
                    recovered_defaults.entry(path).or_insert(agent);
                }
                extended_registry = None;
            }
        } else {
            // M9 storage is optional to legacy providers. Preserve any M9 values still in
            // the interim shared registry before sanitizing that registry. A corrupt primary
            // sidecar itself is never opened, truncated, or replaced here.
            for snapshot in recovery.snapshots {
                recovered_sessions.extend(snapshot.sessions);
                for (path, agent) in snapshot.project_defaults {
                    recovered_defaults.entry(path).or_insert(agent);
                }
            }
            if !migrated_sessions.is_empty() || !migrated_defaults.is_empty() {
                let snapshot = ExtendedRegistry::preserve_recovery(
                    &registry_dir,
                    &migrated_sessions,
                    &migrated_defaults,
                )
                .ok()?;
                recovered_sessions.extend(snapshot.sessions);
                for (path, agent) in snapshot.project_defaults {
                    recovered_defaults.entry(path).or_insert(agent);
                }
            }
            registry.save().ok()?;
        }
        for session in &mut recovered_sessions {
            if !session.state.finished() {
                session.state = State::Lost;
                session.ended_at = None;
            }
        }
        recovered_sessions.sort_by(|left, right| left.id.cmp(&right.id));
        recovered_sessions.dedup_by(|left, right| left.id == right.id);
        recovered_defaults.retain(|path, _| {
            !default_overrides.contains(&ExtendedRegistry::default_override_key(path))
        });
        let profiles = ProfileStore::open(base.join("profiles")).ok()?;
        let sessions = base.join("sessions");
        // Nothing is running: every generated directory is stale.
        let _ = fs::remove_dir_all(&sessions);
        store::private_dir(&sessions).ok()?;
        Some(Self {
            inner: Arc::new(Inner {
                base,
                registry: Mutex::new(registry),
                extended_registry: Mutex::new(extended_registry),
                m9_storage_degraded: AtomicBool::new(false),
                m9_default_clear_blocked: AtomicBool::new(false),
                volatile_sessions: Mutex::new(HashMap::new()),
                recovered_sessions,
                recovered_defaults: Mutex::new(recovered_defaults),
                profiles,
                home,
                path_var,
                chosen: Mutex::new(Vec::new()),
                live: Mutex::new(HashMap::new()),
                found: Mutex::new(None),
                starting: AtomicBool::new(false),
                extra_env: Vec::new(),
                shell: None,
                scrub: SESSION_MARKERS.iter().map(|n| (*n).to_owned()).collect(),
            }),
        })
    }

    fn user_shell(&self) -> Result<PathBuf, wyss_console::terminal::contracts::TerminalError> {
        match &self.inner.shell {
            Some(shell) => Ok(shell.clone()),
            None => TerminalService::user_shell(),
        }
    }

    pub fn profiles(&self) -> &ProfileStore {
        &self.inner.profiles
    }

    // ---- what can be started --------------------------------------------------------------

    fn agents(&self) -> Vec<CatalogueEntry> {
        let mut cache = lock(&self.inner.found);
        if let Some((at, found)) = cache.as_ref()
            && at.elapsed() < FOUND_FOR
        {
            return found.clone();
        }
        let found: Vec<CatalogueEntry> = AGENTS
            .iter()
            .map(|spec| {
                let path = catalogue::discover(spec, self.inner.path_var.clone(), &self.inner.home);
                let version = path
                    .as_deref()
                    .and_then(|path| catalogue::probe_provider_version(spec.id, path));
                CatalogueEntry {
                    agent_id: spec.id,
                    name: spec.name,
                    executable: spec.executable,
                    installed: path.is_some(),
                    path: path.map(|p| p.to_string_lossy().into_owned()),
                    version,
                }
            })
            .collect();
        *cache = Some((Instant::now(), found.clone()));
        found
    }

    fn shell(&self) -> Option<ShellView> {
        let path = self.user_shell().ok()?;
        let family = family(&path).map(|f| match f {
            recipe::Family::Posix => "sh",
            recipe::Family::Bash => "bash",
            recipe::Family::Zsh => "zsh",
            recipe::Family::Fish => "fish",
        });
        Some(ShellView {
            path: path.to_string_lossy().into_owned(),
            family,
            through_shell: family.is_some(),
        })
    }

    pub fn launcher(&self) -> LauncherView {
        self.launcher_view(false)
    }

    /// Full M9 view, called only through the additive extended host operation. The legacy
    /// method remains safe for M1-M6 windows with their closed AgentId contract.
    pub fn launcher_extended(&self) -> LauncherView {
        self.launcher_view(true)
    }

    fn recovered_default(&self, path: &str) -> Option<AgentId> {
        lock(&self.inner.recovered_defaults).get(path).copied()
    }

    fn launcher_view(&self, include_extended: bool) -> LauncherView {
        let home = self.inner.home.to_string_lossy().into_owned();
        let registry = lock(&self.inner.registry);
        let extended = include_extended.then(|| lock(&self.inner.extended_registry));
        let extension_ready = extended.as_ref().is_some_and(|stored| stored.is_some())
            && !self.inner.m9_storage_degraded.load(Ordering::Acquire);
        let m9_native_status = if !include_extended {
            "legacy_host"
        } else if extension_ready {
            "ready"
        } else {
            "storage_unavailable"
        };
        let extended_store = extended.as_ref().and_then(|stored| stored.as_ref());
        let mut projects: Vec<ProjectView> = vec![ProjectView {
            path: home.clone(),
            home: true,
            default_agent: None,
            default_profile: None,
            last_used: 0,
        }];
        let mut known = |path: &str, project: Option<&super::registry::Project>| {
            if projects.iter().any(|p| p.path == path) {
                if let Some(project) = project
                    && let Some(shown) = projects.iter_mut().find(|p| p.path == path)
                {
                    shown.default_agent = extended_store
                        .and_then(|stored| stored.project_default(path))
                        .or(project.default_agent)
                        .or_else(|| self.recovered_default(path))
                        .filter(|agent| include_extended || agent.is_legacy());
                    shown.default_profile.clone_from(&project.default_profile);
                    shown.last_used = project.last_used;
                }
                return;
            }
            projects.push(ProjectView {
                path: path.to_owned(),
                home: false,
                default_agent: extended_store
                    .and_then(|stored| stored.project_default(path))
                    .or_else(|| project.and_then(|p| p.default_agent))
                    .or_else(|| self.recovered_default(path))
                    .filter(|agent| include_extended || agent.is_legacy()),
                default_profile: project.and_then(|p| p.default_profile.clone()),
                last_used: project.map_or(0, |p| p.last_used),
            });
        };
        for project in &registry.data.projects {
            known(&project.path, Some(project));
        }
        for path in lock(&self.inner.chosen).iter() {
            known(path, None);
        }
        // The home folder carries its remembered defaults too.
        if let Some(project) = registry.data.projects.iter().find(|p| p.path == home)
            && let Some(shown) = projects.first_mut()
        {
            shown.default_agent = extended_store
                .and_then(|stored| stored.project_default(&home))
                .or(project.default_agent)
                .or_else(|| self.recovered_default(&home))
                .filter(|agent| include_extended || agent.is_legacy());
            shown.default_profile.clone_from(&project.default_profile);
            shown.last_used = project.last_used;
        }
        let agents = self.agents();
        LauncherView {
            agents: agents
                .into_iter()
                .filter(|entry| entry.agent_id.is_legacy() || (include_extended && extension_ready))
                .collect(),
            shell: self.shell(),
            projects,
            m9_native_status,
        }
    }

    /// A folder the native dialog returned in this run: the only kind a start may name
    /// besides the home and the registry's own.
    pub fn remember_chosen(&self, path: String) {
        let mut chosen = lock(&self.inner.chosen);
        chosen.retain(|p| p != &path);
        if chosen.len() >= 16 {
            chosen.remove(0);
        }
        chosen.push(path);
    }

    fn project_allowed(&self, path: &str) -> Result<PathBuf, NativeError> {
        let known = path == self.inner.home.to_string_lossy()
            || lock(&self.inner.chosen).iter().any(|p| p == path)
            || lock(&self.inner.registry)
                .data
                .projects
                .iter()
                .any(|p| p.path == path);
        let invalid = || NativeError::new("project_invalid");
        if !known || path.len() > 4096 {
            return Err(invalid());
        }
        let canonical = Path::new(path).canonicalize().map_err(|_| invalid())?;
        canonical.is_dir().then_some(canonical).ok_or_else(invalid)
    }

    /// Sets (or clears) the profile a project starts with by default.
    pub fn set_project_default(
        &self,
        path: &str,
        profile_id: Option<String>,
    ) -> Result<(), NativeError> {
        self.project_allowed(path)?;
        if let Some(id) = &profile_id {
            self.inner.profiles.resolve(id)?;
        }
        let mut registry = lock(&self.inner.registry);
        registry.project_mut(path).default_profile = profile_id;
        registry
            .save()
            .map_err(|_| NativeError::new("store_unavailable"))
    }

    // ---- profiles applied -----------------------------------------------------------------

    fn plan_for(
        &self,
        agent: AgentId,
        profile_id: Option<&str>,
    ) -> Result<(LaunchPlan, Option<ProfileRef>, Option<String>), NativeError> {
        let Some(id) = profile_id else {
            return Ok((LaunchPlan::default(), None, None));
        };
        let (resolved, version) = self
            .inner
            .profiles
            .resolve(id)
            .map_err(|_| NativeError::new("profile_unknown"))?;
        let applies = resolved.body.applies_to.contains(&agent);
        let plan = profile::plan(&resolved, agent, applies);
        let profile_ref = ProfileRef {
            id: id.to_owned(),
            name: resolved.body.name.clone(),
            version,
        };
        let material = serde_json::to_vec(&(agent, id, &profile_ref, &resolved, &plan))
            .map_err(|_| NativeError::new("internal"))?;
        let token = format!("{:x}", Sha256::digest(material));
        Ok((plan, Some(profile_ref), Some(token)))
    }

    fn restore_recovered_default_override(
        &self,
        project: &str,
        existed: bool,
        previous: Option<AgentId>,
    ) {
        let registry_dir = self.inner.base.join("registry");
        if ExtendedRegistry::restore_default_override(&registry_dir, project, existed).is_ok()
            && let Some(agent) = previous
        {
            lock(&self.inner.recovered_defaults).insert(project.to_owned(), agent);
        }
    }

    pub fn preview(
        &self,
        agent: AgentId,
        profile_id: Option<&str>,
    ) -> Result<PreviewView, NativeError> {
        let (plan, _, plan_token) = self.plan_for(agent, profile_id)?;
        Ok(PreviewView {
            can_start: plan.not_applicable.is_empty(),
            applied: plan.applied,
            with_cost: plan.with_cost,
            not_applicable: plan.not_applicable,
            through_shell: self.shell().is_some_and(|s| s.through_shell),
            plan_token,
        })
    }

    // ---- starting and stopping ------------------------------------------------------------

    pub fn start(
        &self,
        terminal: &TerminalService,
        request: StartRequest,
    ) -> Result<StartReply, NativeError> {
        self.start_with(terminal, request, None)
    }

    fn start_with(
        &self,
        terminal: &TerminalService,
        request: StartRequest,
        restart: Option<(&str, u32)>,
    ) -> Result<StartReply, NativeError> {
        // One start at a time (05 §4.2).
        if self
            .inner
            .starting
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(NativeError::new("busy"));
        }
        let _starting = Starting(&self.inner.starting);
        if request.agent_id.is_legacy()
            && self.inner.m9_default_clear_blocked.load(Ordering::Acquire)
        {
            return Err(NativeError::new("store_unavailable"));
        }
        if !request.agent_id.is_legacy()
            && (lock(&self.inner.extended_registry).is_none()
                || self.inner.m9_storage_degraded.load(Ordering::Acquire))
        {
            return Err(NativeError::new("store_unavailable"));
        }
        let project = self.project_allowed(&request.project)?;
        let agent = self
            .agents()
            .into_iter()
            .find(|a| a.agent_id == request.agent_id)
            .filter(|a| a.installed)
            .ok_or_else(|| NativeError::new("agent_not_installed"))?;
        let (plan, profile_ref, plan_token) =
            self.plan_for(request.agent_id, request.profile_id.as_deref())?;
        if request.profile_id.is_some() {
            match (request.plan_token.as_deref(), plan_token.as_deref()) {
                (Some(expected), Some(actual)) if expected == actual => {}
                (None, _) => return Err(NativeError::new("profile_preview_required")),
                _ => return Err(NativeError::new("profile_preview_stale")),
            }
        } else if request.plan_token.is_some() {
            return Err(NativeError::new("profile_preview_stale"));
        }
        if !plan.not_applicable.is_empty() {
            return Err(NativeError {
                code: "profile_not_applicable",
                reasons: plan.not_applicable,
            });
        }
        let shell = self
            .user_shell()
            .map_err(|_| NativeError::new("spawn_failed"))?;
        let has_overrides = !plan.applied.is_empty() || !plan.with_cost.is_empty();
        let version_bound_handoff =
            request.agent_id == AgentId::Opencode && request.brief.is_some();
        let version_bound = has_overrides || version_bound_handoff;
        if version_bound {
            let Some(version) = request.expected_cli_version.as_deref() else {
                return Err(NativeError::new("provider_version_unverified"));
            };
            if !capabilities::permits_profile_overrides(request.agent_id, Some(version)) {
                return Err(NativeError::new("provider_version_unverified"));
            }
            if recipe::family(&shell).is_none() {
                let installed_version = agent.path.as_deref().and_then(|path| {
                    catalogue::probe_provider_version(request.agent_id, Path::new(path))
                });
                if installed_version.as_deref() != Some(version) {
                    return Err(NativeError::new("provider_version_unverified"));
                }
            }
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let dir = self.inner.base.join("sessions").join(&id);
        let version_guard = version_bound && recipe::family(&shell).is_some();
        let guard_marker = dir.join("provider-version-verified");
        if version_guard && store::private_dir(&dir).is_err() {
            return Err(NativeError::new("store_unavailable"));
        }
        let mut words = vec![agent.executable.to_owned()];
        for word in &plan.words {
            match word {
                Word::Literal(text) => words.push(text.clone()),
                Word::Overlay => {
                    words.push(dir.join("settings.json").to_string_lossy().into_owned())
                }
            }
        }
        if let Some(brief) = &request.brief {
            if brief.len() > MAX_BRIEF || !brief.starts_with("# ") || brief.contains('\0') {
                return Err(NativeError::new("brief_invalid"));
            }
            match request.agent_id {
                AgentId::Opencode => {
                    words.push("--prompt".to_owned());
                    words.push(brief.clone());
                }
                AgentId::Hermes => return Err(NativeError::new("handoff_target_unverified")),
                AgentId::ClaudeCode | AgentId::Codex => words.push(brief.clone()),
            }
        }
        if let Some(overlay) = &plan.overlay {
            let write = || -> std::io::Result<()> {
                store::private_dir(&dir)?;
                store::write_atomic(&dir.join("settings.json"), overlay.as_bytes())
            };
            if write().is_err() {
                let _ = fs::remove_dir_all(&dir);
                return Err(NativeError::new("store_unavailable"));
            }
        }
        let direct = agent.path.clone().map(PathBuf::from).unwrap_or_default();
        let made = recipe::recipe_with_version_guard(
            &shell,
            &words,
            &direct,
            if version_bound {
                request.expected_cli_version.as_deref()
            } else {
                None
            },
        );
        let now = store::now();
        let mut session = Session {
            id: id.clone(),
            agent_id: request.agent_id,
            agent_name: agent.name.to_owned(),
            version: agent.version.clone(),
            project: project.to_string_lossy().into_owned(),
            profile: profile_ref,
            through_shell: made.through_shell,
            state: State::Requested,
            started_at: now,
            ended_at: None,
            exit_code: None,
            exit_signal: None,
            error: None,
            restart_of: restart.map(|(of, _)| of.to_owned()),
            restarts: restart.map_or(0, |(_, n)| n),
        };
        let mut env = vec![("WYSS_NATIVE_SESSION_ID".to_owned(), id.clone())];
        if version_guard {
            env.push((
                "WYSS_NATIVE_VERSION_GUARD".to_owned(),
                guard_marker.to_string_lossy().into_owned(),
            ));
        }
        env.extend(self.inner.extra_env.iter().cloned());
        // Remove an M9 default durably before a legacy provider can become the selected
        // project agent. If persistence is unavailable, do not launch a provider and leave a
        // stale OpenCode/Hermes default shadowing the newly chosen Claude/Codex session.
        let prior_extended_default = if request.agent_id.is_legacy() {
            let mut guard = lock(&self.inner.extended_registry);
            if let Some(extended) = guard.as_mut() {
                match extended.clear_project_default(&project.to_string_lossy()) {
                    Ok(previous) => Some(previous),
                    Err(_) => {
                        self.inner
                            .m9_storage_degraded
                            .store(true, Ordering::Release);
                        self.inner
                            .m9_default_clear_blocked
                            .store(true, Ordering::Release);
                        return Err(NativeError::new("store_unavailable"));
                    }
                }
            } else {
                None
            }
        } else {
            None
        };
        let project_key = project.to_string_lossy().into_owned();
        let prior_recovery_override =
            if request.agent_id.is_legacy() && lock(&self.inner.extended_registry).is_none() {
                let previous = lock(&self.inner.recovered_defaults)
                    .get(&project_key)
                    .copied();
                if previous.is_some() {
                    let existed = ExtendedRegistry::mark_default_overridden(
                        &self.inner.base.join("registry"),
                        &project_key,
                    )
                    .map_err(|_| NativeError::new("store_unavailable"))?;
                    lock(&self.inner.recovered_defaults).remove(&project_key);
                    Some((existed, previous))
                } else {
                    // A launch for a project with no recovered M9 default needs no tombstone.
                    // This keeps legacy launches independent of optional M9 storage and avoids
                    // filling the bounded override set with irrelevant project paths.
                    None
                }
            } else {
                None
            };
        let launched = terminal.create_launch(
            CreateTerminal {
                size: request.size,
                cwd: Some(session.project.clone()),
            },
            Launch {
                program: made.program,
                args: made.args,
                env,
                remove_env: self.inner.scrub.clone(),
            },
        );
        let info = match launched {
            Ok(info) => info,
            Err(error) => {
                if let Some((existed, previous)) = prior_recovery_override {
                    self.restore_recovered_default_override(&project_key, existed, previous);
                }
                if let Some(Some(previous)) = prior_extended_default
                    && let Some(extended) = lock(&self.inner.extended_registry).as_mut()
                    && extended
                        .set_project_default(&project.to_string_lossy(), previous)
                        .is_err()
                {
                    self.inner
                        .m9_storage_degraded
                        .store(true, Ordering::Release);
                }
                let _ = fs::remove_dir_all(&dir);
                session.state = State::FailedStart;
                session.ended_at = Some(store::now());
                session.error = Some(format!("{:?}", error.code));
                let code = if error.code == ErrorCode::LimitReached {
                    "limit_reached"
                } else {
                    "spawn_failed"
                };
                self.record(session);
                return Err(NativeError::new(code));
            }
        };
        if version_guard && !await_version_guard(terminal, &info.terminal_id, &guard_marker) {
            let _ = terminal.close(&info.terminal_id);
            if let Some((existed, previous)) = prior_recovery_override {
                self.restore_recovered_default_override(&project_key, existed, previous);
            }
            if let Some(Some(previous)) = prior_extended_default
                && let Some(extended) = lock(&self.inner.extended_registry).as_mut()
                && extended
                    .set_project_default(&project.to_string_lossy(), previous)
                    .is_err()
            {
                self.inner
                    .m9_storage_degraded
                    .store(true, Ordering::Release);
            }
            session.state = State::FailedStart;
            session.ended_at = Some(store::now());
            session.error = Some("provider_version_unverified".to_owned());
            self.record(session);
            let _ = fs::remove_dir_all(&dir);
            return Err(NativeError::new("provider_version_unverified"));
        }
        session.state = State::Running;
        lock(&self.inner.live).insert(
            id.clone(),
            Live {
                terminal: info.terminal_id.clone(),
                pid: info.observed_pid,
                last: None,
                usage: None,
            },
        );
        {
            let mut registry = lock(&self.inner.registry);
            let project = registry.project_mut(&session.project);
            project.last_used = now;
            if session.agent_id.is_legacy() {
                project.default_agent = Some(session.agent_id);
            }
            let _ = registry.save();
        }
        if !session.agent_id.is_legacy()
            && let Some(extended) = lock(&self.inner.extended_registry).as_mut()
            && extended
                .set_project_default(&session.project, session.agent_id)
                .is_err()
        {
            // The provider is already running. Keep it alive, but report M9 storage as
            // unavailable and do not claim that its default was saved.
            self.inner
                .m9_storage_degraded
                .store(true, Ordering::Release);
        }
        let view = self.view(
            &session,
            Some(info.terminal_id.as_str()),
            info.observed_pid,
            None,
        );
        self.record(session);
        Ok(StartReply {
            session: view,
            terminal: info,
        })
    }

    fn record(&self, session: Session) {
        if session.agent_id.is_legacy() {
            let mut registry = lock(&self.inner.registry);
            registry.add(session);
            let _ = registry.save();
        } else {
            let session_id = session.id.clone();
            let mut extended_guard = lock(&self.inner.extended_registry);
            if let Some(extended) = extended_guard.as_mut()
                && extended.add(session.clone()).is_err()
            {
                self.inner
                    .m9_storage_degraded
                    .store(true, Ordering::Release);
                if extended.session(&session_id).is_none() {
                    lock(&self.inner.volatile_sessions).insert(session_id, session);
                }
            }
        }
    }

    fn stored_session(&self, id: &str) -> Option<Session> {
        lock(&self.inner.registry)
            .session(id)
            .cloned()
            .or_else(|| {
                lock(&self.inner.extended_registry)
                    .as_ref()
                    .and_then(|extended| extended.session(id))
                    .cloned()
            })
            .or_else(|| lock(&self.inner.volatile_sessions).get(id).cloned())
            .or_else(|| {
                self.inner
                    .recovered_sessions
                    .iter()
                    .find(|session| session.id == id)
                    .cloned()
            })
    }

    fn view(
        &self,
        session: &Session,
        terminal: Option<&str>,
        pid: Option<u32>,
        usage: Option<Usage>,
    ) -> SessionView {
        SessionView {
            session: session.clone(),
            plane: "native",
            terminal_id: terminal.map(str::to_owned),
            pid,
            usage,
        }
    }

    /// Stops a running session: its terminal is closed, which ends the process group.
    pub fn stop(&self, terminal: &TerminalService, id: &str) -> Result<(), NativeError> {
        if !id_ok(id) {
            return Err(NativeError::new("session_unknown"));
        }
        let live = {
            let live = lock(&self.inner.live);
            live.get(id).map(|l| l.terminal.clone())
        };
        {
            let session = self
                .stored_session(id)
                .ok_or_else(|| NativeError::new("session_unknown"))?;
            if session.state != State::Running {
                return Err(NativeError::new("not_running"));
            }
        }
        if let Some(terminal_id) = live {
            // An already closed terminal is a stopped session as well.
            match terminal.close(&terminal_id) {
                Ok(_) => {}
                Err(error) if error.code == ErrorCode::AlreadyClosed => {}
                Err(_) => return Err(NativeError::new("stop_failed")),
            }
        }
        self.finish(id, State::Stopped, None, None);
        Ok(())
    }

    /// A new session with the same agent, project and profile. The old one is stopped only
    /// after the replacement has started, so stale previews or spawn failures leave it usable.
    pub fn restart(
        &self,
        terminal: &TerminalService,
        id: &str,
        size: TerminalSize,
        plan_token: Option<String>,
        expected_cli_version: Option<String>,
    ) -> Result<StartReply, NativeError> {
        if !id_ok(id) {
            return Err(NativeError::new("session_unknown"));
        }
        let old = self
            .stored_session(id)
            .ok_or_else(|| NativeError::new("session_unknown"))?;
        let was_running = old.state == State::Running;
        let reply = self.start_with(
            terminal,
            StartRequest {
                agent_id: old.agent_id,
                project: old.project.clone(),
                profile_id: old.profile.map(|p| p.id),
                size,
                brief: None,
                plan_token,
                expected_cli_version,
            },
            Some((id, old.restarts + 1)),
        )?;
        if was_running {
            // If the old terminal could not be closed, keep both records visible. The new
            // session is already live, so returning an error here would hide a successful start.
            let _ = self.stop(terminal, id);
        }
        Ok(reply)
    }

    /// Removes the record of a session that is not running.
    pub fn forget(&self, id: &str) -> Result<(), NativeError> {
        let mut registry = lock(&self.inner.registry);
        let mut extended = lock(&self.inner.extended_registry);
        let (finished, in_extended) = if let Some(session) = registry.session(id) {
            (session.state.finished(), false)
        } else if let Some(session) = extended.as_ref().and_then(|store| store.session(id)) {
            (session.state.finished(), true)
        } else if self
            .inner
            .recovered_sessions
            .iter()
            .any(|session| session.id == id)
        {
            return Err(NativeError::new("store_unavailable"));
        } else {
            return Err(NativeError::new("session_unknown"));
        };
        if !finished {
            return Err(NativeError::new("still_running"));
        }
        if in_extended {
            let Some(extended) = extended.as_mut() else {
                return Err(NativeError::new("store_unavailable"));
            };
            extended.forget(id);
            extended
                .save()
                .map_err(|_| NativeError::new("store_unavailable"))
        } else {
            registry.forget(id);
            registry
                .save()
                .map_err(|_| NativeError::new("store_unavailable"))
        }
    }

    fn finish(&self, id: &str, state: State, code: Option<u32>, signal: Option<String>) {
        lock(&self.inner.live).remove(id);
        {
            let mut registry = lock(&self.inner.registry);
            if let Some(session) = registry.session_mut(id) {
                if !session.state.finished() {
                    session.state = state;
                    session.ended_at = Some(store::now());
                    session.exit_code = code;
                    session.exit_signal = signal.clone();
                }
                let _ = registry.save();
            } else {
                let mut extended = lock(&self.inner.extended_registry);
                if let Some(extended) = extended.as_mut() {
                    let found = if let Some(session) = extended.session_mut(id) {
                        if !session.state.finished() {
                            session.state = state;
                            session.ended_at = Some(store::now());
                            session.exit_code = code;
                            session.exit_signal = signal.clone();
                        }
                        true
                    } else {
                        false
                    };
                    if found {
                        let _ = extended.save();
                    } else if let Some(session) = lock(&self.inner.volatile_sessions).get_mut(id)
                        && !session.state.finished()
                    {
                        session.state = state;
                        session.ended_at = Some(store::now());
                        session.exit_code = code;
                        session.exit_signal = signal;
                    }
                } else if let Some(session) = lock(&self.inner.volatile_sessions).get_mut(id)
                    && !session.state.finished()
                {
                    session.state = state;
                    session.ended_at = Some(store::now());
                    session.exit_code = code;
                    session.exit_signal = signal;
                }
            }
        }
        if id_ok(id) {
            let _ = fs::remove_dir_all(self.inner.base.join("sessions").join(id));
        }
    }

    /// The native session whose terminal this is, while it runs.
    pub fn tag_of(&self, terminal: &str) -> Option<NativeTag> {
        let session_id = lock(&self.inner.live)
            .iter()
            .find(|(_, l)| l.terminal.as_str() == terminal)
            .map(|(id, _)| id.clone())?;
        let session = self.stored_session(&session_id)?;
        Some(NativeTag {
            session_id,
            agent_name: session.agent_name.clone(),
            project: session.project.clone(),
            profile: session.profile.as_ref().map(|p| p.name.clone()),
            through_shell: session.through_shell,
        })
    }

    /// The terminal of a running native session, and its record.
    pub fn running_terminal(&self, session_id: &str) -> Option<(String, Session)> {
        let terminal = lock(&self.inner.live)
            .get(session_id)?
            .terminal
            .as_str()
            .to_owned();
        let session = self.stored_session(session_id)?;
        Some((terminal, session))
    }

    /// A finished or running session's record.
    pub fn session_record(&self, session_id: &str) -> Option<Session> {
        self.stored_session(session_id)
    }

    /// Whether any native session is running or being started.
    pub fn any_running(&self) -> bool {
        lock(&self.inner.registry)
            .data
            .sessions
            .iter()
            .any(|s| !s.state.finished())
            || lock(&self.inner.extended_registry)
                .as_ref()
                .is_some_and(|store| store.sessions().iter().any(|s| !s.state.finished()))
    }

    // ---- what is running now --------------------------------------------------------------

    /// Every session, each marked with its plane, with what the terminal says now. `sample`
    /// asks for processor and memory too; the window asks only while the overview shows.
    pub fn sessions(&self, terminal: &TerminalService, sample: bool) -> Vec<SessionView> {
        self.sessions_view(terminal, sample, false)
    }

    pub fn sessions_extended(&self, terminal: &TerminalService, sample: bool) -> Vec<SessionView> {
        self.sessions_view(terminal, sample, true)
    }

    fn sessions_view(
        &self,
        terminal: &TerminalService,
        sample: bool,
        include_extended: bool,
    ) -> Vec<SessionView> {
        let running: Vec<(String, TerminalId)> = lock(&self.inner.live)
            .iter()
            .map(|(id, live)| (id.clone(), live.terminal.clone()))
            .collect();
        for (id, terminal_id) in running {
            match terminal.status(&terminal_id) {
                Ok(info) => match info.state {
                    TerminalState::Exited | TerminalState::Closed | TerminalState::Failed => {
                        let (code, signal) = info
                            .exit
                            .as_ref()
                            .map_or((None, None), |e| (Some(e.code), e.signal.clone()));
                        // A closed terminal was closed on purpose (the window's tab, or Stop):
                        // the process was ended, it did not end by itself.
                        let state = match info.state {
                            TerminalState::Failed => State::FailedStart,
                            TerminalState::Closed => State::Stopped,
                            _ => State::Exited,
                        };
                        self.finish(&id, state, code, signal);
                    }
                    _ => {
                        if let Some(live) = lock(&self.inner.live).get_mut(&id) {
                            live.pid = info.observed_pid;
                        }
                    }
                },
                // The terminal service does not know it: nothing says how it ended.
                Err(_) => self.finish(&id, State::Lost, None, None),
            }
        }
        if sample {
            self.sample();
        }
        let registry = lock(&self.inner.registry);
        let extended = lock(&self.inner.extended_registry);
        let volatile = lock(&self.inner.volatile_sessions);
        let live = lock(&self.inner.live);
        registry
            .data
            .sessions
            .iter()
            .chain(
                include_extended
                    .then(|| {
                        extended
                            .as_ref()
                            .into_iter()
                            .flat_map(|store| store.sessions())
                    })
                    .into_iter()
                    .flatten(),
            )
            .chain(
                include_extended
                    .then_some(self.inner.recovered_sessions.iter())
                    .into_iter()
                    .flatten(),
            )
            .chain(
                include_extended
                    .then_some(volatile.values())
                    .into_iter()
                    .flatten(),
            )
            .filter(|session| include_extended || session.agent_id.is_legacy())
            .map(|session| {
                let observed = live.get(&session.id);
                self.view(
                    session,
                    observed.map(|l| l.terminal.as_str()),
                    observed.and_then(|l| l.pid),
                    observed.and_then(|l| l.usage),
                )
            })
            .collect()
    }

    fn sample(&self) {
        let mut live = lock(&self.inner.live);
        if live.is_empty() {
            return;
        }
        // One walk of `/proc` for every session.
        let Some(snapshot) = resources::Snapshot::take() else {
            return;
        };
        for entry in live.values_mut() {
            let Some(pid) = entry.pid.and_then(|p| i32::try_from(p).ok()) else {
                continue;
            };
            let Some(now) = snapshot.tree(pid) else {
                entry.usage = None;
                continue;
            };
            let at = Instant::now();
            let cpu = entry.last.as_ref().and_then(|(before, then)| {
                resources::cpu_percent(before, &now, at.duration_since(*then).as_secs_f32())
            });
            entry.usage = Some(Usage {
                cpu_percent: cpu.or(entry.usage.and_then(|u| u.cpu_percent)),
                rss_bytes: now.rss_bytes,
                processes: now.processes,
            });
            entry.last = Some((now, at));
        }
    }
}

/// Do not admit a shell-started profile as a session until the post-startup PATH check has
/// completed. This makes a restart keep its old session when the shell rejects a new build.
fn await_version_guard(
    terminal: &TerminalService,
    terminal_id: &TerminalId,
    marker: &Path,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        let verified = fs::symlink_metadata(marker).is_ok_and(|meta| meta.file_type().is_file())
            && fs::read(marker).is_ok_and(|bytes| bytes == b"ok\n");
        if verified {
            return true;
        }
        let Ok(info) = terminal.status(terminal_id) else {
            return false;
        };
        if !matches!(info.state, TerminalState::Starting | TerminalState::Running) {
            return false;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
impl NativeService {
    /// Like `open`, with variables added to every launch (a PATH holding a fake agent).
    pub fn open_with_env(
        base: PathBuf,
        home: PathBuf,
        path_var: Option<OsString>,
        extra_env: Vec<(String, String)>,
        shell: PathBuf,
    ) -> Option<Self> {
        let service = Self::open(base, home, path_var)?;
        let mut inner = Arc::try_unwrap(service.inner).ok()?;
        inner.extra_env = extra_env;
        inner.shell = Some(shell);
        inner.scrub.push("WYSS_SCRUB_TEST_MARKER".to_owned());
        Some(Self {
            inner: Arc::new(inner),
        })
    }
}
