//! The closed records of `StoreV1` (contract §2). Every record rejects unknown fields, every
//! optional value is present as `null` (an omitted field is refused), and every enum is
//! closed. Shape is checked here, while deserializing; cross-record rules live in `validate`.
use super::scalar::{
    AbsolutePath, AssociationId, BindingId, Counter, EpochText, HostId, LeafId, ManagedSessionId,
    MappedId, MigrationId, NativeId, OperationId, ProfileVersion, ProjectId, StoreId,
    StoreStreamId, TerminalId, Text, WorkspaceId,
};
use crate::contracts::{UtcTimestamp, b1::Sha256Hex};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An optional field must be written as `null`; a field that is simply absent is a malformed
/// record, not an unset one.
fn required<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

/// Wire marker for `schema: 1`. The codec reads the version first, so a newer store is
/// reported as such; any other value fails here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaV1;

impl Serialize for SchemaV1 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(1)
    }
}

impl<'de> Deserialize<'de> for SchemaV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match u8::deserialize(deserializer)? {
            1 => Ok(Self),
            _ => Err(serde::de::Error::custom("schema")),
        }
    }
}

/// A Native profile as its owner names it: an ID and a version, never a body or credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileRef {
    pub id: NativeId,
    pub version: ProfileVersion,
}

/// The requested Managed profile. It is a closed selector: it grants no execution or
/// approval, and it is never a Native `ProfileRef` or proof of what the authority attached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedSafetyProfile {
    Inference,
    LocalTools,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Native,
    Managed,
}

/// The providers a workspace can prefer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Claude,
    Codex,
}

/// What a launch can start: a provider agent or a plain shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchProvider {
    Claude,
    Codex,
    Shell,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileDefaults {
    #[serde(deserialize_with = "required")]
    pub claude: Option<ProfileRef>,
    #[serde(deserialize_with = "required")]
    pub codex: Option<ProfileRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Project {
    pub id: ProjectId,
    pub label: Text,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    pub revision: Counter,
    pub archived: bool,
    pub tombstone: bool,
    #[serde(deserialize_with = "required")]
    pub default_binding_id: Option<BindingId>,
    #[serde(deserialize_with = "required")]
    pub default_workspace_id: Option<WorkspaceId>,
    pub profile_defaults: ProfileDefaults,
}

/// The device and inode a root had when it was observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub device: Counter,
    pub inode: Counter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckoutKind {
    Folder,
    Git,
    Worktree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingState {
    Valid,
    Missing,
    IdentityChanged,
    Unverified,
    Retired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binding {
    pub id: BindingId,
    pub project_id: ProjectId,
    pub host_id: HostId,
    pub revision: Counter,
    pub path: AbsolutePath,
    pub observation: Observation,
    pub checkout_kind: CheckoutKind,
    pub state: BindingState,
    #[serde(deserialize_with = "required")]
    pub previous_binding_id: Option<BindingId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitDir {
    Row,
    Col,
}

/// A binary layout tree. A leaf names a workspace `Leaf`; depth and ratio are bounded by
/// validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Node {
    Leaf {
        id: LeafId,
    },
    Split {
        dir: SplitDir,
        ratio: f64,
        a: Box<Node>,
        b: Box<Node>,
    },
}

/// A placement. No association means an empty placement, never permission to launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Leaf {
    pub id: LeafId,
    pub title: Text,
    #[serde(deserialize_with = "required")]
    pub association_id: Option<AssociationId>,
}

/// What a workspace pre-fills for a launch. It never grants a lease, authorizes a tool or
/// starts anything, and it never stores `local_tools`: that is chosen on each launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    #[serde(deserialize_with = "required")]
    pub provider: Option<Provider>,
    pub mode: Mode,
    #[serde(deserialize_with = "required")]
    pub native_profile: Option<ProfileRef>,
    #[serde(deserialize_with = "required")]
    pub managed_safety_profile: Option<ManagedSafetyProfile>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Workspace {
    pub id: WorkspaceId,
    /// Null only for the one Scratch workspace.
    #[serde(deserialize_with = "required")]
    pub project_id: Option<ProjectId>,
    pub host_id: HostId,
    pub revision: Counter,
    pub label: Text,
    #[serde(deserialize_with = "required")]
    pub layout: Option<Node>,
    #[serde(deserialize_with = "required")]
    pub focus: Option<LeafId>,
    pub pinned: bool,
    pub leaves: Vec<Leaf>,
    pub preferences: Preferences,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssociationState {
    Attached,
    Detached,
    Unavailable,
    Ambiguous,
}

/// The Managed protocol of the authority that owns a session. It is a number on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManagedProtocol {
    P1,
    P2,
}

impl Serialize for ManagedProtocol {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(match self {
            Self::P1 => 1,
            Self::P2 => 2,
        })
    }
}

impl<'de> Deserialize<'de> for ManagedProtocol {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match u8::deserialize(deserializer)? {
            1 => Ok(Self::P1),
            2 => Ok(Self::P2),
            _ => Err(serde::de::Error::custom("protocol")),
        }
    }
}

