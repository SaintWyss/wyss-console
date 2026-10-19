//! Changing the configuration (F2a): plan, confirm, apply, journal, undo.
//!
//! The service holds no secret and no content: a plan keeps the bytes it would write in
//! memory for at most a minute, a record is sealed by the vault, the journal is closed
//! words and hashes. The page asks for a plan by naming an item or a record the service
//! already knows, never a path and never text, and gets back a redacted view; it applies a
//! plan by its identifier. A plan that needs the native dialog is applied only if the user
//! accepts there, in a window the page cannot draw on or answer for.
use super::author::{self, HookSpec};
use super::change::{Operation, Outcome, Prior, Risk};
use super::codex_author;
use super::codex_plan::{self, CodexOperation, CodexPlanError};
use super::fault::{Faults, Injected, Step};
use super::fsutil::private_dir;
use super::inventory::{Environment, SourceKey};
use super::journal::{Change, Entry, Journal, JournalError, Phase, new_id, now};
use super::model::{
    Bounds, ItemKind, NotWritable, PathTrust, ProviderId, Scope, SourceClass, SourceClass as Class,
    SourceFailure, writability,
};
use super::opencode_plan::{self, McpSpec};
use super::place::{Parent, parent_for_write};
use super::plan::{
    DiffLine, Drafted, PLAN_LIFETIME_SECONDS, PlanError, Prepared, draft_all_hooks, draft_park,
    draft_restore, draft_skill, hash, is_exactly,
};
use super::reader::ConfigReader;
use super::restore::{Protection, RestorationStore, RestoreError, Summary};
use super::service::InventoryService;
use super::vault::{EXPORT_ROUNDS, VaultError};
use super::writer::{
    TargetLock, WriteError, conflict_copy_beside, identity_of, keep_leftover, leftovers,
    lock_target, put_back, read_leftover, remove_leftover, replace,
};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

/// 07 §8: at most eight plans drafted at once.
const MOST_PLANS: usize = 8;

/// What the user is asked in the native dialog. No value of any entry is in it: the entry
/// is named by the same redacted label the plan shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmRequest {
    pub provider: ProviderId,
    pub scope: Scope,
    pub source: SourceKey,
    pub name: String,
    pub operation: Operation,
    pub risk: Risk,
    pub effect: Effect,
    pub holds_hidden_values: bool,
    /// For something the user wrote: what will run, as the backend renders it (cut and plain),
    /// so that the confirmation shows the thing and not only its name.
    pub detail: Option<String>,
}

/// The native confirmation. It has no "do not ask again".
pub trait Confirm: Send + Sync {
    fn ask(&self, request: &ConfirmRequest) -> bool;
}

/// What "applied" means for a running provider (07 §6): said as it is known, never more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// A new Claude Code session follows the file (checked against 2.1.303 with a real
    /// `claude`); a session already running is documented to reload, and not checked.
    HooksNewSessionsChecked,
    /// A new session refuses a skill switched off (checked against 2.1.303); a running
    /// session is not known, and a skill that comes from a plugin is not affected at all.
    SkillsNewSessionsChecked,
    /// `codex mcp list` shows the switch in a new Codex process (checked against 0.159.0); a
    /// running session is not known.
    CodexMcpChecked,
    /// The key is what Codex's files show for a skill; how a running Codex treats it is not
    /// verified.
    CodexSkillListed,
    /// What OpenCode's file says; how a running OpenCode treats it is not verified.
    OpencodeListed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeFailure {
    NotSupported,
    NotWritable,
    SourceMissing,
    SourceUnreadable,
    SourceMalformed,
    EntryGone,
    EntryChanged,
    AlreadyDone,
    RestoreConflict,
    KeyStoreUnavailable,
    RecordUnreadable,
    KeyLost,
    PlanUnknown,
    NotUndoable,
    ImportInvalid,
    NoPassphrase,
    /// What the user wrote is not an entry that can be written.
    InvalidEntry,
    /// The user closed the window that asked for what they wanted to write.
    Cancelled,
    Internal,
}

impl From<PlanError> for ChangeFailure {
    fn from(error: PlanError) -> Self {
        match error {
            PlanError::NotSupported => Self::NotSupported,
            PlanError::Malformed => Self::SourceMalformed,
            PlanError::EntryGone => Self::EntryGone,
            PlanError::EntryChanged => Self::EntryChanged,
            PlanError::AlreadyDone => Self::AlreadyDone,
            PlanError::RestoreConflict => Self::RestoreConflict,
            PlanError::KeyStoreUnavailable => Self::KeyStoreUnavailable,
            PlanError::RecordUnreadable => Self::RecordUnreadable,
            PlanError::Invalid => Self::InvalidEntry,
        }
    }
}

impl From<RestoreError> for ChangeFailure {
    fn from(error: RestoreError) -> Self {
        match error {
            RestoreError::Vault(VaultError::KeyLost) => Self::KeyLost,
            RestoreError::Vault(VaultError::KeyStoreUnavailable)
            | RestoreError::NoKeyStoreForSecret => Self::KeyStoreUnavailable,
            RestoreError::Vault(VaultError::NoPassphrase) => Self::NoPassphrase,
            RestoreError::Vault(VaultError::Tampered | VaultError::Malformed)
            | RestoreError::Malformed => Self::RecordUnreadable,
            RestoreError::Missing => Self::PlanUnknown,
            RestoreError::ImportInvalid => Self::ImportInvalid,
            _ => Self::Internal,
        }
    }
}

/// A drafted plan as the page sees it: redacted, with no bytes of the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanView {
    pub id: String,
    pub operation: Operation,
    pub provider: ProviderId,
    pub source: SourceKey,
    pub scope: Scope,
    pub name: String,
    pub risk: Risk,
    pub needs_native_dialog: bool,
    pub diff: Vec<DiffLine>,
    /// How an entry that leaves the file would be kept; `None` when nothing is kept.
    pub storage: Option<Protection>,
    pub holds_hidden_values: bool,
    pub effect: Effect,
    /// What the change does to managed Inference, **predicted** from the files and the
    /// authority's rule, never read from a running Codex. Codex plans only.
    pub managed_effect: Option<ManagedEffectView>,
    pub expires_in_seconds: u64,
    pub undoes: Option<String>,
    /// An account the owner has named can also write the folder of this file, which is why it
    /// is accepted at all (`declared.rs`): the page says so.
    pub shared_with_declared_writers: bool,
}

/// A prediction, said as one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManagedEffectView {
    pub predicted: bool,
    pub before: super::codex_plan::Verdict,
    pub after: super::codex_plan::Verdict,
}

/// How an apply ended, and the journal entry that says so.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Applied {
    pub change: String,
    pub outcome: Outcome,
    /// What the file holds now, when the apply wrote it and read it back.
    pub revision: u64,
}

