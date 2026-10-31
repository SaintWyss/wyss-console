//! Agent delegation, the same for both providers: an orchestrator session started from Wyss (Claude
//! Code or Codex) asks the terminal host, through the Wyss MCP tool, to run a task with the other
//! provider (or the same one) as a separate worker process. The host checks the request against
//! the owner's policy, the installed provider's own model catalogue and the Arsenal library; plans
//! the worker's skills exactly as a terminal's (only what the request names stays exposed); starts
//! the worker in its own workspace and process group; reads its event stream; checks the
//! configuration the worker reports against the plan; and keeps a record of every delegation.
//! A worker never receives the delegation tool or its token, so delegation does not recurse.
pub mod mcp;
pub mod worker;

use crate::agentconfig::catalogue::{self, LaunchScan};
use crate::arsenal::{self, store, terminal};
use crate::native::arsenal::ArsenalLaunch;
use crate::native::capabilities;
use crate::native::catalogue::AgentId;
use crate::native::profile::{LaunchPlan, Word};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Write as _};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// Variables that carry the delegation right. A worker's environment never has them.
pub const TOKEN_VAR: &str = "WYSS_DELEGATION_TOKEN";
const SESSION_VAR: &str = "WYSS_NATIVE_SESSION_ID";
/// Longest task text a delegation takes.
pub const MAX_TASK: usize = 64 * 1024;
/// Most capabilities one delegation names.
const MAX_ITEMS: usize = 64;
/// Most bytes of a worker's event stream kept on disk.
const MAX_EVENTS: u64 = 32 * 1024 * 1024;
/// Most workspace files listed as artifacts.
const MAX_ARTIFACTS: usize = 200;
/// Process namespace wrapper required to contain worker descendants, including children that call `setsid`.
const BWRAP: &str = "/usr/bin/bwrap";

/// The owner's delegation policy: `~/.config/wyss-console/delegation.json`, every field optional.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    /// Delegated agents running at once.
    pub max_concurrent: usize,
    /// Longest a delegated agent may run, in minutes. A request may ask for less.
    pub max_minutes: u32,
    /// Providers a delegation may start: `codex`, `claude_code`.
    pub providers: Vec<String>,
    /// Claude Code models a delegation may ask for: aliases or full names.
    pub claude_models: Vec<String>,
    /// Whether a Claude Code worker may use its shell tool. Codex's commands are sandboxed instead.
    pub claude_allow_bash: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            max_concurrent: 2,
            max_minutes: 20,
            providers: vec!["codex".into(), "claude_code".into()],
            claude_models: [
                "fable",
                "opus",
                "sonnet",
                "haiku",
                "claude-fable-5-1",
                "claude-opus-5-5",
                "claude-sonnet-5-5",
                "claude-haiku-5-5",
            ]
            .map(String::from)
            .to_vec(),
            claude_allow_bash: false,
        }
    }
}

fn config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .map(|home| home.join(".config"))
        })
}

fn policy_path() -> Option<PathBuf> {
    config_dir().map(|directory| directory.join("wyss-console/delegation.json"))
}

/// The policy as written, bounded; an unreadable or invalid file is reported and the defaults hold.
pub fn load_policy() -> (Policy, Option<String>) {
    let (policy, note, _) = load_policy_details();
    (policy, note)
}

fn load_policy_details() -> (Policy, Option<String>, String) {
    let path = policy_path();
    load_policy_from(path.as_deref())
}

fn load_policy_from(path: Option<&Path>) -> (Policy, Option<String>, String) {
    let Some(path) = path else {
        return (Policy::default(), None, "defaults".into());
    };
    match fs::read_to_string(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (Policy::default(), None, "defaults".into())
        }
        Err(_) => (
            Policy::default(),
            Some(format!(
                "{} could not be read; delegation is blocked until the policy is corrected.",
                path.display()
            )),
            path.display().to_string(),
        ),
        Ok(text) => match parse_policy(&text) {
            Some(policy) => (policy, None, path.display().to_string()),
            None => (
                Policy::default(),
                Some(format!(
                    "{} is not a valid delegation policy; delegation is blocked until it is corrected.",
                    path.display()
                )),
                path.display().to_string(),
            ),
        },
    }
}

/// A policy file's content, if it is valid and within bounds.
pub fn parse_policy(text: &str) -> Option<Policy> {
    serde_json::from_str::<Policy>(text).ok().filter(|policy| {
        (1..=8).contains(&policy.max_concurrent)
            && (1..=120).contains(&policy.max_minutes)
            && policy
                .providers
                .iter()
                .all(|p| p == "codex" || p == "claude_code")
    })
}

