//! External authority foundation. No dependency on Tauri, terminals or Codex.
pub mod client;
mod framing;
mod paths;
mod schema;
mod server;
mod store;
pub use server::run_from_environment;
#[cfg(test)]
mod tests;
