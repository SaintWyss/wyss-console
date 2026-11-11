//! The facade the window's commands use: reads that build views of the store and the owners,
//! the feature gate, the native-pick broker, the migration preview held between preview and
//! commit, and the terminal input claim.
//!
//! Reading never starts, repairs or changes anything. Every view is built from a fresh read of
//! the store and, best effort, of the owners; an owner that cannot be read makes a session
//! `unknown` with a reason, never `ended`.
use super::{
    apply::{Disclosures, Engine, Env, Purpose, Selections, empty_store, store_failure},
    claim::{ClaimError, Claims},
    command::{Envelope, ErrorCode, Failure, Receipt},
    migration::{self, IncrementalPlan, Plan, Sources, Summary},
    owner::{
        HostView, ManagedRow, Owner, OwnerError, RootError, TermRead, TermScreen, WriteOutcome,
    },
    store::{Loaded, Shared, Store, StoreError},
};
use serde::Serialize;
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use wyss_console::contracts::m1::{
    StoreV1,
    records::{
        Association, AssociationState, Binding, BindingState, ManagedProtocol,
        ManagedSafetyProfile, Node, Preferences, Project, Target, Workspace,
    },
    scalar::{AssociationId, ProjectId},
};

/// Most rows a page returns.
pub const PAGE: usize = 100;

/// Most bytes one write to a terminal carries: the host's own limit for one write. A longer paste
/// is sent by the window in pieces of this size, in order.
pub const MAX_WRITE: usize = 4 * 1024;

/// How long what the owners said is shown to every window before they are asked again. A display
/// read inside this time costs the owners nothing, however many windows are open and however
/// often they ask; anything that acts, or that decides what to do, reads the owner itself.
pub const INVENTORY_TTL: Duration = Duration::from_millis(1000);

/// When an answer was read, under which generation, and what it said.
type Kept<T> = (Instant, u64, Result<T, OwnerError>);

/// One owner's answer, kept for `INVENTORY_TTL`. The lock is held while it is read, so windows
/// that ask together share a single read instead of making one each. Forgetting never waits for
/// that read: it moves a generation, and an answer read under an older generation is not used.
struct Remembered<T> {
    answer: Mutex<Option<Kept<T>>>,
    generation: AtomicU64,
}

impl<T: Clone> Remembered<T> {
    fn new() -> Self {
        Self {
            answer: Mutex::new(None),
            generation: AtomicU64::new(0),
        }
    }

    fn get(
        &self,
        ttl: Duration,
        read: impl FnOnce() -> Result<T, OwnerError>,
    ) -> Result<T, OwnerError> {
        let mut held = self.answer.lock().unwrap_or_else(PoisonError::into_inner);
        let now = self.generation.load(Ordering::Acquire);
        if let Some((at, generation, answer)) = held.as_ref()
            && *generation == now
            && at.elapsed() < ttl
        {
            return answer.clone();
        }
        // What is read began under `now`: if something forgot meanwhile, it is stored as of `now`
        // and so is not used by the next ask.
        let answer = read();
        *held = Some((Instant::now(), now, answer.clone()));
        answer
    }