/// An orchestrator session allowed to delegate: started from Wyss with delegation on.
#[derive(Debug, Clone, Serialize)]
pub struct Parent {
    pub session_id: String,
    pub provider: String,
    pub project: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkillPick {
    pub capability: String,
    #[serde(default)]
    pub revision: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub token: String,
    pub provider: String,
    pub model: String,
    pub effort: String,
    pub task: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub skills: Vec<SkillPick>,
    /// An Arsenal preset, by id or name: its capabilities are added.
    #[serde(default)]
    pub preset: Option<String>,
    /// An Arsenal selection of the worker's provider, by id or name: its capabilities and kept
    /// composites are added.
    #[serde(default)]
    pub selection: Option<String>,
    #[serde(default)]
    pub timeout_minutes: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Applied {
    pub provider_version: Option<String>,
    pub evidence: Option<String>,
    /// The capabilities the worker was given, by name and revision.
    pub skills: Vec<Value>,
    pub kept: Vec<String>,
    pub inherited_off: Vec<String>,
    pub switched_off: usize,
    pub config_hash: String,
    pub sandbox: String,
    pub network: String,
    pub approvals: String,
    pub tools_off: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Verification {
    /// `pending`, `verified`, `mismatch` or `unavailable`.
    pub state: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub problems: Vec<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub parent_session: String,
    pub parent_provider: String,
    pub provider: String,
    pub model: String,
    pub effort: String,
    pub title: String,
    pub task: String,
    /// The Arsenal preset requested when this run was planned, if any.
    #[serde(default)]
    pub preset: Option<String>,
    /// The saved Arsenal selection requested when this run was planned, if any.
    #[serde(default)]
    pub selection: Option<String>,
    pub workspace: String,
    pub created_at: u64,
    pub ended_at: Option<u64>,
    pub timeout_minutes: u32,
    /// `running`, `succeeded`, `failed`, `cancelled`, `timed_out` or `interrupted`.
    pub status: String,
    pub error: Option<String>,
    pub applied: Applied,
    pub verification: Verification,
    pub result: Option<String>,
    pub usage: Option<Value>,
    pub steps: u64,
    pub artifacts: Vec<Value>,
    pub thread_id: Option<String>,
    pub exit_code: Option<i32>,
}

impl Record {
    fn finished(&self) -> bool {
        self.ended_at.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub reasons: Vec<String>,
}

impl Refusal {
    fn new(code: &'static str, reason: impl Into<String>) -> Self {
        Self {
            code,
            reasons: vec![reason.into()],
        }
    }
}

#[derive(Clone)]
struct Live {
    record: Arc<Mutex<Record>>,
    child_id: u32,
    /// Read by the tests that check the worker's process group is gone.
    #[cfg_attr(not(test), allow(dead_code))]
    pgid: i32,
}

#[derive(Default)]
struct LiveState {
    workers: BTreeMap<String, Live>,
    starting: usize,
}

struct StartReservation {
    live: Arc<Mutex<LiveState>>,
    active: bool,
}

impl StartReservation {
    fn commit(mut self, id: String, worker: Live) {
        let mut state = lock(&self.live);
        debug_assert!(state.starting > 0);
        state.starting = state.starting.saturating_sub(1);
        state.workers.insert(id, worker);
        self.active = false;
    }
}

impl Drop for StartReservation {
    fn drop(&mut self) {
        if self.active {
            let mut state = lock(&self.live);
            state.starting = state.starting.saturating_sub(1);
        }
    }
}

/// What a worker process is started with, beside its record.
struct WorkerSpec {
    executable: PathBuf,
    argv: Vec<String>,
    dir: PathBuf,
    requested_model: String,
    requested_effort: String,
    excluded: Vec<String>,
    excluded_files: Vec<String>,
    kept_names: Vec<String>,
    kept_files: Vec<PathBuf>,
    deadline: Duration,
}

struct ValidatedRequest {
    parent: Parent,
    agent: AgentId,
    minutes: u32,
}

struct PreparedWorker {
    record: Record,
    spec: WorkerSpec,
}

/// A validated launch plan. It has no execution id and grants no permission to start later.
#[derive(Debug, Clone, Serialize)]
pub struct DelegationPreview {
    pub preview: bool,
    pub provider: String,
    pub model: String,
    pub effort: String,
    pub skills: Vec<Value>,
    pub preset: Option<String>,
    pub selection: Option<String>,
    pub policy: Policy,
    pub policy_source: String,
    pub policy_note: Option<String>,
    pub applied: Applied,
}

struct WorkerDirectory {
    path: PathBuf,
    persist: bool,
}

impl WorkerDirectory {
    fn create(path: PathBuf) -> Result<Self, Refusal> {
        let workspace = path.join("workspace");
        if private_dir(&workspace).is_err() {
            let _ = fs::remove_dir_all(&path);
            return Err(Refusal::new(
                "store_unavailable",
                "The task workspace could not be made.",
            ));
        }
        Ok(Self {
            path,
            persist: false,
        })
    }

    fn keep(&mut self) {
        self.persist = true;
    }
}

impl Drop for WorkerDirectory {
    fn drop(&mut self) {
        if !self.persist {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

pub struct Manager {
    base: PathBuf,
    tokens: Mutex<BTreeMap<String, Parent>>,
    live: Arc<Mutex<LiveState>>,
    models: Mutex<Option<(Instant, Value)>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn private_dir(path: &Path) -> std::io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

static MANAGER: OnceLock<Manager> = OnceLock::new();

/// Withdraws a finished orchestrator's right without initializing a delegation service.
pub fn revoke_parent(session_id: &str) {
    if let Some(manager) = MANAGER.get() {
        lock(&manager.tokens).retain(|_, parent| parent.session_id != session_id);
    }
}

/// The host's one manager: `~/.local/state/wyss-console/delegations`. Records left `running` by a
/// host that stopped are `interrupted`: their processes ended with it.
pub fn manager() -> &'static Manager {
    MANAGER.get_or_init(|| {
        // Tests never read or change the owner's records.
        #[cfg(test)]
        return Manager::at(
            std::env::temp_dir().join(format!("delegate-global-{}", std::process::id())),
        );
        #[cfg(not(test))]
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|h| h.join(".local/state"))
            })
            .unwrap_or_else(|| PathBuf::from("/nonexistent"))
            .join("wyss-console/delegations");
        #[cfg(not(test))]
        Manager::at(base)
    })
}

impl Manager {
    pub fn at(base: PathBuf) -> Self {
        let manager = Self {
            base,
            tokens: Mutex::new(BTreeMap::new()),
            live: Arc::new(Mutex::new(LiveState::default())),
            models: Mutex::new(None),
        };
        for mut record in manager.records() {
            if record.status == "running" {
                record.status = "interrupted".into();
                record.error = Some("The terminal host stopped while this agent ran.".into());
                record.ended_at = Some(now());
                manager.save(&record);
            }
        }
        manager
    }

    /// A new delegation right for an orchestrator session. Kept only in this host's memory.
    pub fn register(&self, parent: Parent) -> String {
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        lock(&self.tokens).insert(token.clone(), parent);
        token
    }

    fn parent(&self, token: &str) -> Option<Parent> {
        lock(&self.tokens).get(token).cloned()
    }

    fn save(&self, record: &Record) {
        save_at(&self.base, record);
    }

    /// Every delegation on record, newest first.
    pub fn records(&self) -> Vec<Record> {
        let Ok(entries) = fs::read_dir(&self.base) else {
            return Vec::new();
        };
        let mut records: Vec<Record> = entries
            .flatten()
            .filter_map(|e| fs::read(e.path().join("record.json")).ok())
            .filter_map(|b| serde_json::from_slice(&b).ok())
            .collect();
        let live = lock(&self.live);
        for record in &mut records {
            if let Some(live) = live.workers.get(&record.id) {
                *record = lock(&live.record).clone();
            }
        }
        records.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
        records
    }

    /// The delegations an orchestrator may see: its own with a token, all of them for the window.
    pub fn list(&self, token: Option<&str>) -> Result<Vec<Record>, Refusal> {
        let parent = match token {
            Some(token) => Some(
                self.parent(token)
                    .ok_or_else(|| Refusal::new("delegation_unauthorized", "unknown token"))?,
            ),
            None => None,
        };
        Ok(self
            .records()
            .into_iter()
            .filter(|r| {
                parent
                    .as_ref()
                    .is_none_or(|p| r.parent_session == p.session_id)
            })
            .collect())
    }

    pub fn get(&self, id: &str, token: Option<&str>) -> Result<Record, Refusal> {
        self.list(token)?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| Refusal::new("delegation_unknown", "no such delegation"))
    }

    /// Stops a running delegation and its whole process group.
    pub fn cancel(&self, id: &str, token: Option<&str>) -> Result<Record, Refusal> {
        let record = self.get(id, token)?;
        if record.status != "running" || record.finished() {
            return Ok(record);
        }
        let live = lock(&self.live).workers.get(id).cloned();
        if let Some(live) = live {
            let mut record = lock(&live.record);
            if record.status == "running" && !record.finished() && !child_has_exited(live.child_id)
            {
                record.status = "cancelled".into();
                record.error = Some("Cancelled.".into());
            }
        }
        self.get(id, token)
    }

    #[cfg(test)]
    fn running(&self) -> usize {
        let live = lock(&self.live);
        live.workers.len() + live.starting
    }

    fn reserve_start(&self, limit: usize) -> Result<StartReservation, Refusal> {
        let mut live = lock(&self.live);
        if live.workers.len() + live.starting >= limit {
            return Err(Refusal::new(
                "delegation_limit",
                format!("{limit} delegated agents are already running"),
            ));
        }
        live.starting += 1;
        Ok(StartReservation {
            live: self.live.clone(),
            active: true,
        })
    }

    /// The models and reasoning levels a provider accepts here, as its own installation says.
    pub fn options(&self, provider: &str, executable: Option<&Path>) -> Value {
        let (policy, note, source) = load_policy_details();
        let model_source = if provider == "codex" {
            "local_provider_catalogue"
        } else {
            "owner_policy_and_cli_syntax"
        };
        let models = match provider {
            "codex" => executable
                .and_then(|path| self.codex_models(path))
                .map(|catalogue| {
                    catalogue["models"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|m| m["visibility"] == "list")
                        .map(|m| {
                            json!({
                                "model": m["slug"],
                                "efforts": m["supported_reasoning_levels"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .filter_map(|l| l["effort"].as_str().or(l.as_str()))
                                    .collect::<Vec<_>>(),
                                "default_effort": m["default_reasoning_level"],
                                "source": model_source,
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            _ => policy
                .claude_models
                .iter()
                .map(|m| json!({"model": m, "efforts": worker::CLAUDE_EFFORTS, "source": model_source}))
                .collect(),
        };
        let runtime_confirmed: Vec<(String, String)> = self
            .records()
            .into_iter()
            .filter(|record| {
                record.provider == provider
                    && record.status == "succeeded"
                    && record.verification.state == "verified"
            })
            .filter_map(|record| Some((record.verification.model?, record.verification.effort?)))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        json!({
            "provider": provider,
            "models": models,
            "model_source": model_source,
            "effort_source": model_source,
            "availability_scope": "CLI and owner policy evidence do not confirm account availability",
            "runtime_confirmed": runtime_confirmed.iter().map(|(model, effort)| json!({"model": model, "effort": effort})).collect::<Vec<_>>(),
            "runtime_confirmed_source": "succeeded_verified_delegation_records",
            "policy": policy,
            "policy_source": source,
            "policy_note": note
        })
    }

    /// `codex debug models` (no inference, no network for the catalogue it reads), kept ten minutes.
    fn codex_models(&self, codex: &Path) -> Option<Value> {
        let mut cache = lock(&self.models);
        if let Some((at, value)) = cache.as_ref()
            && at.elapsed() < Duration::from_secs(600)
        {
            return Some(value.clone());
        }
        let output = Command::new(codex)
            .args(["debug", "models"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .env_remove(TOKEN_VAR)
            .output()
            .ok()?;
        let value: Value = serde_json::from_slice(&output.stdout).ok()?;
        *cache = Some((Instant::now(), value.clone()));
        Some(value)
    }

    fn check_model(
        &self,
        request: &Request,
        executable: &Path,
        policy: &Policy,
    ) -> Result<(), Refusal> {
        match request.provider.as_str() {
            "codex" => {
                let catalogue = self.codex_models(executable).ok_or_else(|| {
                    Refusal::new(
                        "model_catalogue_unavailable",
                        "Codex did not list its models",
                    )
                })?;
                let model = catalogue["models"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|m| m["slug"] == request.model.as_str() && m["visibility"] == "list")
                    .ok_or_else(|| {
                        Refusal::new(
                            "model_unsupported",
                            format!("Codex does not offer the model {}", request.model),
                        )
                    })?;
                let efforts: Vec<&str> = model["supported_reasoning_levels"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|l| l["effort"].as_str().or(l.as_str()))
                    .collect();
                if !efforts.contains(&request.effort.as_str()) {
                    return Err(Refusal::new(
                        "effort_unsupported",
                        format!("{} supports {}", request.model, efforts.join(", ")),
                    ));
                }
            }
            _ => {
                if !policy.claude_models.contains(&request.model) {
                    return Err(Refusal::new(
                        "model_unsupported",
                        format!(
                            "{} is not among the Claude Code models the delegation policy allows",
                            request.model
                        ),
                    ));
                }
                if !worker::CLAUDE_EFFORTS.contains(&request.effort.as_str()) {
                    return Err(Refusal::new(
                        "effort_unsupported",
                        format!("Claude Code supports {}", worker::CLAUDE_EFFORTS.join(", ")),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Validates, plans and starts one delegated agent. Returns its record, `running`.
    pub fn start(&self, request: Request, executable: &Path) -> Result<Record, Refusal> {
        let (policy, note) = load_policy();
        self.start_with_policy(request, executable, policy, note)
    }

    /// Validates and prepares the same launch plan as `start`, without reserving or launching.
    pub fn preview(
        &self,
        request: Request,
        executable: &Path,
    ) -> Result<DelegationPreview, Refusal> {
        let (policy, note, source) = load_policy_details();
        self.preview_with_policy(request, executable, policy, note, source)
    }

    fn preview_with_policy(
        &self,
        request: Request,
        executable: &Path,
        policy: Policy,
        note: Option<String>,
        policy_source: String,
    ) -> Result<DelegationPreview, Refusal> {
        let validated = self.validate_request(&request, &policy, note.as_deref())?;
        self.check_model(&request, executable, &policy)?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let dir = self.base.join(format!(".preview-{id}"));
        let directory = WorkerDirectory::create(dir.clone())?;
        let prepared = self.prepare_worker(&request, executable, &policy, validated, id, &dir)?;
        let record = prepared.record;
        let preview = DelegationPreview {
            preview: true,
            provider: record.provider,
            model: record.model,
            effort: record.effort,
            skills: record.applied.skills.clone(),
            preset: record.preset,
            selection: record.selection,
            policy,
            policy_source,
            policy_note: note,
            applied: record.applied,
        };
        drop(directory);
        Ok(preview)
    }

    fn start_with_policy(
        &self,
        request: Request,
        executable: &Path,
        policy: Policy,
        note: Option<String>,
    ) -> Result<Record, Refusal> {
        let validated = self.validate_request(&request, &policy, note.as_deref())?;
        let reservation = self.reserve_start(policy.max_concurrent)?;
        self.check_model(&request, executable, &policy)?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let dir = self.base.join(&id);
        let mut directory = WorkerDirectory::create(dir.clone())?;
        let prepared = self.prepare_worker(&request, executable, &policy, validated, id, &dir)?;
        // Linearize the launch against session revocation after potentially slow planning.
        let parents = lock(&self.tokens);
        if !parents.contains_key(&request.token) {
            return Err(Refusal::new(
                "delegation_unauthorized",
                "The orchestrator session has ended.",
            ));
        }
        let result =
            self.launch_worker_with_reservation(prepared.record, prepared.spec, Some(reservation));
        drop(parents);
        if result.is_ok() {
            directory.keep();
        }
        result
    }

    fn validate_request(
        &self,
        request: &Request,
        policy: &Policy,
        note: Option<&str>,
    ) -> Result<ValidatedRequest, Refusal> {
        let parent = self.parent(&request.token).ok_or_else(|| {
            Refusal::new("delegation_unauthorized", "This session may not delegate.")
        })?;
        if note.is_some() {
            return Err(Refusal::new(
                "policy_invalid",
                "The delegation policy is unreadable or invalid; no agent was started.",
            ));
        }
        let agent = match request.provider.as_str() {
            "codex" => AgentId::Codex,
            "claude_code" => AgentId::ClaudeCode,
            other => return Err(Refusal::new("provider_unsupported", other.to_owned())),
        };
        if !policy.providers.contains(&request.provider) {
            return Err(Refusal::new(
                "provider_not_allowed",
                format!("The delegation policy does not allow {}", request.provider),
            ));
        }
        if request.task.trim().is_empty()
            || request.task.len() > MAX_TASK
            || request.task.contains('\0')
        {
            return Err(Refusal::new(
                "task_invalid",
                "The task is empty, too long or not text.",
            ));
        }
        let minutes = request.timeout_minutes.unwrap_or(policy.max_minutes);
        if minutes == 0 || minutes > policy.max_minutes {
            return Err(Refusal::new(
                "timeout_not_allowed",
                format!("At most {} minutes", policy.max_minutes),
            ));
        }
        Ok(ValidatedRequest {
            parent,
            agent,
            minutes,
        })
    }

    fn prepare_worker(
        &self,
        request: &Request,
        executable: &Path,
        policy: &Policy,
        validated: ValidatedRequest,
        id: String,
        dir: &Path,
    ) -> Result<PreparedWorker, Refusal> {
        let ValidatedRequest {
            parent,
            agent,
            minutes,
        } = validated;
        let workspace = dir.join("workspace");

        // The worker's skills: exactly the capabilities the request names, planned as a terminal's.
        let version = catalogue_version(agent, executable);
        let (plan, installed, picked) =
            plan_skills(request, &id, agent, version.clone(), &workspace)?;
        let launch = ArsenalLaunch {
            selection: id.clone(),
            selection_name: request
                .title
                .clone()
                .filter(|t| {
                    !t.trim().is_empty()
                        && t.chars().count() <= 80
                        && !t.chars().any(char::is_control)
                })
                .unwrap_or_else(|| "Delegation".into()),
            config_hash: plan.config_hash.clone(),
            skills_off: plan.excluded.clone(),
            skill_files_off: plan.excluded_files.clone(),
        };
        let mut words = LaunchPlan::default();
        crate::native::arsenal::apply(&mut words, agent, &launch);
        if !words.not_applicable.is_empty() {
            return Err(Refusal {
                code: "arsenal_not_applicable",
                reasons: words.not_applicable,
            });
        }

        let last = dir.join("last-message.txt");
        let task = worker::Launch {
            model: &request.model,
            effort: &request.effort,
            workspace: &workspace,
            task: &request.task,
        };
        let enabled_plugins: BTreeSet<String> = installed
            .composites
            .iter()
            .filter(|c| c.enabled == Some(true))
            .filter_map(|c| c.key.split_once(':').map(|(_, key)| key.to_owned()))
            .collect();
        let (argv, tools_off) = match agent {
            AgentId::Codex => {
                // `--no-daemon` belongs to the interactive TUI; `codex exec` runs in its own process.
                let skill_words: Vec<String> = words
                    .words
                    .iter()
                    .filter_map(|w| match w {
                        Word::Literal(text) if text != "--no-daemon" => Some(text.clone()),
                        _ => None,
                    })
                    .collect();
                let argv = worker::codex_argv(
                    &task,
                    &last,
                    &skill_words,
                    &installed.mcp_servers,
                    &enabled_plugins,
                )
                .map_err(|name| {
                    Refusal::new(
                        "tools_not_closable",
                        format!("Codex cannot turn off \"{name}\" from the command line"),
                    )
                })?;
                (
                    argv,
                    vec![
                        format!("{} MCP server(s) off", installed.mcp_servers.len()),
                        format!("{} plugin(s) off", enabled_plugins.len()),
                        "multi-agent subagents off".into(),
                    ],
                )
            }
            _ => {
                let mut overlay: serde_json::Map<String, Value> = words
                    .overlay
                    .as_deref()
                    .and_then(|text| serde_json::from_str(text).ok())
                    .unwrap_or_default();
                overlay.insert(
                    "enabledPlugins".into(),
                    Value::Object(
                        enabled_plugins
                            .iter()
                            .map(|key| (key.clone(), Value::Bool(false)))
                            .collect(),
                    ),
                );
                let settings = dir.join("settings.json");
                let written = serde_json::to_vec_pretty(&Value::Object(overlay))
                    .ok()
                    .is_some_and(|bytes| write_private(&settings, &bytes).is_ok());
                if !written {
                    return Err(Refusal::new("store_unavailable", "settings not written"));
                }
                let mut off = vec![
                    "MCP servers".to_owned(),
                    "plugins".to_owned(),
                    "web tools".to_owned(),
                    "Agent and Task tools".to_owned(),
                ];
                if !policy.claude_allow_bash {
                    off.push("shell".into());
                }
                (
                    worker::claude_argv(&task, &settings, policy.claude_allow_bash),
                    off,
                )
            }
        };

        let kept_names: Vec<String> = plan
            .kept
            .iter()
            .filter(|n| !plan.inherited_off.contains(n))
            .cloned()
            .collect();
        let record = Record {
            id: id.clone(),
            parent_session: parent.session_id.clone(),
            parent_provider: parent.provider.clone(),
            provider: request.provider.clone(),
            model: request.model.clone(),
            effort: request.effort.clone(),
            title: launch.selection_name.clone(),
            task: request.task.clone(),
            preset: request.preset.clone(),
            selection: request.selection.clone(),
            workspace: workspace.display().to_string(),
            created_at: now(),
            timeout_minutes: minutes,
            status: "running".into(),
            applied: Applied {
                provider_version: version,
                evidence: plan.enforcement.evidence.map(|e| e.source.to_owned()),
                skills: picked,
                kept: kept_names.clone(),
                inherited_off: plan.inherited_off.clone(),
                switched_off: plan.excluded.len(),
                config_hash: plan.config_hash.clone(),
                sandbox: if agent == AgentId::Codex {
                    "workspace-write (Codex sandbox); bwrap PID namespace; host filesystem permissions unchanged".into()
                } else {
                    "provider edit permissions; bwrap PID namespace; host filesystem permissions unchanged".into()
                },
                network: if agent == AgentId::ClaudeCode && policy.claude_allow_bash {
                    "web tools off; the allowed shell may use the host network; provider inference stays on".into()
                } else {
                    "worker tools cannot use the network; provider inference stays on".into()
                },
                approvals: "never asked: a request needing more is refused".into(),
                tools_off,
            },
            verification: Verification {
                state: "pending".into(),
                ..Verification::default()
            },
            ..Record::default()
        };

        let kept_files: Vec<PathBuf> = installed
            .skills
            .iter()
            .zip(&installed.skill_files)
            .filter(|(skill, _)| {
                kept_names.contains(&skill.name) && skill.source != "codex_system_skills"
            })
            .filter_map(|(_, file)| file.clone())
            .collect();
        Ok(PreparedWorker {
            record,
            spec: WorkerSpec {
                executable: executable.to_path_buf(),
                argv,
                dir: dir.to_path_buf(),
                requested_model: request.model.clone(),
                requested_effort: request.effort.clone(),
                excluded: plan.excluded.clone(),
                excluded_files: plan.excluded_files.clone(),
                kept_names,
                kept_files,
                deadline: Duration::from_secs(u64::from(minutes) * 60),
            },
        })
    }

    /// Starts a planned worker in its own process group with only its own environment, reads its
    /// stream into the record, checks the configuration it reports, and stops it at its deadline.
    #[cfg(test)]
    fn launch_worker(&self, record: Record, spec: WorkerSpec) -> Result<Record, Refusal> {
        self.launch_worker_with_reservation(record, spec, None)
    }

    fn launch_worker_with_reservation(
        &self,
        mut record: Record,
        spec: WorkerSpec,
        reservation: Option<StartReservation>,
    ) -> Result<Record, Refusal> {
        let WorkerSpec {
            executable,
            argv,
            dir,
            requested_model,
            requested_effort,
            excluded,
            excluded_files,
            kept_names,
            kept_files,
            deadline,
        } = spec;
        let id = record.id.clone();
        let workspace = PathBuf::from(&record.workspace);
        if !Path::new(BWRAP).is_file() {
            return Err(Refusal::new(
                "process_isolation_unavailable",
                "bubblewrap is required to isolate worker processes.",
            ));
        }
        self.save(&record);
        let events = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(dir.join("events.jsonl"));
        let stderr = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(dir.join("stderr.log"));
        let (Ok(events), Ok(stderr)) = (events, stderr) else {
            record.status = "failed".into();
            record.error = Some("The record could not be written.".into());
            self.save(&record);
            return Err(Refusal::new("store_unavailable", "record not writable"));
        };
        let mut command = Command::new(BWRAP);
        command
            .args([
                "--unshare-user",
                "--unshare-pid",
                "--die-with-parent",
                "--new-session",
                "--bind",
                "/",
                "/",
                "--dev",
                "/dev",
                "--proc",
                "/proc",
                "--",
            ])
            .arg(executable)
            .args(&argv)
            .current_dir(&workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr))
            .env_remove(TOKEN_VAR)
            .env_remove(SESSION_VAR)
            .env("WYSS_DELEGATED_BY", &record.parent_session)
            .process_group(0);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                record.status = "failed".into();
                record.error = Some(format!("The worker did not start: {error}"));
                record.ended_at = Some(now());
                self.save(&record);
                return Ok(record);
            }
        };
        let child_id = child.id();
        let pgid = i32::try_from(child_id).unwrap_or(0);
        let stdout = child.stdout.take();
        let shared = Arc::new(Mutex::new(record.clone()));
        let live = Live {
            record: shared.clone(),
            child_id,
            pgid,
        };
        if let Some(reservation) = reservation {
            reservation.commit(id.clone(), live);
        } else {
            lock(&self.live).workers.insert(id.clone(), live);
        }
        let provider = record.provider.clone();
        let base = self.base.clone();
        let (stream_tx, stream_rx) = std::sync::mpsc::channel();
        let stream_provider = provider.clone();
        let stream_model = requested_model.clone();
        let stream_excluded = excluded.clone();
        let stream_kept_names = kept_names.clone();
        let stream_shared = shared.clone();
        std::thread::spawn(move || {
            let mut seen = worker::Observed::default();
            let mut events = events;
            let mut written = 0_u64;
            let mut checked = false;
            if let Some(stdout) = stdout {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if written < MAX_EVENTS {
                        written += line.len() as u64 + 1;
                        let _ = writeln!(events, "{line}");
                    }
                    worker::observe(&stream_provider, &line, &mut seen);
                    let mut record = lock(&stream_shared);
                    record.steps = seen.steps;
                    record.thread_id = seen.thread_id.clone();
                    // Claude Code says what it was given before it does anything: check it then.
                    if stream_provider == "claude_code" && !checked && seen.model.is_some() {
                        checked = true;
                        let problems = worker::verify_claude(
                            &stream_model,
                            &seen,
                            &stream_excluded,
                            &stream_kept_names,
                        );
                        record.verification = Verification {
                            state: if problems.is_empty() {
                                "verified"
                            } else {
                                "mismatch"
                            }
                            .into(),
                            model: seen.model.clone(),
                            effort: None,
                            problems: problems.clone(),
                            source: Some("Claude Code init event".into()),
                        };
                        if !problems.is_empty() {
                            record.status = "failed".into();
                            record.error = Some("The worker did not start with the planned configuration; it was stopped.".into());
                        }
                    }
                }
            }
            let _ = stream_tx.send(seen);
        });

        let live_state = self.live.clone();
        let watch = shared.clone();
        std::thread::spawn(move || {
            let mut child = child;
            let mut termination_started = false;
            while !child_has_exited(child_id) {
                let status = lock(&shared).status.clone();
                if status != "running" && !termination_started {
                    stop_group(&mut child, pgid, Duration::from_secs(5));
                    termination_started = true;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            // The provider may have exited while a descendant still holds its stdout pipe open.
            // Kill remaining members before waiting for the stream reader or reaping the leader.
            stop_group(&mut child, pgid, Duration::from_millis(100));
            let seen = stream_rx.recv().unwrap_or_default();
            let status = child.wait().ok();
            let mut record = lock(&shared);
            record.exit_code = status.and_then(|s| s.code());
            record.ended_at = Some(now());
            record.usage = seen.usage.clone();
            record.result = seen
                .result
                .clone()
                .or_else(|| {
                    fs::read_to_string(dir.join("last-message.txt"))
                        .ok()
                        .map(|t| t.trim().to_owned())
                })
                .or(seen.last_message.clone());
            if provider == "codex" {
                record.verification = codex_verification(
                    seen.thread_id.as_deref(),
                    &requested_model,
                    &requested_effort,
                    &excluded_files,
                    &kept_files,
                );
            }
            if record.status == "running" {
                let ok =
                    status.is_some_and(|s| s.success()) && seen.error.is_none() && seen.finished;
                record.status = if ok { "succeeded" } else { "failed" }.into();
                if !ok {
                    record.error = seen
                        .error
                        .clone()
                        .or(Some("The worker ended without a result.".into()));
                }
                if record.verification.state == "mismatch" {
                    record.status = "failed".into();
                    record.error =
                        Some("The worker's configuration did not match the plan.".into());
                }
            }
            record.artifacts = artifacts(Path::new(&record.workspace));
            save_at(&base, &record);
            drop(record);
            drop(child);
            lock(&live_state).workers.remove(&id);
        });
        std::thread::spawn(move || {
            let started = Instant::now();
            while started.elapsed() < deadline {
                std::thread::sleep(Duration::from_millis(500));
                let record = lock(&watch);
                if record.status != "running" || record.finished() {
                    return;
                }
            }
            let mut record = lock(&watch);
            if record.status == "running" && !child_has_exited(child_id) {
                record.status = "timed_out".into();
                record.error = Some(format!("Stopped after {} second(s).", deadline.as_secs()));
            }
        });
        let mut started = record;
        started.status = "running".into();
        Ok(started)
    }

    /// Arsenal capabilities a delegation may name for a provider, from the library as stored.
    pub fn capabilities(&self, provider: &str, query: Option<&str>) -> Result<Vec<Value>, Refusal> {
        let store = arsenal::open_read_only().map_err(|_| {
            Refusal::new("arsenal_unavailable", "The Arsenal library cannot be read.")
        })?;
        let library = arsenal::commands::library(&store).map_err(|_| {
            Refusal::new("arsenal_unavailable", "The Arsenal library cannot be read.")
        })?;
        let query = query.map(str::to_lowercase);
        Ok(library
            .into_iter()
            .filter(|item| item.summary.provider == provider && item.summary.kind == "skill")
            .filter(|item| {
                query.as_ref().is_none_or(|q| {
                    item.summary.name.to_lowercase().contains(q)
                        || item.summary.description.to_lowercase().contains(q)
                })
            })
            .take(200)
            .map(|item| {
                json!({
                    "capability": item.summary.id,
                    "name": item.summary.name,
                    "revision": item.summary.revision,
                    "description": item.summary.description.chars().take(160).collect::<String>(),
                    "compatibility": item.compatibility.status,
                })
            })
            .collect())
    }
}

/// Writes a record whole: a reader sees the old one or the new one, never part of one.
fn save_at(base: &Path, record: &Record) {
    let dir = base.join(&record.id);
    if private_dir(&dir).is_err() {
        return;
    }
    let Ok(bytes) = serde_json::to_vec_pretty(record) else {
        return;
    };
    let temporary = dir.join("record.json.new");
    if write_private(&temporary, &bytes).is_ok() {
        let _ = fs::rename(temporary, dir.join("record.json"));
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)
}

fn catalogue_version(agent: AgentId, executable: &Path) -> Option<String> {
    crate::native::catalogue::probe_provider_version(agent, executable)
}

/// Ends a worker's whole process group: politely, then for good.
///
/// Callers hold the unreaped child under its mutex until this returns. That keeps its PID reserved
/// while the delayed group SIGKILL is pending, so the numeric PGID cannot be reused by an unrelated
/// process group in between the two signals.
fn stop_group(child: &mut Child, pgid: i32, grace: Duration) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;
    if pgid <= 1 || i32::try_from(child.id()).ok() != Some(pgid) {
        return;
    }
    let group = Pid::from_raw(pgid);
    if killpg(group, Signal::SIGTERM).is_ok() {
        std::thread::sleep(grace);
        let _ = killpg(group, Signal::SIGKILL);
    }
}

/// Observes child exit without reaping it. Its PID remains reserved while the process group is
/// cleaned up; the owning `Child` is reaped only afterward.
fn child_has_exited(child_id: u32) -> bool {
    use nix::sys::wait::{Id, WaitPidFlag, WaitStatus, waitid};
    use nix::unistd::Pid;

    let Ok(pid) = i32::try_from(child_id) else {
        return false;
    };
    waitid(
        Id::Pid(Pid::from_raw(pid)),
        WaitPidFlag::WEXITED | WaitPidFlag::WNOHANG | WaitPidFlag::WNOWAIT,
    )
    .is_ok_and(|status| !matches!(status, WaitStatus::StillAlive))
}

/// The capabilities a request names, as one transient selection of the worker's provider, planned
/// against the skills installed now. Every named capability must be prepared, and the provider build
/// must have its own exclusion evidence: otherwise nothing starts.
fn plan_skills(
    request: &Request,
    id: &str,
    agent: AgentId,
    version: Option<String>,
    workspace: &Path,
) -> Result<(terminal::TerminalPlan, LaunchScan, Vec<Value>), Refusal> {
    let store = arsenal::open_read_only()
        .map_err(|_| Refusal::new("arsenal_unavailable", "The Arsenal library cannot be read."))?;
    let mut items: Vec<store::Item> = Vec::new();
    let mut composites: Vec<String> = Vec::new();
    if let Some(wanted) = &request.preset {
        let preset = store
            .presets()
            .map_err(|_| Refusal::new("arsenal_unavailable", "presets"))?
            .into_iter()
            .find(|p| &p.id == wanted || &p.name == wanted)
            .ok_or_else(|| Refusal::new("preset_unknown", wanted.clone()))?;
        items.extend(preset.items);
    }
    if let Some(wanted) = &request.selection {
        let selection = store
            .selections()
            .map_err(|_| Refusal::new("arsenal_unavailable", "selections"))?
            .into_iter()
            .find(|s| (&s.id == wanted || &s.name == wanted) && s.provider == request.provider)
            .ok_or_else(|| Refusal::new("selection_unknown", wanted.clone()))?;
        items.extend(selection.items);
        composites.extend(selection.composites);
    }
    items.extend(request.skills.iter().map(|pick| store::Item {
        capability: pick.capability.clone(),
        revision: pick.revision,
    }));
    let items = merge_capability_items(items)?;
    if items.len() > MAX_ITEMS {
        return Err(Refusal::new(
            "too_many_capabilities",
            format!("at most {MAX_ITEMS}"),
        ));
    }
    let selection = store::Selection {
        id: format!("delegation-{id}"),
        name: request.title.clone().unwrap_or_else(|| "Delegation".into()),
        provider: request.provider.clone(),
        items,
        composites,
        created_at: 0,
        updated_at: 0,
    };
    let (resolved, _) = arsenal::commands::resolve_items(&store, &selection)
        .map_err(|_| Refusal::new("arsenal_unavailable", "capabilities"))?;
    let picked: Vec<Value> = resolved
        .iter()
        .map(|r| json!({"capability": r.capability, "name": r.name, "revision": r.revision, "pinned": r.pinned}))
        .collect();
    let installed =
        catalogue::launch_scan_process(Some(workspace.to_path_buf()), &request.provider)
            .ok_or_else(|| Refusal::new("discovery_unavailable", "installed skills"))?;
    let evidence = capabilities::arsenal_exclusion(agent, version.as_deref());
    let plan = terminal::plan(terminal::Input {
        selection: &selection,
        items: resolved,
        provider: &request.provider,
        version,
        evidence,
        installed: &installed,
        project: None,
    });
    if plan.enforcement.state != "available" {
        return Err(Refusal::new(
            "arsenal_unqualified",
            plan.enforcement.reason.clone(),
        ));
    }
    let not_carried: Vec<String> = plan
        .items
        .iter()
        .filter(|item| item.state != "prepared")
        .map(|item| format!("{}: {}", item.name, item.reason))
        .collect();
    if !not_carried.is_empty() || !plan.can_apply {
        let mut reasons = not_carried;
        reasons.extend(
            plan.conflicts
                .iter()
                .filter(|c| c.blocking)
                .map(|c| c.names.join(", ")),
        );
        return Err(Refusal {
            code: "arsenal_blocked",
            reasons,
        });
    }
    terminal::validate_exclusion(&plan.excluded, &installed)
        .map_err(|e| Refusal::new("arsenal_blocked", e))?;
    terminal::validate_files(&plan.excluded_files, &installed)
        .map_err(|e| Refusal::new("arsenal_blocked", e))?;
    Ok((plan, installed, picked))
}

fn merge_capability_items(items: Vec<store::Item>) -> Result<Vec<store::Item>, Refusal> {
    let mut merged: Vec<store::Item> = Vec::new();
    let mut positions: BTreeMap<String, usize> = BTreeMap::new();
    for item in items {
        if let Some(index) = positions.get(&item.capability).copied() {
            match (merged[index].revision, item.revision) {
                (Some(existing), Some(requested)) if existing != requested => {
                    return Err(Refusal::new(
                        "capability_revision_conflict",
                        format!("{} has conflicting pinned revisions", item.capability),
                    ));
                }
                (None, Some(requested)) => merged[index].revision = Some(requested),
                _ => {}
            }
        } else {
            positions.insert(item.capability.clone(), merged.len());
            merged.push(item);
        }
    }
    Ok(merged)
}

/// A Codex worker's configuration, read from its own session record after it ran.
fn codex_verification(
    thread: Option<&str>,
    requested_model: &str,
    requested_effort: &str,
    excluded: &[String],
    kept: &[PathBuf],
) -> Verification {
    let Some(thread) = thread else {
        return Verification {
            state: "unavailable".into(),
            problems: vec!["Codex did not report its session".into()],
            ..Verification::default()
        };
    };
    let Some(rollout) = find_rollout(thread) else {
        return Verification {
            state: "unavailable".into(),
            problems: vec!["Codex's session record was not found".into()],
            ..Verification::default()
        };
    };
    let Some(text) = fs::read_to_string(&rollout).ok() else {
        return Verification {
            state: "unavailable".into(),
            problems: vec!["Codex's session record could not be read".into()],
            ..Verification::default()
        };
    };
    codex_verification_from_text(
        &text,
        thread,
        requested_model,
        requested_effort,
        excluded,
        kept,
        rollout.display().to_string(),
    )
}

fn codex_verification_from_text(
    text: &str,
    thread: &str,
    requested_model: &str,
    requested_effort: &str,
    excluded: &[String],
    kept: &[PathBuf],
    source: String,
) -> Verification {
    let listed = worker::codex_listed_files(text);
    let runtime = worker::codex_runtime(text, thread);
    let mut problems = Vec::new();
    let mut mismatch = false;
    if let Some(listed) = &listed {
        let findings = worker::verify_codex(listed, excluded, kept);
        mismatch |= !findings.is_empty();
        problems.extend(findings);
    } else {
        problems.push("Codex's session record lists no skills".into());
    }
    let runtime_findings =
        worker::verify_codex_runtime(requested_model, requested_effort, runtime.as_ref());
    if runtime.is_some() {
        mismatch |= !runtime_findings.is_empty();
    }
    problems.extend(runtime_findings);
    let unavailable = listed.is_none() || runtime.is_none();
    Verification {
        state: if mismatch {
            "mismatch"
        } else if unavailable {
            "unavailable"
        } else {
            "verified"
        }
        .into(),
        model: runtime.as_ref().map(|runtime| runtime.model.clone()),
        effort: runtime.as_ref().map(|runtime| runtime.effort.clone()),
        problems,
        source: Some(source),
    }
}

/// `~/.codex/sessions/<y>/<m>/<d>/rollout-…-<thread>.jsonl`, looked for in the last two days.
fn find_rollout(thread: &str) -> Option<PathBuf> {
    if !thread
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return None;
    }
    let codex = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")))?;
    let sessions = codex.join("sessions");
    let mut days: Vec<PathBuf> = Vec::new();
    for year in fs::read_dir(&sessions).ok()?.flatten() {
        for month in fs::read_dir(year.path()).into_iter().flatten().flatten() {
            for day in fs::read_dir(month.path()).into_iter().flatten().flatten() {
                days.push(day.path());
            }
        }
    }
    days.sort();
    for day in days.iter().rev().take(2) {
        for file in fs::read_dir(day).into_iter().flatten().flatten() {
            let name = file.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("rollout-") && name.ends_with(&format!("{thread}.jsonl")) {
                return Some(file.path());
            }
        }
    }
    None
}

/// The files a worker left in its workspace: relative path and size, no links followed.
fn artifacts(workspace: &Path) -> Vec<Value> {
    let mut found = Vec::new();
    let mut stack = vec![workspace.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
            if found.len() >= MAX_ARTIFACTS {
                return found;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() {
                let size = entry.metadata().map_or(0, |m| m.len());
                let relative = path.strip_prefix(workspace).unwrap_or(&path);
                found.push(json!({"path": relative.display().to_string(), "size": size}));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests;
