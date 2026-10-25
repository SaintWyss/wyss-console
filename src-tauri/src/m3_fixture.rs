//! The Claude Managed fixture runtime, hosted by the real desktop binary. **Only built with the
//! `m3-fixture` feature**: no packaged build contains it and the package check refuses its marker.
//!
//! It lets the integrated application be driven with Native sessions and a Managed fixture in the
//! same window, over disposable roots: `--m3-fixture-sdk-root`, `--m3-fixture-state`,
//! `--m3-fixture-workspace` and `--m3-fixture-script` (all four, absolute). The arguments are taken
//! out before the protocol-2 selection reads the rest.
use std::ffi::OsString;
use std::{path::PathBuf, sync::Arc};
use wyss_console::{
    f8::claude::launch::ClaudeInstall,
    m3::{
        fixture::{FixtureHost, FixtureOptions, extension},
        gui::M3Service,
    },
};

const NAMES: [&str; 4] = [
    "--m3-fixture-sdk-root",
    "--m3-fixture-state",
    "--m3-fixture-workspace",
    "--m3-fixture-script",
];

/// The fixture's service and extension, if all four arguments were given, and the arguments that
/// are not the fixture's.
pub fn take(
    args: impl IntoIterator<Item = OsString>,
) -> (Option<(M3Service, wyss_console::Extension)>, Vec<OsString>) {
    let mut found: [Option<PathBuf>; 4] = [None, None, None, None];
    let mut rest = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match NAMES.iter().position(|n| arg == *n) {
            Some(i) => match args.next() {
                Some(value) => found[i] = Some(PathBuf::from(value)),
                None => fail(&format!("{} needs a value", NAMES[i])),
            },
            None => rest.push(arg),
        }
    }
    if found.iter().all(Option::is_none) {
        return (None, rest);
    }
    let [
        Some(sdk_root),
        Some(state_dir),
        Some(workspace),
        Some(script),
    ] = found
    else {
        fail("the four --m3-fixture-* arguments go together");
    };
    let install = ClaudeInstall::new(sdk_root.clone());
    let host = FixtureHost::start(FixtureOptions {
        sdk_root,
        state_dir,
        workspace,
        script,
    });
    match host {
        Ok(host) => {
            let service = M3Service::with(Some(install), Some(Arc::new(host.clone())));
            (Some((service, extension(host))), rest)
        }
        Err(why) => {
            // The window still opens and says why (an installation the verifier refuses).
            eprintln!("m3 fixture host not started: {why}");
            (
                Some((M3Service::with(Some(install), None), empty_extension())),
                rest,
            )
        }
    }
}

fn empty_extension() -> wyss_console::Extension {
    wyss_console::Extension {
        commands: &[],
        handler: Box::new(|_| false),
        install: Box::new(|b| b),
        on_exit: None,
    }
}

fn fail(text: &str) -> ! {
    eprintln!("wyss-console: {text}");
    std::process::exit(2);
}