    fn forget(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum StoreStatus {
    /// No store: offer a migration preview or an explicit empty start.
    Missing,
    #[serde(rename_all = "camelCase")]
    Ready {
        revision: String,
        host_id: String,
        store_id: String,
        /// The one projectless workspace, which has no project to be listed under.
        scratch_workspace_id: String,
    },
    /// Present and not loadable: read-only recovery, exact bytes kept.
    #[serde(rename_all = "camelCase")]
    Unusable {
        code: ErrorCode,
        field: &'static str,
    },
    /// There is no state directory to keep a store in.
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostStatus {
    pub reachable: bool,
    pub epoch: Option<String>,
    pub features: Vec<String>,
    pub reason: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    /// The release gate: off until the owner adopts the project-centered workspace.
    pub enabled: bool,
    pub store: StoreStatus,
    pub host: HostStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootView {
    pub binding_id: String,
    pub binding_revision: String,
    /// The folder's name only; the path stays on this side.
    pub label: String,
    pub state: BindingState,
    /// What the folder is now: available, missing, identity_changed, ineligible, unknown.
    pub availability: &'static str,
    pub native_eligible: bool,
    pub config_eligible: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub id: String,
    pub label: String,
    pub archived: bool,
    pub tombstone: bool,
    pub revision: String,
    pub default_workspace_id: Option<String>,
    pub root: Option<RootView>,
    pub workspace_ids: Vec<String>,
    pub sessions: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub id: String,
    pub origin_project_id: Option<String>,
    pub state: AssociationState,
    pub kind: &'static str,
    /// live, ended or unknown, from the owner as it answered now.
    pub availability: &'static str,
    pub reason: Option<&'static str>,
    pub native_session_id: Option<String>,
    pub terminal_id: Option<String>,
    pub protocol: Option<u8>,
    pub store_stream_id: Option<String>,
    pub managed_session_id: Option<String>,
    pub requested_safety_profile: Option<ManagedSafetyProfile>,
    pub effective_safety_profile: Option<ManagedSafetyProfile>,
    pub profile_mismatch: bool,
    pub operation_id: Option<String>,
    /// Who holds input: this window, another, or nobody.
    pub input: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeafView {
    pub id: String,
    pub title: String,
    pub session: Option<SessionView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceView {
    pub id: String,
    pub project_id: Option<String>,
    pub label: String,
    pub revision: String,
    pub pinned: bool,
    pub layout: Option<Node>,
    pub focus: Option<String>,
    pub leaves: Vec<LeafView>,
    pub preferences: Preferences,
    /// Launches in flight or of unknown outcome, by leaf, for the window to show and resolve.
    pub unfinished: Vec<UnfinishedView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnfinishedView {
    pub operation_id: String,
    pub leaf_id: String,
    pub state: &'static str,
    /// `native` (a terminal host) or `managed` (an authority): only Managed may ask again.
    pub mode: &'static str,
}

/// A running terminal that no association holds: something an unknown launch could have started.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanView {
    pub terminal_id: String,
    pub native_session_id: Option<String>,
    /// Only the last component of where it started: enough to tell it apart, never a path.
    pub folder: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    /// Opaque; only valid at the store revision it was issued for.
    pub cursor: Option<String>,
    pub store_revision: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickView {
    pub token: String,
    pub label: String,
    pub native_eligible: bool,
    pub config_eligible: bool,
}

struct Held {
    plan: Result<Plan, IncrementalPlan>,
    sources: Sources,
}

/// What the window shows before it asks to move a shell into a project.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptionView {
    /// The project's folder the shell would be recorded under; the adoption names it.
    pub binding_id: String,
    /// The directory the shell is in now.
    pub cwd: String,
    /// Whether it is the project's folder or inside it.
    pub inside: bool,
}

pub struct ProjectsService {
    /// Opened on first use, and only once the gate is on: a build with the workspace off makes
    /// no directory and reads no file of it.
    store_dir: Option<PathBuf>,
    store: OnceLock<Option<Store>>,
    owner: Arc<dyn Owner>,
    env: Arc<dyn Env>,
    selections: Selections,
    disclosures: Disclosures,
    claims: Claims,
    /// Terminals with a write of this process in flight: one at a time each (REQ-03).
    writing: Mutex<std::collections::HashSet<(String, String)>>,
    /// Screens handed to a pane that has not yet said whether it drew them (REQ-02).
    staged: Mutex<Vec<StagedScreen>>,
    held: Mutex<Option<Held>>,
    older_dir: Option<PathBuf>,
    gate: Option<PathBuf>,
    /// What the host and the Managed authorities last said, shared by every window.
    host_seen: Remembered<HostView>,
    managed_seen: Remembered<Vec<ManagedRow>>,
    inventory_ttl: Duration,
}

fn ready_status(store: &StoreV1) -> StoreStatus {
    StoreStatus::Ready {
        revision: store.revision.get().to_string(),
        host_id: store.host_id.as_str().to_owned(),
        store_id: store.store_id.as_str().to_owned(),
        scratch_workspace_id: store
            .workspaces
            .iter()
            .find(|w| w.project_id.is_none())
            .map(|w| w.id.as_str().to_owned())
            .unwrap_or_default(),
    }
}

/// A screen handed to one reader, bound to everything that has to still be true when it is
/// committed. One use: it is removed by the commit that names it, whatever that commit decides.
struct StagedScreen {
    token: String,
    window: String,
    association: String,
    epoch: String,
    terminal: String,
    /// Where in the host's output the screen ends: reading goes on from here once it is drawn.
    at: u64,
}

/// The most screens kept staged at once, over every window. A pane stages one and commits it
/// at once; an older one beyond this is dropped, and committing it is then refused.
const MAX_STAGED: usize = 64;
/// The most a screen can be on a side, and the most base64 of a repaint a window is given: the
/// bounds the ordinary terminal's attach uses.
const MAX_SCREEN_SIDE: u64 = 512;
const MAX_REPAINT_BASE64: usize = 900 * 1024;

/// What a pane is told to draw has to be something it can draw. Anything else is replayed.
fn screen_is_sound(answer: &serde_json::Value) -> bool {
    let side = |key: &str| {
        answer["size"][key]
            .as_u64()
            .is_some_and(|n| (1..=MAX_SCREEN_SIDE).contains(&n))
    };
    answer["repaint"]
        .as_str()
        .is_some_and(|r| r.len() <= MAX_REPAINT_BASE64)
        && answer["at"].is_u64()
        && side("rows")
        && side("cols")
}

/// Releases a terminal's write slot when the write returns, however it returns.
struct WriteSlot<'a> {
    table: &'a Mutex<std::collections::HashSet<(String, String)>>,
    key: (String, String),
}
impl Drop for WriteSlot<'_> {
    fn drop(&mut self) {
        self.table
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.key);
    }
}

fn failure(code: ErrorCode) -> Failure {
    Failure::new(code)
}

fn owner_code(error: OwnerError) -> ErrorCode {
    match error {
        OwnerError::HostOffline => ErrorCode::HostOffline,
        OwnerError::HostChanged => ErrorCode::HostChanged,
        OwnerError::Unsupported => ErrorCode::UnsupportedCapability,
        OwnerError::ProfileUnavailable => ErrorCode::ProfileUnavailable,
        OwnerError::Refused => ErrorCode::OwnerRefused,
    }
}

impl ProjectsService {
    /// `store_dir` is `…/projects/v1`; the feature gate sits beside it, and the older files
    /// are in the Console's state directory two levels up.
    pub fn new(
        store_dir: Option<PathBuf>,
        owner: Arc<dyn Owner>,
        env: Arc<dyn Env>,
        claims: Claims,
    ) -> Self {
        let gate = store_dir
            .as_ref()
            .and_then(|dir| dir.parent())
            .map(|projects| projects.join("feature"));
        let older_dir = store_dir
            .as_ref()
            .and_then(|dir| dir.parent()?.parent())
            .map(PathBuf::from);
        Self {
            store_dir,
            store: OnceLock::new(),
            owner,
            env,
            selections: Selections::default(),
            disclosures: Disclosures::default(),
            claims,
            writing: Mutex::new(std::collections::HashSet::new()),
            staged: Mutex::new(Vec::new()),
            held: Mutex::new(None),
            older_dir,
            gate,
            host_seen: Remembered::new(),
            managed_seen: Remembered::new(),
            inventory_ttl: INVENTORY_TTL,
        }
    }

    /// The time an owner's answer is shared for, for a test that must see it end.
    #[cfg(test)]
    pub fn with_inventory_ttl(mut self, ttl: Duration) -> Self {
        self.inventory_ttl = ttl;
        self
    }

    /// The store, made private and opened on first use. Callers have passed the gate.
    fn opened(&self) -> Option<&Store> {
        self.store
            .get_or_init(|| self.store_dir.clone().and_then(|dir| Store::open(dir).ok()))
            .as_ref()
    }

    fn engine(&self) -> Option<Engine<'_>> {
        self.opened().map(|store| Engine {
            store,
            owner: &*self.owner,
            env: &*self.env,
            selections: &self.selections,
            disclosures: &self.disclosures,
        })
    }

    fn store(&self) -> Result<&Store, Failure> {
        self.opened()
            .ok_or_else(|| failure(ErrorCode::IoFailure).with_field("store"))
    }

    // ---- the release gate ---------------------------------------------------------------------

    /// Whether the project-centered workspace is adopted. It is off until the owner turns it on,
    /// and turning it off returns to the older windows without touching any session.
    pub fn enabled(&self) -> bool {
        self.gate
            .as_ref()
            .and_then(|path| crate::native::store::read_bounded(path, 16))
            .is_some_and(|bytes| bytes == b"enabled")
    }

    pub fn set_enabled(&self, on: bool) -> Result<(), Failure> {
        let path = self
            .gate
            .as_ref()
            .ok_or_else(|| failure(ErrorCode::IoFailure))?;
        let dir = path.parent().ok_or_else(|| failure(ErrorCode::IoFailure))?;
        crate::native::store::private_dir(dir).map_err(|_| failure(ErrorCode::IoFailure))?;
        if on {
            crate::native::store::write_atomic(path, b"enabled")
                .map_err(|_| failure(ErrorCode::IoFailure))
        } else {
            match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(_) => Err(failure(ErrorCode::IoFailure)),
            }
        }
    }

    fn gated(&self) -> Result<(), Failure> {
        if self.enabled() {
            Ok(())
        } else {
            Err(failure(ErrorCode::UnsupportedCapability).with_field("disabled"))
        }
    }

    /// The folder dialog is only opened for a window that is allowed to use the result.
    pub fn gated_for_pick(&self) -> Result<(), Failure> {
        self.gated()
    }

    // ---- reading ----------------------------------------------------------------------------

    fn loaded(&self) -> Result<Loaded, Failure> {
        self.store()?
            .read()
            .map_err(|_| failure(ErrorCode::IoFailure))
    }

    /// The store for display. A version already decoded is shared between views.
    fn ready(&self) -> Result<Arc<StoreV1>, Failure> {
        match self.shared()? {
            Shared::Ready(store) => Ok(store),
            Shared::Missing => Err(failure(ErrorCode::InvalidRecord).with_field("store")),
            Shared::Unusable(unusable) => Err(store_failure(StoreError::Unusable(unusable))),
        }
    }

    fn shared(&self) -> Result<Shared, Failure> {
        self.gated()?;
        self.store()?
            .read_shared()
            .map_err(|_| failure(ErrorCode::IoFailure))
    }

    /// The host as it is now, for what decides or does something.
    fn host_view(&self) -> Result<HostView, OwnerError> {
        self.owner.host(None)
    }

    /// The host as it was a moment ago, for what is only shown. One read serves every window.
    fn host_shown(&self) -> Result<HostView, OwnerError> {
        self.host_seen
            .get(self.inventory_ttl, || self.owner.host(None))
    }

    /// The Managed sessions as they were a moment ago, for what is only shown.
    fn managed_shown(&self) -> Result<Vec<ManagedRow>, OwnerError> {
        self.managed_seen
            .get(self.inventory_ttl, || self.owner.managed())
    }

    /// Whatever an effect may have changed in the owners is read again the next time.
    fn forget_inventory(&self) {
        self.host_seen.forget();
        self.managed_seen.forget();
    }

    pub fn overview(&self) -> Overview {
        // With the workspace off nothing is read, made or asked: no directory, no store, no host.
        if !self.enabled() {
            return Overview {
                enabled: false,
                store: StoreStatus::Missing,
                host: HostStatus {
                    reachable: false,
                    epoch: None,
                    features: Vec::new(),
                    reason: Some("off"),
                },
            };
        }
        let store = match self.opened() {
            None => StoreStatus::Unavailable,
            Some(_) => match self.shared() {
                Ok(Shared::Ready(store)) => ready_status(&store),
                Ok(Shared::Missing) => StoreStatus::Missing,
                Ok(Shared::Unusable(u)) => StoreStatus::Unusable {
                    code: store_failure(StoreError::Unusable(u)).code,
                    field: u.field,
                },
                Err(_) => StoreStatus::Unavailable,
            },
        };
        let host = match self.host_shown() {
            Ok(view) => HostStatus {
                reachable: true,
                epoch: Some(view.epoch),
                features: view.features,
                reason: None,
            },
            Err(error) => HostStatus {
                reachable: false,
                epoch: None,
                features: Vec::new(),
                reason: Some(match error {
                    OwnerError::HostOffline => "host_offline",
                    OwnerError::HostChanged => "host_changed",
                    _ => "host_unsupported",
                }),
            },
        };
        Overview {
            enabled: self.enabled(),
            store,
            host,
        }
    }

    fn root_view(&self, binding: &Binding) -> RootView {
        let (availability, native, config) = match self.owner.root(binding.path.as_str()) {
            Ok(root) => (
                if root.device == binding.observation.device.get()
                    && root.inode == binding.observation.inode.get()
                {
                    "available"
                } else {
                    "identity_changed"
                },
                root.native_eligible,
                root.config_eligible,
            ),
            Err(RootError::Missing) => ("missing", false, false),
            Err(RootError::Ineligible) => ("ineligible", false, false),
            Err(RootError::Shared) => ("shared", false, false),
        };
        RootView {
            binding_id: binding.id.as_str().to_owned(),
            binding_revision: binding.revision.get().to_string(),
            label: binding
                .path
                .as_str()
                .rsplit('/')
                .find(|s| !s.is_empty())
                .unwrap_or("/")
                .to_owned(),
            state: binding.state,
            availability,
            native_eligible: native,
            config_eligible: config,
        }
    }

    fn project_view(&self, store: &StoreV1, project: &Project) -> ProjectView {
        let binding = project
            .default_binding_id
            .as_ref()
            .and_then(|id| store.bindings.iter().find(|b| &b.id == id));
        ProjectView {
            id: project.id.as_str().to_owned(),
            label: project.label.as_str().to_owned(),
            archived: project.archived,
            tombstone: project.tombstone,
            revision: project.revision.get().to_string(),
            default_workspace_id: project
                .default_workspace_id
                .as_ref()
                .map(|w| w.as_str().to_owned()),
            root: binding.map(|b| self.root_view(b)),
            workspace_ids: store
                .workspaces
                .iter()
                .filter(|w| w.project_id.as_ref() == Some(&project.id))
                .map(|w| w.id.as_str().to_owned())
                .collect(),
            sessions: store
                .sessions
                .iter()
                .filter(|a| a.origin_project_id.as_ref() == Some(&project.id))
                .count(),
        }
    }

    fn page_of<T>(
        &self,
        store: &StoreV1,
        mut rows: Vec<T>,
        cursor: Option<&str>,
    ) -> Result<Page<T>, Failure> {
        let start = match cursor {
            None => 0,
            Some(text) => {
                let (revision, offset) = text
                    .split_once(':')
                    .ok_or_else(|| failure(ErrorCode::InvalidRecord))?;
                // A cursor belongs to the revision it was issued at: another one is a refetch.
                if revision != store.revision.get().to_string() {
                    return Err(failure(ErrorCode::RevisionConflict));
                }
                offset
                    .parse::<usize>()
                    .map_err(|_| failure(ErrorCode::InvalidRecord))?
            }
        };
        let total = rows.len();
        let items: Vec<T> = rows.drain(..).skip(start).take(PAGE).collect();
        let next = (start + items.len() < total)
            .then(|| format!("{}:{}", store.revision.get(), start + items.len()));
        Ok(Page {
            items,
            cursor: next,
            store_revision: store.revision.get().to_string(),
        })
    }

    pub fn list_projects(&self, cursor: Option<&str>) -> Result<Page<ProjectView>, Failure> {
        let store = self.ready()?;
        let rows = store
            .projects
            .iter()
            .map(|p| self.project_view(&store, p))
            .collect();
        self.page_of(&store, rows, cursor)
    }

    pub fn get_project(&self, id: &str) -> Result<ProjectView, Failure> {
        let store = self.ready()?;
        store
            .projects
            .iter()
            .find(|p| p.id.as_str() == id)
            .map(|p| self.project_view(&store, p))
            .ok_or_else(|| failure(ErrorCode::ContextStale))
    }

    fn session_view(
        &self,
        association: &Association,
        host: &Result<HostView, OwnerError>,
        managed: &Result<Vec<ManagedRow>, OwnerError>,
        window: Option<&str>,
    ) -> SessionView {
        let mut view = SessionView {
            id: association.id.as_str().to_owned(),
            origin_project_id: association
                .origin_project_id
                .as_ref()
                .map(|p| p.as_str().to_owned()),
            state: association.state,
            kind: "",
            availability: "unknown",
            reason: None,
            native_session_id: None,
            terminal_id: None,
            protocol: None,
            store_stream_id: None,
            managed_session_id: None,
            requested_safety_profile: None,
            effective_safety_profile: None,
            profile_mismatch: false,
            operation_id: association
                .operation_id
                .as_ref()
                .map(|o| o.as_str().to_owned()),
            input: "none",
        };
        match &association.target {
            Target::Native {
                session_id,
                terminal_id,
                host_epoch,
            } => {
                view.kind = "native";
                view.native_session_id = Some(session_id.as_str().to_owned());
                self.terminal_state(
                    &mut view,
                    terminal_id.as_str(),
                    host_epoch.as_str(),
                    host,
                    window,
                );
            }
            Target::Shell {
                terminal_id,
                host_epoch,
            } => {
                view.kind = "shell";
                self.terminal_state(
                    &mut view,
                    terminal_id.as_str(),
                    host_epoch.as_str(),
                    host,
                    window,
                );
            }
            Target::Managed {
                protocol,
                store_stream_id,
                session_id,
                requested_safety_profile,
            } => {
                view.kind = "managed";
                view.protocol = Some(match protocol {
                    ManagedProtocol::P1 => 1,
                    ManagedProtocol::P2 => 2,
                });
                view.store_stream_id = Some(store_stream_id.as_str().to_owned());
                view.managed_session_id = Some(session_id.as_str().to_owned());
                view.requested_safety_profile = *requested_safety_profile;
                match managed {
                    Err(_) => view.reason = Some("owner_offline"),
                    Ok(rows) => match rows.iter().find(|r| {
                        r.protocol == *protocol
                            && r.store_stream_id == store_stream_id.as_str()
                            && r.session_id == session_id.as_str()
                    }) {
                        None => view.reason = Some("not_listed"),
                        Some(row) => {
                            view.availability = "live";
                            view.effective_safety_profile = row.effective_profile;
                            // A request is only a request: the authority's own answer decides.
                            view.profile_mismatch = requested_safety_profile.is_some()
                                && row.effective_profile != *requested_safety_profile;
                        }
                    },
                }
            }
        }
        view
    }

    fn terminal_state(
        &self,
        view: &mut SessionView,
        terminal: &str,
        epoch: &str,
        host: &Result<HostView, OwnerError>,
        window: Option<&str>,
    ) {
        view.terminal_id = Some(terminal.to_owned());
        match host {
            Err(error) => {
                view.reason = Some(match error {
                    OwnerError::HostChanged => "host_changed",
                    _ => "host_offline",
                });
            }
            Ok(host) if host.epoch != epoch => {
                // The terminal belonged to a host that is gone; its shell ended with it.
                view.availability = "ended";
                view.reason = Some("host_changed");
            }
            Ok(host) => {
                let row = host.terminals.iter().find(|t| t.terminal_id == terminal);
                view.availability = if row.is_some_and(|t| t.running) {
                    "live"
                } else {
                    "ended"
                };
                if view.availability == "live" {
                    view.input = match self.claims.holder(epoch, terminal) {
                        Some(holder) if Some(holder.as_str()) == window => "this",
                        Some(_) => "other",
                        None => "none",
                    };
                }
            }
        }
    }

    pub fn get_workspace(&self, id: &str, window: Option<&str>) -> Result<WorkspaceView, Failure> {
        let store = self.ready()?;
        let workspace = store
            .workspaces
            .iter()
            .find(|w| w.id.as_str() == id)
            .ok_or_else(|| failure(ErrorCode::ContextStale))?;
        let host = self.host_shown();
        let managed = self.managed_shown();
        Ok(self.workspace_view(&store, workspace, &host, &managed, window))
    }

    fn workspace_view(
        &self,
        store: &StoreV1,
        workspace: &Workspace,
        host: &Result<HostView, OwnerError>,
        managed: &Result<Vec<ManagedRow>, OwnerError>,
        window: Option<&str>,
    ) -> WorkspaceView {
        WorkspaceView {
            id: workspace.id.as_str().to_owned(),
            project_id: workspace.project_id.as_ref().map(|p| p.as_str().to_owned()),
            label: workspace.label.as_str().to_owned(),
            revision: workspace.revision.get().to_string(),
            pinned: workspace.pinned,
            layout: workspace.layout.clone(),
            focus: workspace.focus.as_ref().map(|f| f.as_str().to_owned()),
            leaves: workspace
                .leaves
                .iter()
                .map(|leaf| LeafView {
                    id: leaf.id.as_str().to_owned(),
                    title: leaf.title.as_str().to_owned(),
                    session: leaf
                        .association_id
                        .as_ref()
                        .and_then(|id| store.sessions.iter().find(|a| &a.id == id))
                        .map(|a| self.session_view(a, host, managed, window)),
                })
                .collect(),
            preferences: workspace.preferences.clone(),
            unfinished: store
                .operations
                .iter()
                .filter(|o| {
                    o.workspace_id == workspace.id
                        && matches!(
                            o.state,
                            wyss_console::contracts::m1::records::OperationState::Pending
                                | wyss_console::contracts::m1::records::OperationState::Ambiguous
                        )
                })
                .map(|o| UnfinishedView {
                    operation_id: o.id.as_str().to_owned(),
                    leaf_id: o.leaf_id.as_str().to_owned(),
                    state: match o.state {
                        wyss_console::contracts::m1::records::OperationState::Pending => "pending",
                        _ => "ambiguous",
                    },
                    mode: match o.mode {
                        wyss_console::contracts::m1::records::Mode::Native => "native",
                        wyss_console::contracts::m1::records::Mode::Managed => "managed",
                    },
                })
                .collect(),
        }
    }

    /// Every session the store knows, newest first, optionally for one project (`Some(None)` is
    /// the unassigned ones). Detached, unavailable and unassigned history is all here.
    pub fn list_sessions(
        &self,
        project: Option<Option<&str>>,
        cursor: Option<&str>,
        window: Option<&str>,
    ) -> Result<Page<SessionView>, Failure> {
        let store = self.ready()?;
        let host = self.host_shown();
        let managed = self.managed_shown();
        let rows = store
            .sessions
            .iter()
            .rev()
            .filter(|a| match project {
                None => true,
                Some(wanted) => a.origin_project_id.as_ref().map(|p| p.as_str()) == wanted,
            })
            .map(|a| self.session_view(a, &host, &managed, window))
            .collect();
        self.page_of(&store, rows, cursor)
    }

    // ---- choosing a folder --------------------------------------------------------------------

    /// Records a folder the native dialog returned and hands the window a one-use token for it.
    pub fn issue_pick(
        &self,
        path: &str,
        purpose: Purpose,
        selection_revision: u64,
    ) -> Result<PickView, Failure> {
        let root = self.owner.root(path).map_err(|e| match e {
            RootError::Missing => failure(ErrorCode::RootMissing),
            RootError::Ineligible => failure(ErrorCode::RootIneligible),
            RootError::Shared => failure(ErrorCode::RootIneligible).with_field("foreignAcl"),
        })?;
        let token = format!("pick-{}", self.env.uuid());
        self.selections.issue(
            token.clone(),
            root.path.clone(),
            purpose,
            selection_revision,
        );
        Ok(PickView {
            token,
            label: root
                .path
                .rsplit('/')
                .find(|s| !s.is_empty())
                .unwrap_or("/")
                .to_owned(),
            native_eligible: root.native_eligible,
            config_eligible: root.config_eligible,
        })
    }

    /// The window switched project or workspace: picks made before are no longer usable, and
    /// neither is a directory shown for an adoption.
    pub fn forget_picks(&self) {
        self.selections.clear();
        self.disclosures.clear();
    }

    /// Where a Scratch shell really is, shown before the user is asked to move it into a project.
    /// The answer is good for that one adoption only.
    pub fn preview_adoption(
        &self,
        association_id: &str,
        project_id: &str,
    ) -> Result<AdoptionView, Failure> {
        self.gated()?;
        let parse = |field: &'static str| failure(ErrorCode::InvalidRecord).with_field(field);
        let association =
            AssociationId::parse(association_id).map_err(|_| parse("associationId"))?;
        let project = ProjectId::parse(project_id).map_err(|_| parse("projectId"))?;
        let engine = self.engine().ok_or_else(|| failure(ErrorCode::IoFailure))?;
        let shown = engine.disclose_adoption(&association, &project)?;
        Ok(AdoptionView {
            binding_id: shown.binding_id.as_str().to_owned(),
            cwd: shown.cwd,
            inside: shown.inside,
        })
    }

    // ---- commands -----------------------------------------------------------------------------

    pub fn execute(&self, envelope: Envelope) -> Result<Receipt, Failure> {
        self.gated()?;
        let engine = self.engine().ok_or_else(|| failure(ErrorCode::IoFailure))?;
        let done = engine.execute(envelope);
        // A launch or a settling changed what the owners hold, whatever the answer was.
        self.forget_inventory();
        done
    }

    /// Creates the empty store (Scratch only) when none exists. An explicit act.
    pub fn initialize_empty(&self) -> Result<StoreV1, Failure> {
        self.gated()?;
        let store = self.store()?;
        let initial = empty_store(&*self.env)?;
        store.create(initial).map_err(|e| match e {
            StoreError::StoreState(_) => failure(ErrorCode::RevisionConflict).with_field("store"),
            other => store_failure(other),
        })
    }

    /// Makes `pending` receipts of a gone process `ambiguous`. Called when the app starts;
    /// never replays a launch.
    pub fn recover(&self) -> usize {
        if !self.enabled() {
            return 0;
        }
        self.engine()
            .and_then(|engine| engine.recover_pending().ok())
            .unwrap_or(0)
    }

    pub fn compact_receipts(&self) -> Result<usize, Failure> {
        self.gated()?;
        self.engine()
            .ok_or_else(|| failure(ErrorCode::IoFailure))?
            .compact_receipts()
    }

    // ---- migration ----------------------------------------------------------------------------

    fn sources(&self) -> Result<Sources, Failure> {
        let dir = self
            .older_dir
            .as_ref()
            .ok_or_else(|| failure(ErrorCode::IoFailure))?;
        migration::read_sources(dir).map_err(|(kind, _)| {
            failure(ErrorCode::InvalidRecord).with_field(match kind {
                migration::ConflictKind::SourceOversized => "oversized",
                migration::ConflictKind::SourceUnstable => "unstable",
                _ => "unreadable",
            })
        })
    }

    /// Reads the older files and the host and offers a plan. Nothing is written or started.
    pub fn preview_migration(&self) -> Result<Summary, Failure> {
        self.gated()?;
        let sources = self.sources()?;
        let host = self.host_view().ok();
        let id = self.env.uuid();
        let existing = match self.loaded()? {
            Loaded::Ready(store) => Some(*store),
            Loaded::Missing => None,
            Loaded::Unusable(u) => return Err(store_failure(StoreError::Unusable(u))),
        };
        let (summary, plan) = match existing {
            None => {
                let plan = migration::plan_initial(
                    &*self.env,
                    &*self.owner,
                    &sources,
                    host.as_ref(),
                    &id,
                )?;
                (plan.summary.clone(), Ok(plan))
            }
            Some(existing) => {
                let plan = migration::plan_incremental(
                    &*self.env,
                    &*self.owner,
                    &sources,
                    host.as_ref(),
                    &existing,
                    &id,
                )?;
                (plan.summary.clone(), Err(plan))
            }
        };
        *self.held.lock().unwrap_or_else(PoisonError::into_inner) = Some(Held { plan, sources });
        Ok(summary)
    }

    /// Commits exactly the preview that was shown: the older files must still have the bytes it
    /// was made from, and the host must be the one it read.
    pub fn commit_migration(
        &self,
        preview_id: &str,
        accept_aliases: bool,
    ) -> Result<StoreStatus, Failure> {
        self.gated()?;
        let held = self
            .held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .ok_or_else(|| failure(ErrorCode::ContextStale))?;
        let shown = match &held.plan {
            Ok(plan) => &plan.summary,
            Err(plan) => &plan.summary,
        };
        if shown.preview_id != preview_id || (shown.needs_alias_acceptance && !accept_aliases) {
            *self.held.lock().unwrap_or_else(PoisonError::into_inner) = Some(held);
            return Err(failure(ErrorCode::ContextStale).with_field("preview"));
        }
        let now = self.sources()?;
        let digest = |bytes: &Option<Vec<u8>>| bytes.as_ref().map(|b| migration::sha256(b));
        if digest(&now.registry) != digest(&held.sources.registry)
            || digest(&now.layout) != digest(&held.sources.layout)
        {
            return Err(failure(ErrorCode::ContextStale).with_field("sources_changed"));
        }
        let epoch = match &held.plan {
            Ok(plan) => plan.epoch.clone(),
            Err(plan) => plan.epoch.clone(),
        };
        if let Some(epoch) = epoch
            && self.owner.host(Some(&epoch)).is_err()
        {
            return Err(failure(ErrorCode::HostChanged));
        }
        let store = self.store()?;
        let mut shown_ok = shown.clone();
        shown_ok.needs_alias_acceptance = false;
        let committed = match held.plan {
            Ok(mut plan) => {
                plan.summary.can_commit = plan.summary.can_commit
                    || accept_aliases
                        && !plan
                            .summary
                            .conflicts
                            .iter()
                            .any(|c| c.kind.blocks_commit());
                migration::commit_initial(store, plan)?
            }
            Err(mut plan) => {
                plan.summary.can_commit = plan.summary.can_commit
                    || accept_aliases
                        && !plan
                            .summary
                            .conflicts
                            .iter()
                            .any(|c| c.kind.blocks_commit());
                // At the revision the preview was made against: a store that moved on is a
                // conflict and a new preview, never additions worked out for another state.
                let expected = plan.revision;
                migration::commit_incremental(store, expected, &plan)?
            }
        };
        Ok(ready_status(&committed))
    }

    // ---- the terminal input claim -------------------------------------------------------------

    fn terminal_of(&self, association: &str) -> Result<(String, String), Failure> {
        let store = self.ready()?;
        let found = store
            .sessions
            .iter()
            .find(|a| a.id.as_str() == association)
            .ok_or_else(|| failure(ErrorCode::ContextStale))?;
        match &found.target {
            Target::Native {
                terminal_id,
                host_epoch,
                ..
            }
            | Target::Shell {
                terminal_id,
                host_epoch,
            } => Ok((
                host_epoch.as_str().to_owned(),
                terminal_id.as_str().to_owned(),
            )),
            Target::Managed { .. } => {
                Err(failure(ErrorCode::UnsupportedCapability).with_field("managed"))
            }
        }
    }

    /// Takes input for `window`. Explicit; refused if another window or Console holds it, or if
    /// the host is not the one the session lives in.
    pub fn claim_input(&self, window: &str, association: &str) -> Result<(), Failure> {
        self.gated()?;
        let (epoch, terminal) = self.terminal_of(association)?;
        self.owner
            .host(Some(&epoch))
            .map_err(|e| failure(owner_code(e)))?;
        self.claims
            .claim(window, &epoch, &terminal)
            .map_err(|e| match e {
                ClaimError::HeldByOther => {
                    failure(ErrorCode::OwnerRefused).with_field("input_claim")
                }
                ClaimError::Unavailable => failure(ErrorCode::IoFailure).with_field("input_claim"),
            })
    }

    pub fn release_input(&self, window: &str, association: &str) {
        if let Ok((epoch, terminal)) = self.terminal_of(association) {
            self.claims.release(window, &epoch, &terminal);
        }
    }

    /// A window blurred, closed or crashed.
    pub fn release_window(&self, window: &str) {
        self.claims.release_window(window);
    }

    /// Output is readable by every view; it never needs the claim.
    pub fn read_output(
        &self,
        association: &str,
        from: u64,
        wait_ms: u64,
    ) -> Result<TermRead, Failure> {
        self.gated()?;
        let (epoch, terminal) = self.terminal_of(association)?;
        self.owner
            .term_read(&epoch, &terminal, from, wait_ms)
            .map_err(|e| failure(owner_code(e)))
    }

    /// Input and resize need the claim: a mirror view is read-only.
    ///
    /// An error means nothing was written: the request was refused here, never left, or the
    /// host refused it before writing. Once the host admitted it the answer is a
    /// `WriteOutcome`, which says how much is known to have been written and never guesses.
    /// One write per terminal is in flight at a time; a second is refused before it is sent.
    pub fn write_input(
        &self,
        window: &str,
        association: &str,
        data: &[u8],
    ) -> Result<WriteOutcome, Failure> {
        self.gated()?;
        // A keystroke or a reply of the terminal, or one piece of a paste: the host takes no more.
        if data.is_empty() || data.len() > MAX_WRITE {
            return Err(failure(ErrorCode::InvalidRecord).with_field("data"));
        }
        let (epoch, terminal) = self.terminal_of(association)?;
        if !self.claims.holds(window, &epoch, &terminal) {
            return Err(failure(ErrorCode::OwnerRefused).with_field("input_claim"));
        }
        let key = (epoch.clone(), terminal.clone());
        if !self
            .writing
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.clone())
        {
            return Err(failure(ErrorCode::OwnerRefused).with_field("input_in_flight"));
        }
        let _slot = WriteSlot {
            table: &self.writing,
            key,
        };
        self.owner
            .term_write(&epoch, &terminal, data)
            .map_err(|e| failure(owner_code(e)))
    }

