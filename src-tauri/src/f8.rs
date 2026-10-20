//! Non-deployed F8 implementation. No server, socket, provider or installed-store entry point.
//! The production authority continues to use protocol 1, schema 7 and one shared runtime.
#[cfg(target_os = "linux")]
pub mod allocator;
pub mod audit;
pub mod budget;
#[cfg(target_os = "linux")]
pub mod engine;
#[cfg(target_os = "linux")]
pub mod migration;
#[cfg(target_os = "linux")]
pub mod ownership;
#[cfg(target_os = "linux")]
pub mod process_worker;
pub mod protocol;
pub mod registry;
pub mod scheduler;
#[cfg(target_os = "linux")]
pub mod staging;
#[cfg(target_os = "linux")]
pub mod writers;
#[cfg(target_os = "linux")]
pub mod v8;

#[cfg(test)]
#[path = "f8/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "f8/audit_tests.rs"]
mod audit_tests;
