//! Approvals of managed turns: what a person needs to decide one, and the decision as an
//! intent.
//!
//! In a local-tools generation the model may propose a command or a file change. The
//! authority holds each proposal as one pending `ManagedApproval`; nothing runs until it
//! committed an Accept. This module reads that approval and asks for a decision. It
//! decides nothing: whether an approval is still pending, whether the action is still
//! confined to the workspace, whether the turn's lease carries the scope, whether the
//! provider may go on, are all the authority's, checked by it when the decision arrives.
//!
//! **Discovery.** The protocol has no operation that lists approvals. Each request for
//! one is a durable event that names it, so the index of `service` remembers the latest
//! ones, and every approval said to exist is read from the authority before anything is
//! shown. The authority holds at most one pending approval at a time.
//!
//! **What crosses.** The approval's own identity, as a handle to answer it with; its
//! session and turn; kind, state, revision, deadline and how it ended; and the action
//! itself while the authority still holds it: the complete command with its directory,
//! or one file of the change set with one page of its diff. The provider's item, the
//! lease, the runtime instance, the action fingerprint and the external-effect marker
//! stay on this side. The action is never written anywhere here, and the types that
//! carry it print nothing of it.
use super::service::{TurnService, scan};
use crate::{
    contracts::{
        Sequence, UtcTimestamp, WireUuid,
        control::{
            ApprovalDecision, ApprovalGet, ApprovalKind, ApprovalReason, ApprovalRespond,
            ApprovalState, ApprovalStatus, ControlError, FileChangeKind,
        },
        identities::{ManagedApprovalId, ManagedSessionId, ManagedTurnId},
    },
    control::client::ControlClient,
    sessions::{SessionError, SessionFailure},
};
use serde::Serialize;
use std::sync::PoisonError;

pub(super) const APPROVALS_SCHEMA_VERSION: u8 = 1;
/// Approvals one list reads from the authority at most, newest first.
pub const APPROVAL_PAGE: usize = 8;

/// What the user can ask for one approval. Never anything broader: no session-wide
/// grant, no rule, no amendment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Approve,
    Deny,
}

/// One approval as the authority holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApprovalSummary {
    /// A handle: it names this approval when it is read or answered, and nothing else.
    pub id: ManagedApprovalId,
    pub session: ManagedSessionId,
    pub turn: ManagedTurnId,
    pub kind: ApprovalKind,
    pub state: ApprovalState,
    pub revision: u64,
    pub requested_at: UtcTimestamp,
    pub expires_at: UtcTimestamp,
    pub ended_at: Option<UtcTimestamp>,
    /// Why it ended, once it has.
    pub reason: Option<ApprovalReason>,
    /// What was decided, when a decision is what ended it.
    pub decision: Option<ApprovalDecision>,
    /// Presentation hint: pending, with the action still held by the authority. Whether
    /// a decision is taken is the authority's to say.
    pub decidable: bool,
}
impl ApprovalSummary {
    fn of(status: &ApprovalStatus) -> Self {
        let approval = &status.approval;
        Self {
            id: approval.managed_approval_id.clone(),
            session: approval.managed_session_id.clone(),
            turn: approval.managed_turn_id.clone(),
            kind: approval.kind,
            state: approval.state,
            revision: approval.revision,
            requested_at: approval.created_at.clone(),
            expires_at: approval.expires_at.clone(),
            ended_at: approval.terminal_at.clone(),
            reason: approval.terminal_reason,
            decision: approval.decision,
            decidable: status.acceptable,
        }
    }
}

/// The command under approval, whole, exactly as the authority presents it.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct CommandView {
    pub command: String,
    pub cwd: String,
}
/// One file of the change set under approval, with one page of its diff.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct FileView {
    pub index: u16,
    pub path: String,
    pub kind: FileChangeKind,
    pub move_path: Option<String>,
    /// Size of the whole diff of this file.
    pub bytes: u32,
    /// Where `chunk` starts in it.
    pub offset: u32,
    pub chunk: String,
    /// Where the next page starts. None when this one ends the diff.
    pub next: Option<u32>,
}
/// An approval with the action it is about, while the authority still holds the action.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct ApprovalDetail {
    pub approval: ApprovalSummary,
    /// The workspace the authority holds for the approval's session.
    pub workspace: Option<String>,
    pub command: Option<CommandView>,
    /// How many files the change set has.
    pub files: Option<u16>,
    pub file: Option<FileView>,
}
// Deliberately not derived: a command or a diff must not enter diagnostics.
impl std::fmt::Debug for ApprovalDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApprovalDetail(<sensitive>)")
    }
}
impl ApprovalDetail {
    fn of(status: ApprovalStatus, workspace: Option<String>) -> Self {
        Self {
            approval: ApprovalSummary::of(&status),
            workspace,
            command: status.command.map(|c| CommandView {
                command: c.command,
                cwd: c.cwd,
            }),
            files: status.file_count,
            file: status.file.map(|f| FileView {
                index: f.index,
                path: f.path,
                kind: f.kind,
                move_path: f.move_path,
                bytes: f.diff_bytes,
                offset: f.offset,
                chunk: f.chunk,
                next: f.next_offset,
            }),
        }
    }
}

