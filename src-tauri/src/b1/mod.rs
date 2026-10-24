//! The constrained B1 candidate (docs 49–52, ADR 0058 proposed): a rootless launch plan, a trusted
//! pre-exec shim, a task workspace materialized from an admitted snapshot, and an exporter that
//! produces typed, inert records after the worker is gone.
//!
//! Nothing here is reachable from the control protocol, the desktop, `METHODS` or any provider
//! launch; the shim and the probe binaries exist only in builds with the `b1-fixture` feature and
//! the package check refuses their markers. **B1 containment and FX-H01 stay NOT_PROVEN**: this is
//! what an independent review has to look at, with the positive and negative evidence of
//! `tests/b1_process.rs`, before any gate can move (doc 51 §6).
pub mod boundary;
pub mod confine;
pub mod plan;
pub mod workspace;

/// A marker the package check looks for (`wyss-b1-synthetic-`). Defence in depth only: a constant that
/// nothing reads need not survive linking. The guard is that the module, the shim and the probe exist
/// only with the `b1-fixture` feature and that no build the packager runs enables it
/// (`tests/package.test.ts`).
pub const FIXTURE_MARKER: &str = "wyss-b1-synthetic-candidate-build";
