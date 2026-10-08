//! Transient state of the one provider action a turn may be waiting on. Proposals, the held
//! request and the action under approval live only in this process: they never reach the
//! store, the audit log or a diagnostic, and nothing here derives `Debug`.
use super::approval_action::{CommandAction, FileAction, FileChange};
use crate::{
    adapters::codex::{
        CodexRequestId,
        safety::{ActionKind, ApprovalRequest, CommandProposal, Offered, ProposedChange},
    },
    contracts::{control::*, identities::ManagedApprovalId},
};
use std::collections::BTreeMap;
use time::OffsetDateTime;

/// Announced actions kept at once. One approval is pending at a time, so more than a few
/// announcements without a request is not a mediated flow.
const MAX_PROPOSALS: usize = 4;
/// A file approval whose change set never arrives is refused after this long. The pinned
/// runtime announces the complete change set before it asks.
pub(super) const PROPOSAL_WAIT_SECONDS: i64 = 10;

pub(super) enum Action {
    Command(CommandAction),
    File(FileAction),
}
impl Action {
    pub fn kind(&self) -> ApprovalKind {
        match self {
            Self::Command(_) => ApprovalKind::CommandExecution,
            Self::File(_) => ApprovalKind::FileChange,
        }
    }
    /// The provider item the action itself was built for.
    pub fn item(&self) -> &str {
        match self {
            Self::Command(c) => c.provider_item_id(),
            Self::File(f) => f.provider_item_id(),
        }
    }
    pub fn fingerprint(&self) -> String {
        match self {
            Self::Command(c) => c.fingerprint(),
            Self::File(f) => f.fingerprint(),
        }
    }
    pub fn structure(&self) -> ApprovalStructure {
        match self {
            Self::Command(c) => c.structure(),
            Self::File(f) => f.structure(),
        }
    }
    /// Workspace confinement, proven again before every Accept.
    pub fn confined(&self, workspace: &str) -> bool {
        match self {
            Self::Command(c) => c.confined(workspace),
            Self::File(f) => f.confined(workspace),
        }
    }
}
enum Proposal {
    Command {
        command: String,
        cwd: String,
        actions: Vec<String>,
    },
    File(Vec<ProposedChange>),
}
/// What a provider request amounts to once it is matched with what was announced.
pub(super) enum Built {
    /// A complete, bounded action. Confinement is the caller's next check.
    Action(Action),
    /// A file request whose complete change set has not arrived.
    Incomplete,
    /// It can never become an acceptable approval: oversized, malformed or unannounced.
    Refused,
    /// The provider contradicted itself. This is not a mediated flow.
    Violation,
}
/// The one provider approval request the authority currently holds.
pub(super) struct Held {
    pub request: CodexRequestId,
    pub item: String,
    pub kind: ActionKind,
    pub offered: Offered,
    /// Absent while the request can never be accepted: no complete proposal, or refused.
    pub approval: Option<ManagedApprovalId>,
    pub action: Option<Action>,
    /// Deadline of the pending approval, for the authority's own scheduler.
    pub expires: Option<OffsetDateTime>,
    /// Deadline for a change set that has not arrived.
    pub wait_until: Option<OffsetDateTime>,
    /// An answer was dispatched, or the request no longer needs one.
    pub answered: bool,
}
#[derive(Default)]
pub(super) struct Mediation {
    proposals: BTreeMap<String, Proposal>,
    pub held: Option<Held>,
    /// Items whose Accept was dispatched: the only ones that may produce output or run.
    accepted: BTreeMap<String, (ManagedApprovalId, ApprovalKind)>,
    /// Accepted commands whose output already reached the stream as deltas.
    streamed: std::collections::BTreeSet<String>,
    /// A file change was accepted in this turn, so the provider may report applied changes.
    pub file_accepted: bool,
    /// The request whose one answer was handed to the supervisor and not yet reported on.
    pub dispatched: Option<CodexRequestId>,
}
impl Mediation {
    /// True when the item is already announced, held or accepted.
    fn known(&self, item: &str) -> bool {
        self.proposals.contains_key(item)
            || self.accepted.contains_key(item)
            || self.held.as_ref().is_some_and(|h| h.item == item)
    }
    /// A command is announced exactly once, before anything is asked about it.
    pub fn command_proposed(&mut self, p: CommandProposal) -> bool {
        if self.known(p.item.as_str()) || self.proposals.len() >= MAX_PROPOSALS {
            return false;
        }
        self.proposals.insert(
            p.item.as_str().to_owned(),
            Proposal::Command {
                command: p.command,
                cwd: p.cwd,
                actions: p.actions,
            },
        );
        true
    }
    pub fn announced(&self, item: &str) -> bool {
        self.proposals.contains_key(item)
    }
    /// A change set may be announced in parts until its approval is requested. Returns
    /// false when the announcement cannot belong to a mediated flow.
    pub fn file_proposed(&mut self, item: &str, changes: Vec<ProposedChange>) -> bool {
        if self.accepted.contains_key(item)
            || matches!(self.proposals.get(item), Some(Proposal::Command { .. }))
            || (!self.proposals.contains_key(item) && self.proposals.len() >= MAX_PROPOSALS)
        {
            return false;
        }
        self.proposals
            .insert(item.to_owned(), Proposal::File(changes));
        true
    }
    /// Matches a request with the announcement of its item. The request may restate the
    /// command; whatever it restates has to be exactly what was announced.
    pub fn build(&self, r: &ApprovalRequest) -> Built {
        let item = r.item.as_str();
        match (r.kind, self.proposals.get(item)) {
            (ActionKind::Command, Some(Proposal::File(_)))
            | (ActionKind::File, Some(Proposal::Command { .. })) => Built::Violation,
            (ActionKind::Command, proposal) => {
                let announced = match proposal {
                    Some(Proposal::Command {
                        command,
                        cwd,
                        actions,
                    }) => Some((command, cwd, actions)),
                    _ => None,
                };
                if announced.is_some_and(|(command, cwd, _)| {
                    r.command.as_ref().is_some_and(|c| c != command)
                        || r.cwd.as_ref().is_some_and(|c| c != cwd)
                }) {
                    return Built::Violation;
                }
                let command = r.command.as_ref().or(announced.map(|a| a.0));
                let cwd = r.cwd.as_ref().or(announced.map(|a| a.1));
                let (Some(command), Some(cwd)) = (command, cwd) else {
                    return Built::Refused;
                };
                let actions = if r.actions.is_empty() {
                    announced.map(|a| a.2.clone()).unwrap_or_default()
                } else {
                    r.actions.clone()
                };
                match CommandAction::new(item, command, cwd, actions) {
                    Ok(action) => Built::Action(Action::Command(action)),
                    Err(_) => Built::Refused,
                }
            }
            (ActionKind::File, _) => self.build_file(item),
        }
    }
    /// The file action of an item, once its complete change set has been announced.
    pub fn build_file(&self, item: &str) -> Built {
        match self.proposals.get(item) {
            Some(Proposal::Command { .. }) => Built::Violation,
            Some(Proposal::File(changes)) if !changes.is_empty() => {
                match Self::file_action(item, changes) {
                    Some(action) => Built::Action(Action::File(action)),
                    None => Built::Refused,
                }
            }
            _ => Built::Incomplete,
        }
    }
    pub fn file_action(item: &str, changes: &[ProposedChange]) -> Option<FileAction> {
        FileAction::new(
            item,
            changes
                .iter()
                .map(|c| FileChange {
                    path: c.path.clone(),
                    kind: c.kind,
                    move_path: c.move_path.clone(),
                    diff: c.diff.clone(),
                })
                .collect(),
        )
        .ok()
    }
    pub fn accept(&mut self, item: &str, approval: ManagedApprovalId, kind: ApprovalKind) {
        self.accepted.insert(item.to_owned(), (approval, kind));
        self.file_accepted |= kind == ApprovalKind::FileChange;
    }
    pub fn accepted(&self, item: &str) -> Option<&(ManagedApprovalId, ApprovalKind)> {
        self.accepted.get(item)
    }
    /// Output of this item was streamed while it ran.
    pub fn streamed(&mut self, item: &str) {
        self.streamed.insert(item.to_owned());
    }
    /// True when the finished item's output has not been shown yet.
    pub fn unshown(&mut self, item: &str) -> bool {
        !self.streamed.remove(item)
    }
    /// The item is over: nothing about it may be announced, asked or produced again.
    pub fn ended(&mut self, item: &str) -> Option<(ManagedApprovalId, ApprovalKind)> {
        self.proposals.remove(item);
        if self.held.as_ref().is_some_and(|h| h.item == item) {
            self.held = None;
        }
        self.accepted.remove(item)
    }
}
