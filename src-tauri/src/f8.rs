//! Non-deployed F8 implementation. No server, socket, provider or installed-store entry point.
//! The production authority continues to use protocol 1, schema 7 and one shared runtime.
#[cfg(target_os = "linux")]
pub mod allocator;
pub mod audit;
#[cfg(target_os = "linux")]
pub mod migration;
pub mod protocol;
pub mod registry;
#[cfg(target_os = "linux")]
pub mod v8;

#[cfg(test)]
#[path = "f8/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "f8/audit_tests.rs"]
mod audit_tests;
