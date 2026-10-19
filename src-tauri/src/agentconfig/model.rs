//! What the configuration engine says about an agent's configuration: closed words,
//! bounded numbers and hashes. Nothing here holds a value read from a provider's file,
//! so nothing here can carry a secret or a path the page did not get from the backend
//! (07 §1 E8, C-07).
use serde::{Deserialize, Serialize};
use std::fmt;

/// The agents whose configuration F1 reads. OpenCode and Hermes come with F4 (06 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    ClaudeCode,
    Codex,
}

/// Who a source affects (07 §2). A session's configuration is a profile (F3) and is not
/// read from a provider's file, so it is not a scope of a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    System,
    Global,
    Project,
}

/// What kind of file a source is, as its adapter declares it (06 §4.1). A link is not a
/// class: the link is never written, and its target is judged by the target's own class
/// (a read records the links it went through).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceClass {
    /// A file the provider documents as the user's to edit.
    User,
    /// The same, inside a project.
    Project,
    /// Root-owned or organisation policy.
    System,
    /// A file the provider rewrites for itself, such as `~/.claude.json`.
    ProviderOwned,
    /// Produced by another tool from another source.
    Generated,
}

/// How far the place of a source can be trusted. Decided from descriptors, one directory
/// at a time, by the rules of `control/paths.rs` applied to a provider's directory
/// (07 §4.2): no ancestor writable by another user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "trust", content = "reason")]
pub enum PathTrust {
    /// Every ancestor and the file belong to this user or to root, and no one else can
    /// write any of them.
    Trusted,
    /// Readable, but never written: something along the way is shared.
    ReadOnly(ReadOnlyReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadOnlyReason {
    /// A directory on the way, or the file, can be written by its group.
    GroupWritable,
    /// The file has a second name somewhere else.
    MultipleNames,
}

/// Why a place is not read at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UntrustedReason {
    /// Not absolute, or with a `..` or a control character in it.
    NotPlain,
    /// A directory on the way is a symbolic link.
    AncestorLink,
    /// A component on the way is not a directory.
    AncestorNotDirectory,
    /// A directory on the way, or the file, belongs to another user.
    ForeignOwner,
    /// A directory on the way, or the file, can be written by anyone.
    WritableByOthers,
    /// The place, or the place a link leads to, is in none of the approved roots.
    OutsideRoots,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedReason {
    /// A FIFO, a socket, a device or a directory where a file was expected.
    NotRegularFile,
    /// More links than the bound, or a loop.
    LinkLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TooLarge {
    /// More bytes than `Bounds::file_bytes`.
    File,
    /// More entries than `Bounds::listing`.
    Listing,
    /// More items than `Bounds::items`.
    Items,
    /// Nested deeper than `Bounds::depth`.
    Depth,
    /// More keys and values than `Bounds::elements`.
    Elements,
}

/// Why a source has no content to show. Each is a closed word: no failure carries a byte
/// of the file, and a malformed one carries only where parsing stopped (07 §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "failure")]
pub enum SourceFailure {
    /// Nothing at that place: `absent` for the inventory, never an error for the whole
    /// provider (TS-CFG-02).
    Missing,
    /// It exists and this user cannot read it. Distinct from `Missing` (TS-FS-05).
    Unreadable,
    /// It changed on every one of the attempts to read it; temporary.
    Unstable,
    UntrustedPath {
        reason: UntrustedReason,
    },
    Unsupported {
        reason: UnsupportedReason,
    },
    TooLarge {
        bound: TooLarge,
    },
    /// A credential file: never opened at all (06 §4.2).
    CredentialFile,
    /// It does not parse. Where parsing stopped, and nothing of what was there.
    Malformed {
        line: u32,
        column: u32,
    },
}

impl SourceFailure {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "source_missing",
            Self::Unreadable => "source_unreadable",
            Self::Unstable => "source_unstable",
            Self::UntrustedPath { .. } => "source_untrusted_path",
            Self::Unsupported { .. } => "source_unsupported",
            Self::TooLarge { .. } => "source_too_large",
            Self::CredentialFile => "source_credential_file",
            Self::Malformed { .. } => "source_malformed",
        }
    }
}

impl fmt::Display for SourceFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::error::Error for SourceFailure {}

/// Why a source can not be written. F1 writes nothing, so every source has one; the
/// reasons that are not `NotInThisPhase` stay true in every later phase (C-09, 07 §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotWritable {
    System,
    ProviderOwned,
    Generated,
    /// Its place is shared or has a second name: read, never written.
    SharedPath,
    /// A user or project file on a trusted path. Writing it is F2a's, by a plan.
    NotInThisPhase,
}

