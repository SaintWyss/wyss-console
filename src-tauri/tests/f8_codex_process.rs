//! The production runtime path against the real `wyss-control --protocol 2 --codex …` process:
//! the attested native Codex adapter in the isolated local-tools namespace, reached through the
//! window's service over the private socket. The `codex` executable is a wrapper around the
//! schema-derived fixture runtime (`tests/fixtures/codex_runtime.py`) with a fixture credential:
//! it speaks Codex's app-server protocol and nothing else. No real Codex, no real credential,
//! no provider quota. Requires bubblewrap and python3.
#![cfg(target_os = "linux")]
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
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
        identities::{ManagedApprovalId, ManagedSessionId, ManagedTurnId},
    },
    f8::gui::{F8Failure, F8Gui, FenceView, SessionDetail, SocketConnector},
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
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(value) = read() {
            return value;
        }
        assert!(Instant::now() < until, "deadline");
        thread::sleep(Duration::from_millis(20));
    }
}
/// Retries an intent while the authority is still working on the same one.
fn settled<T>(mut ask: impl FnMut() -> Result<T, wyss_console::f8::gui::F8Error>) -> T {
    eventually(|| match ask() {
        Ok(value) => Some(value),
        Err(e) if e.code == F8Failure::Busy => None,
        Err(e) => panic!("{e:?}"),
    })
}

