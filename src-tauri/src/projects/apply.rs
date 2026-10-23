//! What each command does to the store (contract §3), given what the owners say.
//!
//! Every command is checked against the context the window captured, applied to a copy under
//! the store lock, validated as a whole store and published by compare-and-swap. A launch is
//! different only in that the owner is called outside the lock: the journal records `pending`
//! first, the owner is asked, and the receipt is completed afterwards. A launch whose outcome
//! is unknown is never repeated; it waits, `ambiguous`, for the user to reconcile it.
use super::{
    command::{
        Command, Context, Created, Effect, Envelope, ErrorCode, Failure, Launch, NextAction,
        Receipt, Resolution, SaveWorkspace, SelectionToken,
    },
    owner::{
        Agent, FolderError, HostView, LaunchError, ManagedLaunch, ManagedLaunched, NativeLaunch,
        NativeLaunched, Owner, OwnerError, RootError, RootInfo, ShellLaunched,
    },
    store::{Loaded, Store, StoreError},
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Mutex, PoisonError},
};
use wyss_console::contracts::{
    UtcTimestamp,
    m1::{
        M1ErrorCode,
        records::{
            Association, AssociationState, Binding, BindingState, LaunchProvider, Leaf,
            ManagedSafetyProfile, Mode, Node, Operation, OperationKind, OperationState,
            Preferences, ProfileDefaults, ProfileRef, Project, StoreV1, Target, Workspace,
        },
        scalar::{
            AbsolutePath, AssociationId, BindingId, Counter, EpochText, LeafId, ManagedSessionId,
            NativeId, OperationId, ProjectId, StoreStreamId, TerminalId, Text, WorkspaceId,
        },
    },
};

/// Time, identities and this process's claim, all injectable so tests are deterministic.
pub trait Env: Send + Sync {
    /// A new random lowercase UUID.
    fn uuid(&self) -> String;
    /// Now, as UTC RFC 3339 text ending in `Z`.
    fn now(&self) -> String;
    /// The claim this process puts on a `pending` receipt.
    fn claim(&self) -> String;
    /// Whether the process that wrote `claim` is still the same running process.
    fn claim_is_live(&self, claim: &str) -> bool;
}

pub struct SystemEnv {
    claim: String,
}

impl SystemEnv {
    pub fn new() -> Self {
        let pid = std::process::id();
        let ticks = super::owner::start_ticks(pid as i32).unwrap_or(0);
        Self {
            claim: format!("c1:{pid}:{ticks}:{}", uuid::Uuid::new_v4().simple()),
        }
    }
}

impl Default for SystemEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl Env for SystemEnv {
    fn uuid(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }

