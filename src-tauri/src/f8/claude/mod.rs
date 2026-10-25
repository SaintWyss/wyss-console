//! Claude Managed on the F8 seam: the worker that speaks the authority's normalized provider
//! vocabulary on one side and the supervised bridge on the other, the authority-owned executor
//! of approved actions, and the launch plan of the bridge process.
//!
//! Admission is closed twice. The accepted protocol-2 profile enumeration and the V8 store are
//! Codex-only, so no wire request can name a Claude runtime; and the launcher and the worker each
//! refuse to start without an `Admission`, which a production build can only obtain when every
//! gate of `m3::gate` is open (none is). The fixture and the tests have their own, compiled only
//! into test builds and the `m3-fixture` feature. Everything here is built and tested against the
//! real SDK, and is unreachable from the installed authority.
pub mod admission;
pub mod credential;
pub mod diff;
#[cfg(test)]
pub(crate) mod doubles;
pub mod exec;
pub mod handoff;
pub mod launch;
pub mod proposal;
pub mod sandbox;
pub mod worker;

pub use worker::{ClaudeConfig, ClaudeWorker};

#[cfg(test)]
mod real_tests;
