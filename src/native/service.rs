//! The native side of the Console: starts an agent in a terminal through the user's shell,
//! keeps the registry, resolves profiles. It takes the terminal service as an argument and
//! holds no thread: state is refreshed from the terminal when the overview asks, and process
//! resources are sampled only then (nothing runs while the view is hidden).
use super::{
    arsenal::{self, ArsenalLaunch},
    capabilities, claude_activity, prompt_queue,
    catalogue::{self, AGENTS, AgentId},
    extended_registry::{ExtendedRegistry, RecoveryRead},
    launch_home, launch_home_codex,
    profile::{self, LaunchPlan, Word},
    profiles::ProfileStore,
    recipe::{self, family},
    registry::{M9Open, ProfileRef, Registry, Session, State},
    resources::{self, Usage},
    store, usage,
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
/// How long a preview's token can start the plan it shows.
const PLAN_TOKEN_FOR: Duration = Duration::from_secs(120);
/// Most preview tokens kept at once; the oldest goes first.
const PLAN_TOKENS_MAX: usize = 64;

/// A random, unguessable token (32 bytes from the OS, hex).
fn random_token() -> Result<String, NativeError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| NativeError::new("internal"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn project_instruction_excludes(
    project: &Path,
    owner: &Path,
    launch_home: Option<&Path>,
) -> Result<Vec<String>, NativeError> {
    let project =
        fs::canonicalize(project).map_err(|_| NativeError::new("arsenal_project_glob"))?;
    let mut protected = vec![
        owner.to_path_buf(),
        fs::canonicalize(owner).unwrap_or_else(|_| owner.to_path_buf()),
    ];
    if let Some(home) = launch_home {
        protected.push(home.to_path_buf());
        protected.push(fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf()));
    }
    // Matching a global instruction target removes the global and role as well.
    let mut targets = Vec::new();
    for name in ["CLAUDE.md", "AGENTS.md"] {
        if let Ok(target) = fs::canonicalize(owner.join(name)) {
            targets.push(target);
        }
    }
    let mut rules = vec![owner.join("rules")];
    while let Some(directory) = rules.pop() {
        for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
            if let Ok(target) = fs::canonicalize(entry.path()) {
                targets.push(target);
            }
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                rules.push(entry.path());
            }
        }
    }
    let mut result = Vec::new();
    let mut add = |path: PathBuf, subtree: bool| -> Result<(), NativeError> {
        let canonical = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        let text = path
            .to_str()
            .ok_or_else(|| NativeError::new("arsenal_project_glob"))?;
        if text.chars().any(|c| "*?[]{}\\".contains(c)) {
            return Err(NativeError::new("arsenal_project_glob"));
        }
        if protected.iter().any(|p| {
            path.starts_with(p)
                || canonical.starts_with(p)
                || (subtree && (p.starts_with(&path) || p.starts_with(&canonical)))
        }) || targets.iter().any(|target| {
            target == &path
                || target == &canonical
                || (subtree && (target.starts_with(&path) || target.starts_with(&canonical)))
        }) {
            return Ok(());
        }
        result.push(if subtree {
            format!("{}/**", text.trim_end_matches('/'))
        } else {
            text.to_owned()
        });
        Ok(())
    };
    add(project.clone(), true)?;
    for ancestor in project.ancestors() {
        add(ancestor.join("CLAUDE.md"), false)?;
        add(ancestor.join("CLAUDE.local.md"), false)?;
        add(ancestor.join(".claude/CLAUDE.md"), false)?;
        add(ancestor.join(".claude/rules"), true)?;
    }
    Ok(result)
}

fn project_instruction_overlay(overlay: &str, excludes: &[String]) -> Result<String, NativeError> {
    let mut value: serde_json::Value =
        serde_json::from_str(overlay).map_err(|_| NativeError::new("store_unavailable"))?;
    value
        .as_object_mut()
        .ok_or_else(|| NativeError::new("store_unavailable"))?
        .insert("claudeMdExcludes".into(), serde_json::json!(excludes));
    serde_json::to_string_pretty(&value).map_err(|_| NativeError::new("store_unavailable"))
}

#[cfg(test)]
mod project_instruction_tests {
    use super::*;

