//! M1: the project-centered workspace (`docs/contracts/workspace.md`, ADR 0052).
//!
//! The Console's presentation metadata, kept in one private versioned store outside the Native
//! registry and outside every Managed authority. This module lists what those owners already
//! hold and remembers which project and workspace shows it; it never starts, stops, resumes,
//! restarts or kills anything because a project was selected, and nothing in it grants an
//! execution, a lease or an approval.
//!
//! * `store`: the locked, compare-and-swap, crash-consistent file.
//! * `command`: the closed command envelope, its results and its errors.
//! * `apply`: what each command does to a store, given what the owners say.
//! * `owner`: the checked, non-starting view of the host and the launch it can be asked for.
//! * `migration`: the deterministic, read-only-first import of the older registry and layout.
//! * `claim`: the cooperative, exclusive terminal input claim.
//! * `service`: the facade the window's commands use.
pub mod apply;
pub mod claim;
pub mod command;
pub mod commands;
pub mod migration;
pub mod owner;
pub mod service;
pub mod store;

#[cfg(test)]
mod tests;
