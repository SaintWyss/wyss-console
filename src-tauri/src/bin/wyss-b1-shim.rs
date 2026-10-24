//! The B1 pre-exec shim (fixture builds only). Started by the launch plan inside the worker's
//! namespaces, before anything the worker controls runs: it closes descriptors, applies the
//! confinement layers, verifies them, reports what held on a pipe the supervisor reads, and only
//! then executes the worker. If a required layer is missing it exits 4 and the worker never runs.
//!
//!   wyss-b1-shim [--read-exec P]… [--read-only P]… [--workspace P]… [--scratch P]… [--device P]…
//!                --require N --min-abi N [--receipt-fd N] -- <program> [args…]

use std::{
    io::Write,
    os::{fd::FromRawFd, unix::process::CommandExt},
    path::PathBuf,
    process::{Command, exit},
};
use wyss_console::b1::confine::{self, Profile};

fn usage() -> ! {
    eprintln!("wyss-b1-shim: usage error");
    exit(2)
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    let mut profile = Profile::default();
    let mut receipt: Option<i32> = None;
    let mut program: Option<std::ffi::OsString> = None;
    while let Some(flag) = args.next() {
        let flag = flag.to_string_lossy().into_owned();
        if flag == "--" {
            program = args.next();
            break;
        }
        let Some(value) = args.next() else { usage() };
        let text = value.to_string_lossy().into_owned();
        match flag.as_str() {
            "--read-exec" => profile.read_exec.push(PathBuf::from(value)),
            "--read-only" => profile.read_only.push(PathBuf::from(value)),
            "--workspace" => profile.workspace.push(PathBuf::from(value)),
            "--scratch" => profile.scratch.push(PathBuf::from(value)),
            "--device" => profile.devices.push(PathBuf::from(value)),
            "--require" => profile.require = text.parse().unwrap_or_else(|_| usage()),
            "--min-abi" => profile.min_abi = text.parse().unwrap_or_else(|_| usage()),
            "--receipt-fd" => receipt = Some(text.parse().unwrap_or_else(|_| usage())),
            _ => usage(),
        }
    }
    let Some(program) = program else { usage() };
    let worker_args: Vec<_> = args.collect();
    let keep: Vec<i32> = receipt.into_iter().collect();
    let applied = match confine::apply(&profile, &keep) {
        Ok(a) => a,
        Err(missing) => {
            eprintln!(
                "wyss-b1-shim: confinement unavailable ({}, layers {:#x}); the worker was not started",
                missing.why, missing.missing
            );
            exit(4)
        }
    };
    if let Some(fd) = receipt {
        // SAFETY: the descriptor was passed to this process for this purpose and is kept open by
        // `apply`; it is closed (and marked close-on-exec) after the report.
        let mut pipe = unsafe { std::fs::File::from_raw_fd(fd) };
        let line = serde_json::json!({
            "confined": true,
            "layers": applied.layers,
            "landlock_abi": applied.landlock_abi,
            "inherited_fds_closed": applied.inherited_fds,
        });
        let _ = writeln!(pipe, "{line}");
    }
    let error = Command::new(&program).args(&worker_args).exec();
    eprintln!("wyss-b1-shim: cannot start the worker: {error}");
    exit(5)
}
