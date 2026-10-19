//! The configuration engine's read side: what an agent's real configuration says, read
//! without writing anything (F1, ADR 0028, 07 §4).
//!
//! This first part holds the typed model, the approved roots, the bounded reader with its
//! path trust and link resolution, and redaction. It runs no provider, receives no path
//! from the window and writes nothing anywhere.
//!
//! It lives in the desktop binary and not in the library the authority links, and it does
//! not name the control module or the authority, nor do they name it: the authority
//! binary never contains it (C-02, TS-SEC-03).
//!
//! Typed constructors and accessors that the tests and the phases after this one use are
//! not dead code, whichever of them the window reaches today.
//!
//! The window reaches it through two read commands only (`configuration.rs`, beside this
//! module, because they must name the window toolkit and this module must not); the
//! service keeps the current reading and watches the files for changes.
#![cfg_attr(not(test), allow(dead_code))]
#[cfg(test)]
mod adapter_tests;
mod claude;
mod codex;
mod inventory;
mod model;
mod place;
mod reader;
pub mod redact;
#[cfg(test)]
mod redact_tests;
mod roots;
pub mod service;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod tests;

// What the rest of the desktop uses.
pub use inventory::{Environment, Inventory};
pub use model::Bounds;
pub use service::{Changes, InventoryService};

// What the tests of this module name through it.
#[cfg(test)]
#[allow(unused_imports)]
pub use inventory::{InventoryError, Item, Outcome, SourceKey, SourceReport, inventory};
#[cfg(test)]
#[allow(unused_imports)]
pub use model::{
    ContentHash, Effective, InvalidBounds, ItemBudget, ItemKind, NotWritable, PathTrust, Persisted,
    ProviderId, ReadOnlyReason, Scope, SessionUse, SourceClass, SourceFailure, TooLarge,
    UnsupportedReason, UntrustedReason, writability,
};
#[cfg(test)]
#[allow(unused_imports)]
pub use reader::{ConfigReader, Entry, EntryKind, Listing, ReadFile, parse_json, parse_toml};
#[cfg(test)]
#[allow(unused_imports)]
pub use roots::{ApprovedRoots, InvalidRoot};
