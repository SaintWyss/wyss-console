//! `wyss-console --help` is usage and an exit: no window, no toolkit, no authority, no state.
//! The process runs with an empty environment (no display, no session bus) and a private home
//! that must still be empty afterwards.
use std::{
    fs,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn run(args: &[&str]) -> (std::process::ExitStatus, String, String, std::path::PathBuf) {
    let home = std::env::temp_dir().join(format!("wyss-help-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&home).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_wyss-console"))
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // It answers at once or it is not usage: a window or a server would still be running.
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "--help did not exit");
        std::thread::sleep(Duration::from_millis(20));
    };
    let output = child.wait_with_output().unwrap();
    (
        status,
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        home,
    )
}

#[test]
fn help_prints_usage_and_exits_without_touching_anything() {
    for args in [
        vec!["--help"],
        vec!["-h"],
        // Help wins over the rest: no authority is looked for, no private root is made.
        vec!["--f8-authority", "/nonexistent-wyss-help", "--help"],
        vec!["--gui-only", "--help"],
    ] {
        let (status, out, err, home) = run(&args);
        assert!(status.success(), "{args:?}: {status:?} {err}");
        assert!(out.starts_with("Wyss Console "), "{args:?}: {out}");
        for flag in ["--f8-authority", "--gui-only", "--diagnose", "--help"] {
            assert!(out.contains(flag), "{args:?} does not mention {flag}");
        }
        assert!(err.is_empty(), "{args:?}: stderr {err}");
        assert_eq!(
            fs::read_dir(&home).unwrap().count(),
            0,
            "{args:?} left state in the home"
        );
        // Nothing of the GUI-only mode (a private root in the temp directory) was made either.
        assert!(!std::path::Path::new("/nonexistent-wyss-help").exists());
        fs::remove_dir(&home).unwrap();
    }
}