    // ---- screen recovery (REQ-02) ------------------------------------------------------------

    /// The screen a pane draws before it reads on, for a session that was already running.
    ///
    /// The association is resolved here to its recorded host epoch and terminal, and the
    /// snapshot is asked through the checked owner: a different host is `HostChanged` and
    /// nothing is asked of it. A usable screen is staged for this window and association and
    /// answered with a one-use token; the read position is the pane's and moves only when it
    /// commits. Without a usable screen the answer is `replay` with the reason, and the pane
    /// reads what the host kept from its start, labelled as such.
    pub fn attach_screen(
        &self,
        window: &str,
        association: &str,
    ) -> Result<serde_json::Value, Failure> {
        self.gated()?;
        let (epoch, terminal) = self.terminal_of(association)?;
        let screen = self
            .owner
            .term_screen(&epoch, &terminal)
            .map_err(|e| failure(owner_code(e)))?;
        let mut staged = self.staged.lock().unwrap_or_else(PoisonError::into_inner);
        // A new attach of the same reader replaces what it had staged: the old token is dead.
        staged.retain(|s| !(s.window == window && s.association == association));
        let answer = match screen {
            TermScreen::Screen(answer) if screen_is_sound(&answer) => answer,
            TermScreen::Screen(answer) => {
                return Ok(serde_json::json!({
                    "mode": "replay", "reason": "invalid_screen", "parser": answer["parser"],
                }));
            }
            TermScreen::Replay(reason) => {
                return Ok(serde_json::json!({
                    "mode": "replay", "reason": reason, "parser": null,
                }));
            }
        };
        let token = uuid::Uuid::new_v4().to_string();
        if staged.len() >= MAX_STAGED {
            staged.remove(0);
        }
        staged.push(StagedScreen {
            token: token.clone(),
            window: window.to_owned(),
            association: association.to_owned(),
            epoch,
            terminal,
            at: answer["at"].as_u64().unwrap_or(0),
        });
        let mut out = answer;
        out["mode"] = serde_json::json!("screen");
        out["token"] = serde_json::json!(token);
        Ok(out)
    }

