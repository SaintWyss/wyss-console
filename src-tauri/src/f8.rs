//! Non-deployed F8 implementation. No server, socket, provider or installed-store entry point.
//! The production authority continues to use protocol 1, schema 7 and one shared runtime.
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
#[cfg(target_os = "linux")]
pub mod provider_adapter;
pub mod registry;
pub mod scheduler;
#[cfg(target_os = "linux")]
pub mod service;
#[cfg(target_os = "linux")]
pub mod staging;
#[cfg(target_os = "linux")]
pub mod v8;
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