    fn now(&self) -> String {
        use time::format_description::well_known::Rfc3339;
        time::OffsetDateTime::now_utc()
            .replace_nanosecond(0)
            .ok()
            .and_then(|t| t.format(&Rfc3339).ok())
            .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_owned())
    }

    fn claim(&self) -> String {
        self.claim.clone()
    }

    fn claim_is_live(&self, claim: &str) -> bool {
        if claim == self.claim {
            return true;
        }
        let mut parts = claim.split(':');
        let (Some("c1"), Some(pid), Some(ticks)) = (parts.next(), parts.next(), parts.next())
        else {
            return false;
        };
        match (pid.parse::<i32>(), ticks.parse::<u64>()) {
            (Ok(pid), Ok(ticks)) => super::owner::start_ticks(pid) == Some(ticks),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Register,
    Parent,
    Relink,
}

struct Picked {
    path: String,
    purpose: Purpose,
    selection_revision: u64,
}

/// Folders the user picked in a native dialog this backend opened. A token is used once, only
/// for the purpose it was issued for, and only in the selection revision it was issued in: a
/// switch of project in the window leaves a stale pick unusable.
#[derive(Default)]
pub struct Selections {
    picked: Mutex<VecDeque<(String, Picked)>>,
}

const MOST_PICKS: usize = 16;

impl Selections {
    pub fn issue(&self, token: String, path: String, purpose: Purpose, selection_revision: u64) {
        let mut picked = self.picked.lock().unwrap_or_else(PoisonError::into_inner);
        if picked.len() == MOST_PICKS {
            picked.pop_front();
        }
        picked.push_back((
            token,
            Picked {
                path,
                purpose,
                selection_revision,
            },
        ));
    }

    /// The path for `token`, removing it. A token that is unknown, spent, of another purpose or
    /// of another selection revision is gone either way.
    pub fn take(&self, token: &SelectionToken, purpose: Purpose, revision: u64) -> Option<String> {
        let mut picked = self.picked.lock().unwrap_or_else(PoisonError::into_inner);
        let at = picked.iter().position(|(t, _)| t == &token.0)?;
        let (_, found) = picked.remove(at)?;
        (found.purpose == purpose && found.selection_revision == revision).then_some(found.path)
    }

    pub fn clear(&self) {
        self.picked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}

pub struct Engine<'a> {
    pub store: &'a Store,
    pub owner: &'a dyn Owner,
    pub env: &'a dyn Env,
    pub selections: &'a Selections,
}

type R<T = ()> = Result<T, Failure>;

fn fail<T>(code: ErrorCode) -> R<T> {
    Err(Failure::new(code))
}

fn bad<T>(field: &'static str) -> R<T> {
    Err(Failure::new(ErrorCode::InvalidRecord).with_field(field))
}

pub fn store_failure(error: StoreError) -> Failure {
    match error {
        StoreError::RevisionConflict { .. } => Failure::new(ErrorCode::RevisionConflict),
        StoreError::StoreState(_) => Failure::new(ErrorCode::InvalidRecord).with_field("store"),
        StoreError::Unusable(unusable) => Failure::new(match unusable.code {
            M1ErrorCode::UnsupportedSchema => ErrorCode::UnsupportedSchema,
            M1ErrorCode::CapacityExceeded => ErrorCode::CapacityExceeded,
            M1ErrorCode::InvalidRecord => ErrorCode::InvalidRecord,
        })
        .with_field(unusable.field),
        StoreError::Invalid(error) => Failure::new(match error.code {
            M1ErrorCode::CapacityExceeded => ErrorCode::CapacityExceeded,
            _ => ErrorCode::InvalidRecord,
        })
        .with_field(error.field),
        StoreError::Io(_) => Failure::new(ErrorCode::IoFailure),
        StoreError::PublishUnknown(_) => {
            Failure::new(ErrorCode::IoFailure).with_effect(Effect::Unknown)
        }
    }
}

fn owner_failure(error: OwnerError) -> Failure {
    Failure::new(match error {
        OwnerError::HostOffline => ErrorCode::HostOffline,
        OwnerError::HostChanged => ErrorCode::HostChanged,
        OwnerError::Unsupported => ErrorCode::UnsupportedCapability,
        OwnerError::ProfileUnavailable => ErrorCode::ProfileUnavailable,
        OwnerError::Refused => ErrorCode::OwnerRefused,
    })
}

fn root_failure(error: RootError) -> Failure {
    Failure::new(match error {
        RootError::Missing => ErrorCode::RootMissing,
        RootError::Ineligible => ErrorCode::RootIneligible,
    })
}

/// A value the contract's own types have already accepted; failing here is a bug, so it is
/// reported as an invalid record instead of a panic.
fn valid<T, E>(value: Result<T, E>, field: &'static str) -> R<T> {
    value.or_else(|_| bad(field))
}

fn label_for(path: &str) -> Text {
    let base = path
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or("Project");
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .take(64)
        .collect();
    Text::parse(&cleaned).unwrap_or_else(|_| Text::parse("Project").expect("static label"))
}

fn counter(value: u64) -> Counter {
    Counter::new(value).expect("a positive literal")
}

fn bump(revision: Counter, field: &'static str) -> R<Counter> {
    revision.next().map_or_else(|| bad(field), Ok)
}

fn native_preferences() -> Preferences {
    Preferences {
        provider: None,
        mode: Mode::Native,
        native_profile: None,
        managed_safety_profile: None,
    }
}

impl Engine<'_> {
    fn load(&self) -> R<StoreV1> {
        match self
            .store
            .read()
            .map_err(|_| Failure::new(ErrorCode::IoFailure))?
        {
            Loaded::Ready(store) => Ok(*store),
            Loaded::Missing => bad("store"),
            Loaded::Unusable(unusable) => Err(store_failure(StoreError::Unusable(unusable))),
        }
    }

    /// Validates the captured context against the store: the host, and every parent relationship
    /// the window named. Nothing here reads current UI selection; it only compares what the
    /// window knew with what the store holds.
    fn check_context(store: &StoreV1, context: &Context) -> R {
        let stale = || fail(ErrorCode::ContextStale);
        if context.host_id != store.host_id {
            return stale();
        }
        let project = match &context.project_id {
            Some(id) => match store.projects.iter().find(|p| &p.id == id) {
                Some(project) => Some(project),
                None => return stale(),
            },
            None => None,
        };
        if let Some(workspace) = &context.workspace_id {
            let Some(found) = store.workspaces.iter().find(|w| &w.id == workspace) else {
                return stale();
            };
            if found.project_id != context.project_id {
                return stale();
            }
        }
        match (&context.binding_id, &context.binding_revision) {
            (None, None) => {}
            (Some(id), Some(revision)) => {
                let found = store.bindings.iter().find(|b| &b.id == id);
                if !found.is_some_and(|b| {
                    b.revision == *revision && Some(&b.project_id) == context.project_id.as_ref()
                }) {
                    return stale();
                }
            }
            _ => return stale(),
        }
        let _ = project;
        Ok(())
    }

    /// A window inside a project acts in that project. A command that names a project, or a
    /// workspace of another one, is for something else than the window was showing: it is stale
    /// and does nothing. A window outside any project (global administration) may name any.
    fn check_scope(store: &StoreV1, context: &Context, command: &Command) -> R {
        let Some(inside) = &context.project_id else {
            return Ok(());
        };
        let of_workspace = |id: &WorkspaceId| {
            store
                .workspaces
                .iter()
                .find(|w| &w.id == id)
                .map(|w| w.project_id.clone())
        };
        let named: Option<Option<ProjectId>> = match command {
            Command::RenameProject { project_id, .. }
            | Command::ArchiveProject { project_id, .. }
            | Command::RemoveProject { project_id, .. }
            | Command::CreateWorkspace { project_id, .. }
            | Command::Relink { project_id, .. } => Some(Some(project_id.clone())),
            Command::DuplicateWorkspace {
                source_workspace_id: id,
                ..
            }
            | Command::CloseWorkspace { workspace_id: id }
            | Command::DeleteWorkspace { workspace_id: id }
            | Command::Attach {
                workspace_id: id, ..
            }
            | Command::Detach {
                workspace_id: id, ..
            } => of_workspace(id),
            Command::SaveWorkspace(save) => of_workspace(&save.workspace_id),
            Command::Launch(launch) => of_workspace(&launch.workspace_id),
            Command::AdoptScratch { project_id, .. } => Some(Some(project_id.clone())),
            // A launch is settled from the project it was made in.
            Command::ReconcileLaunch {
                launch_operation_id,
                ..
            } => store
                .operations
                .iter()
                .find(|o| &o.id == launch_operation_id)
                .map(|o| o.project_id.clone()),
            Command::RegisterExisting { .. } | Command::CreateFolder { .. } => None,
        };
        match named {
            Some(project) if project.as_ref() != Some(inside) => fail(ErrorCode::ContextStale),
            _ => Ok(()),
        }
    }

    /// Runs one command. The caller's expected revision must be the store's current one.
    pub fn execute(&self, envelope: Envelope) -> Result<Receipt, Failure> {
        let tagged = |mut failure: Failure| {
            failure.operation_id = Some(envelope.operation_id.clone());
            failure.context = Some(Box::new(envelope.context.clone()));
            failure
        };
        self.run(&envelope).map_err(tagged)
    }

    fn run(&self, envelope: &Envelope) -> R<Receipt> {
        let current = self.load()?;
        if current.revision != envelope.expected_store_revision {
            return fail(ErrorCode::RevisionConflict);
        }
        Self::check_context(&current, &envelope.context)?;
        Self::check_scope(&current, &envelope.context, &envelope.command)?;
        let revision = envelope.context.selection_revision.get();
        match &envelope.command {
            Command::Launch(launch) => self.launch(&current, envelope, launch),
            Command::ReconcileLaunch {
                launch_operation_id,
                resolution,
            } => self.reconcile(&current, envelope, launch_operation_id, resolution),
            Command::CloseWorkspace { workspace_id } => {
                // Closing releases views and input only; the store has nothing to change.
                if !current.workspaces.iter().any(|w| &w.id == workspace_id) {
                    return fail(ErrorCode::ContextStale);
                }
                Ok(self.receipt(envelope, current.revision, Vec::new()))
            }
            Command::RegisterExisting { selection_token } => {
                let path = self.pick(selection_token, Purpose::Register, revision)?;
                let root = self.owner.root(&path).map_err(root_failure)?;
                self.commit(envelope, move |store, env| register(store, &root, env))
            }
            Command::CreateFolder {
                selection_token,
                name,
            } => {
                let parent = self.pick(selection_token, Purpose::Parent, revision)?;
                let made = self
                    .owner
                    .create_folder(&parent, name)
                    .map_err(|e| match e {
                        FolderError::Name => {
                            Failure::new(ErrorCode::InvalidRecord).with_field("name")
                        }
                        FolderError::Exists => {
                            Failure::new(ErrorCode::OwnerRefused).with_field("name")
                        }
                        FolderError::Io => Failure::new(ErrorCode::IoFailure),
                    })?;
                let root = self.owner.root(&made).map_err(root_failure)?;
                // The folder exists now. If the store cannot take it, it stays, empty and
                // visible, and the user can register it; it is never deleted from here.
                self.commit(envelope, move |store, env| register(store, &root, env))
                    .map_err(|failure| Failure {
                        effect: Effect::Unknown,
                        next_action: NextAction::ChooseAgain,
                        field: Some("createdFolder"),
                        ..failure
                    })
            }
            Command::Relink {
                project_id,
                old_binding_id,
                selection_token,
            } => {
                let path = self.pick(selection_token, Purpose::Relink, revision)?;
                let root = self.owner.root(&path).map_err(root_failure)?;
                let (project_id, old) = (project_id.clone(), old_binding_id.clone());
                self.commit(envelope, move |store, env| {
                    relink(store, &project_id, &old, &root, env)
                })
            }
            Command::RenameProject { project_id, label } => {
                let (project_id, label) = (project_id.clone(), label.clone());
                self.commit(envelope, move |store, env| {
                    let project = live_project(store, &project_id)?;
                    project.label = label;
                    touch(project, env)
                })
            }
            Command::ArchiveProject {
                project_id,
                archived,
            } => {
                let (project_id, archived) = (project_id.clone(), *archived);
                self.commit(envelope, move |store, env| {
                    let project = live_project(store, &project_id)?;
                    project.archived = archived;
                    touch(project, env)
                })
            }
            Command::RemoveProject {
                project_id,
                retain_history,
            } => self.remove_project(&current, envelope, project_id, *retain_history),
            Command::CreateWorkspace { project_id, label } => {
                let (project_id, label) = (project_id.clone(), label.clone());
                self.commit(envelope, move |store, env| {
                    live_project(store, &project_id)?;
                    let id = valid(WorkspaceId::parse(&env.uuid()), "workspaces")?;
                    store.workspaces.push(new_workspace(
                        id.clone(),
                        Some(project_id),
                        store.host_id.clone(),
                        label,
                    ));
                    Ok(vec![created("workspace", id.as_str())])
                })
            }
            Command::DuplicateWorkspace {
                source_workspace_id,
                label,
            } => {
                let (source, label) = (source_workspace_id.clone(), label.clone());
                self.commit(envelope, move |store, env| {
                    duplicate(store, &source, label, env)
                })
            }
            Command::SaveWorkspace(save) => {
                let save = save.clone();
                self.commit(envelope, move |store, _| save_workspace(store, &save))
            }
            Command::DeleteWorkspace { workspace_id } => {
                let workspace_id = workspace_id.clone();
                self.commit(envelope, move |store, env| {
                    delete_workspace(store, &workspace_id, env)
                })
            }
            Command::Attach {
                workspace_id,
                leaf_id,
                association_id,
            } => self.attach(&current, envelope, workspace_id, leaf_id, association_id),
            Command::Detach {
                workspace_id,
                leaf_id,
                association_id,
            } => {
                let (workspace, leaf, association) = (
                    workspace_id.clone(),
                    leaf_id.clone(),
                    association_id.clone(),
                );
                self.commit(envelope, move |store, _| {
                    detach(store, &workspace, &leaf, &association)
                })
            }
            Command::AdoptScratch {
                association_id,
                project_id,
                binding_id,
            } => {
                let (association, project, binding) = (
                    association_id.clone(),
                    project_id.clone(),
                    binding_id.clone(),
                );
                self.commit(envelope, move |store, _| {
                    adopt(store, &association, &project, &binding)
                })
            }
        }
    }

    fn pick(&self, token: &SelectionToken, purpose: Purpose, revision: u64) -> R<String> {
        self.selections
            .take(token, purpose, revision)
            .map_or_else(|| fail(ErrorCode::ContextStale), Ok)
    }

    fn receipt(&self, envelope: &Envelope, revision: Counter, created: Vec<Created>) -> Receipt {
        Receipt {
            operation_id: envelope.operation_id.clone(),
            context: envelope.context.clone(),
            store_revision: revision,
            created,
            replayed: false,
        }
    }

    /// Applies `change` to the store at the expected revision and publishes it.
    fn commit(
        &self,
        envelope: &Envelope,
        change: impl FnOnce(&mut StoreV1, &dyn Env) -> R<Vec<Created>>,
    ) -> R<Receipt> {
        let mut created = Vec::new();
        let committed = self
            .store
            .transact(envelope.expected_store_revision, |store| {
                created = change(store, self.env)?;
                Ok::<(), Failure>(())
            })
            .map_err(store_failure)??;
        Ok(self.receipt(envelope, committed.revision, created))
    }
}

