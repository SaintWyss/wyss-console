//! Desktop-side managed sessions: canonical reads and the product's session intents.
//!
//! The observer of `crate::authority` keeps its own connection and is not touched. Every
//! call here opens one short-lived `ControlClient` connection, sends typed requests and
//! closes it: no watch, no subscriber state and no connection held between calls. One call
//! runs at a time, so the desktop never holds more than one of these next to its observer.
//!
//! Nothing here decides. A session exists, closes or attaches because `wyss-control`
//! committed it; what the window is told is what the authority answered, reduced to a
//! narrow projection. No dependency on Tauri or human terminals.
mod activation;
mod service;
mod view;

pub use service::{SessionService, SessionStats};
pub use view::{
    ActivationProgress, ActivationStage, PAGE_SIZE, SessionError, SessionFailure, SessionPage,
    SessionReceipt, SessionSummary,
};

#[cfg(test)]
mod tests;