struct Production {
    root: PathBuf,
    child: Option<Child>,
}
impl Production {
    /// A private tree with the fixture Codex of `scenario`; nothing is started yet.
    fn new(scenario: &str) -> Self {
        Self::with_decisions(scenario, true)
    }
    /// `all_decisions: false` is Codex 0.159.0's own offer for a command: accept and cancel only.
    fn with_decisions(scenario: &str, all_decisions: bool) -> Self {
        let root = std::env::temp_dir().join(format!("wyss-f8p-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        for dir in ["run", "state", "user", "bin", "repo/.git", "repo/work"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .recursive(true)
                .create(root.join(dir))
                .unwrap();
        }
        let write = |name: &str, bytes: &[u8], mode: u32| {
            fs::write(root.join(name), bytes).unwrap();
            fs::set_permissions(root.join(name), fs::Permissions::from_mode(mode)).unwrap();
        };
        write("user/auth.json", b"FIXTURE-CREDENTIAL", 0o600);
        write(
            "user/models_cache.json",
            br#"{"models":[{"slug":"fixture","tool_mode":"direct","experimental_supported_tools":[],"multi_agent_version":null,"apply_patch_tool_type":"freeform"}]}"#,
            0o600,
        );
        let script = format!(
            "#!/usr/bin/python3\nimport os,runpy\nos.environ['WYSS_RUNTIME_FIXTURE']={:?}\nos.environ['WYSS_FIXTURE_USER_HOME']={:?}\nos.environ['WYSS_RUNTIME_SCENARIO']={:?}\n{}import traceback,sys\ntry:\n    runpy.run_path({:?},run_name='__main__')\nexcept BaseException:\n    open({:?}+'/fixture-error.log','a').write(traceback.format_exc())\n    raise\n",
            root.to_str().unwrap(),
            root.join("user").to_str().unwrap(),
            scenario,
            if all_decisions {
                "os.environ['WYSS_F8_NATIVE_FIXTURE']='1'\n"
            } else {
                ""
            },
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/codex_runtime.py"
            ),
            root.to_str().unwrap(),
        );
        write("bin/codex", script.as_bytes(), 0o700);
        Self { root, child: None }
    }
    fn command(&self, create: bool, credential: bool) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_wyss-control"));
        command
            .args(["--protocol", "2", "--runtime-root"])
            .arg(self.root.join("run"))
            .arg("--state-root")
            .arg(self.root.join("state"))
            .arg("--codex")
            .arg(self.root.join("bin/codex"))
            .arg("--codex-home")
            .arg(self.root.join("user"));
        if credential {
            command.arg("--allow-credential");
        }
        if create {
            command.arg("--create");
        }
        let stderr = if std::env::var_os("F8P_DEBUG").is_some() {
            Stdio::inherit()
        } else {
            Stdio::null()
        };
        if std::env::var_os("F8P_KEEPENV").is_none() {
            command.env_clear();
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr);
        command
    }
    fn start(&mut self, create: bool) {
        self.child = Some(self.command(create, true).spawn().unwrap());
    }
    fn workspace(&self) -> String {
        fs::canonicalize(self.root.join("repo/work"))
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
    }
    fn gui(&self) -> F8Gui {
        let gui = F8Gui::local(SocketConnector::new(self.root.join("run")));
        eventually(|| gui.reconnect().authority.map(|_| ()));
        gui
    }
    fn stop(&mut self, signal: Signal) {
        let mut child = self.child.take().unwrap();
        kill(Pid::from_raw(child.id() as i32), signal).unwrap();
        child.wait().unwrap();
    }
    fn record(&self, name: &str) -> usize {
        fs::read_to_string(self.root.join(name))
            .unwrap_or_default()
            .lines()
            .count()
    }
    fn leftovers(&self) -> usize {
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
            .count()
    }
}
impl Drop for Production {
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

/// A session with a ready runtime, an active lease and an attached binding.
fn session(gui: &F8Gui, workspace: &str, ttl: u32) -> (ManagedSessionId, FenceView, SessionDetail) {
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
    let ready = ready(gui, &session);
    let fence = ready.session.runtime.fence.clone();
    gui.lease_grant(
        id(),
        fence.clone(),
        ready.session.revision,
        SCOPES.to_vec(),
        ttl,
    )
    .unwrap();
    let intent = id();
    settled(|| {
        gui.binding_create(
            intent.clone(),
            fence.clone(),
            ready.session.revision,
            workspace.to_owned(),
        )
    });
    let attached = eventually(|| {
        let d = gui.session(session.clone()).ok()?;
        (d.binding.state == BindingState::BoundAttached).then_some(d)
    });
    (session, fence, attached)
}
/// A turn that proposed an action and waits for it to be decided.
fn turn_with_approval(
    gui: &F8Gui,
    fence: &FenceView,
    detail: &SessionDetail,
) -> (ManagedTurnId, ManagedApprovalId, u64) {
    let intent = id();
    let turn = settled(|| {
        gui.turn_start(
            intent.clone(),
            fence.clone(),
            detail.session.revision,
            "work".into(),
        )
    })
    .id;
    let notice = eventually(|| gui.turn(turn.clone(), None, true).ok()?.approval);
    let approval = gui.approval(notice.id.clone(), None, None).unwrap();
    (turn, notice.id, approval.approval.revision)
}

#[test]
fn slow_version_preparation_does_not_block_control_and_cancel_keeps_capacity_until_cleanup() {
    let mut p = Production::new("approval_command");
    let executable = p.root.join("bin/codex");
    let script = fs::read_to_string(&executable).unwrap();
    fs::write(
        &executable,
        script.replace(
            "import os,runpy",
            "import os,runpy,sys,time\nif sys.argv[1:] == ['--version']: time.sleep(1.5)",
        ),
    )
    .unwrap();
    p.start(true);
    let gui = p.gui();
    let session = gui.prepare(id()).unwrap().id;
    let row = gui.session(session.clone()).unwrap().session;
    let began = Instant::now();
    gui.runtime_start(
        id(),
        session.clone(),
        row.runtime.revision,
        row.runtime.generation,
        AgentMode::LocalTools,
        Some(p.workspace()),
    )
    .unwrap();
    assert!(
        began.elapsed() < Duration::from_millis(500),
        "runtime/start blocked for {:?}",
        began.elapsed()
    );
    let began = Instant::now();
    let detail = gui.session(session.clone()).unwrap();
    assert!(began.elapsed() < Duration::from_millis(500));
    assert_ne!(detail.session.runtime.actual, ActualState::Ready);
    gui.runtime_stop(
        id(),
        detail.session.runtime.fence.clone(),
        detail.session.runtime.revision,
    )
    .unwrap();
    let other = gui.prepare(id()).unwrap().id;
    let other_row = gui.session(other.clone()).unwrap().session;
    assert_eq!(
        gui.runtime_start(
            id(),
            other,
            other_row.runtime.revision,
            other_row.runtime.generation,
            AgentMode::LocalTools,
            Some(p.workspace())
        )
        .unwrap_err()
        .code,
        F8Failure::Busy,
        "cancelled preparation still owns capacity"
    );
    assert_eq!(
        fs::read_dir(p.root.join("state/wyss-console/staging"))
            .unwrap()
            .count(),
        1
    );
    eventually(|| {
        let detail = gui.session(session.clone()).ok()?;
        (detail.session.runtime.actual == ActualState::Stopped).then_some(())
    });
    let cwds = fs::read_to_string(p.root.join("cwds")).unwrap_or_default();
    assert!(
        !cwds.contains("local-"),
        "cancelled preparation launched payload: {cwds}"
    );
    assert_eq!(
        fs::read_dir(p.root.join("state/wyss-console/staging"))
            .map(|d| d.count())
            .unwrap_or(0),
        0
    );
    p.stop(Signal::SIGTERM);
    eventually(|| (p.leftovers() == 0).then_some(()));
}

#[test]
fn timed_out_version_probe_is_reaped_before_capacity_and_staging_are_released() {
    let mut p = Production::new("approval_command");
    let executable = p.root.join("bin/codex");
    let original = fs::read_to_string(&executable).unwrap();
    fs::write(
        &executable,
        original.replace(
            "import os,runpy",
            "import os,runpy,sys,time\nif sys.argv[1:] == ['--version']: time.sleep(10)",
        ),
    )
    .unwrap();
    p.start(true);
    let gui = p.gui();
    let session = gui.prepare(id()).unwrap().id;
    let row = gui.session(session.clone()).unwrap().session;
    gui.runtime_start(
        id(),
        session.clone(),
        row.runtime.revision,
        row.runtime.generation,
        AgentMode::LocalTools,
        Some(p.workspace()),
    )
    .unwrap();
    eventually(|| {
        gui.session(session.clone())
            .ok()
            .filter(|d| d.session.runtime.actual == ActualState::Failed)
    });
    assert_eq!(
        fs::read_dir(p.root.join("state/wyss-console/staging"))
            .unwrap()
            .count(),
        0
    );
    // Repair only our fixture executable and prove capacity was not lost or released early.
    fs::write(&executable, original).unwrap();
    let row = gui.session(session.clone()).unwrap().session;
    gui.runtime_start(
        id(),
        session.clone(),
        row.runtime.revision,
        row.runtime.generation,
        AgentMode::LocalTools,
        Some(p.workspace()),
    )
    .unwrap();
    let ready = ready(&gui, &session);
    gui.runtime_stop(
        id(),
        ready.session.runtime.fence,
        ready.session.runtime.revision,
    )
    .unwrap();
    p.stop(Signal::SIGTERM);
    eventually(|| (p.leftovers() == 0).then_some(()));
}

#[test]
fn the_real_process_starts_the_isolated_runtime_and_runs_a_command_only_when_accepted() {
    for decision in [
        ApprovalDecision::Accept,
        ApprovalDecision::Decline,
        ApprovalDecision::Cancel,
    ] {
        let mut p = Production::new("approval_command");
        p.start(true);
        let gui = p.gui();
        let (session, fence, attached) = session(&gui, &p.workspace(), 300);
        let runtime = &attached.session.runtime;
        assert_eq!(runtime.version.as_deref(), Some("0.159.0"));
        assert!(
            runtime
                .workspace
                .as_deref()
                .is_some_and(|w| w == p.workspace())
        );
        assert!(
            runtime.model_count > 0,
            "the catalogue was read and clamped"
        );
        let (turn, approval, revision) = turn_with_approval(&gui, &fence, &attached);
        let shown = gui.approval(approval.clone(), None, None).unwrap();
        assert_eq!(
            shown.command.as_ref().map(|c| c.command.as_str()),
            Some("printf WYSS_FIXTURE_COMMAND")
        );
        assert_eq!(p.record("executions"), 0, "nothing ran before the decision");
        let intent = id();
        let receipt = settled(|| {
            gui.approval_respond(
                intent.clone(),
                fence.clone(),
                approval.clone(),
                revision,
                decision,
            )
        });
        assert_eq!(
            receipt.state,
            match decision {
                ApprovalDecision::Accept => ApprovalState::Accepted,
                ApprovalDecision::Decline => ApprovalState::Declined,
                ApprovalDecision::Cancel => ApprovalState::Cancelled,
            }
        );
        let ended = eventually(|| {
            let r = gui.turn(turn.clone(), None, false).ok()?;
            r.turn.turn.state.terminal().then_some(r)
        });
        assert_eq!(
            ended.turn.turn.state,
            if decision == ApprovalDecision::Cancel {
                TurnState::Interrupted
            } else {
                TurnState::Completed
            }
        );
        assert_eq!(
            p.record("executions"),
            usize::from(decision == ApprovalDecision::Accept),
            "{decision:?}"
        );
        // Stop: the runtime is reaped and nothing of the namespace is left.
        let now = gui.session(session.clone()).unwrap();
        gui.runtime_stop(
            id(),
            now.session.runtime.fence.clone(),
            now.session.runtime.revision,
        )
        .unwrap();
        eventually(|| {
            let d = gui.session(session.clone()).ok()?;
            (d.session.runtime.actual == ActualState::Stopped).then_some(())
        });
        p.stop(Signal::SIGTERM);
        eventually(|| (p.leftovers() == 0).then_some(()));
    }
}

#[test]
fn a_file_change_is_written_only_when_accepted() {
    for decision in [ApprovalDecision::Accept, ApprovalDecision::Decline] {
        let mut p = Production::new("approval_file");
        p.start(true);
        let gui = p.gui();
        let (_, fence, attached) = session(&gui, &p.workspace(), 300);
        let (turn, approval, revision) = turn_with_approval(&gui, &fence, &attached);
        let shown = gui.approval(approval.clone(), None, None).unwrap();
        assert_eq!(shown.files, Some(1));
        assert_eq!(p.record("mutations"), 0);
        let intent = id();
        settled(|| {
            gui.approval_respond(
                intent.clone(),
                fence.clone(),
                approval.clone(),
                revision,
                decision,
            )
        });
        eventually(|| {
            let r = gui.turn(turn.clone(), None, false).ok()?;
            r.turn.turn.state.terminal().then_some(())
        });
        assert_eq!(
            p.record("mutations"),
            usize::from(decision == ApprovalDecision::Accept)
        );
        p.stop(Signal::SIGTERM);
    }
}

#[test]
fn the_inference_profile_is_refused_and_nothing_is_started() {
    let mut p = Production::new("approval_command");
    p.start(true);
    let gui = p.gui();
    let session = gui.prepare(id()).unwrap().id;
    let row = gui.session(session.clone()).unwrap().session;
    let refused = gui
        .runtime_start(
            id(),
            session.clone(),
            row.runtime.revision,
            row.runtime.generation,
            AgentMode::Inference,
            None,
        )
        .unwrap_err();
    assert_eq!(refused.code, F8Failure::Invalid);
    assert_eq!(p.record("pids"), 0, "no process of the runtime was started");
    p.stop(Signal::SIGTERM);
}

#[test]
fn a_credential_is_never_bound_unless_the_operator_said_so() {
    let p = Production::new("approval_command");
    // The same command without `--allow-credential`: the authority will not start.
    let status = p.command(true, false).status().unwrap();
    assert_eq!(status.code(), Some(2));
    assert!(!p.root.join("run/wyss-console/control.sock").exists());
    // Without a credential in the home it starts without the flag, and a launch has none to bind.
    fs::remove_file(p.root.join("user/auth.json")).unwrap();
    let mut child = p.command(true, false).spawn().unwrap();
    let gui = p.gui();
    assert!(gui.state().authority.is_some());
    // A launch binds nothing: the runtime sees an empty placeholder where a credential would be.
    let (session, _, _) = session(&gui, &p.workspace(), 300);
    let seen: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(p.root.join("local.json")).unwrap()).unwrap();
    assert_eq!(seen["credential"], "", "no credential crossed");
    // And one that appears later is refused at the next start, not bound.
    let now = gui.session(session.clone()).unwrap();
    gui.runtime_stop(
        id(),
        now.session.runtime.fence.clone(),
        now.session.runtime.revision,
    )
    .unwrap();
    eventually(|| {
        let d = gui.session(session.clone()).ok()?;
        (d.session.runtime.actual == ActualState::Stopped).then_some(())
    });
    fs::write(p.root.join("user/auth.json"), "FIXTURE-CREDENTIAL").unwrap();
    fs::set_permissions(
        p.root.join("user/auth.json"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let again = gui.prepare(id()).unwrap().id;
    let row = gui.session(again.clone()).unwrap().session;
    let refused = gui.runtime_start(
        id(),
        again,
        row.runtime.revision,
        row.runtime.generation,
        AgentMode::LocalTools,
        Some(p.workspace()),
    );
    // The start is accepted by the authority and the launch is refused: no runtime of it is Ready.
    let _ = refused;
    thread::sleep(Duration::from_millis(500));
    let detail = gui.sessions(None).unwrap();
    assert!(
        detail
            .sessions
            .iter()
            .all(|row| row.runtime.actual != ActualState::Ready),
        "a credential that appeared later was bound"
    );
    kill(Pid::from_raw(child.id() as i32), Signal::SIGTERM).unwrap();
    assert!(child.wait().unwrap().success());
}

#[test]
fn a_refused_start_leaves_nothing_behind() {
    let t = Production::new("approval_command");
    // No `--allow-credential` and a credential present: refused before anything is created.
    assert_eq!(t.command(true, false).status().unwrap().code(), Some(2));
    for path in [
        "state/wyss-console/authority.sqlite3",
        "state/wyss-console/staging",
        "run/wyss-console/runtime-cwd",
        "run/wyss-console/control.sock",
    ] {
        assert!(!t.root.join(path).exists(), "{path}");
    }
}

#[test]
fn a_runtime_that_crashes_is_reported_failed_and_leaves_no_process() {
    let mut p = Production::new("approval_command_crash_after");
    p.start(true);
    let gui = p.gui();
    let (session, fence, attached) = session(&gui, &p.workspace(), 300);
    let (_, approval, revision) = turn_with_approval(&gui, &fence, &attached);
    let intent = id();
    settled(|| {
        gui.approval_respond(
            intent.clone(),
            fence.clone(),
            approval.clone(),
            revision,
            ApprovalDecision::Accept,
        )
    });
    let failed = eventually(|| {
        let d = gui.session(session.clone()).ok()?;
        (d.session.runtime.actual == ActualState::Failed).then_some(d)
    });
    assert!(failed.session.runtime.diagnostic.is_some());
    // The fence the window held belongs to a launch that is gone: nothing is decided against it.
    let stale = gui.turn_start(id(), fence, failed.session.revision, "again".into());
    assert!(matches!(
        stale.unwrap_err().code,
        F8Failure::Stale | F8Failure::PermissionDenied | F8Failure::RuntimeUnavailable
    ));
    p.stop(Signal::SIGTERM);
    eventually(|| (p.leftovers() == 0).then_some(()));
}

#[test]
fn an_expired_lease_refuses_the_next_action_and_a_restart_recovers_the_history() {
    let mut p = Production::new("approval_command");
    p.start(true);
    let gui = p.gui();
    // The shortest lease the window can ask for.
    let (session, fence, attached) = session(&gui, &p.workspace(), 1);
    thread::sleep(Duration::from_millis(1600));
    let late = gui.turn_start(id(), fence, attached.session.revision, "work".into());
    assert!(
        matches!(late.unwrap_err().code, F8Failure::PermissionDenied),
        "an expired lease starts nothing"
    );
    // The authority dies hard; the window sees the loss and does not pretend to know.
    let before = gui.state().authority.unwrap();
    p.stop(Signal::SIGKILL);
    eventually(|| (p.leftovers() == 0).then_some(()));
    assert_eq!(gui.sessions(None).unwrap_err().code, F8Failure::Unavailable);
    // Restarted on the same image: another instance, the session kept, the runtime stopped.
    p.start(false);
    let again = p.gui();
    assert_ne!(
        again.state().authority.unwrap().instance_id,
        before.instance_id
    );
    let detail = eventually(|| again.session(session.clone()).ok());
    assert_eq!(detail.session.runtime.actual, ActualState::Stopped);
    p.stop(Signal::SIGTERM);
}

#[test]
fn a_damaged_image_is_refused_and_not_repaired() {
    let mut p = Production::new("approval_command");
    p.start(true);
    let _ = p.gui();
    p.stop(Signal::SIGTERM);
    let image = p.root.join("state/wyss-console/authority.sqlite3");
    let mut bytes = fs::read(&image).unwrap();
    // Zero a page in the middle of the file: the image no longer validates.
    let middle = bytes.len() / 2;
    for byte in &mut bytes[middle..middle + 4096] {
        *byte = 0;
    }
    fs::write(&image, &bytes).unwrap();
    let before = fs::read(&image).unwrap();
    let mut again = p.command(false, true).spawn().unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = again.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < end, "it kept running over a damaged image");
        thread::sleep(Duration::from_millis(20));
    };
    assert!(!status.success());
    assert!(!p.root.join("run/wyss-console/control.sock").exists());
    assert_eq!(
        fs::read(&image).unwrap(),
        before,
        "the image was not touched"
    );
}

