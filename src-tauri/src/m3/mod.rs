//! M3 — Managed provider parity: the provider-neutral facts that sit beside the accepted
//! protocol-2 authority without widening it.
//!
//! Nothing here changes a wire schema, the V8 store or the runtime capacity of one. The
//! modules are separate, independently versioned capabilities, each disabled for live use
//! until the decision that governs it is accepted (see `gate`):
//!
//! - `gate`: what is authorized, as constants a reviewed change flips;
//! - `usage`: provider-reported token usage and limits, with provenance;
//! - `model`: the four facts about a model (offered, configured, requested, reported) and the
//!   per-turn request of ADR0054;
//! - `profiles`: which provider profile is available and why not;
//! - `history`: the optional local metrics history.
pub mod fixture;
pub mod gate;
pub mod gui;
pub mod history;
pub mod model;
pub mod profiles;
pub mod report;
pub mod usage;

/// The independently versioned capability these modules form. It is not the authority
/// protocol version (which stays 2) and is never inferred from it.
pub const CAPABILITY_VERSION: &str = "m3.1";

#[cfg(test)]
mod golden_tests;