/// The approvals the durable stream names, newest first, each as the authority holds it.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct ApprovalList {
    pub schema_version: u8,
    /// The durable head read before the approvals: the list is at least this new.
    pub head: Option<Sequence>,
    /// Waiting for a decision, with their action.
    pub pending: Vec<ApprovalDetail>,
    /// Ended. The authority no longer holds their action.
    pub recent: Vec<ApprovalSummary>,
    /// Older history exists that this desktop did not read.
    pub partial: bool,
}
impl std::fmt::Debug for ApprovalList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApprovalList(<sensitive>)")
    }
}

/// The authority committed a decision for this approval. What the approval and its turn
/// are now is read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DecisionReceipt {
    pub id: ManagedApprovalId,
    pub state: ApprovalState,
}

impl TurnService {
    /// The latest approvals, newest first: those waiting for a decision with their
    /// action, and those that ended.
    pub fn approvals(&self) -> Result<ApprovalList, SessionError> {
        let mut index = self
            .inner
            .index
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut client = self.inner.sessions.connect()?;
        // The head first, then what it describes: the list is never older than it.
        let head = client.hello_status().last_sequence;
        scan(&mut client, &mut index).map_err(SessionError::of)?;
        let known: Vec<ManagedApprovalId> = index
            .approvals
            .iter()
            .rev()
            .take(APPROVAL_PAGE)
            .cloned()
            .collect();
        let (mut pending, mut recent) = (Vec::new(), Vec::new());
        for id in known {
            let detail = read(&mut client, id, None, None)?;
            if detail.approval.state == ApprovalState::Pending {
                pending.push(detail);
            } else {
                recent.push(detail.approval);
            }
        }
        Ok(ApprovalList {
            schema_version: APPROVALS_SCHEMA_VERSION,
            head,
            pending,
            recent,
            partial: index.partial,
        })
    }

    /// One approval as the authority holds it now. `file` and `offset` select a file of
    /// a change set and a page of its diff; without them it is the first of each.
    pub fn approval(
        &self,
        id: ManagedApprovalId,
        file: Option<u16>,
        offset: Option<u32>,
    ) -> Result<ApprovalDetail, SessionError> {
        read(&mut self.inner.sessions.connect()?, id, file, offset)
    }

    /// Asks the authority for one decision on one approval, at the revision the caller
    /// saw. `intent` names the user's action: sent again unchanged it is the same
    /// decision, and the authority answers it with what it committed the first time.
    pub fn decide(
        &self,
        intent: WireUuid,
        id: ManagedApprovalId,
        expected_revision: u64,
        decision: Decision,
    ) -> Result<DecisionReceipt, SessionError> {
        if expected_revision == 0 || expected_revision > i64::MAX as u64 {
            return Err(SessionError::new(SessionFailure::Invalid));
        }
        let request = ApprovalRespond {
            intent_id: intent,
            managed_approval_id: id.clone(),
            expected_revision,
            decision: match decision {
                Decision::Approve => ApprovalDecision::Accept,
                // A refusal of this one action. What it does to the turn is the
                // provider's and the authority's: it is not a cancel of the turn.
                Decision::Deny => ApprovalDecision::Decline,
            },
        };
        self.inner.sessions.call(|client| {
            let receipt = client.approval_respond(request).map_err(SessionError::of)?;
            // A receipt about another approval is not an answer to this request.
            if receipt.managed_approval_id != id {
                return Err(SessionError::of(ControlError::CorrelationError));
            }
            Ok(DecisionReceipt {
                id: receipt.managed_approval_id,
                state: receipt.state,
            })
        })
    }
}

/// Reads one approval, and the workspace of its session to show it against.
fn read(
    client: &mut ControlClient,
    id: ManagedApprovalId,
    file: Option<u16>,
    offset: Option<u32>,
) -> Result<ApprovalDetail, SessionError> {
    let status = client
        .approval_status(ApprovalGet {
            managed_approval_id: id.clone(),
            file_index: file,
            offset,
        })
        .map_err(|error| match error {
            // A file or a page the approval does not have: the request was not one to send.
            ControlError::InvalidRequest => SessionError {
                cause: Some(error),
                ..SessionError::new(SessionFailure::Invalid)
            },
            _ => SessionError::of(error),
        })?;
    if status.approval.managed_approval_id != id {
        return Err(SessionError::of(ControlError::CorrelationError));
    }
    let workspace = client
        .binding_status(status.approval.managed_session_id.clone())
        .map_err(SessionError::of)?
        .binding
        .map(|b| b.workspace_root);
    Ok(ApprovalDetail::of(status, workspace))
}
