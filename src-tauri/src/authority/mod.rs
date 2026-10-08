//! Desktop-side reader of the external authority. No dependency on Tauri or human terminals.
//!
//! One application-lifetime service owns at most one `ControlClient` connection and reduces
//! what `wyss-control` answers to a narrow read-only view. It decides nothing: every
//! request it sends is a canonical read, and an authority that is absent, restarted,
//! incompatible or malformed only changes what the view reports.
mod link;
mod service;
mod view;

pub use link::{Connection, Fault};
pub use service::{AuthorityService, LinkStats};
pub use view::{AgentSummary, AuthoritySummary, AuthorityView};

#[cfg(test)]
mod tests;
