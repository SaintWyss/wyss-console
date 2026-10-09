//! Desktop-side managed turns: start and cancel as intents, and canonical reads.
//!
//! A turn is a real `ManagedTurn` of `wyss-control`: one input, admitted by the
//! authority's execution gate under a lease, run by the runtime the authority
//! supervises, and ended by the authority. Nothing here decides whether a turn may
//! start, whether an action needs an approval, whether a cancel took effect or what the
//! provider did. What the window is told is what the authority answered, reduced to a
//! narrow projection. No dependency on Tauri or human terminals.
//!
//! There is nothing here that answers an approval. A turn that waits on one is shown as
//! waiting, and stays waiting.
mod service;
mod view;

pub use service::TurnService;
pub use view::{
    ApprovalNotice, SessionFacts, TURN_PAGE, TurnEvent, TurnEventKind, TurnList, TurnReceipt,
    TurnSummary, TurnView,
};

#[cfg(test)]
mod tests;
