//! The isolated candidate authority: `wyss-control --protocol 2` whose runtime is a fixture
//! child (not Codex) in bubblewrap, for validating the installed window without a provider.
//! Built only with `--features f8-fixture`; no package carries it. Same arguments as
//! `wyss-control --protocol 2`; its workspace is `<state>/wyss-console/workspace`.
use std::{os::unix::fs::DirBuilderExt, path::Path};
use wyss_console::{
    contracts::control::{ControlError, ControlResult},
    control::serve::{Serve, serve},
    f8::fixture::FixtureSpawner,
};

fn workspace(state: &Path) -> ControlResult<FixtureSpawner> {
    let workspace = state.join("workspace");
    match std::fs::DirBuilder::new().mode(0o700).create(&workspace) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(_) => return Err(ControlError::UnsafePath),
    }
    Ok(FixtureSpawner::new(workspace))
}

fn main() {
    eprintln!(
        "wyss-f8-fixture: candidate authority, fixture runtime (not Codex), not for production"
    );
    let result =
        Serve::parse(std::env::args_os().skip(1)).and_then(|config| serve(&config, workspace));
    if let Err(error) = result {
        eprintln!("wyss-f8-fixture: {error}");
        std::process::exit(match error {
            ControlError::AlreadyRunning
            | ControlError::UnsafePath
            | ControlError::InvalidRequest
            | ControlError::StorageIncompatible => 2,
            _ => 1,
        });
    }
}