    /// The second half: the pane says whether it drew the staged screen, and is told where to
    /// read from. Drawn: where the screen ends. Not drawn: the start of what the host kept, so
    /// a screen that could not be drawn loses no output.
    ///
    /// The token is used up by this call. It is honoured only for the window and association
    /// it was staged for, while the association still names the same terminal in the same host
    /// epoch and that host is still the one answering. Anything else is `context_stale` and
    /// moves nothing: the pane must not read on over a screen the backend does not vouch for.
    pub fn commit_screen(
        &self,
        window: &str,
        association: &str,
        token: Option<&str>,
        drawn: bool,
    ) -> Result<u64, Failure> {
        self.gated()?;
        let stale = || failure(ErrorCode::ContextStale).with_field("attach_token");
        let Some(token) = token else {
            // The answer could not be read at all: whatever was staged for this reader is
            // dropped, and nothing can have been drawn from it.
            if drawn {
                return Err(failure(ErrorCode::InvalidRecord).with_field("attach_token"));
            }
            self.staged
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retain(|s| !(s.window == window && s.association == association));
            return Ok(0);
        };
        let staged = {
            let mut all = self.staged.lock().unwrap_or_else(PoisonError::into_inner);
            let at = all
                .iter()
                .position(|s| s.token == token)
                .ok_or_else(stale)?;
            all.remove(at)
        };
        if staged.window != window || staged.association != association {
            return Err(stale());
        }
        let (epoch, terminal) = self.terminal_of(association)?;
        if epoch != staged.epoch || terminal != staged.terminal {
            return Err(stale());
        }
        // The host that made the screen must be the one whose output is read from here on.
        self.owner
            .host(Some(&epoch))
            .map_err(|e| failure(owner_code(e)))?;
        Ok(if drawn { staged.at } else { 0 })
    }