#[test]
fn a_decision_codex_did_not_offer_is_refused_and_the_others_still_work() {
    // Codex 0.159.0 offers a command approval as accept / accept-with-amendment / cancel:
    // there is no decline for it. The authority never writes a decision that was not offered.
    let mut p = Production::with_decisions("approval_command", false);
    p.start(true);
    let gui = p.gui();
    let (_, fence, attached) = session(&gui, &p.workspace(), 300);
    let (turn, approval, revision) = turn_with_approval(&gui, &fence, &attached);
    let shown = gui.approval(approval.clone(), None, None).unwrap();
    assert_eq!(
        shown.decisions,
        vec![ApprovalDecision::Accept, ApprovalDecision::Cancel]
    );
    let intent = id();
    let refused = eventually(|| {
        match gui.approval_respond(
            intent.clone(),
            fence.clone(),
            approval.clone(),
            revision,
            ApprovalDecision::Decline,
        ) {
            Err(e) if e.code == F8Failure::Busy => None,
            other => Some(other),
        }
    });
    assert!(refused.is_err());
    assert_eq!(p.record("decisions"), 0, "nothing was written to Codex");
    let still_pending = gui.approval(approval.clone(), None, None).unwrap();
    assert_eq!(still_pending.approval.state, ApprovalState::Pending);
    assert_eq!(still_pending.approval.revision, revision);
    let accepted_intent = id();
    settled(|| {
        gui.approval_respond(
            accepted_intent.clone(),
            fence.clone(),
            approval.clone(),
            revision,
            ApprovalDecision::Accept,
        )
    });
    eventually(|| {
        gui.turn(turn.clone(), None, false)
            .ok()
            .filter(|t| t.turn.turn.state.terminal())
    });
    assert_eq!(p.record("decisions"), 1);
    assert_eq!(p.record("executions"), 1);
    p.stop(Signal::SIGTERM);
}

