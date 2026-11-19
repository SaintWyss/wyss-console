//! Reads provider configuration and discovers the entries available to Arsenal and launches.
//! Shared helpers cover path permissions, secret redaction, and credential storage.

mod acl;
#[cfg(test)]
mod acl_tests;

#[cfg(test)]
mod adapter_tests;
pub mod catalogue;
mod claude;
mod codex;
mod declared;
pub mod guide;
mod inventory;
mod model;
mod opencode;
mod place;
mod project;
mod reader;
pub mod redact;
#[cfg(test)]
mod redact_tests;
mod roots;
#[cfg(test)]
mod tests;
pub(crate) mod tomlspan;
#[cfg(test)]
mod tomlspan_tests;
pub mod vault;
#[cfg(test)]
mod vault_tests;

// What the rest of the desktop uses.
pub use inventory::{Environment, Inventory, inventory};
pub use model::{Bounds, ProviderId};
pub use vault::{SecretToolStore, Vault};

// What the tests of this module name through it.
#[cfg(test)]
#[allow(unused_imports)]
pub use inventory::{InventoryError, Item, Outcome, SourceKey, SourceReport};
#[cfg(test)]
#[allow(unused_imports)]
pub use model::{
    ContentHash, Effective, InvalidBounds, ItemKind, NotWritable, PathTrust, Persisted,
    ReadOnlyReason, Scope, SourceClass, SourceFailure, TooLarge, UnsupportedReason,
    UntrustedReason, writability,
};
#[cfg(test)]
#[allow(unused_imports)]
pub use reader::{
    ConfigReader, Entry, EntryKind, Listing, ReadFile, parse_json, parse_jsonc, parse_toml,
};
#[cfg(test)]
#[allow(unused_imports)]
pub use roots::{ApprovedRoots, InvalidRoot};