    pub fn resize(
        &self,
        window: &str,
        association: &str,
        rows: u16,
        cols: u16,
    ) -> Result<(), Failure> {
        self.gated()?;
        let (epoch, terminal) = self.terminal_of(association)?;
        if !self.claims.holds(window, &epoch, &terminal) {
            return Err(failure(ErrorCode::OwnerRefused).with_field("input_claim"));
        }
        self.owner
            .term_resize(&epoch, &terminal, rows, cols)
            .map_err(|e| failure(owner_code(e)))
    }

    // ---- rollback -----------------------------------------------------------------------------

    /// The running terminals no association holds that a launch of unknown outcome could have
    /// started, for the user to pick from. A read: nothing is attached or started by it.
    pub fn orphans(&self, launch_operation_id: &str) -> Result<Vec<OrphanView>, Failure> {
        let store = self.ready()?;
        let operation = store
            .operations
            .iter()
            .find(|o| o.id.as_str() == launch_operation_id)
            .ok_or_else(|| failure(ErrorCode::ContextStale))?;
        if operation.mode != wyss_console::contracts::m1::records::Mode::Native {
            return Err(failure(ErrorCode::UnsupportedCapability).with_field("mode"));
        }
        let shell =
            operation.provider == wyss_console::contracts::m1::records::LaunchProvider::Shell;
        let held: std::collections::HashSet<&str> = store
            .sessions
            .iter()
            .filter_map(|a| match &a.target {
                Target::Native { terminal_id, .. } | Target::Shell { terminal_id, .. } => {
                    Some(terminal_id.as_str())
                }
                Target::Managed { .. } => None,
            })
            .collect();
        let view = self.host_view().map_err(|e| failure(owner_code(e)))?;
        Ok(view
            .terminals
            .iter()
            .filter(|t| {
                t.running
                    && !held.contains(t.terminal_id.as_str())
                    && shell == t.native_session.is_none()
            })
            .map(|t| OrphanView {
                terminal_id: t.terminal_id.clone(),
                native_session_id: t.native_session.clone(),
                folder: t
                    .cwd
                    .as_deref()
                    .and_then(|c| c.rsplit('/').find(|part| !part.is_empty()))
                    .map(str::to_owned),
            })
            .collect())
    }

