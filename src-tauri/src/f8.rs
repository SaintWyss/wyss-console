//! Non-deployed F8 implementation. No server, socket, provider or installed-store entry point.
//! The production authority continues to use protocol 1, schema 7 and one shared runtime.
#[cfg(target_os = "linux")]
pub mod migration;
pub mod protocol;
pub mod registry;

#[cfg(test)]
#[path = "f8/tests.rs"]
mod tests;