fn created(role: &'static str, id: &str) -> Created {
    Created {
        role,
        id: id.to_owned(),
    }
}

fn live_project<'a>(store: &'a mut StoreV1, id: &ProjectId) -> R<&'a mut Project> {
    match store.projects.iter_mut().find(|p| &p.id == id) {
        Some(project) if !project.tombstone => Ok(project),
        _ => fail(ErrorCode::ContextStale),
    }
}

fn touch(project: &mut Project, env: &dyn Env) -> R<Vec<Created>> {
    project.revision = bump(project.revision, "projects.revision")?;
    project.updated_at = valid(UtcTimestamp::try_from(env.now()), "projects.updatedAt")?;
    Ok(Vec::new())
}

fn new_workspace(
    id: WorkspaceId,
    project: Option<ProjectId>,
    host: wyss_console::contracts::m1::scalar::HostId,
    label: Text,
) -> Workspace {
    Workspace {
        id,
        project_id: project,
        host_id: host,
        revision: counter(1),
        label,
        layout: None,
        focus: None,
        pinned: false,
        leaves: Vec::new(),
        preferences: native_preferences(),
    }
}

/// Registers a folder as a project, or returns the project that already has it.
fn register(store: &mut StoreV1, root: &RootInfo, env: &dyn Env) -> R<Vec<Created>> {
    // A root already bound to a live project on this host is that project, never a second one.
    if let Some(existing) = store.bindings.iter().find(|b| {
        b.path.as_str() == root.path
            && b.state != BindingState::Retired
            && store
                .projects
                .iter()
                .any(|p| p.id == b.project_id && !p.tombstone)
    }) {
        return Ok(vec![created("project", existing.project_id.as_str())]);
    }
    let now = valid(UtcTimestamp::try_from(env.now()), "projects.createdAt")?;
    let project_id = valid(ProjectId::parse(&env.uuid()), "projects")?;
    let binding_id = valid(BindingId::parse(&env.uuid()), "bindings")?;
    let workspace_id = valid(WorkspaceId::parse(&env.uuid()), "workspaces")?;
    store.bindings.push(Binding {
        id: binding_id.clone(),
        project_id: project_id.clone(),
        host_id: store.host_id.clone(),
        revision: counter(1),
        path: valid(AbsolutePath::parse(&root.path), "bindings.path")?,
        observation: wyss_console::contracts::m1::records::Observation {
            device: valid(Counter::new(root.device), "bindings.observation")?,
            inode: valid(Counter::new(root.inode), "bindings.observation")?,
        },
        checkout_kind: root.kind,
        state: BindingState::Valid,
        previous_binding_id: None,
    });
    store.workspaces.push(new_workspace(
        workspace_id.clone(),
        Some(project_id.clone()),
        store.host_id.clone(),
        valid(Text::parse("Main"), "workspaces.label")?,
    ));
    store.projects.push(Project {
        id: project_id.clone(),
        label: label_for(&root.path),
        created_at: now.clone(),
        updated_at: now,
        revision: counter(1),
        archived: false,
        tombstone: false,
        default_binding_id: Some(binding_id.clone()),
        default_workspace_id: Some(workspace_id.clone()),
        profile_defaults: ProfileDefaults {
            claude: None,
            codex: None,
        },
    });
    Ok(vec![
        created("project", project_id.as_str()),
        created("binding", binding_id.as_str()),
        created("workspace", workspace_id.as_str()),
    ])
}

/// A new binding for the project that replaces `old`, which is retired for future effects but
/// kept for the sessions it already has. Only the choice for future launches moves.
fn relink(
    store: &mut StoreV1,
    project_id: &ProjectId,
    old: &BindingId,
    root: &RootInfo,
    env: &dyn Env,
) -> R<Vec<Created>> {
    live_project(store, project_id)?;
    let host = store.host_id.clone();
    let Some(previous) = store
        .bindings
        .iter_mut()
        .find(|b| &b.id == old && &b.project_id == project_id && b.state != BindingState::Retired)
    else {
        return fail(ErrorCode::ContextStale);
    };
    previous.state = BindingState::Retired;
    previous.revision = bump(previous.revision, "bindings.revision")?;
    let id = valid(BindingId::parse(&env.uuid()), "bindings")?;
    store.bindings.push(Binding {
        id: id.clone(),
        project_id: project_id.clone(),
        host_id: host,
        revision: counter(1),
        path: valid(AbsolutePath::parse(&root.path), "bindings.path")?,
        observation: wyss_console::contracts::m1::records::Observation {
            device: valid(Counter::new(root.device), "bindings.observation")?,
            inode: valid(Counter::new(root.inode), "bindings.observation")?,
        },
        checkout_kind: root.kind,
        state: BindingState::Valid,
        previous_binding_id: Some(old.clone()),
    });
    let project = live_project(store, project_id)?;
    if project.default_binding_id.as_ref() == Some(old) {
        project.default_binding_id = Some(id.clone());
    }
    touch(project, env)?;
    Ok(vec![created("binding", id.as_str())])
}

fn duplicate(
    store: &mut StoreV1,
    source: &WorkspaceId,
    label: Text,
    env: &dyn Env,
) -> R<Vec<Created>> {
    let Some(found) = store.workspaces.iter().find(|w| &w.id == source) else {
        return fail(ErrorCode::ContextStale);
    };
    // Scratch is the one projectless workspace; it cannot have a twin.
    let Some(project) = found.project_id.clone() else {
        return bad("workspaces.scratch");
    };
    live_project(store, &project)?;
    let found = store
        .workspaces
        .iter()
        .find(|w| &w.id == source)
        .expect("checked above");
    let mut renamed: HashMap<String, LeafId> = HashMap::new();
    let mut leaves = Vec::with_capacity(found.leaves.len());
    for leaf in &found.leaves {
        let id = valid(LeafId::parse(&env.uuid()), "leaves")?;
        renamed.insert(leaf.id.as_str().to_owned(), id.clone());
        // A copy holds no session: it copies the shape and the preferences, nothing running.
        leaves.push(Leaf {
            id,
            title: leaf.title.clone(),
            association_id: None,
        });
    }
    fn remap(node: &Node, renamed: &HashMap<String, LeafId>) -> Option<Node> {
        Some(match node {
            Node::Leaf { id } => Node::Leaf {
                id: renamed.get(id.as_str())?.clone(),
            },
            Node::Split { dir, ratio, a, b } => Node::Split {
                dir: *dir,
                ratio: *ratio,
                a: Box::new(remap(a, renamed)?),
                b: Box::new(remap(b, renamed)?),
            },
        })
    }
    let layout = match &found.layout {
        Some(node) => Some(remap(node, &renamed).map_or_else(|| bad("layout"), Ok)?),
        None => None,
    };
    let id = valid(WorkspaceId::parse(&env.uuid()), "workspaces")?;
    let preferences = found.preferences.clone();
    let host = store.host_id.clone();
    let mut copy = new_workspace(id.clone(), Some(project), host, label);
    copy.layout = layout;
    copy.leaves = leaves;
    copy.preferences = preferences;
    store.workspaces.push(copy);
    Ok(vec![created("workspace", id.as_str())])
}