    /// What a return to the older windows needs to account for: unfinished launches block it,
    /// and everything the new store holds that the older state does not is counted. The report
    /// is written privately beside the store; nothing is restored and no session is touched.
    pub fn rollback_report(&self) -> Result<RollbackReport, Failure> {
        let store = self.ready()?;
        let unfinished = store
            .operations
            .iter()
            .filter(|o| {
                matches!(
                    o.state,
                    wyss_console::contracts::m1::records::OperationState::Pending
                        | wyss_console::contracts::m1::records::OperationState::Ambiguous
                )
            })
            .count();
        let mapped = store.migration.as_ref().map_or(0, |m| m.map.len());
        let report = RollbackReport {
            can_disable: unfinished == 0,
            unfinished_launches: unfinished,
            projects: store.projects.len(),
            workspaces: store.workspaces.len(),
            sessions: store.sessions.len(),
            mapped_older_records: mapped,
            note: "Disabling returns to the older windows. The store, its tombstones and the migration snapshot are kept for forward recovery; no older file is restored over a live writer and no session is stopped. The older window can recreate missing shells by its own behaviour.",
        };
        if let Some(store) = self.opened() {
            let path = store.dir().join("rollback-report.json");
            let bytes =
                serde_json::to_vec_pretty(&report).map_err(|_| failure(ErrorCode::IoFailure))?;
            let _ = crate::native::store::write_atomic(&path, &bytes);
        }
        Ok(report)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackReport {
    pub can_disable: bool,
    pub unfinished_launches: usize,
    pub projects: usize,
    pub workspaces: usize,
    pub sessions: usize,
    pub mapped_older_records: usize,
    pub note: &'static str,
}

#[cfg(test)]
mod remembered_tests {
    use super::*;

    #[test]
    fn forgetting_never_waits_for_a_read_and_what_was_read_meanwhile_is_not_reused() {
        let cache = Arc::new(Remembered::<u32>::new());
        let (started, running) = std::sync::mpsc::channel();
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let reader = {
            let cache = cache.clone();
            std::thread::spawn(move || {
                cache.get(Duration::from_secs(60), || {
                    started.send(()).unwrap();
                    gate.recv().unwrap();
                    Ok(1)
                })
            })
        };
        running.recv().unwrap();
        // The read is in flight and holds the lock: forgetting must still return at once.
        cache.forget();
        release.send(()).unwrap();
        assert_eq!(reader.join().unwrap(), Ok(1));
        // What began before the forgetting is not served again; what is read after it is.
        assert_eq!(cache.get(Duration::from_secs(60), || Ok(2)), Ok(2));
        assert_eq!(cache.get(Duration::from_secs(60), || Ok(3)), Ok(2));
    }
}