/// The write capability of a source. There is no writable answer in F1: the type has no
/// value for it, so no caller can be handed one by mistake (TS-CFG-09).
pub const fn writability(class: SourceClass, trust: PathTrust) -> NotWritable {
    match (class, trust) {
        (SourceClass::System, _) => NotWritable::System,
        (SourceClass::ProviderOwned, _) => NotWritable::ProviderOwned,
        (SourceClass::Generated, _) => NotWritable::Generated,
        (_, PathTrust::ReadOnly(_)) => NotWritable::SharedPath,
        (SourceClass::User | SourceClass::Project, PathTrust::Trusted) => {
            NotWritable::NotInThisPhase
        }
    }
}

/// The kinds of entry the inventory lists (07 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Hook,
    RuleFile,
    Skill,
    McpServer,
    ModelPreference,
}

/// What the file says (07 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Persisted {
    PresentEnabled,
    PresentDisabled,
    Absent,
    /// Taken out by the Console and kept. Nothing is parked before F2.
    Parked,
    Unreadable,
}

/// What the provider resolved. `Documented` is a deduction from the provider's manual,
/// not an observation, and is drawn differently from `Verified*` (FR-UX-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effective {
    VerifiedEnabled,
    VerifiedDisabled,
    Documented,
    NotVerifiable,
}

/// What one running session is using (07 §3). `Using` needs an observation of that
/// session; a documented reload alone gives `DocumentedUsing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionUse {
    Using,
    DocumentedUsing,
    NotUsing,
    Stale,
    Unknown,
}

/// SHA-256 of the exact bytes a source was read from, in lower-case hex.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    pub fn of(bytes: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        Self(Sha256::digest(bytes).into())
    }
    pub fn hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl fmt::Debug for ContentHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "ContentHash({})", self.hex())
    }
}

impl Serialize for ContentHash {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.hex())
    }
}

/// Every bound of C-11. The values are the ones proposed in 07 §4.2; the final ones are
/// set with the fixtures and the owner's real volumes (OQ-22).
///
/// Outside this module a `Bounds` comes from `PROPOSED` or from `new`, which refuses
/// zero and anything past a hard ceiling: no caller can ask for an unbounded read.
/// Inside it, every use is still checked or saturating, so a value that skipped `new`
/// cannot overflow either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub(super) file_bytes: u64,
    pub(super) items: usize,
    pub(super) elements: usize,
    pub(super) depth: usize,
    pub(super) link_hops: usize,
    pub(super) listing: usize,
    /// Reads of a file that keeps changing before it is `Unstable`.
    pub(super) read_attempts: usize,
}

/// A bound that is zero or past its ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidBounds;

impl fmt::Display for InvalidBounds {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounds_invalid")
    }
}

impl std::error::Error for InvalidBounds {}

impl Bounds {
    pub const PROPOSED: Self = Self {
        file_bytes: 4 * 1024 * 1024,
        items: 4096,
        elements: 262_144,
        depth: 32,
        link_hops: 4,
        listing: 2048,
        read_attempts: 3,
    };
    /// What `new` accepts at most. Each is far above `PROPOSED` and far below anything
    /// that exhausts memory, descriptors or the parser's own recursion (serde_json stops
    /// at 128 levels).
    const CEILING: Self = Self {
        file_bytes: 64 * 1024 * 1024,
        items: 1 << 20,
        elements: 1 << 24,
        depth: 100,
        link_hops: 40,
        listing: 1 << 16,
        read_attempts: 10,
    };

    pub fn new(
        file_bytes: u64,
        items: usize,
        elements: usize,
        depth: usize,
        link_hops: usize,
        listing: usize,
        read_attempts: usize,
    ) -> Result<Self, InvalidBounds> {
        let bounds = Self {
            file_bytes,
            items,
            elements,
            depth,
            link_hops,
            listing,
            read_attempts,
        };
        bounds.validated()
    }

    /// Zero is allowed only where "none" is meaningful: no link may be followed.
    pub(super) fn validated(self) -> Result<Self, InvalidBounds> {
        let ceiling = Self::CEILING;
        let within = |value: usize, max: usize, min: usize| (min..=max).contains(&value);
        if (1..=ceiling.file_bytes).contains(&self.file_bytes)
            && within(self.items, ceiling.items, 1)
            && within(self.elements, ceiling.elements, 1)
            && within(self.depth, ceiling.depth, 1)
            && within(self.link_hops, ceiling.link_hops, 0)
            && within(self.listing, ceiling.listing, 1)
            && within(self.read_attempts, ceiling.read_attempts, 1)
        {
            Ok(self)
        } else {
            Err(InvalidBounds)
        }
    }
}

impl Default for Bounds {
    fn default() -> Self {
        Self::PROPOSED
    }
}

/// Counts the items an adapter takes from one source, and stops it at the bound instead
/// of after it.
#[derive(Debug)]
pub struct ItemBudget {
    left: usize,
}

impl ItemBudget {
    pub fn new(bounds: &Bounds) -> Self {
        Self { left: bounds.items }
    }
    pub fn take(&mut self) -> Result<(), SourceFailure> {
        self.left = self.left.checked_sub(1).ok_or(SourceFailure::TooLarge {
            bound: TooLarge::Items,
        })?;
        Ok(())
    }
}
