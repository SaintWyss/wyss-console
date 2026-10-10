//! Preferences of the desktop: how Wyss Console looks and where it opens.
//!
//! The desktop's own, kept in a small file of this user's. Not the authority's: nothing
//! here is stored with its canonical state, sent to it or read from it.
//!
//! A preference is not an authority. Nothing in this module can enable a runtime mode,
//! remember or skip an approval, choose a workspace, name a program, a socket or a
//! service, change a safety profile or switch the record of events off, because the type
//! has no field for any of them and the window can send nothing the type does not have.
mod model;
mod service;
mod store;
#[cfg(test)]
mod tests;

pub use model::{
    CursorStyle, EventFilter, Events, FONT_SIZE, Interface, Invalid, LandingPage, Preferences,
    SCHEMA_VERSION, SCROLLBACK, Terminal, TimeFormat,
};
pub use service::{About, SettingsError, SettingsFailure, SettingsService, SettingsView, Storage};
pub use store::{Loaded, MAX_FILE, Store, StoreError};
