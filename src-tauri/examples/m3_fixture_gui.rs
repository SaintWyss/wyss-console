//! The desktop with a fixture Claude Managed runtime: the real SDK and CLI, a mock provider API
//! on loopback and this process as the host. For looking at and driving the M3 window offline;
//! development and isolated acceptance only.
//!
//! ```text
//! cargo run --example m3_fixture_gui --features m3-fixture -- \
//!     --sdk-root <venv> --state <dir> --workspace <dir> --script <mock-script.json>
//! ```
use std::{path::PathBuf, sync::Arc};
use wyss_console::{
    f8::{claude::launch::ClaudeInstall, gui::F8Gui},
    m3::{
        fixture::{FixtureHost, FixtureOptions, extension},
        gui::M3Service,
    },
};

fn arg(name: &str) -> PathBuf {
    let mut it = std::env::args_os();
    while let Some(a) = it.next() {
        if a == name {
            return it
                .next()
                .map(PathBuf::from)
                .unwrap_or_else(|| panic!("{name} needs a value"));
        }
    }
    panic!("missing {name}");
}

fn main() {
    let (sdk_root, state, workspace, script) = (
        arg("--sdk-root"),
        arg("--state"),
        arg("--workspace"),
        arg("--script"),
    );
    let install = ClaudeInstall::new(sdk_root.clone());
    // An installation that is refused still opens the window: it says so on the profile cards and
    // offers no fixture runtime, which is what the window must do on a machine like that.
    let host = FixtureHost::start(FixtureOptions {
        sdk_root,
        state_dir: state,
        workspace,
        script,
    });
    match host {
        Ok(host) => {
            let service = M3Service::with(Some(install), Some(Arc::new(host.clone())));
            wyss_console::run_with_m3(None, Some(extension(host)), F8Gui::inactive(), service);
        }
        Err(why) => {
            eprintln!("fixture host not started: {why}");
            wyss_console::run_with_m3(
                None,
                None,
                F8Gui::inactive(),
                M3Service::with(Some(install), None),
            );
        }
    }
}