    #[test]
    fn project_excludes_protect_global_symlink_targets() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("role-targets-{}", uuid::Uuid::new_v4()));
        let owner = root.join(".claude");
        let project = root.join("repo");
        fs::create_dir_all(owner.join("rules")).unwrap();
        fs::create_dir_all(&project).unwrap();
        fs::write(root.join("CLAUDE.md"), "global").unwrap();
        fs::write(root.join("CLAUDE.local.md"), "rule").unwrap();
        symlink(root.join("CLAUDE.md"), owner.join("CLAUDE.md")).unwrap();
        symlink(root.join("CLAUDE.local.md"), owner.join("rules/global.md")).unwrap();
        let list = project_instruction_excludes(&project, &owner, None).unwrap();
        assert!(!list.contains(&root.join("CLAUDE.md").display().to_string()));
        assert!(!list.contains(&root.join("CLAUDE.local.md").display().to_string()));
        assert!(!list.contains(&owner.join("CLAUDE.md").display().to_string()));
        assert!(!list.contains(&format!("{}/**", owner.join("rules").display())));
        assert!(list.contains(&format!("{}/**", project.display())));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_folder_that_holds_the_owner_or_the_launch_home_never_gets_a_subtree_pattern() {
        let root = std::env::temp_dir().join(format!("role-holds-{}", uuid::Uuid::new_v4()));
        let owner = root.join(".claude");
        let home = root.join(".local/share/wyss-console/launch-homes/p1");
        fs::create_dir_all(&owner).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::write(owner.join("CLAUDE.md"), "global").unwrap();
        let canonical = fs::canonicalize(&root).unwrap();
        // The folder is the owner's HOME: it holds both.
        for list in [
            project_instruction_excludes(&root, &owner, Some(&home)).unwrap(),
            project_instruction_excludes(&root.join(".local"), &owner, Some(&home)).unwrap(),
        ] {
            for pattern in &list {
                let base = pattern.trim_end_matches("/**");
                for kept in [owner.join("CLAUDE.md"), home.clone()] {
                    let kept = fs::canonicalize(&kept).unwrap();
                    assert!(
                        !(pattern.ends_with("/**") && kept.starts_with(base)),
                        "{pattern} covers {}",
                        kept.display()
                    );
                    assert_ne!(Path::new(pattern), kept.as_path(), "{pattern}");
                }
            }
            assert!(!list.contains(&format!("{}/**", canonical.display())));
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_excludes_ancestors_and_protects_owner() {
        let root = std::env::temp_dir().join(format!("role-globs-{}", uuid::Uuid::new_v4()));
        let owner = root.join(".claude");
        let project = root.join("parent/project");
        fs::create_dir_all(&owner).unwrap();
        fs::create_dir_all(&project).unwrap();
        let list = project_instruction_excludes(&project, &owner, None).unwrap();
        assert!(list.contains(&format!("{}/**", project.display())));
        let overlay =
            project_instruction_overlay(r#"{"skillOverrides":{"design":"off"}}"#, &list).unwrap();
        let value: serde_json::Value = serde_json::from_str(&overlay).unwrap();
        assert_eq!(value["skillOverrides"]["design"], "off");
        assert!(
            value["claudeMdExcludes"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!(format!("{}/**", project.display())))
        );
        assert!(list.contains(&root.join("parent/CLAUDE.md").display().to_string()));
        assert!(
            !list
                .iter()
                .any(|item| item.starts_with(&owner.display().to_string()))
        );
        let bad = root.join("project[bad]");
        fs::create_dir(&bad).unwrap();
        assert_eq!(
            project_instruction_excludes(&bad, &owner, None)
                .unwrap_err()
                .code,
            "arsenal_project_glob"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

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
    /// Older clients using the shared provider list do not know this field. Extended clients use it to distinguish an older
    /// host from a damaged/unavailable extended sidecar without hiding the shared-registry providers.
    pub m9_native_status: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviewView {
    pub applied: Vec<String>,
    pub with_cost: Vec<String>,
    pub not_applicable: Vec<String>,
    pub can_start: bool,
    pub through_shell: bool,
    pub provider_version: Option<String>,
    pub bypass: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub omitted_options: Vec<String>,
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
    /// A reviewed handoff brief: the agent's first message. At most 16 KiB, starts with
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
    /// Installed skills an Arsenal selection switches off for this session. Bound to the plan
    /// token like a profile, and refused for any build without its own exclusion evidence.
    #[serde(default)]
    pub arsenal: Option<ArsenalLaunch>,
    /// Gives this session the Wyss delegation tool (Claude Code and Codex): it may start agents of
    /// either provider within the owner's delegation policy.
    #[serde(default)]
    pub delegation: bool,
}

/// A session as the window sees it: the registry's record plus what is observed now.
#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    #[serde(flatten)]
    pub session: Session,
    pub plane: &'static str,
    /// Whether the current profile plan for a restart grants bypass.
    pub restart_bypass: Option<bool>,
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
    /// Stored skills of the preset that were not prepared, each with its reason. The agent
    /// started without them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skills_skipped: Vec<arsenal::SkippedSkill>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub omitted_options: Vec<String>,
}

/// Whether a launch has a folder of its own only to hold copied skills: no environment and no
/// role of the preset's own need it.
fn home_only_for_copies(launch: &ArsenalLaunch, agent: AgentId) -> bool {
    !launch.materialize.is_empty()
        && (!matches!(
            launch.launch_instructions,
            Some(crate::arsenal::role_instructions::Choice::Role { .. })
        ) || (agent == AgentId::ClaudeCode && launch.project_instructions_reviewed))
        && match agent {
            AgentId::ClaudeCode => launch.home.as_ref().is_none_or(|home| home.is_default()),
            AgentId::Codex => launch
                .codex_home
                .as_ref()
                .is_none_or(|home| !home.needs_launch_home()),
            _ => false,
        }
}

fn skip_all(skipped: &mut Vec<arsenal::SkippedSkill>, launch: &ArsenalLaunch, reason: &str) {
    skipped.extend(launch.materialize.iter().map(|item| arsenal::SkippedSkill {
        name: item.name.clone(),
        reason: reason.to_owned(),
    }));
}

/// The stored skills of a launch, read from the Arsenal library by the host itself.
fn read_copies(
    launch: &ArsenalLaunch,
    skipped: &mut Vec<arsenal::SkippedSkill>,
) -> Vec<arsenal::ReadySkill> {
    if launch.materialize.is_empty() {
        return Vec::new();
    }
    match crate::arsenal::open_read_only() {
        Ok(store) => {
            let (ready, unread) = arsenal::read_skills(&store, &launch.materialize);
            skipped.extend(unread);
            ready
        }
        Err(_) => {
            skip_all(skipped, launch, "library_unreadable");
            Vec::new()
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ActivitySession {
    pub id: String,
    pub agent: AgentId,
    pub name: String,
    pub started_at: u64,
    pub terminal_id: Option<String>,
    pub context: Option<f64>,
    pub turn: Option<prompt_queue::Turn>,
    pub input_revision: Option<u64>,
    pub queue_revision: Option<u64>,
    pub queued: usize,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ActivityView {
    pub plans: usage::Plans,
    pub sessions: Vec<ActivitySession>,
}

struct Live {
    terminal: TerminalId,
    agent: AgentId,
    pid: Option<u32>,
    last: Option<(resources::Sample, Instant)>,
    usage: Option<Usage>,
    codex_rollouts: Option<(PathBuf, PathBuf)>,
    queue_attempt: Option<prompt_queue::TurnId>,
}

struct Inner {
    base: PathBuf,
    registry: Mutex<Registry>,
    extended_registry: Mutex<Option<ExtendedRegistry>>,
    /// Sticky for this service lifetime after an extended sidecar write reports failure. It keeps
    /// optional providers unavailable until reopen while Claude/Codex launches remain independent.
    extended_storage_degraded: AtomicBool,
    /// Projects whose actual extended default could not be removed durably. Only those projects
    /// refuse Claude/Codex launches; the entries end with a reopen, which re-reads the sidecar.
    blocked_default_clears: Mutex<BTreeSet<String>>,
    /// extended sessions whose process started but whose sidecar record could not be committed.
    /// This volatile entry keeps the live process manageable without claiming durability.
    volatile_sessions: Mutex<HashMap<String, Session>>,
    recovered_sessions: Vec<Session>,
    recovered_defaults: Mutex<BTreeMap<String, AgentId>>,
    profiles: ProfileStore,
    home: PathBuf,
    path_var: Option<OsString>,
    chosen: Mutex<Vec<String>>,
    live: Mutex<HashMap<String, Live>>,
    readings: Mutex<usage::Usage>,
    prompts: Mutex<prompt_queue::Book>,
    starting: AtomicBool,
    /// Variables added to every launch, besides the session's id. Empty outside tests.
    extra_env: Vec<(String, String)>,
    /// A shell standing in for the user's. Never set outside tests.
    shell: Option<PathBuf>,
    /// Names removed from the environment an agent inherits.
    scrub: Vec<String>,
    /// Tokens a preview handed out: token -> (digest of the plan it showed, when it was issued).
    /// A start consumes the token of a matching plan; a token is never computed from the plan,
    /// so it cannot be forged or replayed.
    plan_tokens: Mutex<HashMap<String, (String, Instant)>>,
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
                // Copy pending extended records to bounded recovery storage before saving the
                // shared registry projection. If recovery cannot hold them, refuse startup and
                // leave the source registry intact.
                ExtendedRegistry::preserve_recovery(
                    &registry_dir,
                    &migration_sessions,
                    &pending_defaults,
                )
                .ok()?;
            }
            // Sidecar writes sync their parent directory before this source registry is
            // atomically replaced with the shared projection.
            registry.save().ok()?;
            if migration_complete
                && ExtendedRegistry::clear_recovery(&registry_dir).is_ok()
                && ExtendedRegistry::clear_default_overrides(&registry_dir).is_ok()
            {
                // All extended metadata is now represented by the durable sidecar.
            } else {
                // A valid but over-cap or otherwise unmergeable extended extension must not make
                // Claude/Codex unavailable. Preserve the current sidecar view plus every
                // immutable recovery/migration source and explicitly block extended admission.
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
            // extended storage is optional to shared-registry providers. Preserve any extended values still in
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
        // A sign-in token a host that stopped mid-start left behind goes first, even where the
        // rest of a stale directory cannot be removed.
        sweep_token_files(&sessions);
        // Nothing is running: every generated directory is stale.
        let _ = fs::remove_dir_all(&sessions);
        store::private_dir(&sessions).ok()?;
        Some(Self {
            inner: Arc::new(Inner {
                readings: Mutex::new(usage::Usage::new(&base)),
                prompts: Mutex::new(prompt_queue::Book::open(&base)),
                base,
                registry: Mutex::new(registry),
                extended_registry: Mutex::new(extended_registry),
                extended_storage_degraded: AtomicBool::new(false),
                blocked_default_clears: Mutex::new(BTreeSet::new()),
                volatile_sessions: Mutex::new(HashMap::new()),
                recovered_sessions,
                recovered_defaults: Mutex::new(recovered_defaults),
                profiles,
                home,
                path_var,
                chosen: Mutex::new(Vec::new()),
                live: Mutex::new(HashMap::new()),
                starting: AtomicBool::new(false),
                extra_env: Vec::new(),
                shell: None,
                scrub: SESSION_MARKERS.iter().map(|n| (*n).to_owned()).collect(),
                plan_tokens: Mutex::new(HashMap::new()),
            }),
        })
    }

    fn user_shell(&self) -> Result<PathBuf, wyss_console::terminal::contracts::TerminalError> {
        match &self.inner.shell {
            Some(shell) => Ok(shell.clone()),
            None => TerminalService::user_shell(),
        }
    }

    #[cfg(test)]
    #[cfg(test)]
    pub(crate) fn profiles(&self) -> &ProfileStore {
        &self.inner.profiles
    }

    // ---- what can be started --------------------------------------------------------------

    fn agent_path(&self, spec: &catalogue::AgentSpec) -> Option<PathBuf> {
        catalogue::discover(spec, self.inner.path_var.clone(), &self.inner.home)
    }

    fn catalogue_entry(&self, spec: &catalogue::AgentSpec) -> CatalogueEntry {
        let path = self.agent_path(spec);
        let version = path
            .as_deref()
            .and_then(|path| catalogue::cached_provider_version(spec.id, path));
        CatalogueEntry {
            agent_id: spec.id,
            name: spec.name,
            executable: spec.executable,
            installed: path.is_some(),
            path: path.map(|p| p.to_string_lossy().into_owned()),
            version,
        }
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

    fn recovered_default(&self, path: &str) -> Option<AgentId> {
        lock(&self.inner.recovered_defaults).get(path).copied()
    }

    fn launcher_view(&self, include_extended: bool) -> LauncherView {
        let home = self.inner.home.to_string_lossy().into_owned();
        let registry = lock(&self.inner.registry);
        let extended = include_extended.then(|| lock(&self.inner.extended_registry));
        let extension_ready = extended.as_ref().is_some_and(|stored| stored.is_some())
            && !self.inner.extended_storage_degraded.load(Ordering::Acquire);
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
                        .filter(|agent| include_extended || agent.fits_shared_registry());
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
                    .filter(|agent| include_extended || agent.fits_shared_registry()),
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
                .filter(|agent| include_extended || agent.fits_shared_registry());
            shown.default_profile.clone_from(&project.default_profile);
            shown.last_used = project.last_used;
        }
        // Project/storage facts above are already owned snapshots. Version discovery can
        // block on external CLIs; it must not hold storage locks or delay live readiness.
        drop(extended);
        drop(registry);
        LauncherView {
            agents: AGENTS
                .iter()
                .filter(|spec| spec.id.fits_shared_registry() || extension_ready)
                .map(|spec| self.catalogue_entry(spec))
                .collect(),
            shell: self.shell(),
            projects,
            m9_native_status,
        }
    }

    /// A folder a session may start in: one already known, or any real folder inside the home
    /// (a saved launch names it before any session has run there). A link out of the home is
    /// judged by where it leads.
    pub(super) fn project_allowed(&self, path: &str) -> Result<PathBuf, NativeError> {
        let known = path == self.inner.home.to_string_lossy()
            || lock(&self.inner.chosen).iter().any(|p| p == path)
            || lock(&self.inner.registry)
                .data
                .projects
                .iter()
                .any(|p| p.path == path);
        let invalid = || NativeError::new("project_invalid");
        if path.len() > 4096 {
            return Err(invalid());
        }
        let canonical = Path::new(path).canonicalize().map_err(|_| invalid())?;
        let in_home = self
            .inner
            .home
            .canonicalize()
            .is_ok_and(|home| canonical.starts_with(home));
        (canonical.is_dir() && (known || in_home))
            .then_some(canonical)
            .ok_or_else(invalid)
    }

    // ---- profiles applied -----------------------------------------------------------------

    /// The plan of a profile and an Arsenal exclusion together, the profile's reference, the
    /// token that binds both to a preview, and whether the profile alone changes the launch.
    fn plan_for(
        &self,
        agent: AgentId,
        profile_id: Option<&str>,
        launch: Option<&ArsenalLaunch>,
    ) -> Result<(LaunchPlan, Option<ProfileRef>, Option<String>, bool), NativeError> {
        let (mut plan, profile_ref, token) = self.profile_plan_for(agent, profile_id)?;
        let profile_effective = !plan.applied.is_empty() || !plan.with_cost.is_empty();
        let Some(launch) = launch else {
            return Ok((plan, profile_ref, token, profile_effective));
        };
        arsenal::apply(&mut plan, agent, launch);
        let material = serde_json::to_vec(&(agent, profile_id, &token, launch, &plan))
            .map_err(|_| NativeError::new("internal"))?;
        let token = format!("{:x}", Sha256::digest(material));
        Ok((plan, profile_ref, Some(token), profile_effective))
    }

    fn profile_plan_for(
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
        launch: Option<&ArsenalLaunch>,
    ) -> Result<PreviewView, NativeError> {
        let (mut plan, _, digest, profile_effective) = self.plan_for(agent, profile_id, launch)?;
        let path = AGENTS
            .iter()
            .find(|spec| spec.id == agent)
            .and_then(|spec| self.agent_path(spec));
        let detected = agent.fits_shared_registry();
        let provider_version = path.as_deref().and_then(|path| {
            if detected {
                catalogue::cached_provider_version(agent, path)
            } else {
                catalogue::probe_provider_version(agent, path)
            }
        });
        let omitted_options = if detected && (profile_id.is_some() || launch.is_some()) {
            let help = path
                .as_deref()
                .map(catalogue::cached_cli_help)
                .unwrap_or_default();
            capabilities::apply_help(&mut plan, agent, &help, true, launch)
        } else {
            Vec::new()
        };
        let version_ok = detected
            || (!profile_effective && launch.is_none())
            || check_launch_version(agent, provider_version.as_deref(), profile_effective).is_ok();
        let bypass = plan
            .with_cost
            .iter()
            .any(|cost| cost == profile::BYPASS_COST);
        let plan_token = digest
            .map(|digest| {
                self.issue_plan_token(if detected && bypass {
                    bypass_digest(&digest)
                } else {
                    digest
                })
            })
            .transpose()?;
        Ok(PreviewView {
            can_start: plan.not_applicable.is_empty() && version_ok,
            applied: plan.applied,
            with_cost: plan.with_cost,
            not_applicable: plan.not_applicable,
            through_shell: self.shell().is_some_and(|s| s.through_shell),
            provider_version,
            bypass,
            omitted_options,
            plan_token,
        })
    }

    /// Keeps a fresh token for the plan `digest` and returns it.
    fn issue_plan_token(&self, digest: String) -> Result<String, NativeError> {
        let token = random_token()?;
        let mut tokens = lock(&self.inner.plan_tokens);
        tokens.retain(|_, (_, issued)| issued.elapsed() < PLAN_TOKEN_FOR);
        while tokens.len() >= PLAN_TOKENS_MAX {
            let Some(oldest) = tokens
                .iter()
                .min_by_key(|(_, (_, issued))| *issued)
                .map(|(token, _)| token.clone())
            else {
                break;
            };
            tokens.remove(&oldest);
        }
        tokens.insert(token.clone(), (digest, Instant::now()));
        Ok(token)
    }

    /// Consumes `token` when it is live and was issued for the plan `digest`. A token shown for
    /// another plan stays (the preview it belongs to is still valid); an expired one is dropped.
    fn consume_plan_token(&self, token: &str, digest: &str) -> Result<(), NativeError> {
        let mut tokens = lock(&self.inner.plan_tokens);
        match tokens.get(token) {
            Some((_, issued)) if issued.elapsed() >= PLAN_TOKEN_FOR => {
                tokens.remove(token);
                Err(NativeError::new("profile_preview_stale"))
            }
            Some((expected, _)) if expected == digest => {
                tokens.remove(token);
                Ok(())
            }
            _ => Err(NativeError::new("profile_preview_stale")),
        }
    }

    #[cfg(test)]
    pub(crate) fn expire_plan_tokens(&self) {
        for (_, issued) in lock(&self.inner.plan_tokens).values_mut() {
            *issued = Instant::now()
                .checked_sub(PLAN_TOKEN_FOR)
                .unwrap_or_else(Instant::now);
        }
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
        // One start at a time.
        if self
            .inner
            .starting
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(NativeError::new("busy"));
        }
        let _starting = Starting(&self.inner.starting);
        if !request.agent_id.fits_shared_registry()
            && (lock(&self.inner.extended_registry).is_none()
                || self.inner.extended_storage_degraded.load(Ordering::Acquire))
        {
            return Err(NativeError::new("store_unavailable"));
        }
        let project = self.project_allowed(&request.project)?;
        let project_key = project.to_string_lossy().into_owned();
        // Only the project whose extended default could not be removed is fenced. A canonical path
        // identifies it, so an alias of the same project is fenced too, and unrelated projects
        // keep their Claude/Codex launches.
        if request.agent_id.fits_shared_registry()
            && lock(&self.inner.blocked_default_clears).contains(&project_key)
        {
            return Err(NativeError::new("store_unavailable"));
        }
        let agent = AGENTS
            .iter()
            .find(|spec| spec.id == request.agent_id)
            .ok_or_else(|| NativeError::new("agent_not_installed"))?;
        let direct = self
            .agent_path(agent)
            .ok_or_else(|| NativeError::new("agent_not_installed"))?;
        let (mut plan, profile_ref, digest, profile_effective) = self.plan_for(
            request.agent_id,
            request.profile_id.as_deref(),
            request.arsenal.as_ref(),
        )?;
        // Stored skills of the preset that could not be prepared. They never stop the start.
        let mut skills_skipped: Vec<arsenal::SkippedSkill> = Vec::new();
        let agent_id = request.agent_id;
        let detected = agent_id.fits_shared_registry();
        let mut bypass_authorized = false;
        if request.profile_id.is_some() || request.arsenal.is_some() {
            match (request.plan_token.as_deref(), digest.as_deref()) {
                (None, _) => return Err(NativeError::new("profile_preview_required")),
                (Some(token), Some(digest)) => {
                    if detected
                        && plan
                            .with_cost
                            .iter()
                            .any(|cost| cost == profile::BYPASS_COST)
                        && self
                            .consume_plan_token(token, &bypass_digest(digest))
                            .is_ok()
                    {
                        bypass_authorized = true;
                    } else {
                        self.consume_plan_token(token, digest)?;
                    }
                }
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
        let version_bound_handoff = agent_id == AgentId::Opencode && request.brief.is_some();
        let version_bound =
            !detected && (profile_effective || request.arsenal.is_some() || version_bound_handoff);
        let version_guard = version_bound && recipe::family(&shell).is_some();
        let version = if detected {
            catalogue::cached_provider_version(agent_id, &direct)
        } else if version_bound {
            let expected = request.expected_cli_version.as_deref();
            check_launch_version(
                agent_id,
                expected,
                profile_effective || version_bound_handoff,
            )?;
            if !version_guard
                && catalogue::probe_provider_version(agent_id, &direct).as_deref() != expected
            {
                return Err(NativeError::new("provider_version_unverified"));
            }
            request.expected_cli_version.clone()
        } else {
            catalogue::probe_provider_version(agent_id, &direct)
        };
        let (omitted_options, project_instructions_supported) =
            if detected && (request.profile_id.is_some() || request.arsenal.is_some()) {
                let help = catalogue::cached_cli_help(&direct);
                (
                    capabilities::apply_help(
                        &mut plan,
                        agent_id,
                        &help,
                        bypass_authorized,
                        request.arsenal.as_ref(),
                    ),
                    capabilities::project_instructions_supported(agent_id, &help),
                )
            } else {
                (Vec::new(), false)
            };
        if !plan.not_applicable.is_empty() {
            return Err(NativeError {
                code: "profile_not_applicable",
                reasons: plan.not_applicable,
            });
        }
        if agent_id == AgentId::Codex
            && request.arsenal.as_ref().is_some_and(|launch| launch.project_instructions_reviewed)
            && plan.codex_home.as_ref().is_none_or(|home| home.instructions)
        {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or_else(|| NativeError::new("store_unavailable"))?;
            let owner = launch_home_codex::CodexOwner::from_process(&home);
            if ["AGENTS.override.md", "AGENTS.md"].iter().any(|name| {
                owner.dir.join(name).canonicalize().is_ok_and(|target| target.starts_with(&project))
            }) {
                return Err(NativeError::new("arsenal_project_glob"));
            }
        }
        // A launch with a folder of its own never runs where the agent would read the owner's
        // folder again as the project's: the hooks, instructions and skills the preset left out
        // would come back. Refused before anything is written, with no way around it.
        if plan.home.is_some() || plan.codex_home.is_some() {
            let owner_home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .ok_or_else(|| NativeError::new("store_unavailable"))?;
            let claude_owner = launch_home::Owner::from_home(&owner_home).dir;
            let codex_owner = launch_home_codex::CodexOwner::from_process(&owner_home).dir;
            if launch_home::cwd_would_reload_owner_config(
                &project,
                &owner_home,
                &[&claude_owner, &codex_owner],
            ) {
                // A folder of its own only for the copied skills is left out: the agent starts
                // as the owner's own, and the copies are reported as not prepared.
                match request.arsenal.as_ref() {
                    Some(launch) if home_only_for_copies(launch, request.agent_id) => {
                        plan.home = None;
                        plan.codex_home = None;
                        skip_all(&mut skills_skipped, launch, "cwd_is_home");
                    }
                    _ => return Err(NativeError::new("arsenal_home_cwd_is_home")),
                }
            }
        }
        if let Some(overlay) = &plan.overlay {
            let overlay = if request.agent_id == AgentId::ClaudeCode
                && request
                    .arsenal
                    .as_ref()
                    .is_some_and(|launch| launch.launch_instructions.is_some())
            {
                let owner_home = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .ok_or_else(|| NativeError::new("store_unavailable"))?;
                let owner = launch_home::Owner::from_home(&owner_home).dir;
                let launch_home = request
                    .arsenal
                    .as_ref()
                    .map(|launch| self.inner.base.join("launch-homes").join(&launch.selection));
                let excludes =
                    project_instruction_excludes(&project, &owner, launch_home.as_deref())?;
                if request.arsenal.as_ref().is_some_and(|launch| launch.project_instructions_reviewed)
                    && !excludes.contains(&format!("{}/**", project_key.trim_end_matches('/')))
                {
                    return Err(NativeError::new("arsenal_project_glob"));
                }
                project_instruction_overlay(overlay, &excludes)?
            } else {
                overlay.clone()
            };
            plan.overlay = Some(overlay);
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let dir = self.inner.base.join("sessions").join(&id);
        let guard_marker = dir.join("provider-launch-ready");
        if version_guard && store::private_dir(&dir).is_err() {
            return Err(NativeError::new("store_unavailable"));
        }
        // A startup file may reorder PATH. Use the same absolute executable we probed.
        let executable = if detected {
            direct.to_string_lossy().into_owned()
        } else {
            agent.executable.to_owned()
        };
        let mut words = vec![executable];
        for word in &plan.words {
            match word {
                Word::Literal(text) => words.push(text.clone()),
                Word::Overlay => {
                    words.push(dir.join("settings.json").to_string_lossy().into_owned())
                }
            }
        }
        // The launch home: its own configuration folder, and the stored token instead of the
        // owner's sign-in, so this session never refreshes it.
        let mut home_env: Vec<(String, String)> = Vec::new();
        // The folder variable of the launch home, set again in the `exec` that becomes the agent:
        // a start-up file of the shell that exports the same name must not put the owner's own
        // folder (and the hooks this launch left out) back.
        let mut home_pin: Option<(&'static str, String)> = None;
        // The stored sign-in token of a Claude launch home. It stays zeroized until the child's
        // environment is put together. Limit: Claude Code reads it only from its environment, so
        // the user's shell between (and its start-up files), the agent and every process the agent
        // starts can read it. It is never an argv word, which any process of the user could read.
        let mut oauth_token: Option<zeroize::Zeroizing<String>> = None;
        if let (Some(_), Some(launch)) = (&plan.home, request.arsenal.as_ref())
            && home_only_for_copies(launch, request.agent_id)
            && launch_home::token().is_none()
        {
            // Without the stored token there is no folder of its own to copy into: the agent
            // starts as the owner's own instead, and the copies are reported.
            plan.home = None;
            skip_all(&mut skills_skipped, launch, "token_missing");
        }
        if let (Some(spec), Some(launch)) = (&plan.home, request.arsenal.as_ref()) {
            'claude_home: {
                let token = launch_home::token()
                    .ok_or_else(|| NativeError::new("arsenal_token_missing"))?;
                let owner_home = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .ok_or_else(|| NativeError::new("store_unavailable"))?;
                let home_dir = self.inner.base.join("launch-homes").join(&launch.selection);
                let owner = launch_home::Owner::from_home(&owner_home);
                let ready = read_copies(launch, &mut skills_skipped);
                let (built, unwritten) =
                    match launch_home::build_with_skills(&owner, &home_dir, spec, &ready) {
                        Ok(built) => built,
                        // A folder only for the copies that cannot be built: the agent starts as the
                        // owner's own, and the copies are reported.
                        Err(_) if home_only_for_copies(launch, request.agent_id) => {
                            skills_skipped.clear();
                            skip_all(&mut skills_skipped, launch, "write_failed");
                            break 'claude_home;
                        }
                        Err(_) => return Err(NativeError::new("arsenal_home_failed")),
                    };
                skills_skipped.extend(unwritten);
                if let Some(crate::arsenal::role_instructions::Choice::Role { role }) =
                    &launch.launch_instructions
                    && project_instructions_supported
                    && !launch.project_instructions_reviewed
                {
                    launch_home::write_role(&owner, &home_dir, spec, role)
                        .map_err(|_| NativeError::new("arsenal_home_failed"))?;
                }
                if let Some(mcp) = &built.mcp_config {
                    words.push("--strict-mcp-config".to_owned());
                    words.push("--mcp-config".to_owned());
                    words.push(mcp.to_string_lossy().into_owned());
                }
                let built_dir = built.dir.to_string_lossy().into_owned();
                home_env.push(("CLAUDE_CONFIG_DIR".to_owned(), built_dir.clone()));
                home_pin = Some(("CLAUDE_CONFIG_DIR", built_dir));
                oauth_token = Some(token);
                if !spec.memory {
                    home_env.push(("CLAUDE_CODE_DISABLE_AUTO_MEMORY".to_owned(), "1".to_owned()));
                }
            }
        }
        // Codex: its own folder too, with the real `HOME` and no token. The sign-in is the owner's
        // own, through the home's link to it. A home that cannot be built stops the start: the
        // session never runs with the hooks the preset left out.
        if let (Some(spec), Some(launch)) = (&plan.codex_home, request.arsenal.as_ref()) {
            'codex_home: {
                let owner_home = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .ok_or_else(|| NativeError::new("store_unavailable"))?;
                let mut home_dir = self.inner.base.join("launch-homes").join("codex").join(&launch.selection);
                if launch.project_instructions_reviewed
                    && let Some(crate::arsenal::role_instructions::Choice::Role { role }) = &launch.launch_instructions
                {
                    // One stable home per reviewed role. Another selection cannot change the
                    // instructions under a running Codex session.
                    home_dir.push(format!("role-{:x}", Sha256::digest(role.as_bytes())));
                }
                let owner = launch_home_codex::CodexOwner::from_process(&owner_home);
                let ready = read_copies(launch, &mut skills_skipped);
                let (built, unwritten) =
                    match launch_home_codex::build_with_skills(&owner, &home_dir, spec, &ready) {
                        Ok(built) => built,
                        Err(_) if home_only_for_copies(launch, request.agent_id) => {
                            skills_skipped.clear();
                            skip_all(&mut skills_skipped, launch, "write_failed");
                            break 'codex_home;
                        }
                        Err(_) => return Err(NativeError::new("arsenal_home_failed")),
                    };
                skills_skipped.extend(unwritten);
                if let Some(crate::arsenal::role_instructions::Choice::Role { role }) =
                    &launch.launch_instructions
                    && project_instructions_supported
                {
                    launch_home_codex::write_role(&owner, &home_dir, spec, role).map_err(
                        |error| {
                            NativeError::new(if error.to_string() == "arsenal_role_too_long" {
                                "arsenal_role_too_long"
                            } else {
                                "arsenal_home_failed"
                            })
                        },
                    )?;
                }
                let built_dir = built.to_string_lossy().into_owned();
                home_env.push(("CODEX_HOME".to_owned(), built_dir.clone()));
                home_pin = Some(("CODEX_HOME", built_dir));
            }
        }
        if let Some(launch) = &request.arsenal
            && launch.project_instructions_reviewed
            && request.agent_id == AgentId::ClaudeCode
            && let Some(crate::arsenal::role_instructions::Choice::Role { role }) = &launch.launch_instructions
        {
            let path = dir.join("ROL.md");
            store::private_dir(&dir)
                .and_then(|()| store::write_atomic(&path, role.as_bytes()))
                .map_err(|_| NativeError::new("store_unavailable"))?;
            words.extend(["--append-system-prompt-file".to_owned(), path.to_string_lossy().into_owned()]);
        }
        let codex_rollouts = if agent_id == AgentId::Codex
            && version.as_deref() == Some("codex-cli 0.170.0")
        {
            let home = home_pin.as_ref().map_or_else(
                || launch_home_codex::CodexOwner::from_process(&self.inner.home).dir,
                |(_, path)| PathBuf::from(path),
            );
            direct.canonicalize().ok().map(|executable| (home, executable))
        } else {
            None
        };
        if let Some(brief) = &request.brief {
            if brief.len() > MAX_BRIEF || !brief.starts_with("# ") || brief.contains('\0') {
                let _ = fs::remove_dir_all(&dir);
                return Err(NativeError::new("brief_invalid"));
            }
            match request.agent_id {
                AgentId::Opencode => {
                    words.push("--prompt".to_owned());
                    words.push(brief.clone());
                }
                AgentId::Hermes => {
                    let _ = fs::remove_dir_all(&dir);
                    return Err(NativeError::new("handoff_target_unverified"));
                }
                AgentId::ClaudeCode | AgentId::Codex => words.push(brief.clone()),
            }
        }
        if request.delegation {
            let _ = fs::remove_dir_all(&dir);
            return Err(NativeError::new("delegation_unavailable"));
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
        let mut launch_exe = direct.clone();
        // Monitoring is wired for every Claude Code version; the owner does not want a version gate.
        if agent_id == AgentId::ClaudeCode
            && let Ok(helper) = std::env::current_exe()
            && helper.file_name().is_some_and(|name| name == "wyss-host")
            && store::private_dir(&dir).is_ok()
        {
            if let Some((_, pinned)) = &home_pin {
                // The launch home is already known. Prepare before the shell reads its
                // sign-in file so the monitoring executable never receives the stored token.
                let argv: Vec<_> = words[1..].iter().map(OsString::from).collect();
                if let Some(prepared) = claude_activity::prepare(
                    &dir, &helper, Path::new(pinned), &project, &argv,
                ).and_then(|args| args.into_iter().map(|word| word.into_string().ok()).collect::<Option<Vec<_>>>()) {
                    words.splice(1.., prepared);
                }
            } else if !version_bound
                && let (Some(exe), Some(dir_name)) = (helper.to_str(), dir.to_str())
            {
                // ponytail: a version-pinned launch goes unmonitored, the shell guard must probe the real CLI.
                // With the owner's home, check the config directory selected by shell startup.
                words.splice(..0, [exe.to_owned(), claude_activity::LAUNCH.to_owned(), dir_name.to_owned()]);
                launch_exe = helper;
            }
        }
        // Shell launches read the token from a private file after shell initialization.
        // The readiness marker keeps that file alive until the shell has consumed it.
        let token_file = dir.join(TOKEN_FILE);
        let token_through_file = oauth_token.is_some() && recipe::family(&shell).is_some();
        let launch_guard = version_guard || token_through_file;
        // Removed on every return from here on. The recipe removes the file before it writes the
        // version marker, so a successful start has already removed it.
        let _token_guard = token_through_file.then(|| TokenFileGuard(token_file.clone()));
        if token_through_file
            && let Some(token) = &oauth_token
            && store::private_dir(&dir)
                .and_then(|()| {
                    write_token_file(
                        &token_file,
                        zeroize::Zeroizing::new(format!("{}\n", token.as_str())).as_bytes(),
                    )
                })
                .is_err()
        {
            let _ = fs::remove_dir_all(&dir);
            return Err(NativeError::new("store_unavailable"));
        }
        let made = recipe::recipe_pinned_secret(
            &shell,
            &words,
            &launch_exe,
            if version_bound {
                request.expected_cli_version.as_deref()
            } else {
                None
            },
            home_pin.as_ref().map(|(name, dir)| (*name, dir.as_str())),
            token_through_file,
        );
        let now = store::now();
        let mut session = Session {
            id: id.clone(),
            agent_id: request.agent_id,
            agent_name: agent.name.to_owned(),
            version,
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
            arsenal: request.arsenal.clone(),
            delegation: request.delegation,
        };
        let mut env = vec![("WYSS_NATIVE_SESSION_ID".to_owned(), id.clone())];
        if launch_guard {
            env.push((
                "WYSS_NATIVE_VERSION_GUARD".to_owned(),
                guard_marker.to_string_lossy().into_owned(),
            ));
        }
        env.extend(self.inner.extra_env.iter().cloned());
        env.extend(home_env);
        if token_through_file {
            env.push((
                "WYSS_NATIVE_TOKEN_FILE".to_owned(),
                token_file.to_string_lossy().into_owned(),
            ));
        } else if let Some(token) = &oauth_token {
            // The one plain copy the terminal's launch API takes; it lives in that environment.
            env.push(("CLAUDE_CODE_OAUTH_TOKEN".to_owned(), String::clone(token)));
        }
        drop(oauth_token);
        // Remove an extended default durably before a shared-registry provider can become the selected
        // project agent. If persistence is unavailable, do not launch a provider and leave a
        // stale OpenCode/Hermes default shadowing the newly chosen Claude/Codex session.
        let prior_extended_default = if request.agent_id.fits_shared_registry() {
            let mut guard = lock(&self.inner.extended_registry);
            if let Some(extended) = guard.as_mut() {
                match extended.clear_project_default(&project.to_string_lossy()) {
                    Ok(previous) => Some(previous),
                    Err(_) => {
                        self.inner
                            .extended_storage_degraded
                            .store(true, Ordering::Release);
                        lock(&self.inner.blocked_default_clears).insert(project_key.clone());
                        return Err(NativeError::new("store_unavailable"));
                    }
                }
            } else {
                None
            }
        } else {
            None
        };
        let prior_recovery_override =
            if request.agent_id.fits_shared_registry() && lock(&self.inner.extended_registry).is_none() {
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
                    // A launch for a project with no recovered extended default needs no tombstone.
                    // This keeps Claude/Codex launches independent of optional extended storage and avoids
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
                        .extended_storage_degraded
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
        if launch_guard && !await_launch_guard(terminal, &info.terminal_id, &guard_marker) {
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
                    .extended_storage_degraded
                    .store(true, Ordering::Release);
            }
            session.state = State::FailedStart;
            session.ended_at = Some(store::now());
            let code = if version_guard {
                "provider_version_unverified"
            } else {
                "arsenal_home_failed"
            };
            session.error = Some(code.to_owned());
            self.record(session);
            let _ = fs::remove_dir_all(&dir);
            return Err(NativeError::new(code));
        }
        session.state = State::Running;
        lock(&self.inner.live).insert(
            id.clone(),
            Live {
                terminal: info.terminal_id.clone(),
                agent: agent_id,
                pid: info.observed_pid,
                last: None,
                usage: None,
                codex_rollouts,
                queue_attempt: None,
            },
        );
        {
            let mut registry = lock(&self.inner.registry);
            let project = registry.project_mut(&session.project);
            project.last_used = now;
            if session.agent_id.fits_shared_registry() {
                project.default_agent = Some(session.agent_id);
            }
            let _ = registry.save();
        }
        if !session.agent_id.fits_shared_registry()
            && let Some(extended) = lock(&self.inner.extended_registry).as_mut()
            && extended
                .set_project_default(&session.project, session.agent_id)
                .is_err()
        {
            // The provider is already running. Keep it alive, but report extended storage as
            // unavailable and do not claim that its default was saved.
            self.inner
                .extended_storage_degraded
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
            skills_skipped,
            omitted_options,
        })
    }

    fn record(&self, session: Session) {
        if session.agent_id.fits_shared_registry() {
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
                    .extended_storage_degraded
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
        let restart_bypass = self
            .profile_plan_for(
                session.agent_id,
                session.profile.as_ref().map(|profile| profile.id.as_str()),
            )
            .ok()
            .map(|(plan, _, _)| plan.with_cost.iter().any(|cost| cost == profile::BYPASS_COST));
        SessionView {
            session: session.clone(),
            plane: "native",
            restart_bypass,
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
        // The process ended above. Success is reported only when that ending was written. If the
        // write failed, the caller learns it now, while the observed Stopped state stays in memory.
        if self.finish(id, State::Stopped, None, None) {
            Ok(())
        } else {
            Err(NativeError::new("store_unavailable"))
        }
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
        self.restart_with_review(terminal, id, size, plan_token, expected_cli_version, None)
    }

    pub(crate) fn restart_with_review(
        &self,
        terminal: &TerminalService,
        id: &str,
        size: TerminalSize,
        plan_token: Option<String>,
        expected_cli_version: Option<String>,
        review: Option<arsenal::InstructionReview>,
    ) -> Result<StartReply, NativeError> {
        if !id_ok(id) {
            return Err(NativeError::new("session_unknown"));
        }
        let mut old = self
            .stored_session(id)
            .ok_or_else(|| NativeError::new("session_unknown"))?;
        if let Some(review) = review {
            let launch = old.arsenal.as_mut().ok_or_else(|| NativeError::new("arsenal_invalid"))?;
            launch.apply_review(review);
            launch.validate().map_err(NativeError::new)?;
        }
        let was_running = old.state == State::Running;
        // The Console restarts with the id alone. A plan without full access is the one the
        // owner already started and gets its token here; full access always needs a preview of
        // its own, so a restart never grants it without the owner seeing it again.
        let plan_token = match plan_token {
            Some(token) => Some(token),
            None if old.profile.is_some() || old.arsenal.is_some() => {
                let (plan, _, digest, _) = self.plan_for(
                    old.agent_id,
                    old.profile.as_ref().map(|p| p.id.as_str()),
                    old.arsenal.as_ref(),
                )?;
                if plan.with_cost.iter().any(|cost| cost == profile::BYPASS_COST) {
                    return Err(NativeError::new("restart_needs_preview"));
                }
                digest.map(|digest| self.issue_plan_token(digest)).transpose()?
            }
            None => None,
        };
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
                arsenal: old.arsenal.clone(),
                delegation: false,
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
            extended
                .forget(id)
                .map(|_| ())
                .map_err(|_| NativeError::new("store_unavailable"))
        } else {
            registry.forget(id);
            registry.save().map_err(|_| NativeError::new("store_unavailable"))?;
            drop(extended);
            drop(registry);
            lock(&self.inner.readings).forget(id);
            lock(&self.inner.prompts).forget(id);
            Ok(())
        }
    }

    /// Records how a session ended. The process effect is already applied when this runs, so
    /// the in-memory state always follows the observed process. The return value is `false`
    /// only when this ending was written to the extended sidecar and that write failed; a failed
    /// write also withdraws extended storage. Volatile sessions have no write here, and their storage
    /// failure was already reported when they started.
    fn finish(&self, id: &str, state: State, code: Option<u32>, signal: Option<String>) -> bool {
        let live = lock(&self.inner.live).remove(id);
        if let Some(live) = live {
            let mut readings = lock(&self.inner.readings);
            let dir = self.inner.base.join("sessions").join(id);
            readings.observe(id, live.agent, &dir, None);
            readings.finished(id);
            readings.plans();
        }
        let mut persisted = true;
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
                        if extended.save().is_err() {
                            // Not a durable ending: keep the observed state in memory, but do
                            // not advertise extended storage as ready. A reopen re-reads the sidecar,
                            // where the session still appears running and is recovered as lost.
                            self.inner
                                .extended_storage_degraded
                                .store(true, Ordering::Release);
                            persisted = false;
                        }
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
        persisted
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
                Err(_) => {
                    self.finish(&id, State::Lost, None, None);
                }
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
            .filter(|session| include_extended || session.agent_id.fits_shared_registry())
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

    pub(crate) fn activity(&self, terminal: &TerminalService) -> ActivityView {
        let sessions = self.sessions(terminal, false);
        let mut readings = lock(&self.inner.readings);
        let live = lock(&self.inner.live);
        let prompts = lock(&self.inner.prompts);
        readings.retain(|id| sessions.iter().any(|session| session.session.id == id));
        let mut views = Vec::new();
        for session in sessions {
            let id = session.session.id;
            if !id_ok(&id) { continue; }
            let agent = session.session.agent_id;
            let dir = self.inner.base.join("sessions").join(&id);
            if let Some(live) = live.get(&id) {
                readings.observe(&id, agent, &dir, live.pid.zip(live.codex_rollouts.as_ref()));
            }
            let folder = session.session.project.rsplit('/').find(|p| !p.is_empty()).unwrap_or("/");
            let name = format!("{} · {folder} · {}", session.session.agent_name, &id[..7]);
            let turn = (agent == AgentId::ClaudeCode && session.terminal_id.is_some())
                .then(|| claude_activity::turn(&dir)).flatten();
            let queue = prompts.summary(&id);
            views.push(ActivitySession {
                context: readings.context(&id), id, agent, name,
                started_at: session.session.started_at,
                terminal_id: session.terminal_id, turn, input_revision: None,
                queue_revision: queue.map(|(revision, _)| revision),
                queued: queue.map_or(0, |(_, count)| count),
            });
        }
        readings.observe_codex_account(&launch_home_codex::CodexOwner::from_process(&self.inner.home).dir);
        ActivityView { plans: readings.plans(), sessions: views }
    }

    fn prompt_session(&self, id: &str) -> Result<Session, NativeError> {
        if id_ok(id)
            && let Some(session) = self.stored_session(id)
            && matches!(session.agent_id, AgentId::ClaudeCode | AgentId::Codex)
        {
            return Ok(session);
        }
        Err(NativeError::new("session_unknown"))
    }

    pub(crate) fn queue(&self, id: &str) -> Result<prompt_queue::Queue, NativeError> {
        self.prompt_session(id)?;
        lock(&self.inner.prompts).get(id).ok_or_else(|| NativeError::new("store_unavailable"))
    }

    pub(crate) fn edit_queue(&self, id: &str, revision: u64, edit: prompt_queue::Edit) -> Result<prompt_queue::Queue, NativeError> {
        self.prompt_session(id)?;
        lock(&self.inner.prompts).edit(id, revision, edit).ok_or_else(|| NativeError::new("queue_edit_failed"))
    }

    pub(crate) fn send_queue(
        &self, terminal: &TerminalService, id: &str, tid: &TerminalId,
        turn_id: prompt_queue::TurnId, revision: u64,
    ) -> Result<prompt_queue::Queue, NativeError> {
        use prompt_queue::{Dispatch, dispatch};
        let session = self.prompt_session(id)?;
        let mut live = lock(&self.inner.live);
        let mut prompts = lock(&self.inner.prompts);
        let entry = live.get_mut(id).filter(|entry| &entry.terminal == tid);
        let Some(entry) = entry.filter(|_| session.state == State::Running && session.agent_id == AgentId::ClaudeCode)
        else { return Err(NativeError::new("queue_unsafe")); };
        let dir = self.inner.base.join("sessions").join(id);
        let turn = claude_activity::turn(&dir).filter(|turn| turn.id == turn_id);
        let running = || terminal.status(tid).is_ok_and(|i| i.state == TerminalState::Running);
        if dispatch(running(), true, turn, entry.queue_attempt, store::now()) != Dispatch::Send {
            return Err(NativeError::new("queue_unsafe"));
        }
        entry.queue_attempt = Some(turn_id);
        prompts.send(id, revision, |bytes| {
            // Recheck after the durable pending marker. A changed turn or stopped terminal makes
            // this attempt fail held rather than sending into a different prompt.
            let turn = claude_activity::turn(&dir).filter(|turn| turn.id == turn_id);
            dispatch(running(), true, turn, None, store::now()) == Dispatch::Send
                && {
                    let len = bytes.len();
                    terminal.write(tid, bytes).is_ok_and(|count| count == len)
                }
        }).ok_or_else(|| NativeError::new("queue_send_failed"))
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
/// The private file a checked Claude launch reads its sign-in token from (`recipe::POSIX_SECRET_READ`).
const TOKEN_FILE: &str = "oauth-token";

/// Removes the sign-in token file when the start returns, whichever way it returns.
struct TokenFileGuard(PathBuf);

impl Drop for TokenFileGuard {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.0)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("Wyss: could not remove an OAuth token file: {error}");
        }
    }
}

/// Writes the token file: new, 0600, never through a link. It is not synchronized to disk: it
/// lives for one start and must not outlive a power cut.
fn write_token_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)?
        .write_all(bytes)
}

/// Removes every `sessions/<id>/oauth-token`, making a stale session directory writable to the
/// owner first when it has to.
fn sweep_token_files(sessions: &Path) {
    let entries = match fs::read_dir(sessions) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            eprintln!("Wyss: could not enumerate session directories: {error}");
            return;
        }
    };
    for entry in entries {
        let dir = match entry.and_then(|entry| entry.file_type().map(|kind| (entry, kind))) {
            Ok((entry, kind)) if kind.is_dir() => entry.path(),
            Ok(_) => continue,
            Err(error) => {
                eprintln!("Wyss: could not inspect a session directory entry: {error}");
                continue;
            }
        };
        let token = dir.join(TOKEN_FILE);
        if let Err(error) = remove_stale_token(&dir, &token) {
            eprintln!("Wyss: could not remove a stale OAuth token file: {error}");
        }
    }
}

fn remove_stale_token(dir: &Path, token: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let removed = match fs::remove_file(token) {
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied
            && fs::symlink_metadata(dir).is_ok_and(|meta| meta.is_dir()) =>
        {
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
            fs::remove_file(token)
        }
        result => result,
    };
    removed.or_else(|error| {
        if error.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(error) }
    })
}

fn bypass_digest(digest: &str) -> String {
    format!("{digest}:bypass")
}

fn check_launch_version(
    agent: AgentId,
    version: Option<&str>,
    profile_bound: bool,
) -> Result<(), NativeError> {
    if version.is_none()
        || (profile_bound && !capabilities::permits_profile_overrides(agent, version))
    {
        return Err(NativeError::new("provider_version_unverified"));
    }
    Ok(())
}

fn await_launch_guard(terminal: &TerminalService, terminal_id: &TerminalId, marker: &Path) -> bool {
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_reports_the_resolved_restart_permission() {
        let root = std::env::temp_dir().join(format!("wyss-restart-permission-{}", uuid::Uuid::new_v4()));
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();
        let base = root.join("native");
        let service = NativeService::open(base, home.clone(), None).unwrap();
        let mut profile = profile::Body {
            name: "Custom automation".into(),
            applies_to: vec![AgentId::ClaudeCode],
            permission: Some(profile::Permission::Bypass),
            ..profile::Body::default()
        };
        let id = service.profiles().create(profile.clone()).unwrap();
        let (plan, profile_ref, _) = service.profile_plan_for(AgentId::ClaudeCode, Some(&id)).unwrap();
        assert!(plan.with_cost.iter().any(|cost| cost == profile::BYPASS_COST));
        service.record(Session {
            id: "0123456789abcdef0123456789abcdef".into(),
            agent_id: AgentId::ClaudeCode,
            agent_name: "Fixture provider".into(),
            project: home.to_string_lossy().into_owned(),
            profile: profile_ref,
            version: None,
            through_shell: false,
            state: State::Stopped,
            started_at: 1,
            ended_at: Some(2),
            exit_code: None,
            exit_signal: None,
            error: None,
            restart_of: None,
            restarts: 0,
            arsenal: None,
            delegation: false,
        });
        let terminal = TerminalService::default();
        let listed = || serde_json::to_value(service.sessions(&terminal, false)).unwrap();
        assert_eq!(listed()[0]["restart_bypass"], true);
        profile.permission = None;
        service.profiles().save(&id, profile).unwrap();
        assert_eq!(listed()[0]["restart_bypass"], false);
    }

    #[test]
    fn token_sweep_logs_removal_failure() {
        const CHILD: &str = "WYSS_TEST_TOKEN_SWEEP_CHILD";
        if let Some(sessions) = std::env::var_os(CHILD) {
            sweep_token_files(Path::new(&sessions));
            return;
        }
        let root = std::env::temp_dir().join(format!("wyss-token-sweep-{}", uuid::Uuid::new_v4()));
        let stale = root.join("fixture-session").join(TOKEN_FILE);
        // A directory is not removable with remove_file, even when the test runs as root.
        fs::create_dir_all(&stale).unwrap();
        fs::write(stale.join("fixture"), "synthetic-secret-content").unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["token_sweep_logs_removal_failure", "--nocapture"])
            .env(CHILD, &root)
            .output().unwrap();
        assert!(output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("could not remove a stale OAuth token file"));
        assert!(!stderr.contains("synthetic-secret-content"));
    }
    #[test]
    fn token_cleanup_logs_guard_and_directory_enumeration_failures() {
        const CHILD: &str = "WYSS_TEST_TOKEN_CLEANUP_CHILD";
        if let Some(root) = std::env::var_os(CHILD) {
            let root = Path::new(&root);
            drop(TokenFileGuard(root.join("guard").join(TOKEN_FILE)));
            sweep_token_files(&root.join("not-a-directory"));
            return;
        }
        let root =
            std::env::temp_dir().join(format!("wyss-token-cleanup-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("guard").join(TOKEN_FILE)).unwrap();
        fs::write(root.join("not-a-directory"), "synthetic-secret-content").unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "token_cleanup_logs_guard_and_directory_enumeration_failures",
                "--nocapture",
            ])
            .env(CHILD, &root)
            .output()
            .unwrap();
        assert!(output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("could not remove an OAuth token file")
                && stderr.contains("could not enumerate session directories"),
            "expected both cleanup diagnostics in stderr, got {stderr:?}"
        );
        assert!(!stderr.contains("synthetic-secret-content"));
        assert!(!stderr.contains(root.to_str().unwrap()));
    }
}
