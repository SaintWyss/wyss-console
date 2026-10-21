//! The whole window flow against the real `wyss-f8-fixture` process: sessions, a contained
//! runtime, a lease, a binding, a turn, a native-style approval, accept/decline/cancel, a
//! stop, and recovery after the authority is killed. The runtime is a fixture child, not
//! Codex; no credential is read. Run with `--features f8-fixture`.
#![cfg(all(target_os = "linux", feature = "f8-fixture"))]
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use wyss_console::{
    contracts::{
        WireUuid,
        control::{
            ActualState, ApprovalDecision, ApprovalState, BindingState, ExecutionScope, TurnState,
        },
        identities::ManagedSessionId,
    },
    f8::gui::{F8Failure, F8Gui, SessionDetail, SocketConnector},
    sessions::AgentMode,
};

fn id() -> WireUuid {
    WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap()
}
const SCOPES: [ExecutionScope; 5] = [
    ExecutionScope::SessionCreate,
    ExecutionScope::SessionResume,
    ExecutionScope::TurnStart,
    ExecutionScope::CommandExecute,
    ExecutionScope::FileChange,
];
fn eventually<T>(mut read: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(value) = read() {
            return value;
        }
        assert!(Instant::now() < until, "deadline");
        thread::sleep(Duration::from_millis(20));
    }
}

