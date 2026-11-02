//! Protocol-2 per-session authority, selected explicitly through `control::serve`.
//! P1 remains separate; validated V8/V9/V10 support does not imply installed adoption,
//! provider admission or production guarded coordination. See docs/ARCHITECTURE.md.
#[cfg(target_os = "linux")]
pub mod allocator;
pub mod audit;
#[cfg(target_os = "linux")]
pub mod authority;
pub mod budget;
#[cfg(target_os = "linux")]
pub mod claude;
#[cfg(all(feature = "f8-dev", target_os = "linux"))]
pub mod dev;
#[cfg(target_os = "linux")]
pub mod engine;
#[cfg(all(any(feature = "f8-dev", feature = "f8-fixture"), target_os = "linux"))]
pub mod fixture;
#[cfg(target_os = "linux")]
pub mod guard_protocol;
#[cfg(target_os = "linux")]
pub mod gui;
#[cfg(target_os = "linux")]
pub mod launcher;
#[cfg(target_os = "linux")]
pub mod migration;
#[cfg(target_os = "linux")]
pub mod operations;
#[cfg(target_os = "linux")]
pub mod ownership;
#[cfg(target_os = "linux")]
pub mod process_worker;
#[cfg(target_os = "linux")]
pub mod production;
pub mod protocol;
pub mod provider;
#[cfg(target_os = "linux")]
pub mod provider_adapter;
pub mod registry;
pub mod scheduler;
#[cfg(target_os = "linux")]
pub mod service;
#[cfg(target_os = "linux")]
pub mod staging;
#[cfg(target_os = "linux")]
pub mod v10;
#[cfg(target_os = "linux")]
pub mod v8;
#[cfg(target_os = "linux")]
pub mod v9;
#[cfg(test)]
pub(crate) mod v9_candidate;
#[cfg(target_os = "linux")]
pub mod workspace_guard;
#[cfg(target_os = "linux")]
pub mod writers;

#[cfg(test)]
#[path = "f8/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "f8/audit_tests.rs"]
mod audit_tests;

#[cfg(test)]
#[path = "f8/storage_tests.rs"]
mod storage_tests;

#[cfg(test)]
#[path = "f8/process_integration_tests.rs"]
mod process_integration_tests;

#[cfg(test)]
#[path = "f8/read_tests.rs"]
mod read_tests;

#[cfg(all(test, target_os = "linux"))]
mod acceptance;
#[cfg(all(test, target_os = "linux"))]
mod application_tests;
#[cfg(test)]
mod lifecycle_tests;

#[cfg(all(test, target_os = "linux"))]
#[path = "f8/watch_tests.rs"]
mod watch_tests;
