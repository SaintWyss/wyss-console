//! Agent session launching, persistence and profiles.
pub mod arsenal;
pub mod capabilities;
pub mod catalogue;
pub mod claude_activity;
mod extended_registry;
pub mod instruction_text;
pub mod launch_home;
pub mod launch_home_codex;
pub(crate) mod profile;
mod profiles;
pub mod prompt_queue;
#[cfg(test)]
mod profiles_tests;
mod recipe;
#[cfg(test)]
mod recipe_tests;
mod registry;
mod resources;
pub mod service;
#[cfg(test)]
pub(crate) mod service_tests;
pub mod store;
pub mod usage;

/// Opens the native service over the user's own state directory and home, or `None` when
/// there is no home to put them in.
pub fn open_from_environment() -> Option<service::NativeService> {
    let (base, home) = state_paths()?;
    service::NativeService::open(base, home, std::env::var_os("PATH"))
}

pub fn state_paths() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute());
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home.as_ref().map(|h| h.join(".local/state")))
        .map(|b| b.join("wyss-console").join("native"));
    Some((base?, home?))
}