fn save_workspace(store: &mut StoreV1, save: &SaveWorkspace) -> R<Vec<Created>> {
    let unfinished: Vec<LeafId> = store
        .operations
        .iter()
        .filter(|o| o.workspace_id == save.workspace_id && o.state == OperationState::Pending)
        .map(|o| o.leaf_id.clone())
        .collect();
    let Some(workspace) = store
        .workspaces
        .iter_mut()
        .find(|w| w.id == save.workspace_id)
    else {
        return fail(ErrorCode::ContextStale);
    };
    if workspace.revision != save.expected_workspace_revision {
        return fail(ErrorCode::RevisionConflict);
    }
    // A leaf with a launch in flight cannot be taken away from under it.
    if unfinished
        .iter()
        .any(|leaf| !save.leaves.iter().any(|l| &l.id == leaf))
    {
        return fail(ErrorCode::LeafBusy);
    }
    workspace.revision = bump(workspace.revision, "workspaces.revision")?;
    workspace.label = save.label.clone();
    workspace.layout = save.layout.clone();
    workspace.focus = save.focus.clone();
    workspace.pinned = save.pinned;
    workspace.leaves = save.leaves.clone();
    workspace.preferences = save.preferences.clone();
    Ok(Vec::new())
}

fn delete_workspace(
    store: &mut StoreV1,
    workspace_id: &WorkspaceId,
    env: &dyn Env,
) -> R<Vec<Created>> {
    let Some(at) = store.workspaces.iter().position(|w| &w.id == workspace_id) else {
        return fail(ErrorCode::ContextStale);
    };
    let Some(project_id) = store.workspaces[at].project_id.clone() else {
        return bad("workspaces.scratch");
    };
    if store
        .operations
        .iter()
        .any(|o| &o.workspace_id == workspace_id && o.state == OperationState::Pending)
    {
        return fail(ErrorCode::LeafBusy);
    }
    let removed = store.workspaces.remove(at);
    // Its sessions are detached, not closed: they keep running and stay discoverable.
    let held: Vec<AssociationId> = removed
        .leaves
        .iter()
        .filter_map(|l| l.association_id.clone())
        .collect();
    for association in &held {
        let shown_elsewhere = store.workspaces.iter().any(|w| {
            w.leaves
                .iter()
                .any(|l| l.association_id.as_ref() == Some(association))
        });
        if !shown_elsewhere
            && let Some(found) = store.sessions.iter_mut().find(|a| &a.id == association)
        {
            found.state = AssociationState::Detached;
        }
    }
    let mut made = Vec::new();
    let replacement = store
        .workspaces
        .iter()
        .find(|w| w.project_id.as_ref() == Some(&project_id))
        .map(|w| w.id.clone());
    let default = match replacement {
        Some(id) => id,
        None => {
            let id = valid(WorkspaceId::parse(&env.uuid()), "workspaces")?;
            let host = store.host_id.clone();
            store.workspaces.push(new_workspace(
                id.clone(),
                Some(project_id.clone()),
                host,
                valid(Text::parse("Main"), "workspaces.label")?,
            ));
            made.push(created("workspace", id.as_str()));
            id
        }
    };
    let project = live_project(store, &project_id)?;
    if project.default_workspace_id.as_ref() == Some(workspace_id) {
        project.default_workspace_id = Some(default);
    }
    touch(project, env)?;
    Ok(made)
}

fn detach(
    store: &mut StoreV1,
    workspace_id: &WorkspaceId,
    leaf_id: &LeafId,
    association_id: &AssociationId,
) -> R<Vec<Created>> {
    let Some(workspace) = store.workspaces.iter_mut().find(|w| &w.id == workspace_id) else {
        return fail(ErrorCode::ContextStale);
    };
    let Some(leaf) = workspace.leaves.iter_mut().find(|l| &l.id == leaf_id) else {
        return fail(ErrorCode::ContextStale);
    };
    if leaf.association_id.as_ref() != Some(association_id) {
        return fail(ErrorCode::ContextStale);
    }
    leaf.association_id = None;
    workspace.revision = bump(workspace.revision, "workspaces.revision")?;
    let still_shown = store.workspaces.iter().any(|w| {
        w.leaves
            .iter()
            .any(|l| l.association_id.as_ref() == Some(association_id))
    });
    if !still_shown && let Some(found) = store.sessions.iter_mut().find(|a| &a.id == association_id)
    {
        // Detaching never closes the terminal or the runtime; it only stops showing it.
        found.state = AssociationState::Detached;
    }
    Ok(Vec::new())
}

fn adopt(
    store: &mut StoreV1,
    association_id: &AssociationId,
    project_id: &ProjectId,
    binding_id: &BindingId,
) -> R<Vec<Created>> {
    live_project(store, project_id)?;
    let binding = store
        .bindings
        .iter()
        .find(|b| {
            &b.id == binding_id && &b.project_id == project_id && b.state == BindingState::Valid
        })
        .map(|b| b.revision);
    let Some(revision) = binding else {
        return fail(ErrorCode::RootIneligible);
    };
    let Some(found) = store.sessions.iter_mut().find(|a| &a.id == association_id) else {
        return fail(ErrorCode::ContextStale);
    };
    // Only an unassigned plain shell is adopted; Native sessions belong to their registry.
    if found.origin_project_id.is_some() || !matches!(found.target, Target::Shell { .. }) {
        return bad("sessions.originProjectId");
    }
    found.origin_project_id = Some(project_id.clone());
    found.binding_id = Some(binding_id.clone());
    found.binding_revision = Some(revision);
    found.state = AssociationState::Detached;
    // A Scratch leaf no longer shows what is not Scratch's: the shell stays discoverable.
    for workspace in store
        .workspaces
        .iter_mut()
        .filter(|w| w.project_id.is_none())
    {
        for leaf in workspace
            .leaves
            .iter_mut()
            .filter(|l| l.association_id.as_ref() == Some(association_id))
        {
            leaf.association_id = None;
        }
    }
    Ok(Vec::new())
}

/// What a launch needs from the store and the window's context, fixed before anything is asked
/// of an owner. It is recorded in the receipt and never recomputed from current selection.
struct Plan {
    project: Option<ProjectId>,
    binding: Option<(BindingId, Counter, String)>,
    workspace: WorkspaceId,
    leaf: LeafId,
    provider: LaunchProvider,
    mode: Mode,
    native: Option<ProfileRef>,
    safety: Option<ManagedSafetyProfile>,
}

enum Dispatched {
    Native(NativeLaunched),
    Shell(ShellLaunched),
    Managed(ManagedLaunched),
}

fn terminal_state(state: OperationState) -> bool {
    matches!(state, OperationState::Committed | OperationState::Refused)
}