struct Stored {
    id: String,
    drafted: Drafted,
    source: SourceKey,
    undoes: Option<String>,
    shared_with_declared_writers: bool,
    /// What the native confirmation shows beside the name, for authored plans.
    detail: Option<String>,
    at: Instant,
}

pub struct ChangeService {
    reader: ConfigReader,
    environment: Mutex<Environment>,
    inventory: InventoryService,
    journal: Journal,
    store: RestorationStore,
    faults: Faults,
    locks: PathBuf,
    confirm: Box<dyn Confirm>,
    plans: Mutex<Vec<Stored>>,
    bounds: Bounds,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The sources a change may be made to: this user's Claude Code settings files and Codex's
/// user configuration.
fn writable(key: SourceKey) -> bool {
    matches!(
        key,
        SourceKey::ClaudeUserSettings
            | SourceKey::ClaudeProjectSettings
            | SourceKey::ClaudeProjectLocalSettings
            | SourceKey::CodexUserConfig
            | SourceKey::OpencodeUserConfig
    ) && matches!(key.class(), SourceClass::User | SourceClass::Project)
}

fn effect_of(operation: Operation, source: SourceKey) -> Effect {
    match operation {
        Operation::RevertAuthored | Operation::RestoreHook
            if source == SourceKey::OpencodeUserConfig =>
        {
            Effect::OpencodeListed
        }
        Operation::RevertAuthored | Operation::RestoreHook
            if source == SourceKey::CodexUserConfig =>
        {
            Effect::CodexMcpChecked
        }
        Operation::AddMcp | Operation::EditMcp | Operation::ParkMcp => Effect::OpencodeListed,
        Operation::CodexAddMcp | Operation::CodexEditMcp | Operation::CodexParkMcp => {
            Effect::CodexMcpChecked
        }
        Operation::SkillOff | Operation::SkillOn => Effect::SkillsNewSessionsChecked,
        Operation::CodexMcpOff | Operation::CodexMcpOn => Effect::CodexMcpChecked,
        Operation::CodexSkillOff | Operation::CodexSkillOn => Effect::CodexSkillListed,
        Operation::OpencodeMcpOff | Operation::OpencodeMcpOn | Operation::ParkPlugin => {
            Effect::OpencodeListed
        }
        _ => Effect::HooksNewSessionsChecked,
    }
}

impl ChangeService {
    pub fn new(
        reader: ConfigReader,
        environment: Environment,
        inventory: InventoryService,
        state: &std::path::Path,
        store: RestorationStore,
        confirm: Box<dyn Confirm>,
        faults: Faults,
    ) -> Result<Self, super::fsutil::StoreError> {
        let bounds = *reader.bounds();
        Ok(Self {
            journal: Journal::open(state, faults.clone())?,
            locks: private_dir(&state.join("locks"))?,
            reader,
            environment: Mutex::new(environment),
            inventory,
            store,
            faults,
            confirm,
            plans: Mutex::new(Vec::new()),
            bounds,
        })
    }

    /// A service over the sources a change may be made to: Claude Code's settings files and
    /// Codex's user configuration, as the desktop builds it.
    pub fn for_providers(
        environment: Environment,
        inventory: InventoryService,
        state: &std::path::Path,
        vault: super::vault::Vault,
        confirm: Box<dyn Confirm>,
        faults: Faults,
    ) -> Result<Self, ChangeFailure> {
        let roots = super::claude::roots(&environment)
            .and_then(|claude| claude.union(&super::codex::roots(&environment)?))
            .and_then(|roots| roots.union(&super::opencode::roots(&environment)?))
            .map_err(|_| ChangeFailure::Internal)?;
        let reader =
            ConfigReader::new(roots, Bounds::PROPOSED).map_err(|_| ChangeFailure::Internal)?;
        let store = RestorationStore::open(state, vault, faults.clone())?;
        Self::new(
            reader,
            environment,
            inventory,
            state,
            store,
            confirm,
            faults,
        )
        .map_err(|_| ChangeFailure::Internal)
    }

    /// Makes the drafted plans older, for the test of their expiry.
    #[cfg(test)]
    pub fn age_plans(&self, by: Duration) {
        for plan in lock(&self.plans).iter_mut() {
            if let Some(older) = plan.at.checked_sub(by) {
                plan.at = older;
            }
        }
    }

    fn path_of(&self, key: SourceKey) -> Option<PathBuf> {
        let environment = lock(&self.environment);
        let project = environment.project.as_ref();
        match key {
            SourceKey::ClaudeUserSettings => Some(environment.claude_dir().join("settings.json")),
            SourceKey::ClaudeProjectSettings => Some(project?.join(".claude/settings.json")),
            SourceKey::ClaudeProjectLocalSettings => {
                Some(project?.join(".claude/settings.local.json"))
            }
            SourceKey::CodexUserConfig => Some(environment.codex_dir().join("config.toml")),
            SourceKey::OpencodeUserConfig => {
                Some(super::opencode::config_in(&environment.opencode_dir()))
            }
            _ => None,
        }
    }

    /// The file a plan is made on: read through the approved roots, and writable only if it
    /// is this user's, in a place nobody else can change.
    fn read_target(&self, key: SourceKey) -> Result<(PathBuf, Vec<u8>), ChangeFailure> {
        if !writable(key) {
            return Err(ChangeFailure::NotWritable);
        }
        let path = self.path_of(key).ok_or(ChangeFailure::SourceMissing)?;
        let file = self
            .reader
            .read_file(&path)
            .map_err(|failure| match failure {
                SourceFailure::Missing => ChangeFailure::SourceMissing,
                _ => ChangeFailure::SourceUnreadable,
            })?;
        if writability(key.class(), file.trust()) != NotWritable::NotInThisPhase {
            return Err(ChangeFailure::NotWritable);
        }
        Ok((path, file.bytes().to_vec()))
    }

    fn view(&self, stored: &Stored) -> PlanView {
        let drafted = &stored.drafted;
        PlanView {
            id: stored.id.clone(),
            operation: drafted.operation,
            provider: stored.source.provider(),
            source: stored.source,
            scope: stored.source.scope(),
            name: drafted.name.clone(),
            risk: drafted.risk,
            needs_native_dialog: drafted.risk.needs_native_dialog(),
            diff: drafted.diff.clone(),
            storage: match &drafted.prepared {
                Prepared::Park { protection, .. } | Prepared::Inverse { protection, .. } => {
                    Some(*protection)
                }
                _ => None,
            },
            holds_hidden_values: drafted.holds_hidden_values,
            effect: effect_of(drafted.operation, stored.source),
            managed_effect: drafted.managed.as_ref().map(|m| ManagedEffectView {
                predicted: true,
                before: m.before.clone(),
                after: m.after.clone(),
            }),
            expires_in_seconds: PLAN_LIFETIME_SECONDS.saturating_sub(stored.at.elapsed().as_secs()),
            undoes: stored.undoes.clone(),
            shared_with_declared_writers: stored.shared_with_declared_writers,
        }
    }

