//! The configuration engine: what an agent's real configuration says (F1, ADR 0028, 07 §4),
//! and the changes the user can make to it with a way back (F2a, ADR 0029).
//!
//! It holds the typed model, the approved roots, the bounded reader with its path trust and
//! link resolution, redaction, the adapters, and for changes the planner, the writer, the
//! journal and the encrypted restoration store. It runs no provider and receives no path or
//! text of an entry from the window.
//!
//! It lives in the desktop binary and not in the library the authority links, and it does
//! not name the control module or the authority, nor do they name it: the authority
//! binary never contains it (C-02, TS-SEC-03).
//!
//! Typed constructors and accessors that the tests and the phases after this one use are
//! not dead code, whichever of them the window reaches today.
//!
//! The window reaches it through the commands of `configuration.rs`, beside this module
//! because they must name the window toolkit and this module must not. Two read the
//! inventory the service keeps current; the others plan, confirm, apply, journal and undo
//! changes to Claude Code's settings (F2a, ADR 0029).
#![cfg_attr(not(test), allow(dead_code))]
mod acl;
#[cfg(test)]
mod acl_tests;
#[cfg(test)]
pub(crate) use acl::{
    ACL_GROUP, ACL_GROUP_OBJ, ACL_MASK, ACL_OTHER, ACL_USER_OBJ, set_acl_for_test,
};
#[cfg(test)]
mod adapter_tests;
mod change;
pub mod changes;
#[cfg(test)]
mod changes_tests;
mod classify;
#[cfg(test)]
mod classify_tests;
mod claude;
mod codex;
pub mod codex_plan;
#[cfg(test)]
mod codex_plan_tests;
pub mod declared;
mod fault;
mod fsutil;
mod inventory;
pub mod journal;
mod jsonspan;
mod model;
mod place;
mod plan;
#[cfg(test)]
mod plan_tests;
mod reader;
pub mod redact;
#[cfg(test)]
mod redact_tests;
pub mod restore;
mod roots;
pub mod service;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod tests;
mod tomlspan;
#[cfg(test)]
mod tomlspan_tests;
pub mod vault;
#[cfg(test)]
mod vault_tests;
mod writer;

// What the rest of the desktop uses.
pub use change::{Operation, Risk};
pub use fault::Faults;
pub use inventory::{Environment, Inventory};
pub use journal::Change;
pub use model::{Bounds, ProviderId, Scope};
pub use restore::Summary;
pub use service::{Changes, InventoryService};
pub use vault::{SecretToolStore, Vault};

// What the tests of this module name through it.
#[cfg(test)]
#[allow(unused_imports)]
pub use inventory::{InventoryError, Item, Outcome, SourceKey, SourceReport, inventory};
#[cfg(test)]
#[allow(unused_imports)]
pub use model::{
    ContentHash, Effective, InvalidBounds, ItemBudget, ItemKind, NotWritable, PathTrust, Persisted,
    ReadOnlyReason, SessionUse, SourceClass, SourceFailure, TooLarge, UnsupportedReason,
    UntrustedReason, writability,
};
#[cfg(test)]
#[allow(unused_imports)]
pub use reader::{ConfigReader, Entry, EntryKind, Listing, ReadFile, parse_json, parse_toml};
#[cfg(test)]
#[allow(unused_imports)]
pub use roots::{ApprovedRoots, InvalidRoot};
