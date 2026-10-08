//! External authority foundation. No dependency on Tauri or human terminals.
mod approval_action;
pub mod client;
mod conversations;
mod framing;
mod gate;
mod paths;
mod runtime;
mod schema;
mod server;
mod store;
mod turn_stream;
mod turns;
pub use server::run_from_environment;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod turn_tests;
