//! The closed mutation envelope of the M1 store (contract §3), its results and its errors.
//!
//! The window sends commands; it never sends a path, an argument vector, a prompt or a
//! credential. Folders arrive as one-use selection tokens the backend issued for a native
//! dialog, and every ID is one the backend gave out. Unknown fields and unknown variants are
//! refused when the envelope is read.
use serde::{Deserialize, Serialize};
use wyss_console::contracts::m1::{
    records::{LaunchProvider, Leaf, Mode, Node, Preferences},
    scalar::{
        AssociationId, BindingId, Counter, HostId, LeafId, OperationId, ProfileVersion,
        ProjectId, Text, WorkspaceId,
    },
};

/// What a window knew when it sent the command. The backend validates every parent
/// relationship against it and keeps it, unchanged, with the receipt: a switch of project in
/// the window afterwards never retargets work already confirmed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    pub host_id: HostId,
    pub project_id: Option<ProjectId>,
    pub workspace_id: Option<WorkspaceId>,
    pub binding_id: Option<BindingId>,
    pub binding_revision: Option<Counter>,
    /// Per window, not an execution identity: it only invalidates unconfirmed plans.
    pub selection_revision: Counter,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Envelope {
    pub schema: SchemaOne,
    pub operation_id: OperationId,
    pub expected_store_revision: Counter,
    pub context: Context,
    pub command: Command,
}

/// `schema: 1`, the only version this backend reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaOne;

impl<'de> Deserialize<'de> for SchemaOne {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match u8::deserialize(deserializer)? {
            1 => Ok(Self),
            _ => Err(serde::de::Error::custom("schema")),
        }
    }
}

/// A one-use token for a folder the user picked in a native dialog this backend opened.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SelectionToken(pub String);

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Command {
    #[serde(rename_all = "camelCase")]
    RegisterExisting { selection_token: SelectionToken },
    #[serde(rename_all = "camelCase")]
    CreateFolder {
        selection_token: SelectionToken,
        name: String,
    },
    #[serde(rename_all = "camelCase")]
    Relink {
        project_id: ProjectId,
        old_binding_id: BindingId,
        selection_token: SelectionToken,
    },
    #[serde(rename_all = "camelCase")]
    RenameProject { project_id: ProjectId, label: Text },
    #[serde(rename_all = "camelCase")]
    ArchiveProject {
        project_id: ProjectId,
        archived: bool,
    },
    #[serde(rename_all = "camelCase")]
    RemoveProject {
        project_id: ProjectId,
        retain_history: bool,
    },
    #[serde(rename_all = "camelCase")]
    CreateWorkspace { project_id: ProjectId, label: Text },
    #[serde(rename_all = "camelCase")]
    DuplicateWorkspace {
        source_workspace_id: WorkspaceId,
        label: Text,
    },
    #[serde(rename_all = "camelCase")]
    SaveWorkspace(Box<SaveWorkspace>),
    #[serde(rename_all = "camelCase")]
    CloseWorkspace { workspace_id: WorkspaceId },
    #[serde(rename_all = "camelCase")]
    DeleteWorkspace { workspace_id: WorkspaceId },
    #[serde(rename_all = "camelCase")]
    Attach {
        workspace_id: WorkspaceId,
        leaf_id: LeafId,
        association_id: AssociationId,
    },
    #[serde(rename_all = "camelCase")]
    Detach {
        workspace_id: WorkspaceId,
        leaf_id: LeafId,
        association_id: AssociationId,
    },
    #[serde(rename_all = "camelCase")]
    AdoptScratch {
        association_id: AssociationId,
        project_id: ProjectId,
        binding_id: BindingId,
    },
    #[serde(rename_all = "camelCase")]
    Launch(Box<Launch>),
    /// The user's decision on a launch whose outcome is unknown, checked against the owner.
    #[serde(rename_all = "camelCase")]
    ReconcileLaunch {
        launch_operation_id: OperationId,
        resolution: Resolution,
    },
}