struct Candidate {
    root: PathBuf,
    child: Option<Child>,
}
impl Candidate {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("wyss-f8c-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        for d in ["run", "state"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(d))
                .unwrap();
        }
        Self { root, child: None }
    }
    fn start(&mut self, create: bool) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_wyss-f8-fixture"));
        command
            .args(["--protocol", "2", "--runtime-root"])
            .arg(self.root.join("run"))
            .arg("--state-root")
            .arg(self.root.join("state"));
        if create {
            command.arg("--create");
        }
        self.child = Some(
            command
                .env_clear()
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
    }
    fn workspace(&self) -> String {
        self.root
            .join("state/wyss-console/workspace")
            .to_str()
            .unwrap()
            .to_owned()
    }
    fn gui(&self) -> F8Gui {
        let gui = F8Gui::local(SocketConnector::new(self.root.join("run")));
        eventually(|| gui.reconnect().authority.map(|_| ()));
        gui
    }
    /// Processes (other than this test's own) whose command line names this candidate's
    /// directory: the fixture child and its bubblewrap, nothing else can.
    fn leftovers(&self) -> Vec<u32> {
        let needle = self.root.to_str().unwrap().as_bytes().to_vec();
        let me = std::process::id();
        fs::read_dir("/proc")
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
            .filter(|pid| *pid != me)
            .filter(|pid| {
                fs::read(format!("/proc/{pid}/cmdline"))
                    .is_ok_and(|c| c.windows(needle.len()).any(|w| w == needle.as_slice()))
            })
            .collect()
    }
    fn stop(&mut self, signal: Signal) {
        let mut child = self.child.take().unwrap();
        kill(Pid::from_raw(child.id() as i32), signal).unwrap();
        child.wait().unwrap();
    }
}
impl Drop for Candidate {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn ready(gui: &F8Gui, session: &ManagedSessionId) -> SessionDetail {
    eventually(|| {
        let detail = gui.session(session.clone()).ok()?;
        (detail.session.runtime.actual == ActualState::Ready
            && detail.session.runtime.instance.is_some())
        .then_some(detail)
    })
}

/// Runs a session until a turn waits on an approval; returns what the window needs to decide.
fn until_approval(
    gui: &F8Gui,
    workspace: &str,
) -> (
    ManagedSessionId,
    wyss_console::f8::gui::FenceView,
    wyss_console::contracts::identities::ManagedTurnId,
    wyss_console::contracts::identities::ManagedApprovalId,
) {
    let session = gui.prepare(id()).unwrap().id;
    let row = gui.session(session.clone()).unwrap().session;
    gui.runtime_start(
        id(),
        session.clone(),
        row.runtime.revision,
        row.runtime.generation,
        AgentMode::LocalTools,
        Some(workspace.to_owned()),
    )
    .unwrap();
    let detail = ready(gui, &session);
    let fence = detail.session.runtime.fence.clone();
    gui.lease_grant(
        id(),
        fence.clone(),
        detail.session.revision,
        SCOPES.to_vec(),
        300,
    )
    .unwrap();
    let intent = id();
    eventually(|| {
        match gui.binding_create(
            intent.clone(),
            fence.clone(),
            detail.session.revision,
            workspace.to_owned(),
        ) {
            Ok(_) => Some(()),
            Err(e) if e.code == F8Failure::Busy => None,
            Err(e) => panic!("{e:?}"),
        }
    });
    let bound = eventually(|| {
        let d = gui.session(session.clone()).ok()?;
        (d.binding.state == BindingState::BoundAttached).then_some(d)
    });
    let turn = gui
        .turn_start(id(), fence.clone(), bound.session.revision, "ask".into())
        .unwrap()
        .id;
    let reading = eventually(|| {
        let r = gui.turn(turn.clone(), None, true).ok()?;
        r.approval.is_some().then_some(r)
    });
    let approval = reading.approval.unwrap().id;
    (session, fence, turn, approval)
}

#[test]
fn the_window_flow_runs_end_to_end_against_the_real_process() {
    let mut candidate = Candidate::new();
    candidate.start(true);
    let gui = candidate.gui();
    let (session, fence, turn, approval) = until_approval(&gui, &candidate.workspace());

    let detail = gui.approval(approval.clone(), None, None).unwrap();
    assert!(detail.approval.decidable);
    assert_eq!(
        detail.command.as_ref().map(|c| c.command.as_str()),
        Some("printf fixture")
    );
    let decided = gui
        .approval_respond(
            id(),
            fence.clone(),
            approval,
            detail.approval.revision,
            ApprovalDecision::Accept,
        )
        .unwrap();
    assert_eq!(decided.state, ApprovalState::Accepted);

    let current = gui.turn(turn.clone(), None, false).unwrap();
    gui.turn_cancel(id(), fence, turn.clone(), current.turn.turn.revision)
        .unwrap();
    let ended = eventually(|| {
        let r = gui.turn(turn.clone(), None, false).ok()?;
        r.turn.turn.state.terminal().then_some(r)
    });
    assert_eq!(ended.turn.turn.state, TurnState::Interrupted);
    let after = gui.session(session.clone()).unwrap();
    gui.runtime_stop(
        id(),
        after.session.runtime.fence.clone(),
        after.session.runtime.revision,
    )
    .unwrap();
    eventually(|| {
        let d = gui.session(session.clone()).ok()?;
        (d.session.runtime.actual == ActualState::Stopped).then_some(())
    });
    assert!(!gui.audit_latest().unwrap().page.events.is_empty());
    candidate.stop(Signal::SIGTERM);
}

#[test]
fn a_decline_and_a_cancel_are_decisions_of_their_own() {
    let mut candidate = Candidate::new();
    candidate.start(true);
    let gui = candidate.gui();
    let (_, fence, _, approval) = until_approval(&gui, &candidate.workspace());
    let detail = gui.approval(approval.clone(), None, None).unwrap();
    let declined = gui
        .approval_respond(
            id(),
            fence,
            approval,
            detail.approval.revision,
            ApprovalDecision::Decline,
        )
        .unwrap();
    assert_eq!(declined.state, ApprovalState::Declined);
    candidate.stop(Signal::SIGTERM);
}

#[test]
fn killing_the_authority_leaves_no_runtime_child_and_the_history_recovers() {
    let mut candidate = Candidate::new();
    candidate.start(true);
    let gui = candidate.gui();
    let (session, _, turn, _) = until_approval(&gui, &candidate.workspace());
    let before = gui.state().authority.unwrap();
    assert!(
        !candidate.leftovers().is_empty(),
        "the runtime child is alive"
    );
    candidate.stop(Signal::SIGKILL);
    eventually(|| candidate.leftovers().is_empty().then_some(()));

    // The window sees the loss, and does not pretend the outcome is known.
    let lost = gui.sessions(None).unwrap_err();
    assert!(matches!(lost.code, F8Failure::Unavailable));

    candidate.start(false);
    let again = candidate.gui();
    let state = again.state().authority.unwrap();
    assert_ne!(state.instance_id, before.instance_id);
    let detail = eventually(|| again.session(session.clone()).ok());
    // Nothing is restored live: the runtime is stopped and the turn that waited is not running.
    assert_eq!(detail.session.runtime.actual, ActualState::Stopped);
    let reading = again.turn(turn, None, false).unwrap();
    assert!(
        reading.turn.turn.state.terminal(),
        "{:?}",
        reading.turn.turn.state
    );
    candidate.stop(Signal::SIGTERM);
}