    fn keep(&self, drafted: Drafted, source: SourceKey, undoes: Option<String>) -> PlanView {
        self.keep_with(drafted, source, undoes, None)
    }

    fn keep_with(
        &self,
        drafted: Drafted,
        source: SourceKey,
        undoes: Option<String>,
        detail: Option<String>,
    ) -> PlanView {
        let stored = Stored {
            id: new_id(),
            drafted,
            source,
            undoes,
            detail,
            shared_with_declared_writers: self
                .path_of(source)
                .and_then(|path| {
                    parent_for_write(self.reader.roots(), &path, self.bounds.link_hops).ok()
                })
                .is_some_and(|parent| parent.shared_with_declared),
            at: Instant::now(),
        };
        let view = self.view(&stored);
        let mut plans = lock(&self.plans);
        plans.retain(|p| p.at.elapsed() < Duration::from_secs(PLAN_LIFETIME_SECONDS));
        while plans.len() >= MOST_PLANS {
            plans.remove(0);
        }
        plans.push(stored);
        view
    }

    /// `~/.codex/hooks.json`, if there is one and it reads: part of what decides managed
    /// Inference, so part of the prediction.
    fn codex_hooks_json(&self) -> Option<serde_json::Value> {
        let path = lock(&self.environment).codex_dir().join("hooks.json");
        let file = self.reader.read_file(&path).ok()?;
        super::reader::parse_json(file.bytes(), &self.bounds)
            .ok()
            .flatten()
    }

    fn item(&self, id: &str) -> Option<super::inventory::Item> {
        self.inventory
            .snapshot()
            .inventory
            .items
            .iter()
            .find(|item| item.id == id)
            .cloned()
    }

    /// Drafts a change. `target` is an item's identifier, a record's, or a source's word,
    /// as the operation needs; the page has nothing else to name.
    pub fn plan(&self, operation: Operation, target: &str) -> Result<PlanView, ChangeFailure> {
        self.plan_with(operation, target, None, None)
    }

    /// The command of a hook as the file has it, for the window that lets the user change it.
    /// It goes to that window and nowhere else: no page command returns it.
    pub fn hook_command(&self, item: &str) -> Option<String> {
        let found = self.item(item)?;
        if found.kind != ItemKind::Hook || found.source != SourceKey::ClaudeUserSettings {
            return None;
        }
        let (_, bytes) = self.read_target(found.source).ok()?;
        let (root, value) = super::plan::parse(&bytes).ok()?;
        let hook = super::plan::find_hook(&root, found.source, item)?;
        value
            .pointer(&format!(
                "/hooks/{}/{}/hooks/{}/command",
                hook.event.replace('~', "~0").replace('/', "~1"),
                hook.group,
                hook.index
            ))?
            .as_str()
            .map(str::to_owned)
    }

    /// Which agent an item belongs to, as the inventory holds it.
    pub fn item_provider(&self, item: &str) -> Option<ProviderId> {
        self.item(item).map(|i| i.provider)
    }

    /// The backend's own name for an item (redacted), for a window that says which one it is.
    pub fn item_label(&self, item: &str) -> Option<String> {
        self.item(item).map(|i| i.name.as_str().to_owned())
    }

