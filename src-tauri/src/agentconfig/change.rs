//! The closed vocabulary of changing a configuration: what can be done, how risky it is,
//! how it ended. These words are what the journal records and the window is shown; none of
//! them carries content.
use serde::{Deserialize, Serialize};

/// What a plan does. Each is one the adapter declares for the provider it is planned on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    /// An instruction file reviewed in the native editor, with an exact sealed inverse.
    EditDocument,
    /// Claude Code has no per-hook switch: the handler leaves the file and is kept.
    ParkHook,
    /// Puts a parked handler back where it was.
    RestoreHook,
    /// `disableAllHooks: true`.
    DisableAllHooks,
    /// Removes `disableAllHooks`, as it was before.
    EnableAllHooks,
    /// `skillOverrides.<name>: "off"`.
    SkillOff,
    /// Puts the skill's override as it was before it was switched off.
    SkillOn,
    /// Codex: `enabled = false` on a `[[skills.config]]` entry.
    CodexSkillOff,
    /// Codex: that entry's `enabled` as it was, or `true`.
    CodexSkillOn,
    /// Codex: `enabled = false` on an MCP server.
    CodexMcpOff,
    /// Codex: that server's `enabled` as it was, or `true`.
    CodexMcpOn,
    /// OpenCode: `enabled: false` on an MCP server.
    OpencodeMcpOff,
    /// OpenCode: that server's `enabled` as it was, or `true`.
    OpencodeMcpOn,
    /// OpenCode has no per-plugin switch: the entry leaves the `plugin` list and is kept. It is
    /// put back with `RestoreHook`, which restores whatever a record holds.
    ParkPlugin,
    /// A hook of the user's own, written by the user in a native dialog.
    AddHook,
    /// The command of a hook, changed by the user in a native dialog.
    EditHook,
    /// OpenCode: a new MCP server, written by the user in a native dialog.
    AddMcp,
    /// OpenCode: the command or address of a server, changed in a native dialog.
    EditMcp,
    /// OpenCode: a server leaves the file and is kept (restored with `RestoreHook`).
    ParkMcp,
    /// Puts back what an authored change replaced, exactly, while the file is as it left it.
    RevertAuthored,
    /// Codex: a new MCP server, written by the user in a native window.
    CodexAddMcp,
    /// Codex: the program, arguments or address of a server, changed in a native window.
    CodexEditMcp,
    /// Codex: a server leaves the file and is kept (restored with `RestoreHook`).
    CodexParkMcp,
}

/// What a switch's key held before a change made it: the one thing needed to put it back
/// exactly. A closed word, never content (11 §5.5): absent (the line was added by the change
/// and is removed by the undo), or the boolean that was there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Prior {
    Absent,
    Enabled,
    Disabled,
}

/// What a plan can do to the user's safety, decided by the backend from what the plan does
/// (11 §4). `forbidden` is not a class a plan has: a forbidden plan does not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    Normal,
    WeakensSecurity,
    ExecutesCode,
}

impl Risk {
    /// Whether the user must confirm in a native dialog the page cannot draw on.
    pub const fn needs_native_dialog(self) -> bool {
        !matches!(self, Self::Normal)
    }
}

/// How an `apply` ended. The first group wrote nothing to the provider's file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Applied,
    /// The user refused in the native dialog, or closed it.
    RefusedByUser,
    /// The file is not what the plan was made on.
    ChangedOutside,
    /// The entry the plan names is no longer there.
    EntryGone,
    /// The entry is there but is not the one that was planned.
    EntryChanged,
    /// A `*.sync-conflict-*` copy sits beside the file.
    ConflictCopyPresent,
    /// The plan was drafted too long ago, or its source changed.
    Expired,
    /// The key store is unavailable and the entry may hold a secret (ADR 0034).
    KeyStoreUnavailable,
    /// The restoration record cannot be read: its key is gone, or it was altered.
    RecordUnreadable,
    /// The file could not be written; it is as it was.
    WriteFailed,
    /// Written, and what the file holds now is not what was planned.
    VerifyFailed,
    /// The journal could not record the intent, so nothing was done.
    NotRecorded,
    /// Found at start: the application ended mid-write and the file holds the new content.
    RecoveredApplied,
    /// Found at start: it ended mid-write and the file is as it was.
    RecoveredNotApplied,
    /// Found at start: it ended mid-write and the file holds neither.
    RecoveredUnverified,
    /// Found at start: it ended mid-write and a newer version written by the provider in that
    /// instant had been displaced; it was put back, and nothing of ours was kept.
    RecoveredProviderKept,
}

impl Outcome {
    /// Whether the provider's file now holds the change.
    pub const fn changed_the_file(self) -> bool {
        matches!(
            self,
            Self::Applied | Self::VerifyFailed | Self::RecoveredApplied | Self::RecoveredUnverified
        )
    }
}
