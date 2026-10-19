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
pub(crate) mod service_tests;
pub mod store;

pub use commands::extension;

/// Opens the native service over the user's own state directory and home, or `None` when
/// there is no home to put them in.
pub fn open_from_environment() -> Option<service::NativeService> {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute());
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home.as_ref().map(|h| h.join(".local/state")))
        .map(|b| b.join("wyss-console").join("native"));
    service::NativeService::open(base?, home?, std::env::var_os("PATH"))
}