impl Engine<'_> {
    /// Rebuilds the immutable fields of a receipt from a request, to compare a repeat with the
    /// original. The owner is not asked anything.
    fn same_request(existing: &Operation, plan: &Plan, context: &Context) -> bool {
        existing.kind == OperationKind::Launch
            && existing.workspace_id == plan.workspace
            && existing.leaf_id == plan.leaf
            && existing.project_id == plan.project
            && existing.binding_id == context.binding_id
            && existing.binding_revision == context.binding_revision
            && existing.provider == plan.provider
            && existing.mode == plan.mode
            && existing.native_profile == plan.native
            && existing.managed_safety_profile == plan.safety
    }

    /// Checks the request against the store and the context and fixes the plan. Nothing is
    /// changed and no owner is asked about a session.
    fn plan(&self, current: &StoreV1, context: &Context, launch: &Launch) -> R<Plan> {
        if context.workspace_id.as_ref() != Some(&launch.workspace_id) {
            return fail(ErrorCode::ContextStale);
        }
        let Some(workspace) = current
            .workspaces
            .iter()
            .find(|w| w.id == launch.workspace_id)
        else {
            return fail(ErrorCode::ContextStale);
        };
        if !workspace.leaves.iter().any(|l| l.id == launch.leaf_id) {
            return fail(ErrorCode::ContextStale);
        }
        let native = launch.native_profile.as_ref().map(|choice| ProfileRef {
            id: choice.id.clone(),
            version: choice.version,
        });
        let safety = launch.managed_safety_profile;
        // The two profiles are different things; a launch names the one its mode uses and never
        // a stand-in for the other.
        match (launch.mode, launch.provider) {
            (Mode::Native, LaunchProvider::Shell) if native.is_some() || safety.is_some() => {
                return bad("profile");
            }
            (Mode::Native, _) if safety.is_some() => return bad("profile"),
            (Mode::Managed, LaunchProvider::Shell) => return bad("profile"),
            (Mode::Managed, _) if native.is_some() || safety.is_none() => return bad("profile"),
            _ => {}
        }
        // Claude has no Managed adapter in this release.
        if launch.mode == Mode::Managed && launch.provider == LaunchProvider::Claude {
            return fail(ErrorCode::UnsupportedCapability);
        }
        let (project, binding) = match &workspace.project_id {
            Some(project_id) => {
                let project = current
                    .projects
                    .iter()
                    .find(|p| &p.id == project_id && !p.tombstone)
                    .map_or_else(|| fail(ErrorCode::ContextStale), Ok)?;
                let binding = project
                    .default_binding_id
                    .as_ref()
                    .and_then(|id| current.bindings.iter().find(|b| &b.id == id));
                let Some(binding) = binding else {
                    return fail(ErrorCode::RootIneligible);
                };
                if context.binding_id.as_ref() != Some(&binding.id)
                    || context.binding_revision != Some(binding.revision)
                {
                    return fail(ErrorCode::ContextStale);
                }
                if binding.state != BindingState::Valid {
                    return fail(match binding.state {
                        BindingState::Missing => ErrorCode::RootMissing,
                        BindingState::IdentityChanged => ErrorCode::RootIdentityChanged,
                        _ => ErrorCode::RootIneligible,
                    });
                }
                (
                    Some(project_id.clone()),
                    Some((
                        binding.id.clone(),
                        binding.revision,
                        binding.path.as_str().to_owned(),
                    )),
                )
            }
            None => {
                // Scratch has no binding, no provider and no authority: a plain shell only.
                if launch.provider != LaunchProvider::Shell || context.binding_id.is_some() {
                    return fail(ErrorCode::UnsupportedCapability);
                }
                (None, None)
            }
        };
        Ok(Plan {
            project,
            binding,
            workspace: launch.workspace_id.clone(),
            leaf: launch.leaf_id.clone(),
            provider: launch.provider,
            mode: launch.mode,
            native,
            safety,
        })
    }

    fn launch(&self, current: &StoreV1, envelope: &Envelope, launch: &Launch) -> R<Receipt> {
        let plan = self.plan(current, &envelope.context, launch)?;
        let id = &envelope.operation_id;
        if current.closed_operations.iter().any(|c| c == id) {
            return Err(Failure::new(ErrorCode::InvalidRecord)
                .with_field("operationId")
                .with_effect(Effect::Unknown));
        }
        if let Some(existing) = current.operations.iter().find(|o| &o.id == id) {
            // The same operation again: never a second dispatch.
            if !Self::same_request(existing, &plan, &envelope.context) {
                return bad("operationId");
            }
            return match existing.state {
                OperationState::Committed => {
                    let mut receipt = self.receipt(envelope, current.revision, Vec::new());
                    receipt.replayed = true;
                    Ok(receipt)
                }
                OperationState::Pending => {
                    Err(Failure::new(ErrorCode::LeafBusy).with_effect(Effect::Unknown))
                }
                OperationState::Ambiguous => {
                    Err(Failure::new(ErrorCode::EffectAmbiguous).with_effect(Effect::Unknown))
                }
                OperationState::Refused => fail(ErrorCode::OwnerRefused),
            };
        }
        let occupied = current
            .workspaces
            .iter()
            .find(|w| w.id == plan.workspace)
            .and_then(|w| w.leaves.iter().find(|l| l.id == plan.leaf))
            .is_some_and(|l| l.association_id.is_some());
        if occupied {
            return fail(ErrorCode::LeafBusy);
        }
        // The root is looked at again now: a replaced or missing folder blocks the effect.
        if let (Some((_, _, path)), Some(binding)) = (
            &plan.binding,
            plan.binding
                .as_ref()
                .and_then(|(id, _, _)| current.bindings.iter().find(|b| &b.id == id)),
        ) {
            let root = self.owner.root(path).map_err(root_failure)?;
            if root.device != binding.observation.device.get()
                || root.inode != binding.observation.inode.get()
            {
                return fail(ErrorCode::RootIdentityChanged);
            }
        }
        // Native work and shells need the host as it is now; it is read, never started.
        let host: Option<HostView> = if plan.mode == Mode::Native {
            Some(self.owner.host(None).map_err(owner_failure)?)
        } else {
            None
        };
        let epoch = host.as_ref().map(|h| h.epoch.clone());

        // Phase A: the journal says `pending` before anything is dispatched.
        let claim = self.env.claim();
        let pending = Operation {
            id: id.clone(),
            kind: OperationKind::Launch,
            project_id: plan.project.clone(),
            workspace_id: plan.workspace.clone(),
            leaf_id: plan.leaf.clone(),
            binding_id: plan.binding.as_ref().map(|(id, _, _)| id.clone()),
            binding_revision: plan.binding.as_ref().map(|(_, r, _)| *r),
            provider: plan.provider,
            mode: plan.mode,
            native_profile: plan.native.clone(),
            managed_safety_profile: plan.safety,
            state: OperationState::Pending,
            association_id: None,
            claim: Some(valid(EpochText::parse(&claim), "operations.claim")?),
            created_at: valid(
                UtcTimestamp::try_from(self.env.now()),
                "operations.createdAt",
            )?,
        };
        self.store
            .transact(envelope.expected_store_revision, |store| {
                let busy = store.operations.iter().any(|o| {
                    o.workspace_id == plan.workspace
                        && o.leaf_id == plan.leaf
                        && matches!(o.state, OperationState::Pending | OperationState::Ambiguous)
                });
                if busy {
                    return fail(ErrorCode::LeafBusy);
                }
                // A ceiling is refused before anything is asked of the owner: a process started
                // here that the store could not then record would be one nobody holds.
                if store.sessions.len() >= wyss_console::contracts::m1::MAX_ASSOCIATIONS {
                    return fail(ErrorCode::CapacityExceeded);
                }
                store.operations.push(pending);
                Ok::<(), Failure>(())
            })
            .map_err(store_failure)??;

        // Phase B: the owner is asked, outside the lock, once.
        let dispatched = self.dispatch(&plan, epoch.as_deref(), id);

        // Phase C: the receipt is completed against whatever the store is by now.
        self.finish(envelope, &plan, epoch, dispatched)
    }

    fn dispatch(
        &self,
        plan: &Plan,
        epoch: Option<&str>,
        operation: &OperationId,
    ) -> Result<Dispatched, LaunchError> {
        let path = plan.binding.as_ref().map(|(_, _, path)| path.clone());
        let not_dispatched = |e| LaunchError::NotDispatched(e);
        let agent = match plan.provider {
            LaunchProvider::Claude => Some(Agent::ClaudeCode),
            LaunchProvider::Codex => Some(Agent::Codex),
            LaunchProvider::Shell => None,
        };
        match (plan.mode, agent) {
            (Mode::Native, Some(agent)) => {
                let path = path.ok_or(not_dispatched(OwnerError::Refused))?;
                let epoch = epoch.ok_or(not_dispatched(OwnerError::HostOffline))?;
                self.owner
                    .launch_native(
                        &NativeLaunch {
                            agent,
                            path,
                            profile: plan
                                .native
                                .as_ref()
                                .map(|p| (p.id.as_str().to_owned(), p.version.get())),
                        },
                        epoch,
                    )
                    .map(Dispatched::Native)
            }
            (Mode::Native, None) => {
                let epoch = epoch.ok_or(not_dispatched(OwnerError::HostOffline))?;
                // Scratch has no folder: the host's own default applies.
                let path = path.unwrap_or_default();
                self.owner.launch_shell(&path, epoch).map(Dispatched::Shell)
            }
            (Mode::Managed, Some(agent)) => {
                let path = path.ok_or(not_dispatched(OwnerError::Refused))?;
                let requested = plan.safety.ok_or(not_dispatched(OwnerError::Refused))?;
                self.owner
                    .launch_managed(&ManagedLaunch {
                        agent,
                        path,
                        requested,
                        // The operation names the whole action: a retry is the same intent.
                        intent: operation.as_str().to_owned(),
                    })
                    .map(Dispatched::Managed)
            }
            (Mode::Managed, None) => Err(not_dispatched(OwnerError::Unsupported)),
        }
    }

    /// Completes a receipt whose dispatch has been tried. Store conflicts are re-read and
    /// re-applied; the owner is never asked again.
    fn finish(
        &self,
        envelope: &Envelope,
        plan: &Plan,
        epoch: Option<String>,
        dispatched: Result<Dispatched, LaunchError>,
    ) -> R<Receipt> {
        let id = &envelope.operation_id;
        let target = match &dispatched {
            Ok(done) => self.target_of(done, epoch.as_deref(), plan.safety),
            Err(_) => None,
        };
        let association_id = valid(AssociationId::parse(&self.env.uuid()), "sessions")?;
        for _ in 0..8 {
            let current = self.load();
            let current = match current {
                Ok(current) => current,
                Err(_) => break,
            };
            let mut attached = false;
            let result = self.store.transact(current.revision, |store| {
                let Some(at) = store.operations.iter().position(|o| &o.id == id) else {
                    return fail(ErrorCode::ContextStale);
                };
                match (&dispatched, &target) {
                    (Ok(_), Some(target)) => {
                        let still_there = store
                            .workspaces
                            .iter()
                            .find(|w| w.id == plan.workspace)
                            .and_then(|w| w.leaves.iter().find(|l| l.id == plan.leaf))
                            .is_some_and(|l| l.association_id.is_none())
                            && plan.binding.as_ref().is_none_or(|(binding, revision, _)| {
                                store
                                    .bindings
                                    .iter()
                                    .any(|b| &b.id == binding && b.revision == *revision)
                            });
                        store.sessions.push(Association {
                            id: association_id.clone(),
                            origin_project_id: plan.project.clone(),
                            host_id: store.host_id.clone(),
                            binding_id: plan.binding.as_ref().map(|(id, _, _)| id.clone()),
                            binding_revision: plan.binding.as_ref().map(|(_, r, _)| *r),
                            target: target.clone(),
                            state: if still_there {
                                AssociationState::Attached
                            } else {
                                AssociationState::Detached
                            },
                            operation_id: Some(id.clone()),
                        });
                        let operation = &mut store.operations[at];
                        operation.association_id = Some(association_id.clone());
                        operation.claim = None;
                        if still_there {
                            operation.state = OperationState::Committed;
                            if let Some(leaf) = store
                                .workspaces
                                .iter_mut()
                                .find(|w| w.id == plan.workspace)
                                .and_then(|w| w.leaves.iter_mut().find(|l| l.id == plan.leaf))
                            {
                                leaf.association_id = Some(association_id.clone());
                            }
                            attached = true;
                        } else {
                            // The leaf, workspace or binding moved while the owner worked: the
                            // session exists and stays discoverable, unattached.
                            operation.state = OperationState::Ambiguous;
                        }
                    }
                    (Err(LaunchError::NotDispatched(_)), _) => {
                        let operation = &mut store.operations[at];
                        operation.state = OperationState::Refused;
                        operation.claim = None;
                    }
                    _ => {
                        let operation = &mut store.operations[at];
                        operation.state = OperationState::Ambiguous;
                        operation.claim = None;
                    }
                }
                Ok::<(), Failure>(())
            });
            match result {
                Ok(Ok(committed)) => {
                    return match dispatched {
                        Ok(_) if attached => Ok(self.receipt(
                            envelope,
                            committed.revision,
                            vec![created("association", association_id.as_str())],
                        )),
                        Ok(_) => {
                            Err(Failure::new(ErrorCode::EffectAmbiguous)
                                .with_effect(Effect::Committed))
                        }
                        Err(LaunchError::NotDispatched(error)) => Err(owner_failure(error)),
                        Err(LaunchError::Ambiguous) => {
                            Err(Failure::new(ErrorCode::EffectAmbiguous)
                                .with_effect(Effect::Unknown))
                        }
                    };
                }
                Ok(Err(failure)) => return Err(failure),
                Err(StoreError::RevisionConflict { .. }) => continue,
                Err(_) => break,
            }
        }
        // The journal could not be completed. A dispatched request is unknown from here on and
        // is surfaced as such; a refused one only leaves its `pending` for recovery.
        match dispatched {
            Err(LaunchError::NotDispatched(error)) => Err(owner_failure(error)),
            _ => Err(Failure::new(ErrorCode::EffectAmbiguous).with_effect(Effect::Unknown)),
        }
    }

    /// The target an owner's reply names, or none if the owner's identifiers are not ones the
    /// contract can hold. Those are never coerced.
    fn target_of(
        &self,
        dispatched: &Dispatched,
        epoch: Option<&str>,
        safety: Option<ManagedSafetyProfile>,
    ) -> Option<Target> {
        match dispatched {
            Dispatched::Native(done) => Some(Target::Native {
                session_id: NativeId::parse(&done.session_id).ok()?,
                terminal_id: TerminalId::parse(&done.terminal_id).ok()?,
                host_epoch: EpochText::parse(epoch?).ok()?,
            }),
            Dispatched::Shell(done) => Some(Target::Shell {
                terminal_id: TerminalId::parse(&done.terminal_id).ok()?,
                host_epoch: EpochText::parse(epoch?).ok()?,
            }),
            Dispatched::Managed(done) => Some(Target::Managed {
                protocol: done.protocol,
                store_stream_id: StoreStreamId::parse(&done.store_stream_id).ok()?,
                session_id: ManagedSessionId::parse(&done.session_id).ok()?,
                requested_safety_profile: safety,
            }),
        }
    }

    fn attach(
        &self,
        current: &StoreV1,
        envelope: &Envelope,
        workspace_id: &WorkspaceId,
        leaf_id: &LeafId,
        association_id: &AssociationId,
    ) -> R<Receipt> {
        let Some(association) = current.sessions.iter().find(|a| &a.id == association_id) else {
            return fail(ErrorCode::ContextStale);
        };
        // The owner is asked, read-only, whether the target is there as the association says.
        match &association.target {
            Target::Native {
                terminal_id,
                host_epoch,
                session_id,
            } => {
                let view = self
                    .owner
                    .host(Some(host_epoch.as_str()))
                    .map_err(owner_failure)?;
                let row = view
                    .terminals
                    .iter()
                    .find(|t| t.terminal_id == terminal_id.as_str() && t.running);
                if !row.is_some_and(|t| t.native_session.as_deref() == Some(session_id.as_str())) {
                    return Err(Failure::new(ErrorCode::OwnerRefused).with_field("target"));
                }
            }
            Target::Shell {
                terminal_id,
                host_epoch,
            } => {
                let view = self
                    .owner
                    .host(Some(host_epoch.as_str()))
                    .map_err(owner_failure)?;
                if !view
                    .terminals
                    .iter()
                    .any(|t| t.terminal_id == terminal_id.as_str() && t.running)
                {
                    return Err(Failure::new(ErrorCode::OwnerRefused).with_field("target"));
                }
            }
            Target::Managed {
                protocol,
                store_stream_id,
                session_id,
                requested_safety_profile,
            } => {
                let rows = self.owner.managed().map_err(owner_failure)?;
                let row = rows.iter().find(|r| {
                    r.protocol == *protocol
                        && r.store_stream_id == store_stream_id.as_str()
                        && r.session_id == session_id.as_str()
                });
                let Some(row) = row else {
                    return Err(Failure::new(ErrorCode::OwnerRefused).with_field("target"));
                };
                // The requested profile is only a request: it must match what the authority
                // attached, and an unknown effective profile is not a match.
                if requested_safety_profile.is_some()
                    && row.effective_profile != *requested_safety_profile
                {
                    return Err(
                        Failure::new(ErrorCode::OwnerRefused).with_field("managedSafetyProfile")
                    );
                }
            }
        }
        let (workspace_id, leaf_id, association_id) = (
            workspace_id.clone(),
            leaf_id.clone(),
            association_id.clone(),
        );
        self.commit(envelope, move |store, _| {
            let Some(workspace) = store.workspaces.iter_mut().find(|w| w.id == workspace_id) else {
                return fail(ErrorCode::ContextStale);
            };
            let Some(leaf) = workspace.leaves.iter_mut().find(|l| l.id == leaf_id) else {
                return fail(ErrorCode::ContextStale);
            };
            if leaf.association_id.is_some()
                && leaf.association_id.as_ref() != Some(&association_id)
            {
                return fail(ErrorCode::LeafBusy);
            }
            leaf.association_id = Some(association_id.clone());
            workspace.revision = bump(workspace.revision, "workspaces.revision")?;
            if let Some(found) = store.sessions.iter_mut().find(|a| a.id == association_id) {
                found.state = AssociationState::Attached;
            }
            Ok(Vec::new())
        })
    }

    fn remove_project(
        &self,
        current: &StoreV1,
        envelope: &Envelope,
        project_id: &ProjectId,
        retain_history: bool,
    ) -> R<Receipt> {
        let Some(project) = current
            .projects
            .iter()
            .find(|p| &p.id == project_id && !p.tombstone)
        else {
            return fail(ErrorCode::ContextStale);
        };
        let workspaces: HashSet<&WorkspaceId> = current
            .workspaces
            .iter()
            .filter(|w| w.project_id.as_ref() == Some(&project.id))
            .map(|w| &w.id)
            .collect();
        // A launch in flight or of unknown outcome must be settled first.
        if current.operations.iter().any(|o| {
            workspaces.contains(&o.workspace_id)
                && matches!(o.state, OperationState::Pending | OperationState::Ambiguous)
        }) {
            return fail(ErrorCode::LeafBusy);
        }
        let mine: Vec<&Association> = current
            .sessions
            .iter()
            .filter(|a| a.origin_project_id.as_ref() == Some(project_id))
            .collect();
        if !mine.is_empty() {
            // Removal needs proof that nothing of the project is still alive: the owners are
            // asked, and an owner that cannot answer proves nothing.
            let host = self.owner.host(None).map_err(owner_failure)?;
            let managed = self.owner.managed();
            for association in &mine {
                let alive = match &association.target {
                    Target::Native {
                        terminal_id,
                        host_epoch,
                        ..
                    }
                    | Target::Shell {
                        terminal_id,
                        host_epoch,
                    } => {
                        host.epoch == host_epoch.as_str()
                            && host
                                .terminals
                                .iter()
                                .any(|t| t.terminal_id == terminal_id.as_str() && t.running)
                    }
                    Target::Managed {
                        protocol,
                        store_stream_id,
                        session_id,
                        ..
                    } => match &managed {
                        Err(_) => true,
                        Ok(rows) => rows
                            .iter()
                            .find(|r| {
                                r.protocol == *protocol
                                    && r.store_stream_id == store_stream_id.as_str()
                                    && r.session_id == session_id.as_str()
                            })
                            .is_none_or(|r| r.attached),
                    },
                };
                if alive {
                    return Err(Failure::new(ErrorCode::OwnerRefused).with_field("sessions"));
                }
            }
        }
        let project_id = project_id.clone();
        self.commit(envelope, move |store, env| {
            let bindings: HashSet<BindingId> = store
                .bindings
                .iter()
                .filter(|b| b.project_id == project_id)
                .map(|b| b.id.clone())
                .collect();
            store
                .workspaces
                .retain(|w| w.project_id.as_ref() != Some(&project_id));
            store.bindings.retain(|b| b.project_id != project_id);
            if retain_history {
                // History keeps its origin; it no longer names a binding that is gone.
                for association in store
                    .sessions
                    .iter_mut()
                    .filter(|a| a.origin_project_id.as_ref() == Some(&project_id))
                {
                    if association
                        .binding_id
                        .as_ref()
                        .is_some_and(|b| bindings.contains(b))
                    {
                        association.binding_id = None;
                        association.binding_revision = None;
                    }
                    association.state = AssociationState::Unavailable;
                }
                let project = store
                    .projects
                    .iter_mut()
                    .find(|p| p.id == project_id)
                    .expect("checked above");
                project.tombstone = true;
                project.default_binding_id = None;
                project.default_workspace_id = None;
                touch(project, env)?;
            } else {
                store
                    .sessions
                    .retain(|a| a.origin_project_id.as_ref() != Some(&project_id));
                store.projects.retain(|p| p.id != project_id);
            }
            Ok(Vec::new())
        })
    }

    /// The user's decision about a launch whose outcome was unknown.
    fn reconcile(
        &self,
        current: &StoreV1,
        envelope: &Envelope,
        launch_id: &OperationId,
        resolution: &Resolution,
    ) -> R<Receipt> {
        let Some(operation) = current.operations.iter().find(|o| &o.id == launch_id) else {
            return fail(ErrorCode::ContextStale);
        };
        if operation.state != OperationState::Ambiguous || operation.association_id.is_some() {
            return bad("operationId");
        }
        // The pane that shows the unknown launch is in the workspace that made it.
        if envelope
            .context
            .workspace_id
            .as_ref()
            .is_some_and(|w| w != &operation.workspace_id)
        {
            return fail(ErrorCode::ContextStale);
        }
        let held: HashSet<&str> = current
            .sessions
            .iter()
            .filter_map(|a| match &a.target {
                Target::Native { terminal_id, .. } | Target::Shell { terminal_id, .. } => {
                    Some(terminal_id.as_str())
                }
                Target::Managed { .. } => None,
            })
            .collect();
        let launch_id = launch_id.clone();
        match resolution {
            Resolution::Absent => {
                // The owner must be readable and show nothing that no association holds.
                match operation.mode {
                    Mode::Native => {
                        let view = self.owner.host(None).map_err(owner_failure)?;
                        if view
                            .terminals
                            .iter()
                            .any(|t| t.running && !held.contains(t.terminal_id.as_str()))
                        {
                            return Err(
                                Failure::new(ErrorCode::OwnerRefused).with_field("candidates")
                            );
                        }
                    }
                    Mode::Managed => {
                        let rows = self.owner.managed().map_err(owner_failure)?;
                        let known: HashSet<&str> = current
                            .sessions
                            .iter()
                            .filter_map(|a| match &a.target {
                                Target::Managed { session_id, .. } => Some(session_id.as_str()),
                                _ => None,
                            })
                            .collect();
                        if rows.iter().any(|r| !known.contains(r.session_id.as_str())) {
                            return Err(
                                Failure::new(ErrorCode::OwnerRefused).with_field("candidates")
                            );
                        }
                    }
                }
                self.commit(envelope, move |store, _| {
                    let Some(operation) = store.operations.iter_mut().find(|o| o.id == launch_id)
                    else {
                        return fail(ErrorCode::ContextStale);
                    };
                    operation.state = OperationState::Refused;
                    operation.claim = None;
                    Ok(Vec::new())
                })
            }
            Resolution::RetryManaged => {
                if operation.mode != Mode::Managed {
                    return fail(ErrorCode::UnsupportedCapability);
                }
                let agent = match operation.provider {
                    LaunchProvider::Codex => Agent::Codex,
                    LaunchProvider::Claude => Agent::ClaudeCode,
                    LaunchProvider::Shell => return bad("provider"),
                };
                let requested = operation
                    .managed_safety_profile
                    .map_or_else(|| bad("profile"), Ok)?;
                let path = operation
                    .binding_id
                    .as_ref()
                    .and_then(|id| current.bindings.iter().find(|b| &b.id == id))
                    .map(|b| b.path.as_str().to_owned())
                    .map_or_else(|| fail(ErrorCode::RootIneligible), Ok)?;
                // The same intent the first attempt used: the owner settles it, it does not repeat it.
                let done = match self.owner.launch_managed(&ManagedLaunch {
                    agent,
                    path,
                    requested,
                    intent: launch_id.as_str().to_owned(),
                }) {
                    Ok(done) => done,
                    Err(LaunchError::NotDispatched(error)) => return Err(owner_failure(error)),
                    Err(LaunchError::Ambiguous) => {
                        return Err(
                            Failure::new(ErrorCode::EffectAmbiguous).with_effect(Effect::Unknown)
                        );
                    }
                };
                let target = self
                    .target_of(&Dispatched::Managed(done), None, Some(requested))
                    .map_or_else(|| bad("target"), Ok)?;
                let association_id = valid(AssociationId::parse(&self.env.uuid()), "sessions")?;
                self.commit(envelope, move |store, _| {
                    complete_with(store, &launch_id, association_id, target)
                })
            }
            Resolution::Identified {
                terminal_id,
                session_id,
            } => {
                if operation.mode != Mode::Native {
                    return fail(ErrorCode::UnsupportedCapability);
                }
                let view = self.owner.host(None).map_err(owner_failure)?;
                let row = view.terminals.iter().find(|t| {
                    t.running
                        && t.terminal_id == terminal_id.as_str()
                        && !held.contains(t.terminal_id.as_str())
                });
                let Some(row) = row else {
                    return Err(Failure::new(ErrorCode::OwnerRefused).with_field("target"));
                };
                let shell = operation.provider == LaunchProvider::Shell;
                if shell != row.native_session.is_none()
                    || (!shell
                        && row.native_session.as_deref() != session_id.as_ref().map(|s| s.as_str()))
                {
                    return Err(Failure::new(ErrorCode::OwnerRefused).with_field("target"));
                }
                let target = if shell {
                    Target::Shell {
                        terminal_id: terminal_id.clone(),
                        host_epoch: valid(EpochText::parse(&view.epoch), "epoch")?,
                    }
                } else {
                    Target::Native {
                        session_id: session_id.clone().expect("matched above"),
                        terminal_id: terminal_id.clone(),
                        host_epoch: valid(EpochText::parse(&view.epoch), "epoch")?,
                    }
                };
                let association_id = valid(AssociationId::parse(&self.env.uuid()), "sessions")?;
                self.commit(envelope, move |store, _| {
                    complete_with(store, &launch_id, association_id, target)
                })
            }
        }
    }

    // ---- recovery and housekeeping, never part of selecting a project ------------------------

    /// A `pending` receipt whose claiming process is gone cannot be told from one dispatched
    /// just before the crash: it becomes `ambiguous` and is never replayed. A live claim, which
    /// another window may still be completing, is left alone.
    pub fn recover_pending(&self) -> R<usize> {
        for _ in 0..8 {
            let current = self.load()?;
            let dead: Vec<OperationId> = current
                .operations
                .iter()
                .filter(|o| {
                    o.state == OperationState::Pending
                        && o.claim
                            .as_ref()
                            .is_none_or(|c| !self.env.claim_is_live(c.as_str()))
                })
                .map(|o| o.id.clone())
                .collect();
            if dead.is_empty() {
                return Ok(0);
            }
            let count = dead.len();
            match self.store.transact(current.revision, |store| {
                for operation in store.operations.iter_mut().filter(|o| dead.contains(&o.id)) {
                    operation.state = OperationState::Ambiguous;
                    operation.claim = None;
                }
                Ok::<(), Failure>(())
            }) {
                Ok(Ok(_)) => return Ok(count),
                Ok(Err(failure)) => return Err(failure),
                Err(StoreError::RevisionConflict { .. }) => continue,
                Err(other) => return Err(store_failure(other)),
            }
        }
        fail(ErrorCode::RevisionConflict)
    }

    /// Moves finished receipts to the closed list so the receipt ceiling can admit new work.
    /// Receipts of unknown outcome stay, and every association is untouched.
    pub fn compact_receipts(&self) -> R<usize> {
        for _ in 0..8 {
            let current = self.load()?;
            let finished: Vec<OperationId> = current
                .operations
                .iter()
                .filter(|o| terminal_state(o.state))
                .map(|o| o.id.clone())
                .collect();
            if finished.is_empty() {
                return Ok(0);
            }
            let count = finished.len();
            match self.store.transact(current.revision, |store| {
                store.operations.retain(|o| !finished.contains(&o.id));
                store.closed_operations.extend(finished.iter().cloned());
                Ok::<(), Failure>(())
            }) {
                Ok(Ok(_)) => return Ok(count),
                Ok(Err(failure)) => return Err(failure),
                Err(StoreError::RevisionConflict { .. }) => continue,
                Err(other) => return Err(store_failure(other)),
            }
        }
        fail(ErrorCode::RevisionConflict)
    }
}

