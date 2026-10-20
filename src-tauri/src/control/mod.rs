//! External authority foundation. No dependency on Tauri or human terminals.
mod anchor;
mod approval_action;
pub mod client;
mod conversations;
mod framing;
mod gate;
mod mediation;
// The safe-path rules are the authority's; the desktop's installer follows the same ones.
pub(crate) mod paths;
mod runtime;
pub(crate) mod schema;
mod server;
pub(crate) mod store;
mod turn_stream;
mod turns;
mod watch;
pub use server::run_from_environment;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod turn_approval_tests;
#[cfg(test)]
mod turn_tests;

#[cfg(test)]
pub(crate) use turn_stream::fixture_page as stream_page_fixture;
