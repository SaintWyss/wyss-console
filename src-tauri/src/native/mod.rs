//! Native agent sessions (F3): the launcher, the registry and the profiles. Desktop only: like
//! the configuration engine, it is not in the library the authority links (C-02).
pub mod catalogue;
pub mod commands;
pub mod profile;
pub mod profiles;
#[cfg(test)]
mod profiles_tests;
pub mod recipe;
#[cfg(test)]
mod recipe_tests;
pub mod registry;
pub mod resources;
pub mod service;
#[cfg(test)]
mod service_tests;
pub mod store;

pub use commands::extension;
