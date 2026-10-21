//! External authority foundation. No dependency on Tauri or human terminals.
mod anchor;
#[cfg(target_os = "linux")]
pub mod application;
pub(crate) mod approval_action;
pub mod client;
mod conversations;
#[cfg(test)]
pub(crate) mod f8_fixture;
pub(crate) mod framing;
mod gate;
mod mediation;
// The safe-path rules are the authority's; the desktop's installer follows the same ones.
pub(crate) mod paths;
mod runtime;
pub(crate) mod schema;
mod server;
pub(crate) mod store;
pub(crate) mod turn_stream;
mod turns;
mod watch;
pub use application::run_from_environment;
#[cfg(target_os = "linux")]
pub use server::F8Transport;
pub(crate) use server::peer;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod turn_approval_tests;
#[cfg(test)]
mod turn_tests;

#[cfg(test)]
pub(crate) use turn_stream::fixture_page as stream_page_fixture;
