//! Test-only seeder for the native E2E harness. It gives the disposable authority that a
//! desktop window reads two managed sessions in one workspace, each with completed turns,
//! through the same services the window uses. It starts no authority, reads no production
//! state and runs only when the harness asks for it (`--ignored`, with the sandbox named).
//!
//! It fails closed: the sandbox root must be named, the home and the runtime directory must
//! lie inside it, and nothing is sent otherwise.
#![cfg(target_os = "linux")]

use std::{
    env, fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use wyss_console::{
    contracts::{WireUuid, identities::ManagedSessionId},
    sessions::{ActivationStage, SessionService},
    turns::{TurnService, TurnView},
};

fn id() -> WireUuid {
    WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap()
}

/// The runtime directory of the sandbox, after proving it is one.
fn sandbox_runtime() -> PathBuf {
    let sandbox = PathBuf::from(
        env::var_os("WYSS_E2E_SANDBOX").expect("WYSS_E2E_SANDBOX names the sandbox root"),
    );
    let home = PathBuf::from(env::var_os("HOME").expect("HOME is set"));
    let runtime = PathBuf::from(env::var_os("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR is set"));
    assert!(sandbox.is_absolute(), "the sandbox root is absolute");
    assert_eq!(home, sandbox.join("home"), "HOME is the sandbox home");
    assert!(
        runtime.starts_with(&sandbox),
        "the runtime {runtime:?} is outside the sandbox"
    );
    runtime
}

/// A session prepared and activated on `workspace`, as the window does it.
fn create(sessions: &SessionService, workspace: &str) -> ManagedSessionId {
    let session = sessions.prepare(id()).unwrap().id;
    let operation = id();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let progress = sessions
            .activate(operation.clone(), session.clone(), 1, workspace.to_owned())
            .unwrap();
        if progress.stage == ActivationStage::Ready {
            return session;
        }
        assert!(Instant::now() < deadline, "activation at {progress:?}");
        thread::sleep(Duration::from_millis(20));
    }
}

/// Runs one turn to its end and returns once it has ended and nothing more is held.
fn run_turn(turns: &TurnService, session: &ManagedSessionId, prompt: String) {
    let revision = turns.list(session.clone()).unwrap().session.revision;
    let receipt = turns
        .start(id(), session.clone(), revision, prompt)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut cursor = None;
    loop {
        let view: TurnView = turns.read(receipt.id.clone(), cursor, true).unwrap();
        cursor = view.next;
        if !view.turn.cancellable && !view.more {
            return;
        }
        assert!(Instant::now() < deadline, "turn stalled at {:?}", view.turn);
    }
}

#[test]
#[ignore = "native E2E seeder: run by scripts/e2e-native.mjs seed"]
fn seed_two_sessions_in_one_workspace() {
    let runtime = sandbox_runtime();
    let turns_each: usize = env::var("WYSS_E2E_TURNS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(45);
    let workspace = env::var("WYSS_E2E_WORKSPACE").expect("WYSS_E2E_WORKSPACE names the project");
    let report = env::var("WYSS_E2E_REPORT").expect("WYSS_E2E_REPORT names the output file");
    assert!(
        Path::new(&workspace).is_absolute(),
        "the workspace is absolute"
    );
    // A test binary must not be able to write a report or run a session in an
    // arbitrary host directory through environment variables.
    let sandbox = PathBuf::from(env::var_os("WYSS_E2E_SANDBOX").unwrap());
    assert_eq!(Path::new(&workspace), sandbox.join("workspace"));
    assert_eq!(Path::new(&report), sandbox.join("seed-report.json"));

    let sessions = SessionService::new(Some(runtime));
    let turns = TurnService::new(sessions.clone());
    let first = create(&sessions, &workspace);
    let second = create(&sessions, &workspace);

    for (label, session) in [("A", &first), ("B", &second)] {
        for n in 1..=turns_each {
            run_turn(
                &turns,
                session,
                format!("E2E-{label}-{n:03} synthetic prompt"),
            );
        }
    }

    let lines = format!(
        "{{\"workspace\":{workspace:?},\"sessions\":[{:?},{:?}],\"turnsEach\":{turns_each}}}\n",
        first.as_str(),
        second.as_str(),
    );
    fs::write(report, lines).unwrap();
}
