//! The native side of the Console: starts an agent in a terminal through the user's shell,
//! keeps the registry, resolves profiles. It takes the terminal service as an argument and
//! holds no thread: state is refreshed from the terminal when the overview asks, and process
//! resources are sampled only then (nothing runs while the view is hidden).
use super::{
    catalogue::{self, AGENTS, AgentId},
    profile::{self, LaunchPlan, Word},
    profiles::ProfileStore,
    recipe::{self, family},
    registry::{ProfileRef, Registry, Session, State},
    resources::{self, Usage},
    store,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
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
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviewView {
    pub applied: Vec<String>,
    pub with_cost: Vec<String>,
    pub not_applicable: Vec<String>,
    pub can_start: bool,
    pub through_shell: bool,
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
        let (registry, _lost) = Registry::open(base.join("registry")).ok()?;
        let profiles = ProfileStore::open(base.join("profiles")).ok()?;
        let sessions = base.join("sessions");
        // Nothing is running: every generated directory is stale.
        let _ = fs::remove_dir_all(&sessions);
        store::private_dir(&sessions).ok()?;
        Some(Self {
            inner: Arc::new(Inner {
                base,
                registry: Mutex::new(registry),
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
                let version = path.as_deref().and_then(catalogue::probe_version);
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
        let home = self.inner.home.to_string_lossy().into_owned();
        let registry = lock(&self.inner.registry);
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
                    shown.default_agent = project.default_agent;
                    shown.default_profile.clone_from(&project.default_profile);
                    shown.last_used = project.last_used;
                }
                return;
            }
            projects.push(ProjectView {
                path: path.to_owned(),
                home: false,
                default_agent: project.and_then(|p| p.default_agent),
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
            shown.default_agent = project.default_agent;
            shown.default_profile.clone_from(&project.default_profile);
            shown.last_used = project.last_used;
        }
        LauncherView {
            agents: self.agents(),
            shell: self.shell(),
            projects,
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
    ) -> Result<(LaunchPlan, Option<ProfileRef>), NativeError> {
        let Some(id) = profile_id else {
            return Ok((LaunchPlan::default(), None));
        };
        let (resolved, version) = self
            .inner
            .profiles
            .resolve(id)
            .map_err(|_| NativeError::new("profile_unknown"))?;
        let applies = resolved.body.applies_to.contains(&agent);
        let plan = profile::plan(&resolved, agent, applies);
        Ok((
            plan,
            Some(ProfileRef {
                id: id.to_owned(),
                name: resolved.body.name,
                version,
            }),
        ))
    }

    pub fn preview(
        &self,
        agent: AgentId,
        profile_id: Option<&str>,
    ) -> Result<PreviewView, NativeError> {
        let (plan, _) = self.plan_for(agent, profile_id)?;
        Ok(PreviewView {
            can_start: plan.not_applicable.is_empty(),
            applied: plan.applied,
            with_cost: plan.with_cost,
            not_applicable: plan.not_applicable,
            through_shell: self.shell().is_some_and(|s| s.through_shell),
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
        let project = self.project_allowed(&request.project)?;
        let agent = self
            .agents()
            .into_iter()
            .find(|a| a.agent_id == request.agent_id)
            .filter(|a| a.installed)
            .ok_or_else(|| NativeError::new("agent_not_installed"))?;
        let (plan, profile_ref) = self.plan_for(request.agent_id, request.profile_id.as_deref())?;
        if !plan.not_applicable.is_empty() {
            return Err(NativeError {
                code: "profile_not_applicable",
                reasons: plan.not_applicable,
            });
        }
        let shell = self
            .user_shell()
            .map_err(|_| NativeError::new("spawn_failed"))?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let dir = self.inner.base.join("sessions").join(&id);
        let mut words = vec![agent.executable.to_owned()];
        for word in &plan.words {
            match word {
                Word::Literal(text) => words.push(text.clone()),
                Word::Overlay => {
                    words.push(dir.join("settings.json").to_string_lossy().into_owned())
                }
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
        let made = recipe::recipe(&shell, &words, &direct);
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
        let launched = terminal.create_launch(
            CreateTerminal {
                size: request.size,
                cwd: Some(session.project.clone()),
            },
            Launch {
                program: made.program,
                args: made.args,
                env: std::iter::once(("WYSS_NATIVE_SESSION_ID".to_owned(), id.clone()))
                    .chain(self.inner.extra_env.iter().cloned())
                    .collect(),
                remove_env: self.inner.scrub.clone(),
            },
        );
        let info = match launched {
            Ok(info) => info,
            Err(error) => {
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
            project.default_agent = Some(session.agent_id);
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
        let mut registry = lock(&self.inner.registry);
        registry.add(session);
        let _ = registry.save();
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
            let registry = lock(&self.inner.registry);
            let session = registry
                .session(id)
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

    /// A new session with the same agent, project and profile. The old one is stopped first
    /// when it still runs; it stays in the registry as it ended.
    pub fn restart(
        &self,
        terminal: &TerminalService,
        id: &str,
        size: TerminalSize,
    ) -> Result<StartReply, NativeError> {
        if !id_ok(id) {
            return Err(NativeError::new("session_unknown"));
        }
        let old = lock(&self.inner.registry)
            .session(id)
            .cloned()
            .ok_or_else(|| NativeError::new("session_unknown"))?;
        if old.state == State::Running {
            self.stop(terminal, id)?;
        }
        self.start_with(
            terminal,
            StartRequest {
                agent_id: old.agent_id,
                project: old.project.clone(),
                profile_id: old.profile.map(|p| p.id),
                size,
            },
            Some((id, old.restarts + 1)),
        )
    }

    /// Removes the record of a session that is not running.
    pub fn forget(&self, id: &str) -> Result<(), NativeError> {
        let mut registry = lock(&self.inner.registry);
        let session = registry
            .session(id)
            .ok_or_else(|| NativeError::new("session_unknown"))?;
        if !session.state.finished() {
            return Err(NativeError::new("still_running"));
        }
        registry.forget(id);
        registry
            .save()
            .map_err(|_| NativeError::new("store_unavailable"))
    }

    fn finish(&self, id: &str, state: State, code: Option<u32>, signal: Option<String>) {
        lock(&self.inner.live).remove(id);
        {
            let mut registry = lock(&self.inner.registry);
            if let Some(session) = registry.session_mut(id)
                && !session.state.finished()
            {
                session.state = state;
                session.ended_at = Some(store::now());
                session.exit_code = code;
                session.exit_signal = signal;
                let _ = registry.save();
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
        let registry = lock(&self.inner.registry);
        let session = registry.session(&session_id)?;
        Some(NativeTag {
            session_id,
            agent_name: session.agent_name.clone(),
            project: session.project.clone(),
            profile: session.profile.as_ref().map(|p| p.name.clone()),
            through_shell: session.through_shell,
        })
    }

    /// Whether any native session is running or being started.
    pub fn any_running(&self) -> bool {
        lock(&self.inner.registry)
            .data
            .sessions
            .iter()
            .any(|s| !s.state.finished())
    }

    // ---- what is running now --------------------------------------------------------------

    /// Every session, each marked with its plane, with what the terminal says now. `sample`
    /// asks for processor and memory too; the window asks only while the overview shows.
    pub fn sessions(&self, terminal: &TerminalService, sample: bool) -> Vec<SessionView> {
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
        let live = lock(&self.inner.live);
        registry
            .data
            .sessions
            .iter()
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