/// Manual: the real Codex 0.159.0 in the real namespace with **no credential at all**: the home
/// holds a copy of the user's model cache and nothing else, so no provider is contacted for
/// anything that needs an account. `F8_REAL_CODEX=<path> F8_REAL_CACHE=<models_cache.json>`.
#[test]
#[ignore = "needs the real Codex executable and a model cache"]
fn real_codex_without_a_credential_reaches_ready_and_lists_models() {
    let (Some(codex), Some(cache)) = (
        std::env::var_os("F8_REAL_CODEX"),
        std::env::var_os("F8_REAL_CACHE"),
    ) else {
        panic!("F8_REAL_CODEX and F8_REAL_CACHE name the executable and a models_cache.json");
    };
    let mut p = Production::new("unused");
    fs::remove_file(p.root.join("user/auth.json")).unwrap();
    fs::copy(&cache, p.root.join("user/models_cache.json")).unwrap();
    fs::set_permissions(
        p.root.join("user/models_cache.json"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_wyss-control"));
    command
        .args(["--protocol", "2", "--runtime-root"])
        .arg(p.root.join("run"))
        .arg("--state-root")
        .arg(p.root.join("state"))
        .args(["--create", "--codex"])
        .arg(&codex)
        .arg("--codex-home")
        .arg(p.root.join("user"))
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    p.child = Some(command.spawn().unwrap());
    let gui = p.gui();
    let session = gui.prepare(id()).unwrap().id;
    let row = gui.session(session.clone()).unwrap().session;
    let started = Instant::now();
    gui.runtime_start(
        id(),
        session.clone(),
        row.runtime.revision,
        row.runtime.generation,
        AgentMode::LocalTools,
        Some(p.workspace()),
    )
    .unwrap();
    let detail = ready(&gui, &session);
    eprintln!(
        "REAL ready in {:?}: version {:?}, {} models, profile {:?}, workspace {:?}",
        started.elapsed(),
        detail.session.runtime.version,
        detail.session.runtime.model_count,
        detail.session.runtime.profile,
        detail.session.runtime.workspace
    );
    assert_eq!(detail.session.runtime.version.as_deref(), Some("0.159.0"));
    assert!(detail.session.runtime.model_count > 0);
    let models = gui.models(detail.session.runtime.fence.clone()).unwrap();
    eprintln!(
        "REAL models: {} complete={:?}",
        models.entries.len(),
        models.complete
    );
    // A lease and a binding need no account; a turn does, and there is none to use.
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
    settled(|| {
        gui.binding_create(
            intent.clone(),
            fence.clone(),
            detail.session.revision,
            p.workspace(),
        )
    });
    let bound = eventually(|| {
        let d = gui.session(session.clone()).ok()?;
        (d.binding.state == BindingState::BoundAttached).then_some(d)
    });
    eprintln!(
        "REAL bound: model in use {:?}",
        bound.binding.observed_model
    );
    let intent = id();
    let turn = settled(|| {
        gui.turn_start(
            intent.clone(),
            fence.clone(),
            bound.session.revision,
            "say hello".into(),
        )
    })
    .id;
    let ended = eventually(|| {
        let r = gui.turn(turn.clone(), None, false).ok()?;
        r.turn.turn.state.terminal().then_some(r)
    });
    eprintln!(
        "REAL turn without credential ended {:?} reason {:?}",
        ended.turn.turn.state, ended.turn.turn.reason
    );
    gui.runtime_stop(
        id(),
        detail.session.runtime.fence.clone(),
        detail.session.runtime.revision,
    )
    .unwrap();
    eventually(|| {
        let d = gui.session(session.clone()).ok()?;
        (d.session.runtime.actual == ActualState::Stopped).then_some(())
    });
    p.stop(Signal::SIGTERM);
    eventually(|| (p.leftovers() == 0).then_some(()));
}
