//! Claude Managed on the F8 seam: the worker that speaks the authority's normalized provider
//! vocabulary on one side and the supervised bridge on the other, the authority-owned executor
//! of approved actions, and the launch plan of the bridge process.
//!
//! Admission is closed by construction: the accepted protocol-2 profile enumeration and the V8
//! store are Codex-only, so no wire request can name a Claude runtime, and `m3::gate` records
//! the decisions that would have to be taken first. Everything here is built and tested against
//! the real SDK, and is unreachable from the installed authority.
pub mod diff;
#[cfg(test)]
pub(crate) mod doubles;
pub mod exec;
pub mod launch;
pub mod proposal;
pub mod sandbox;
pub mod worker;

pub use worker::{ClaudeConfig, ClaudeWorker};

#[cfg(test)]
mod real_tests;