    /// An OpenCode server's program and arguments, or its address, for the same purpose.
    pub fn mcp_current(&self, item: &str) -> Option<McpSpec> {
        let found = self.item(item)?;
        if found.kind == ItemKind::McpServer && found.source == SourceKey::CodexUserConfig {
            let (_, bytes) = self.read_target(found.source).ok()?;
            let text = std::str::from_utf8(&bytes).ok()?;
            let name = codex_author::resolve_server(text, item).ok()?;
            return codex_author::current(text, &name);
        }
        if found.kind != ItemKind::McpServer || found.source != SourceKey::OpencodeUserConfig {
            return None;
        }
        let (_, bytes) = self.read_target(found.source).ok()?;
        let name = opencode_plan::resolve_server(&bytes, found.source, item).ok()?;
        let value = super::reader::parse_jsonc(&bytes, &self.bounds).ok()??;
        let entry = value.get("mcp")?.get(&name)?;
        match entry.get("type")?.as_str()? {
            "local" => Some(McpSpec::Local {
                command: entry
                    .get("command")?
                    .as_array()?
                    .iter()
                    .map(|a| a.as_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()?,
            }),
            "remote" => Some(McpSpec::Remote {
                url: entry.get("url")?.as_str()?.to_owned(),
            }),
            _ => None,
        }
    }

    /// Drafts a change the user wrote. What they wrote arrives as typed data, from a window of
    /// the desktop's own (never from the page), and is validated here before anything is read.
    pub fn plan_authored(&self, request: Authoring) -> Result<PlanView, ChangeFailure> {
        let protection = |secret: bool| self.store.protection_for(secret);
        let detail;
        let (source, drafted) = match request {
            Authoring::AddHook(spec) => {
                detail = Some(plain_detail("command", &spec.command));
                let source = SourceKey::ClaudeUserSettings;
                let (_, bytes) = self.read_target(source)?;
                (
                    source,
                    author::draft_add_hook(&bytes, source, &spec, &self.bounds, protection)?,
                )
            }
            Authoring::EditHook {
                item,
                command,
                expected,
            } => {
                detail = Some(plain_detail("new command", &command));
                if self.hook_command(&item).as_deref() != Some(expected.as_str()) {
                    return Err(ChangeFailure::EntryChanged);
                }
                let found = self.item(&item).ok_or(ChangeFailure::EntryGone)?;
                if found.kind != ItemKind::Hook
                    || found.provider != ProviderId::ClaudeCode
                    || found.source != SourceKey::ClaudeUserSettings
                {
                    return Err(ChangeFailure::NotSupported);
                }
                let (_, bytes) = self.read_target(found.source)?;
                (
                    found.source,
                    author::draft_edit_hook(
                        &bytes,
                        found.source,
                        &item,
                        &command,
                        &self.bounds,
                        protection,
                    )?,
                )
            }
            Authoring::AddMcp { name, spec } => {
                detail = Some(plain_mcp(&spec));
                let source = SourceKey::OpencodeUserConfig;
                let (_, bytes) = self.read_target(source)?;
                (
                    source,
                    opencode_plan::draft_add_mcp(&bytes, &name, &spec, &self.bounds, protection)?,
                )
            }
            Authoring::EditMcp {
                item,
                spec,
                expected,
            } => {
                detail = Some(plain_mcp(&spec));
                if self.mcp_current(&item).as_ref() != Some(&expected) {
                    return Err(ChangeFailure::EntryChanged);
                }
                let found = self.item(&item).ok_or(ChangeFailure::EntryGone)?;
                if found.kind != ItemKind::McpServer
                    || found.provider != ProviderId::Opencode
                    || found.source != SourceKey::OpencodeUserConfig
                {
                    return Err(ChangeFailure::NotSupported);
                }
                let (_, bytes) = self.read_target(found.source)?;
                let name = opencode_plan::resolve_server(&bytes, found.source, &item)?;
                (
                    found.source,
                    opencode_plan::draft_edit_mcp(&bytes, &name, &spec, &self.bounds, protection)?,
                )
            }
            Authoring::AddCodexMcp { name, spec } => {
                detail = Some(plain_mcp(&spec));
                let source = SourceKey::CodexUserConfig;
                let (_, bytes) = self.read_target(source)?;
                let text =
                    std::str::from_utf8(&bytes).map_err(|_| ChangeFailure::SourceMalformed)?;
                (
                    source,
                    codex_author::draft_add(text, &name, &spec, &self.bounds, protection)?,
                )
            }
            Authoring::EditCodexMcp {
                item,
                spec,
                expected,
            } => {
                detail = Some(plain_mcp(&spec));
                if self.mcp_current(&item).as_ref() != Some(&expected) {
                    return Err(ChangeFailure::EntryChanged);
                }
                let found = self.item(&item).ok_or(ChangeFailure::EntryGone)?;
                if found.kind != ItemKind::McpServer
                    || found.provider != ProviderId::Codex
                    || found.source != SourceKey::CodexUserConfig
                {
                    return Err(ChangeFailure::NotSupported);
                }
                let (_, bytes) = self.read_target(found.source)?;
                let text =
                    std::str::from_utf8(&bytes).map_err(|_| ChangeFailure::SourceMalformed)?;
                let name = codex_author::resolve_server(text, &item)?;
                (
                    found.source,
                    codex_author::draft_edit(text, &name, &spec, &self.bounds, protection)?,
                )
            }
        };
        Ok(self.keep_with(drafted, source, None, detail))
    }

    fn plan_with(
        &self,
        operation: Operation,
        target: &str,
        undoes: Option<String>,
        restore: Option<Prior>,
    ) -> Result<PlanView, ChangeFailure> {
        let (source, drafted) = match operation {
            Operation::ParkHook => {
                let item = self.item(target).ok_or(ChangeFailure::EntryGone)?;
                if item.kind != ItemKind::Hook || item.provider != ProviderId::ClaudeCode {
                    return Err(ChangeFailure::NotSupported);
                }
                let (_, bytes) = self.read_target(item.source)?;
                let drafted = draft_park(&bytes, item.source, target, &self.bounds, |secret| {
                    self.store.protection_for(secret)
                })?;
                (item.source, drafted)
            }
            Operation::SkillOff | Operation::SkillOn => {
                let item = self.item(target).ok_or(ChangeFailure::EntryGone)?;
                if item.kind != ItemKind::Skill || item.provider != ProviderId::ClaudeCode {
                    return Err(ChangeFailure::NotSupported);
                }
                let source = match item.source {
                    SourceKey::ClaudeUserSkills => SourceKey::ClaudeUserSettings,
                    SourceKey::ClaudeProjectSkills => SourceKey::ClaudeProjectSettings,
                    _ => return Err(ChangeFailure::NotSupported),
                };
                let name = item.name.as_str();
                // A name that redaction changed is not a name the file has.
                if name.contains(super::redact::MASK) {
                    return Err(ChangeFailure::NotSupported);
                }
                let (_, bytes) = self.read_target(source)?;
                let drafted = draft_skill(&bytes, target, name, operation == Operation::SkillOff)?;
                (source, drafted)
            }
            Operation::DisableAllHooks | Operation::EnableAllHooks => {
                let source: SourceKey =
                    serde_json::from_value(serde_json::Value::String(target.to_owned()))
                        .map_err(|_| ChangeFailure::NotSupported)?;
                let (_, bytes) = self.read_target(source)?;
                (
                    source,
                    draft_all_hooks(&bytes, source, operation == Operation::DisableAllHooks)?,
                )
            }
            Operation::CodexSkillOff
            | Operation::CodexSkillOn
            | Operation::CodexMcpOff
            | Operation::CodexMcpOn => {
                let skill = matches!(
                    operation,
                    Operation::CodexSkillOff | Operation::CodexSkillOn
                );
                let item = self.item(target).ok_or(ChangeFailure::EntryGone)?;
                let right_kind = if skill {
                    ItemKind::Skill
                } else {
                    ItemKind::McpServer
                };
                if item.provider != ProviderId::Codex
                    || item.source != SourceKey::CodexUserConfig
                    || item.kind != right_kind
                {
                    return Err(ChangeFailure::NotSupported);
                }
                let (_, bytes) = self.read_target(item.source)?;
                let text =
                    std::str::from_utf8(&bytes).map_err(|_| ChangeFailure::SourceMalformed)?;
                let name =
                    codex_plan::resolve(text, item.source, skill, target).map_err(codex_failure)?;
                let hooks_json = self.codex_hooks_json();
                let cop = match operation {
                    Operation::CodexSkillOff => CodexOperation::SkillOff,
                    Operation::CodexSkillOn => CodexOperation::SkillOn,
                    Operation::CodexMcpOff => CodexOperation::McpOff,
                    _ => CodexOperation::McpOn,
                };
                let planned = codex_plan::draft(text, hooks_json.as_ref(), cop, &name, restore)
                    .map_err(codex_failure)?;
                // On for a server starts a program: the native dialog, as for a hook put
                // back (11 §4). Everything else Codex-side is a switch off, or a skill.
                let risk = if operation == Operation::CodexMcpOn {
                    Risk::ExecutesCode
                } else {
                    Risk::Normal
                };
                let word = |on: bool| if on { "true" } else { "false" };
                let diff = match planned.logical {
                    codex_plan::Logical::Set {
                        value: serde_json::Value::Bool(on),
                        ..
                    } => vec![
                        DiffLine {
                            kind: super::plan::DiffKind::Removed,
                            text: format!(
                                "enabled: {}",
                                match planned.prior {
                                    Prior::Absent => "(not set, so on)",
                                    Prior::Enabled => "true",
                                    Prior::Disabled => "false",
                                }
                            ),
                        },
                        DiffLine {
                            kind: super::plan::DiffKind::Added,
                            text: format!("enabled: {}", word(on)),
                        },
                    ],
                    _ => vec![DiffLine {
                        kind: super::plan::DiffKind::Removed,
                        text: format!(
                            "enabled: {} (the line is removed)",
                            if planned.prior == Prior::Disabled {
                                "false"
                            } else {
                                "true"
                            }
                        ),
                    }],
                };
                (
                    item.source,
                    Drafted {
                        operation,
                        item: target.to_owned(),
                        name: item.name.as_str().to_owned(),
                        risk,
                        h0: hash(&bytes),
                        h1: hash(&planned.new_bytes),
                        new_bytes: planned.new_bytes,
                        logical: super::plan::Logical::Toml(planned.logical),
                        prepared: Prepared::Nothing,
                        diff,
                        holds_hidden_values: false,
                        prior: Some(planned.prior),
                        managed: Some(planned.prediction),
                    },
                )
            }
            // These carry text the user wrote, which only `plan_authored` accepts.
            Operation::AddHook
            | Operation::EditHook
            | Operation::AddMcp
            | Operation::EditMcp
            | Operation::CodexAddMcp
            | Operation::CodexEditMcp => {
                return Err(ChangeFailure::NotSupported);
            }
            Operation::CodexParkMcp => {
                let item = self.item(target).ok_or(ChangeFailure::EntryGone)?;
                if item.provider != ProviderId::Codex
                    || item.source != SourceKey::CodexUserConfig
                    || item.kind != ItemKind::McpServer
                {
                    return Err(ChangeFailure::NotSupported);
                }
                let (_, bytes) = self.read_target(item.source)?;
                let text =
                    std::str::from_utf8(&bytes).map_err(|_| ChangeFailure::SourceMalformed)?;
                let name = codex_author::resolve_server(text, target)?;
                let drafted = codex_author::draft_park(text, &name, &self.bounds, |s| {
                    self.store.protection_for(s)
                })?;
                (item.source, drafted)
            }
            Operation::ParkMcp => {
                let item = self.item(target).ok_or(ChangeFailure::EntryGone)?;
                if item.provider != ProviderId::Opencode
                    || item.source != SourceKey::OpencodeUserConfig
                    || item.kind != ItemKind::McpServer
                {
                    return Err(ChangeFailure::NotSupported);
                }
                let (_, bytes) = self.read_target(item.source)?;
                let name = opencode_plan::resolve_server(&bytes, item.source, target)?;
                let drafted = opencode_plan::draft_park_mcp(&bytes, &name, &self.bounds, |s| {
                    self.store.protection_for(s)
                })?;
                (item.source, drafted)
            }
            Operation::RevertAuthored => {
                let body = self.store.read(target)?;
                let (_, bytes) = self.read_target(body.source)?;
                (body.source, author::draft_revert(&bytes, target, &body)?)
            }
            Operation::OpencodeMcpOff | Operation::OpencodeMcpOn => {
                let item = self.item(target).ok_or(ChangeFailure::EntryGone)?;
                if item.provider != ProviderId::Opencode
                    || item.source != SourceKey::OpencodeUserConfig
                    || item.kind != ItemKind::McpServer
                {
                    return Err(ChangeFailure::NotSupported);
                }
                let (_, bytes) = self.read_target(item.source)?;
                let name = opencode_plan::resolve_server(&bytes, item.source, target)?;
                let drafted = opencode_plan::drafted_mcp(
                    &bytes,
                    target,
                    &name,
                    item.name.as_str(),
                    operation == Operation::OpencodeMcpOn,
                    restore,
                )?;
                (item.source, drafted)
            }
            Operation::ParkPlugin => {
                let item = self.item(target).ok_or(ChangeFailure::EntryGone)?;
                if item.provider != ProviderId::Opencode
                    || item.source != SourceKey::OpencodeUserConfig
                    || item.kind != ItemKind::Plugin
                {
                    return Err(ChangeFailure::NotSupported);
                }
                let (_, bytes) = self.read_target(item.source)?;
                let (index, _) = opencode_plan::resolve_plugin(&bytes, item.source, target)?;
                let drafted = opencode_plan::drafted_park(
                    &bytes,
                    item.source,
                    target,
                    index,
                    &self.bounds,
                    |secret| self.store.protection_for(secret),
                )?;
                (item.source, drafted)
            }
            Operation::RestoreHook => {
                let body = self.store.read(target)?;
                let secret_bearing = self
                    .store
                    .list()
                    .iter()
                    .find(|s| s.id == target)
                    .is_some_and(|s| s.secret_bearing);
                let (_, bytes) = self.read_target(body.source)?;
                let drafted = if body.provider == ProviderId::Codex && body.event == "mcp" {
                    let text =
                        std::str::from_utf8(&bytes).map_err(|_| ChangeFailure::SourceMalformed)?;
                    codex_author::drafted_restore(text, target, &body, &self.bounds)?
                } else if body.provider == ProviderId::Opencode && body.event == "mcp" {
                    opencode_plan::drafted_restore_mcp(&bytes, target, &body, &self.bounds)?
                } else if body.provider == ProviderId::Opencode {
                    opencode_plan::drafted_restore(
                        &bytes,
                        target,
                        &body,
                        secret_bearing,
                        &self.bounds,
                    )?
                } else if body.event == "author" {
                    return Err(ChangeFailure::NotSupported);
                } else {
                    draft_restore(&bytes, target, &body, secret_bearing, &self.bounds)?
                };
                (body.source, drafted)
            }
        };
        Ok(self.keep(drafted, source, undoes))
    }

    fn journal_entry(
        &self,
        id: &str,
        phase: Phase,
        stored: &Stored,
        record: Option<String>,
        h1: Option<String>,
        outcome: Option<Outcome>,
    ) -> Entry {
        Entry {
            v: 1,
            id: id.to_owned(),
            at: now(),
            phase,
            provider: stored.source.provider(),
            source: stored.source,
            operation: stored.drafted.operation,
            item: stored.drafted.item.clone(),
            risk: stored.drafted.risk,
            h0: stored.drafted.h0.clone(),
            h1,
            outcome,
            record,
            undoes: stored.undoes.clone(),
            prior: stored.drafted.prior,
        }
    }

    /// Carries a plan out. The page's call is the user's confirmation in the page; a risk
    /// above `normal` also needs the native dialog, which only the desktop can show.
    pub fn apply(&self, plan: &str) -> Result<Applied, ChangeFailure> {
        let stored = {
            let mut plans = lock(&self.plans);
            let index = plans
                .iter()
                .position(|p| p.id == plan)
                .ok_or(ChangeFailure::PlanUnknown)?;
            // Single use: whatever happens next, this plan is gone.
            plans.remove(index)
        };
        let change = new_id();
        let applied = |outcome: Outcome| Applied {
            change: change.clone(),
            outcome,
            revision: self.inventory.snapshot().revision,
        };
        if stored.at.elapsed() >= Duration::from_secs(PLAN_LIFETIME_SECONDS) {
            return Ok(applied(Outcome::Expired));
        }
        let source = stored.source;
        let drafted = &stored.drafted;

        // The native dialog, before anything is written or recorded as done.
        if drafted.risk.needs_native_dialog() {
            let request = ConfirmRequest {
                provider: source.provider(),
                scope: source.scope(),
                source,
                name: drafted.name.clone(),
                operation: drafted.operation,
                risk: drafted.risk,
                effect: effect_of(drafted.operation, source),
                holds_hidden_values: drafted.holds_hidden_values,
                detail: stored.detail.clone(),
            };
            if !self.confirm.ask(&request) {
                // Recorded: what was refused, where, and how risky. No content.
                let intent = self.journal_entry(&change, Phase::Intent, &stored, None, None, None);
                let outcome = self.journal_entry(
                    &change,
                    Phase::Outcome,
                    &stored,
                    None,
                    None,
                    Some(Outcome::RefusedByUser),
                );
                let _ = self.journal.append(&intent, Step::IntentJournaled);
                let _ = self.journal.append(&outcome, Step::OutcomeJournaled);
                return Ok(applied(Outcome::RefusedByUser));
            }
        }

        let Some(path) = self.path_of(source) else {
            return Ok(applied(Outcome::ChangedOutside));
        };
        let Ok(_target_lock): Result<TargetLock, _> = lock_target(&self.locks, &path) else {
            return Ok(applied(Outcome::WriteFailed));
        };

        // Read again under the lock: the plan was made on these bytes or on nothing.
        let parent: Parent =
            match parent_for_write(self.reader.roots(), &path, self.bounds.link_hops) {
                Ok(parent) if parent.trust == PathTrust::Trusted => parent,
                Ok(_) | Err(_) => return Ok(applied(Outcome::WriteFailed)),
            };
        let Ok(before) = identity_of(&parent) else {
            return Ok(applied(Outcome::ChangedOutside));
        };
        let current = match self.reader.read_file(&path) {
            Ok(file) if file.trust() == PathTrust::Trusted => file,
            Ok(_) => return Ok(applied(Outcome::WriteFailed)),
            Err(SourceFailure::Missing) => return Ok(applied(Outcome::ChangedOutside)),
            Err(_) => return Ok(applied(Outcome::WriteFailed)),
        };
        let after = identity_of(&parent).ok();
        if after != Some(before) || hash(current.bytes()) != drafted.h0 {
            return Ok(applied(Outcome::ChangedOutside));
        }
        if let Ok(listing) = self
            .reader
            .list_directory(parent.path.parent().unwrap_or(&parent.path))
        {
            let names: Vec<_> = listing.entries.iter().map(|e| e.name.clone()).collect();
            if conflict_copy_beside(&names, &parent.name) {
                return Ok(applied(Outcome::ConflictCopyPresent));
            }
        }

        // What an operation needs kept is decided before the intent, so the intent can
        // name it: a start after a crash finds the record by this identifier.
        let record_id = match &drafted.prepared {
            Prepared::Park { .. } | Prepared::Inverse { .. } => Some(new_id()),
            Prepared::Restore { record, .. } => Some(record.clone()),
            Prepared::Nothing => None,
        };
        let intent = self.journal_entry(
            &change,
            Phase::Intent,
            &stored,
            record_id.clone(),
            Some(drafted.h1.clone()),
            None,
        );
        if let Err(error) = self.journal.append(&intent, Step::IntentJournaled) {
            return match error {
                JournalError::Crashed => Err(ChangeFailure::Internal),
                JournalError::NotRecorded => Ok(applied(Outcome::NotRecorded)),
            };
        }
        let finish = |outcome: Outcome, h1: Option<String>| -> Result<Applied, ChangeFailure> {
            let entry = self.journal_entry(
                &change,
                Phase::Outcome,
                &stored,
                record_id.clone(),
                h1,
                Some(outcome),
            );
            match self.journal.append(&entry, Step::OutcomeJournaled) {
                Err(JournalError::Crashed) => Err(ChangeFailure::Internal),
                _ => {
                    if outcome.changed_the_file() {
                        self.inventory.rescan();
                    }
                    Ok(applied(outcome))
                }
            }
        };

        // A record is made before the file changes; it is removed again if the file does not.
        let mut made_record = false;
        let to_keep = match &drafted.prepared {
            Prepared::Park {
                body,
                label,
                secret_bearing,
                protection,
            } => Some((
                body,
                label,
                *secret_bearing,
                *protection,
                super::restore::State::Parked,
            )),
            // The way back from an authored change is kept, and not offered as turned off.
            Prepared::Inverse {
                body,
                label,
                secret_bearing,
                protection,
            } => Some((
                body,
                label,
                *secret_bearing,
                *protection,
                super::restore::State::Used,
            )),
            _ => None,
        };
        if let (Some((body, label, secret_bearing, protection, state)), Some(id)) =
            (to_keep, &record_id)
        {
            match self
                .store
                .keep_as(id, label, secret_bearing, protection, body, state)
            {
                Ok(_) => made_record = true,
                Err(RestoreError::Crashed) => return Err(ChangeFailure::Internal),
                Err(error) => {
                    let outcome = match error {
                        RestoreError::NoKeyStoreForSecret
                        | RestoreError::Vault(VaultError::KeyStoreUnavailable) => {
                            Outcome::KeyStoreUnavailable
                        }
                        _ => Outcome::WriteFailed,
                    };
                    return finish(outcome, None);
                }
            }
        }
        let undo_record = || {
            if let (true, Some(id)) = (made_record, &record_id) {
                let _ = self.store.delete(id);
            }
        };

        // A restoration whose entry is already in the file writes nothing.
        let writes_the_file = drafted.new_bytes != current.bytes();
        if writes_the_file {
            match replace(&parent, &drafted.new_bytes, before, &self.faults) {
                // In place either way; the read-back below says what is there.
                Ok(()) | Err(WriteError::AfterRename) => (),
                Err(WriteError::Crashed) => return Err(ChangeFailure::Internal),
                Err(WriteError::Replaced) => {
                    undo_record();
                    return finish(Outcome::ChangedOutside, None);
                }
                Err(WriteError::Failed | WriteError::NotOwner) => {
                    undo_record();
                    return finish(Outcome::WriteFailed, None);
                }
            }
        }

        // Read back: the file holds what was planned, or the user is told it does not.
        let read_back = match self.faults.check(Step::ReadBack) {
            Err(Injected::Crashed) => return Err(ChangeFailure::Internal),
            Err(Injected::Failed) => Err(SourceFailure::Unreadable),
            Ok(()) => self.reader.read_file(&path),
        };
        let (verified, found) = match &read_back {
            Ok(file) => {
                let actual = hash(file.bytes());
                let same = actual == drafted.h1
                    && match &drafted.logical {
                        super::plan::Logical::Nothing => true,
                        super::plan::Logical::Toml(edit) => {
                            super::codex_plan::is_exactly(current.bytes(), file.bytes(), edit)
                        }
                        super::plan::Logical::CodexAuthor(edit) => {
                            match (
                                std::str::from_utf8(current.bytes()),
                                std::str::from_utf8(file.bytes()),
                            ) {
                                (Ok(before), Ok(after)) => {
                                    codex_author::is_exactly(before, after, edit)
                                }
                                _ => false,
                            }
                        }
                        super::plan::Logical::Opencode(edit) => {
                            opencode_plan::is_exactly(current.bytes(), file.bytes(), edit)
                        }
                        other => is_exactly(current.bytes(), file.bytes(), other),
                    };
                (same, Some(actual))
            }
            Err(_) => (false, None),
        };
        if !verified {
            return finish(Outcome::VerifyFailed, found);
        }
        if let Prepared::Restore { record, .. } = &drafted.prepared
            && let Err(RestoreError::Crashed) = self.store.mark(record, super::restore::State::Used)
        {
            return Err(ChangeFailure::Internal);
        }
        finish(Outcome::Applied, found)
    }

    /// A page of changes, newest first.
    pub fn history(&self, cursor: Option<&str>, limit: usize) -> (Vec<Change>, Option<String>) {
        self.journal.history(cursor, limit)
    }

    /// The change that undoes `change`, as a plan: exact when the file is as that change
    /// left it, otherwise planned against the file as it is.
    pub fn undo(&self, change_id: &str) -> Result<PlanView, ChangeFailure> {
        let change = self
            .journal
            .change(change_id)
            .ok_or(ChangeFailure::NotUndoable)?;
        // Anything that changed the file can be undone, verified or not: the undo is a plan
        // made against the file as it is, and refused if it does not fit.
        if !change.outcome.is_some_and(Outcome::changed_the_file) || change.undoes.is_some() {
            return Err(ChangeFailure::NotUndoable);
        }
        // Already undone: a later change names this one.
        if self.journal.changes().iter().any(|later| {
            later.undoes.as_deref() == Some(change_id)
                && later.outcome.is_some_and(Outcome::changed_the_file)
        }) {
            return Err(ChangeFailure::NotUndoable);
        }
        let undoes = Some(change.id.clone());
        match change.operation {
            Operation::ParkHook => {
                let record = change.record.ok_or(ChangeFailure::NotUndoable)?;
                self.plan_with(Operation::RestoreHook, &record, undoes, None)
            }
            Operation::ParkPlugin | Operation::ParkMcp | Operation::CodexParkMcp => {
                let record = change.record.ok_or(ChangeFailure::NotUndoable)?;
                self.plan_with(Operation::RestoreHook, &record, undoes, None)
            }
            // What the user wrote goes back to exactly what it replaced, while the file is as
            // the change left it; after that, it is theirs to turn off.
            Operation::AddHook
            | Operation::EditHook
            | Operation::AddMcp
            | Operation::EditMcp
            | Operation::CodexAddMcp
            | Operation::CodexEditMcp => {
                let record = change.record.ok_or(ChangeFailure::NotUndoable)?;
                self.plan_with(Operation::RevertAuthored, &record, undoes, None)
            }
            Operation::RevertAuthored => Err(ChangeFailure::NotUndoable),
            Operation::OpencodeMcpOff | Operation::OpencodeMcpOn => {
                let prior = change.prior.ok_or(ChangeFailure::NotUndoable)?;
                let inverse = if change.operation == Operation::OpencodeMcpOff {
                    Operation::OpencodeMcpOn
                } else {
                    Operation::OpencodeMcpOff
                };
                self.plan_with(inverse, &change.item, undoes, Some(prior))
            }
            Operation::RestoreHook => {
                let record = change.record.ok_or(ChangeFailure::NotUndoable)?;
                let body = self.store.read(&record)?;
                if body.provider == ProviderId::Codex && body.event == "mcp" {
                    let name = body.matcher.clone().ok_or(ChangeFailure::NotUndoable)?;
                    let item = super::inventory::item_id_for(body.source, &[&name]);
                    return self.plan_with(Operation::CodexParkMcp, &item, undoes, None);
                }
                if body.provider == ProviderId::Opencode && body.event == "mcp" {
                    let name = body.matcher.clone().ok_or(ChangeFailure::NotUndoable)?;
                    let item = super::inventory::item_id_for(body.source, &[&name]);
                    return self.plan_with(Operation::ParkMcp, &item, undoes, None);
                }
                if body.provider == ProviderId::Opencode {
                    let spec = serde_json::from_str::<serde_json::Value>(&body.unit_text)
                        .ok()
                        .and_then(|v| match v {
                            serde_json::Value::String(s) => Some(s),
                            serde_json::Value::Array(a) => {
                                a.first().and_then(|s| s.as_str()).map(str::to_owned)
                            }
                            _ => None,
                        })
                        .ok_or(ChangeFailure::NotUndoable)?;
                    let item = super::inventory::item_id_for(body.source, &["plugin", &spec]);
                    return self.plan_with(Operation::ParkPlugin, &item, undoes, None);
                }
                let item = super::inventory::item_id_for(
                    body.source,
                    &[
                        &body.event,
                        &body.group.to_string(),
                        &body.index.to_string(),
                    ],
                );
                self.plan_with(Operation::ParkHook, &item, undoes, None)
            }
            Operation::DisableAllHooks => self.plan_with(
                Operation::EnableAllHooks,
                &source_word(change.source),
                undoes,
                None,
            ),
            Operation::EnableAllHooks => self.plan_with(
                Operation::DisableAllHooks,
                &source_word(change.source),
                undoes,
                None,
            ),
            Operation::SkillOff => self.plan_with(Operation::SkillOn, &change.item, undoes, None),
            Operation::SkillOn => self.plan_with(Operation::SkillOff, &change.item, undoes, None),
            // A Codex switch is put back to what its key held: exact, and needing nothing
            // but the closed word the journal kept.
            Operation::CodexSkillOff
            | Operation::CodexSkillOn
            | Operation::CodexMcpOff
            | Operation::CodexMcpOn => {
                let prior = change.prior.ok_or(ChangeFailure::NotUndoable)?;
                let inverse = match change.operation {
                    Operation::CodexSkillOff => Operation::CodexSkillOn,
                    Operation::CodexSkillOn => Operation::CodexSkillOff,
                    Operation::CodexMcpOff => Operation::CodexMcpOn,
                    _ => Operation::CodexMcpOff,
                };
                self.plan_with(inverse, &change.item, undoes, Some(prior))
            }
        }
    }

    pub fn records(&self) -> Vec<Summary> {
        self.store.list()
    }

    pub fn delete_record(&self, id: &str) -> Result<(), ChangeFailure> {
        Ok(self.store.delete(id)?)
    }

    pub fn export_record(&self, id: &str, passphrase: &str) -> Result<Vec<u8>, ChangeFailure> {
        Ok(self.store.export(id, passphrase, EXPORT_ROUNDS)?)
    }

    pub fn import_record(&self, bytes: &[u8], passphrase: &str) -> Result<String, ChangeFailure> {
        // A wrong passphrase and a file that is not a record look the same, on purpose: both
        // are "this file did not open", never "the kept entry is damaged".
        self.store
            .import(bytes, passphrase)
            .map_err(|error| match error {
                RestoreError::Vault(VaultError::Tampered | VaultError::Malformed) => {
                    ChangeFailure::ImportInvalid
                }
                other => other.into(),
            })
    }

    /// At start: finish what a crash left (14 §4). An apply with an intent and no outcome is
    /// found, the file's hash says how far it got, and the outcome is written.
    pub fn recover(&self) -> usize {
        let unfinished = self.journal.unfinished();
        let sources = [
            SourceKey::ClaudeUserSettings,
            SourceKey::ClaudeProjectSettings,
            SourceKey::ClaudeProjectLocalSettings,
            SourceKey::CodexUserConfig,
            SourceKey::OpencodeUserConfig,
        ];
        // Temporary files of a write that was cut short, judged one by one. Nothing is
        // deleted that cannot be accounted for: a file left at a temporary name may be the
        // provider's newer version, displaced by the swap in the instant before the crash.
        let mut put_back_for: std::collections::HashSet<String> = std::collections::HashSet::new();
        for key in sources {
            let Some(parent) = self.path_of(key).and_then(|path| {
                parent_for_write(self.reader.roots(), &path, self.bounds.link_hops).ok()
            }) else {
                continue;
            };
            let names = leftovers(&parent);
            if names.is_empty() {
                continue;
            }
            let target = self
                .path_of(key)
                .and_then(|path| self.reader.read_file(&path).ok())
                .map(|file| hash(file.bytes()));
            let intents: Vec<&Entry> = unfinished.iter().filter(|i| i.source == key).collect();
            let swapped = intents
                .iter()
                .find(|i| target.is_some() && target.as_deref() == i.h1.as_deref());
            let not_swapped = intents
                .iter()
                .any(|i| target.as_deref() == Some(i.h0.as_str()));
            for name in names {
                let found = read_leftover(&parent, &name, self.bounds.file_bytes as usize)
                    .map(|bytes| hash(&bytes));
                match (found.as_deref(), swapped) {
                    // Our old file, displaced by our own swap: spare.
                    (Some(found), Some(intent)) if found == intent.h0 => {
                        remove_leftover(&parent, &name);
                    }
                    // Somebody else's newer file, displaced by our swap: it goes back.
                    (Some(_), Some(intent)) => {
                        if put_back(&parent, &name) {
                            put_back_for.insert(intent.id.clone());
                        } else {
                            keep_leftover(&parent, &name, "unrestored");
                        }
                    }
                    // The swap never happened: this is our own partial or whole temporary.
                    _ if not_swapped => {
                        remove_leftover(&parent, &name);
                    }
                    (Some(found), None) => {
                        keep_leftover(&parent, &name, &found[..12.min(found.len())]);
                    }
                    (None, _) => {
                        keep_leftover(&parent, &name, "unreadable");
                    }
                }
            }
        }

        let mut repaired = 0;
        for intent in unfinished {
            let Some(path) = self.path_of(intent.source) else {
                continue;
            };
            let current = self.reader.read_file(&path).ok().map(|f| hash(f.bytes()));
            let outcome = if put_back_for.contains(&intent.id) {
                Outcome::RecoveredProviderKept
            } else {
                match current.as_deref() {
                    Some(found) if Some(found) == intent.h1.as_deref() => Outcome::RecoveredApplied,
                    Some(found) if found == intent.h0 => Outcome::RecoveredNotApplied,
                    _ => Outcome::RecoveredUnverified,
                }
            };
            match (outcome, intent.operation, &intent.record) {
                // The file was not changed, so the entry is still in it: the record is spare.
                (
                    Outcome::RecoveredNotApplied | Outcome::RecoveredProviderKept,
                    Operation::ParkHook
                    | Operation::ParkPlugin
                    | Operation::ParkMcp
                    | Operation::CodexParkMcp
                    | Operation::CodexAddMcp
                    | Operation::CodexEditMcp
                    | Operation::AddHook
                    | Operation::EditHook
                    | Operation::AddMcp
                    | Operation::EditMcp,
                    Some(record),
                ) => {
                    let _ = self.store.delete(record);
                }
                (Outcome::RecoveredApplied, Operation::RestoreHook, Some(record)) => {
                    let _ = self.store.mark(record, super::restore::State::Used);
                }
                _ => (),
            }
            let entry = Entry {
                phase: Phase::Outcome,
                at: now(),
                h1: current,
                outcome: Some(outcome),
                ..intent
            };
            if self.journal.append(&entry, Step::OutcomeJournaled).is_ok() {
                repaired += 1;
            }
        }
        let _ = self.journal.prune(now());
        let _ = self.store.prune();
        repaired
    }
}

fn codex_failure(error: CodexPlanError) -> ChangeFailure {
    match error {
        CodexPlanError::Malformed => ChangeFailure::SourceMalformed,
        CodexPlanError::Unsupported => ChangeFailure::NotSupported,
        CodexPlanError::EntryGone => ChangeFailure::EntryGone,
        CodexPlanError::EntryChanged => ChangeFailure::EntryChanged,
        CodexPlanError::AlreadyDone => ChangeFailure::AlreadyDone,
    }
}

fn source_word(key: SourceKey) -> String {
    serde_json::to_value(key)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[allow(unused)]
fn _unused(_: Class) {}

/// What the user wrote for a change, as typed data. It comes from the desktop's own window.
#[derive(Clone, PartialEq, Eq)]
pub enum Authoring {
    AddHook(HookSpec),
    /// `expected` is the command the user was shown when they started: if the file holds
    /// another by the time the plan is made, nothing is replaced.
    EditHook {
        item: String,
        command: String,
        expected: String,
    },
    AddMcp {
        name: String,
        spec: McpSpec,
    },
    AddCodexMcp {
        name: String,
        spec: McpSpec,
    },
    EditCodexMcp {
        item: String,
        spec: McpSpec,
        expected: McpSpec,
    },
    EditMcp {
        item: String,
        spec: McpSpec,
        expected: McpSpec,
    },
}

/// A command or address as a confirmation shows it: printable, on one line, cut.
fn plain_detail(label: &str, text: &str) -> String {
    const MOST: usize = 400;
    let mut shown: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .filter(|c| !author::disguises(*c))
        .take(MOST)
        .collect();
    if text.chars().count() > MOST {
        shown.push('…');
    }
    format!("{label}: {shown}")
}

fn plain_mcp(spec: &McpSpec) -> String {
    match spec {
        McpSpec::Local { command } => plain_detail("starts", &command.join(" ")),
        McpSpec::Remote { url } => plain_detail("connects to", url),
    }
}

/// What the user wrote is a command or an address that may hold a token: only the kind is printed.
impl std::fmt::Debug for Authoring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::AddHook(_) => "Authoring::AddHook(..)",
            Self::EditHook { .. } => "Authoring::EditHook(..)",
            Self::AddMcp { .. } => "Authoring::AddMcp(..)",
            Self::EditMcp { .. } => "Authoring::EditMcp(..)",
            Self::AddCodexMcp { .. } => "Authoring::AddCodexMcp(..)",
            Self::EditCodexMcp { .. } => "Authoring::EditCodexMcp(..)",
        })
    }
}
