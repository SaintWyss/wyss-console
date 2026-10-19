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
//! Nothing here is used, and nothing is re-exported to a user, until the adapters of
//! F1-PR2 consume it, hence the allowance.
#![allow(dead_code, unused_imports)]
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
#[cfg(test)]
mod tests;

pub use inventory::{
    Environment, Inventory, InventoryError, Item, Outcome, SourceKey, SourceReport, inventory,
};
pub use model::{
    Bounds, ContentHash, Effective, InvalidBounds, ItemBudget, ItemKind, NotWritable, PathTrust,
    Persisted, ProviderId, ReadOnlyReason, Scope, SessionUse, SourceClass, SourceFailure, TooLarge,
    UnsupportedReason, UntrustedReason, writability,
};
pub use reader::{ConfigReader, Entry, EntryKind, Listing, ReadFile, parse_json, parse_toml};
pub use roots::{ApprovedRoots, InvalidRoot};