/// The smallest valid store: the Scratch workspace and nothing else. Creating it is an
/// explicit act (a migration commit or an empty initialisation), never a fallback.
pub fn empty_store(env: &dyn Env) -> R<StoreV1> {
    let host = valid(
        wyss_console::contracts::m1::scalar::HostId::parse(&env.uuid()),
        "hostId",
    )?;
    let scratch = new_workspace(
        valid(WorkspaceId::parse(&env.uuid()), "workspaces")?,
        None,
        host.clone(),
        valid(Text::parse("Scratch"), "workspaces.label")?,
    );
    Ok(StoreV1 {
        schema: wyss_console::contracts::m1::records::SchemaV1,
        store_id: valid(
            wyss_console::contracts::m1::scalar::StoreId::parse(&env.uuid()),
            "storeId",
        )?,
        revision: counter(1),
        host_id: host,
        projects: Vec::new(),
        bindings: Vec::new(),
        workspaces: vec![scratch],
        sessions: Vec::new(),
        operations: Vec::new(),
        closed_operations: Vec::new(),
        migration: None,
    })
}

/// Gives an ambiguous receipt the session the user (or the owner's own settling of an intent)
/// established: an association, in the receipt's leaf when it is still free, and `committed`.
fn complete_with(
    store: &mut StoreV1,
    launch_id: &OperationId,
    association_id: AssociationId,
    target: Target,
) -> R<Vec<Created>> {
    let Some(at) = store.operations.iter().position(|o| &o.id == launch_id) else {
        return fail(ErrorCode::ContextStale);
    };
    let (workspace, leaf, project, binding) = {
        let o = &store.operations[at];
        (
            o.workspace_id.clone(),
            o.leaf_id.clone(),
            o.project_id.clone(),
            o.binding_id.clone().zip(o.binding_revision),
        )
    };
    let free = store
        .workspaces
        .iter()
        .find(|w| w.id == workspace)
        .and_then(|w| w.leaves.iter().find(|l| l.id == leaf))
        .is_some_and(|l| l.association_id.is_none());
    store.sessions.push(Association {
        id: association_id.clone(),
        origin_project_id: project,
        host_id: store.host_id.clone(),
        binding_id: binding.as_ref().map(|(id, _)| id.clone()),
        binding_revision: binding.map(|(_, r)| r),
        target,
        state: if free {
            AssociationState::Attached
        } else {
            AssociationState::Detached
        },
        operation_id: Some(launch_id.clone()),
    });
    let operation = &mut store.operations[at];
    operation.state = OperationState::Committed;
    operation.association_id = Some(association_id.clone());
    operation.claim = None;
    if free
        && let Some(leaf) = store
            .workspaces
            .iter_mut()
            .find(|w| w.id == workspace)
            .and_then(|w| w.leaves.iter_mut().find(|l| l.id == leaf))
    {
        leaf.association_id = Some(association_id.clone());
    }
    Ok(vec![created("association", association_id.as_str())])
}
