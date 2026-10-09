//! Desktop-side managed turns: start and cancel as intents, and canonical reads.
//!
//! A turn is a real `ManagedTurn` of `wyss-control`: one input, admitted by the
//! authority's execution gate under a lease, run by the runtime the authority
//! supervises, and ended by the authority. Nothing here decides whether a turn may
//! start, whether an action needs an approval, whether a cancel took effect or what the
//! provider did. What the window is told is what the authority answered, reduced to a
//! narrow projection. No dependency on Tauri or human terminals.
//!
//! An approval a turn waits on is read here and answered as an intent (`approvals`); a
//! session enters a mode here, because that may stop the runtime a turn runs on (`modes`).
mod approvals;
mod modes;
mod service;
mod view;

pub use approvals::{
    APPROVAL_PAGE, ApprovalDetail, ApprovalList, ApprovalSummary, CommandView, Decision,
    DecisionReceipt, FileView,
};
pub use service::TurnService;
pub use view::{
    ApprovalNotice, SessionFacts, TURN_PAGE, TurnEvent, TurnEventKind, TurnList, TurnReceipt,
    TurnSummary, TurnView,
};

#[cfg(test)]
mod tests;
