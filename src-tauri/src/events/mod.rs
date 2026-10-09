//! Desktop-side history: the authority's durable events, read and reduced.
//!
//! `wyss-control` appends one durable event for every state change it commits, to one
//! stream per store. This module reads bounded windows of that stream with `audit/read`
//! and turns each envelope into a narrow row a person can read. It mutates nothing and
//! has no method that could: it is evidence of what happened, shown as that.
//!
//! **Evidence, not state.** A row says that something was recorded: a turn was admitted,
//! an approval was asked for. It does not say what that turn or approval is now. What is
//! true now is read from the authority's own state by the session and turn services; the
//! window goes there for it and never derives it from history.
//!
//! **What crosses.** An event's position and time, a closed vocabulary of what it was,
//! Wyss's own identities of the session, turn and approval it concerns, and a few facts
//! from closed vocabularies: why a turn ended, what an approval was about in kind, which
//! scopes a lease carried. The envelope's payload never crosses: the provider's thread
//! and turn, the runtime instance, the lease, the action fingerprint and the operation
//! identity stay on this side, and an event of a kind this build does not know crosses as
//! nothing but its position and time. No dependency on Tauri or human terminals.
mod service;
mod view;

pub use service::EventService;
pub use view::{
    Disposition, EVENTS_PAGE, EventCategory, EventFacts, EventKind, EventRow, EventsPage,
};

#[cfg(test)]
mod tests;