/// The owner's reference to a session. Each variant carries the identities its owner uses, so
/// a Native ID, a terminal ID and a Managed session ID cannot be mistaken for one another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    #[serde(rename_all = "camelCase")]
    Native {
        session_id: NativeId,
        terminal_id: TerminalId,
        host_epoch: EpochText,
    },
    #[serde(rename_all = "camelCase")]
    Shell {
        terminal_id: TerminalId,
        host_epoch: EpochText,
    },
    #[serde(rename_all = "camelCase")]
    Managed {
        protocol: ManagedProtocol,
        store_stream_id: StoreStreamId,
        session_id: ManagedSessionId,
        /// Display only: never proof of the profile the authority actually attached.
        #[serde(deserialize_with = "required")]
        requested_safety_profile: Option<ManagedSafetyProfile>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Association {
    pub id: AssociationId,
    #[serde(deserialize_with = "required")]
    pub origin_project_id: Option<ProjectId>,
    pub host_id: HostId,
    #[serde(deserialize_with = "required")]
    pub binding_id: Option<BindingId>,
    #[serde(deserialize_with = "required")]
    pub binding_revision: Option<Counter>,
    pub target: Target,
    pub state: AssociationState,
    #[serde(deserialize_with = "required")]
    pub operation_id: Option<OperationId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Launch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Pending,
    Committed,
    Ambiguous,
    Refused,
}

/// The Managed authority a launch was first asked of: its protocol and the identity of its
/// durable stream. A retry is made of this authority and of no other, so a desktop that changed
/// between protocol 1 and 2 cannot prepare a second session elsewhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagedAuthority {
    pub protocol: ManagedProtocol,
    pub store_stream_id: StoreStreamId,
}

/// The durable receipt of a launch request. It records what was asked, with the Native
/// profile and the requested Managed profile kept apart. It is never an approval, a
/// credential or proof of the profile that ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Operation {
    pub id: OperationId,
    pub kind: OperationKind,
    #[serde(deserialize_with = "required")]
    pub project_id: Option<ProjectId>,
    pub workspace_id: WorkspaceId,
    pub leaf_id: LeafId,
    #[serde(deserialize_with = "required")]
    pub binding_id: Option<BindingId>,
    #[serde(deserialize_with = "required")]
    pub binding_revision: Option<Counter>,
    pub provider: LaunchProvider,
    pub mode: Mode,
    #[serde(deserialize_with = "required")]
    pub native_profile: Option<ProfileRef>,
    #[serde(deserialize_with = "required")]
    pub managed_safety_profile: Option<ManagedSafetyProfile>,
    /// Present exactly for a Managed launch: the authority that was asked first.
    #[serde(deserialize_with = "required")]
    pub managed_authority: Option<ManagedAuthority>,
    pub state: OperationState,
    #[serde(deserialize_with = "required")]
    pub association_id: Option<AssociationId>,
    /// The dispatching process's epoch observation; present exactly while `pending`.
    #[serde(deserialize_with = "required")]
    pub claim: Option<EpochText>,
    pub created_at: UtcTimestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationSourceKind {
    Registry,
    Layout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapSourceKind {
    RegistryProject,
    RegistrySession,
    LayoutLeaf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationState {
    Committed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationSource {
    pub kind: MigrationSourceKind,
    pub digest: Sha256Hex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrationMapRow {
    pub source_kind: MapSourceKind,
    /// An opaque source-record digest, not a public path.
    pub source_key: Text,
    pub target_id: MappedId,
}

/// Version 1 of the deterministic legacy migration, once committed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Migration {
    pub version: MigrationVersion,
    pub id: MigrationId,
    pub sources: Vec<MigrationSource>,
    pub map: Vec<MigrationMapRow>,
    pub state: MigrationState,
}

/// Wire marker for `version: 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationVersion;

impl Serialize for MigrationVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(1)
    }
}

impl<'de> Deserialize<'de> for MigrationVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match u8::deserialize(deserializer)? {
            1 => Ok(Self),
            _ => Err(serde::de::Error::custom("version")),
        }
    }
}

/// The whole versioned store. Decoding it goes through `codec::decode`, which bounds the
/// input and validates every rule; a value built by hand is checked by `validate`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoreV1 {
    pub schema: SchemaV1,
    pub store_id: StoreId,
    pub revision: Counter,
    pub host_id: HostId,
    pub projects: Vec<Project>,
    pub bindings: Vec<Binding>,
    pub workspaces: Vec<Workspace>,
    pub sessions: Vec<Association>,
    pub operations: Vec<Operation>,
    pub closed_operations: Vec<OperationId>,
    #[serde(deserialize_with = "required")]
    pub migration: Option<Migration>,
}