/// How a launch of unknown outcome ends. Neither choice starts anything.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Resolution {
    /// The user found the session the launch started, among those no association holds.
    #[serde(rename_all = "camelCase")]
    Identified {
        terminal_id: wyss_console::contracts::m1::scalar::TerminalId,
        session_id: Option<wyss_console::contracts::m1::scalar::NativeId>,
    },
    /// The owner shows nothing the launch could have started, and the user confirms it.
    Absent,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveWorkspace {
    pub workspace_id: WorkspaceId,
    pub expected_workspace_revision: Counter,
    pub label: Text,
    pub layout: Option<Node>,
    pub focus: Option<LeafId>,
    pub pinned: bool,
    pub leaves: Vec<Leaf>,
    pub preferences: Preferences,
}

/// A launch names a provider and a mode, a Native profile or a Managed safety profile, and
/// nothing else: no argument vector, environment, prompt or path.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Launch {
    pub workspace_id: WorkspaceId,
    pub leaf_id: LeafId,
    pub provider: LaunchProvider,
    pub mode: Mode,
    pub native_profile: Option<NativeProfileChoice>,
    pub managed_safety_profile: Option<wyss_console::contracts::m1::records::ManagedSafetyProfile>,
}

/// The Native profile the user chose; the owner resolves its availability.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeProfileChoice {
    pub id: wyss_console::contracts::m1::scalar::NativeId,
    pub version: ProfileVersion,
}

/// The contract's error vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    UnsupportedSchema,
    InvalidRecord,
    CapacityExceeded,
    RevisionConflict,
    ContextStale,
    LeafBusy,
    RootMissing,
    RootIdentityChanged,
    RootIneligible,
    HostOffline,
    HostChanged,
    UnsupportedCapability,
    ProfileUnavailable,
    OwnerRefused,
    EffectAmbiguous,
    IoFailure,
}

/// Whether a failed command may have changed anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    None,
    Committed,
    Unknown,
}

/// What to offer the user next. Never raw provider or configuration bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NextAction {
    Refetch,
    Retry,
    ChooseAgain,
    Reconcile,
    OpenRecovery,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub code: ErrorCode,
    pub effect: Effect,
    pub next_action: NextAction,
    pub operation_id: Option<OperationId>,
    pub context: Option<Context>,
    /// A closed field name for the window to point at, never user content.
    pub field: Option<&'static str>,
}

impl Failure {
    pub fn new(code: ErrorCode) -> Self {
        let next_action = match code {
            ErrorCode::RevisionConflict | ErrorCode::ContextStale | ErrorCode::LeafBusy => {
                NextAction::Refetch
            }
            ErrorCode::RootMissing
            | ErrorCode::RootIdentityChanged
            | ErrorCode::RootIneligible
            | ErrorCode::ProfileUnavailable => NextAction::ChooseAgain,
            ErrorCode::EffectAmbiguous => NextAction::Reconcile,
            ErrorCode::UnsupportedSchema => NextAction::OpenRecovery,
            ErrorCode::HostOffline | ErrorCode::IoFailure => NextAction::Retry,
            _ => NextAction::None,
        };
        Self {
            code,
            effect: Effect::None,
            next_action,
            operation_id: None,
            context: None,
            field: None,
        }
    }

    pub fn with_field(mut self, field: &'static str) -> Self {
        self.field = Some(field);
        self
    }

    pub fn with_effect(mut self, effect: Effect) -> Self {
        self.effect = effect;
        self
    }
}

/// The answer to a command that committed (or was already committed under its operation ID).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub operation_id: OperationId,
    pub context: Context,
    pub store_revision: Counter,
    /// IDs the command created or touched, by role, for the window to select.
    pub created: Vec<Created>,
    /// True when this answer repeats an earlier commit of the same operation ID.
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Created {
    pub role: &'static str,
    pub id: String,
}
