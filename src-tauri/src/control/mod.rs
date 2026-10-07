//! External authority foundation. No dependency on Tauri or human terminals.
pub mod client;
mod framing;
mod paths;
mod runtime;
mod schema;
mod server;
mod store;
pub use server::run_from_environment;
#[cfg(test)]
mod tests;
