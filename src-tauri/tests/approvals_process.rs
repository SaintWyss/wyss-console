//! Local tools and approvals, as the desktop asks for them, against the real
//! `wyss-control` process in isolated temporary XDG directories. The runtime is the
//! repository's Codex fixture, launched by the authority inside its real `bwrap`
//! namespace: no real Codex is launched and no production state is read.
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
    sync::{Mutex, MutexGuard, PoisonError},
    thread,
    time::{Duration, Instant},
};
use wyss_console::{
    contracts::{
        WireUuid,
        control::*,
        identities::{ManagedSessionId, ManagedTurnId},
    },
    control::client::ControlClient,
    sessions::{
        ActivationProgress, ActivationStage, AgentMode, SessionError, SessionFailure, SessionMode,
        SessionService,
    },
    turns::{Decision, TurnEventKind, TurnService, TurnView},
};

/// A fixture executable must not be forked while it is open for writing, and the
/// provider is one process per authority: these tests run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());
fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}
fn id() -> WireUuid {
    WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap()
}
fn mutation() -> RuntimeMutation {
    RuntimeMutation {
        intent_id: id(),
        adapter_id: RuntimeTarget::codex().adapter_id,
        runtime_kind: RuntimeTarget::codex().runtime_kind,
        expected_revision: None,
        safety_profile: None,
        workspace_root: None,
    }
}
const PROMPT: &str = "WYSS_3E_PROMPT_SENTINEL list the files";

struct Sandbox {
    root: PathBuf,
    scenario: &'static str,
}
impl Sandbox {
    fn new(scenario: &'static str) -> Self {
        let root = std::env::temp_dir().join(format!("wyss-ap-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        for d in ["run", "state", "bin", "codex-home"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(d))
                .unwrap();
        }
        fs::write(
            root.join("codex-home/models_cache.json"),
            r#"{"fetched_at":"2026-10-07T00:00:00Z","models":[{"slug":"fixture-model","tool_mode":"code_mode_only","shell_type":"unified_exec","apply_patch_tool_type":"freeform","multi_agent_version":"v2","experimental_supported_tools":["send_user_message_async","clock"]}]}"#,
        )
        .unwrap();
        let fake = root.join("bin/codex");
        fs::write(&fake, include_str!("fixtures/codex_runtime.py")).unwrap();
        fs::set_permissions(fake, fs::Permissions::from_mode(0o700)).unwrap();
        // The layout a local-tools generation is isolated in. The credential is the
        // fixture's own text.
        let home = root.join("codex-home");
        fs::write(home.join("auth.json"), "FIXTURE-CREDENTIAL").unwrap();
        fs::set_permissions(home.join("auth.json"), fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(home.join("config.toml"), "approval_policy = \"never\"\n").unwrap();
        for dir in ["repo/.git", "repo/.codex", "repo/sub/ws"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(
            root.join("layers.json"),
            serde_json::to_string(&[root.join("repo/.codex")]).unwrap(),
        )
        .unwrap();
        Self { root, scenario }
    }
    fn run(&self) -> PathBuf {
        self.root.join("run")
    }
    fn workspace(&self) -> String {
        self.root.join("repo/sub/ws").to_str().unwrap().to_owned()
    }
    fn start(&self) -> Authority {
        Authority(
            Command::new(env!("CARGO_BIN_EXE_wyss-control"))
                .env("XDG_RUNTIME_DIR", self.run())
                .env("XDG_STATE_HOME", self.root.join("state"))
                .env("PATH", self.root.join("bin"))
                .env("CODEX_HOME", self.root.join("codex-home"))
                .env("WYSS_RUNTIME_FIXTURE", &self.root)
                .env("WYSS_RUNTIME_SCENARIO", self.scenario)
                .env("WYSS_FIXTURE_USER_HOME", self.root.join("codex-home"))
                .env(
                    "WYSS_FIXTURE_LAYERS",
                    fs::read_to_string(self.root.join("layers.json")).unwrap_or("[]".into()),
                )
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
    /// An independent client of the same authority, as the developer client is.
    fn client(&self) -> ControlClient {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            match ControlClient::connect_in_runtime(&self.run()) {
                Ok(c) => return c,
                Err(e) => {
                    assert!(Instant::now() < end, "client startup: {e}");
                    thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }
    fn services(&self) -> (SessionService, TurnService) {
        // The authority is up before the desktop's services are asked anything.
        drop(self.client());
        let sessions = SessionService::new(Some(self.run()));
        let turns = TurnService::new(sessions.clone());
        (sessions, turns)
    }
    fn ready(&self, state: ActualState) {
        let mut c = self.client();
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let s = c.runtime_status().unwrap();
            if s.actual_state == state {
                return;
            }
            assert!(Instant::now() < deadline, "expected {state:?}, got {s:?}");
            thread::sleep(Duration::from_millis(5));
        }
    }
    /// How many times the provider was sent `method`.
    fn sent(&self, method: &str) -> usize {
        fs::read_to_string(self.root.join("methods"))
            .unwrap_or_default()
            .lines()
            .filter(|m| *m == method)
            .count()
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
struct Authority(Child);
impl Authority {
    fn stop(&mut self, hard: bool) {
        let signal = if hard {
            Signal::SIGKILL
        } else {
            Signal::SIGTERM
        };
        kill(Pid::from_raw(self.0.id() as i32), signal).unwrap();
        let end = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(hard || status.success(), "authority exit: {status}");
                break;
            }
            assert!(Instant::now() < end, "shutdown deadline");
            thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Authority {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn failed<T: std::fmt::Debug>(result: Result<T, SessionError>) -> (SessionFailure, bool) {
    let error = result.unwrap_err();
    (error.code, error.retry)
}
/// Every durable event the authority holds, whole, in order.
fn trail(c: &mut ControlClient) -> Vec<serde_json::Value> {
    let stream = c.hello_status().stream_id.clone();
    let mut after = None;
    let mut all = Vec::new();
    loop {
        let ResultValue::Audit { events, next } = c
            .audit(AuditRead {
                stream_id: stream.clone(),
                after_sequence: after,
                limit: MAX_PAGE,
            })
            .unwrap()
        else {
            unreachable!()
        };
        all.extend(events.iter().map(|e| serde_json::to_value(e).unwrap()));
        if next.is_none() {
            return all;
        }
        after = next;
    }
}
/// What a follower sees of one turn: every reading, as the window would make them.
struct Followed {
    views: Vec<TurnView>,
    text: String,
    kinds: Vec<TurnEventKind>,
    sequences: Vec<u64>,
}
/// Reads a turn the way the window follows it, until `done` says to stop.
fn follow(turns: &TurnService, turn: &ManagedTurnId, done: impl Fn(&TurnView) -> bool) -> Followed {
    follow_from(turns, turn, None, done)
}
/// The same, from a cursor an earlier reading returned.
fn follow_from(
    turns: &TurnService,
    turn: &ManagedTurnId,
    from: Option<wyss_console::contracts::Sequence>,
    done: impl Fn(&TurnView) -> bool,
) -> Followed {
    let mut followed = Followed {
        views: Vec::new(),
        text: String::new(),
        kinds: Vec::new(),
        sequences: Vec::new(),
    };
    let (mut cursor, deadline) = (from, Instant::now() + Duration::from_secs(12));
    loop {
        let view = turns.read(turn.clone(), cursor, true).unwrap();
        cursor = view.next;
        for event in &view.events {
            followed.sequences.push(event.sequence.get());
            followed.kinds.push(event.kind);
            if let Some(text) = &event.text
                && event.kind == TurnEventKind::Text
            {
                followed.text.push_str(text);
            }
        }
        let stop = done(&view) && !view.more;
        followed.views.push(view);
        if stop {
            return followed;
        }
        assert!(
            Instant::now() < deadline,
            "turn at {:?}",
            followed.views.last().unwrap().turn
        );
    }
}
fn ended(view: &TurnView) -> bool {
    !view.turn.cancellable
}

/// A session created and activated by the desktop in `mode`, as the window does it: the
/// same request until the answer is `ready`. Returns the stages it went through.
fn activated(
    turns: &TurnService,
    sessions: &SessionService,
    workspace: &str,
    mode: AgentMode,
) -> (ManagedSessionId, Vec<ActivationStage>) {
    let session = sessions.prepare(id()).unwrap().id;
    let operation = id();
    let stages = until_ready(|| {
        turns.activate(
            operation.clone(),
            session.clone(),
            1,
            workspace.to_owned(),
            mode,
        )
    });
    (session, stages)
}
/// A bound session entered in `mode`, as the window does it.
fn entered(
    turns: &TurnService,
    session: &ManagedSessionId,
    mode: AgentMode,
) -> Vec<ActivationStage> {
    let operation = id();
    until_ready(|| turns.enter(operation.clone(), session.clone(), 1, mode))
}
fn until_ready(
    mut step: impl FnMut() -> Result<ActivationProgress, SessionError>,
) -> Vec<ActivationStage> {
    let mut stages: Vec<ActivationStage> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        let stage = step().unwrap().stage;
        if stages.last() != Some(&stage) {
            stages.push(stage);
        }
        if stage == ActivationStage::Ready {
            return stages;
        }
        assert!(Instant::now() < deadline, "stuck at {stages:?}");
        thread::sleep(Duration::from_millis(20));
    }
}
fn shown(sessions: &SessionService, session: &ManagedSessionId) -> (BindingState, SessionMode) {
    let page = sessions.page(None).unwrap();
    let it = page.sessions.iter().find(|s| &s.id == session).unwrap();
    (it.binding, it.mode)
}
/// How many answers the provider was given to a request of its own, and what they were.
fn decisions(env: &Sandbox) -> Vec<String> {
    fs::read_to_string(env.root.join("decisions"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}
fn tally(env: &Sandbox, name: &str) -> usize {
    fs::read_to_string(env.root.join(name))
        .unwrap_or_default()
        .lines()
        .count()
}
/// The scopes of the lease the authority granted last.
fn last_grant(c: &mut ControlClient) -> serde_json::Value {
    trail(c)
        .into_iter()
        .rev()
        .find(|e| e["event_type"] == "authority.execution_lease.granted")
        .map(|e| e["payload"]["scopes"].clone())
        .unwrap()
}

#[test]
fn local_tools_is_an_explicit_mode_and_a_command_runs_only_after_it_is_approved() {
    let _serial = serial();
    // This provider proposes a command and asks before running it.
    let env = Sandbox::new("approval_command");
    let mut authority = env.start();
    let (sessions, turns) = env.services();
    let workspace = env.workspace();
    let mut other = env.client();

    // Nothing runs yet. The session is created in local tools, by name: the desktop
    // asks the authority for that profile and for this one folder.
    let (session, stages) = activated(&turns, &sessions, &workspace, AgentMode::LocalTools);
    assert_eq!(
        stages,
        [
            ActivationStage::RuntimeStarting,
            ActivationStage::Bound,
            ActivationStage::Ready
        ]
    );
    let runtime = other.runtime_status().unwrap();
    assert_eq!(
        (
            runtime.runtime_safety_profile,
            runtime.workspace_root.clone()
        ),
        (
            Some(RuntimeSafetyProfile::CodexLocalToolsV1),
            Some(workspace.clone())
        )
    );
    // It ran inside the authority's namespace, with the authority's own policy.
    let inside: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(env.root.join("local.json")).unwrap()).unwrap();
    assert!(
        inside["requirements"]
            .as_str()
            .unwrap()
            .contains("unified_exec = false")
    );
    // Of the authority's own runtime directory only its read-only model catalog is there:
    // no control socket, no lock.
    assert_eq!(
        inside["authority_runtime"],
        serde_json::json!(["codex-local-tools-catalog.json"])
    );
    assert_eq!(inside["credential"], "FIXTURE-CREDENTIAL");
    // What the window is told: the session's mode, and the runtime every session shares.
    let page = sessions.page(None).unwrap();
    assert_eq!(page.sessions[0].mode, SessionMode::LocalTools);
    assert_eq!(
        serde_json::to_value(&page.runtime).unwrap(),
        serde_json::json!({"mode": "local_tools", "workspace": workspace})
    );
    assert_eq!(
        turns.list(session.clone()).unwrap().session.mode,
        SessionMode::LocalTools
    );
    assert_eq!(other.lease_active(session.clone()).unwrap(), None);

    // A turn. Its lease carries what an action of it would need, because the authority
    // accepts one only under the lease the turn was admitted under.
    let started = turns
        .start(id(), session.clone(), 1, PROMPT.into())
        .unwrap();
    assert_eq!(
        last_grant(&mut other),
        serde_json::json!(["turn_start", "command_execute", "file_change"])
    );
    let waiting = follow(&turns, &started.id, |v| v.approval.is_some());
    let notice = waiting.views.last().unwrap().approval.clone().unwrap();
    assert_eq!(notice.kind, ApprovalKind::CommandExecution);
    // Nothing ran: the provider was answered nothing.
    assert!(decisions(&env).is_empty());
    assert_eq!(tally(&env, "executions"), 0);

    // The approval, found through the durable stream and read from the authority.
    let list = turns.approvals().unwrap();
    assert_eq!((list.pending.len(), list.recent.len()), (1, 0));
    let detail = &list.pending[0];
    assert_eq!(detail.approval.id, notice.id);
    assert_eq!(
        (&detail.approval.session, &detail.approval.turn),
        (&session, &started.id)
    );
    assert!(detail.approval.decidable);
    assert_eq!(detail.approval.state, ApprovalState::Pending);
    assert_eq!(detail.workspace.as_deref(), Some(workspace.as_str()));
    // The command, whole, and where it would run. That is what is being decided.
    let command = detail.command.as_ref().unwrap();
    assert_eq!(command.command, "printf WYSS_FIXTURE_COMMAND");
    assert_eq!(command.cwd, workspace);
    assert!(detail.files.is_none() && detail.file.is_none());
    // And nothing the decision has no use for.
    let canonical = other.approval_pending(started.id.clone()).unwrap().unwrap();
    let wire = serde_json::to_string(&list).unwrap();
    for private in [
        canonical.approval.action_fingerprint.as_str(),
        canonical.approval.admitted_lease_id.as_str(),
        canonical.approval.runtime_instance_id.as_str(),
        "call-1",
        "provider_item",
        "fingerprint",
        "lease",
        "runtime_instance",
        "parsed_actions",
        "FIXTURE-CREDENTIAL",
    ] {
        assert!(!wire.contains(private), "{private} crossed to the window");
    }
    // The same approval read by its handle; a command has no file to select.
    let read = turns.approval(notice.id.clone(), None, None).unwrap();
    assert_eq!(
        serde_json::to_value(&read).unwrap(),
        serde_json::to_value(detail).unwrap()
    );
    assert_eq!(
        failed(turns.approval(notice.id.clone(), Some(0), Some(0))),
        (SessionFailure::Invalid, false)
    );

    // A decision at a revision that is not the approval's is refused, and decides nothing.
    assert_eq!(
        failed(turns.decide(
            id(),
            notice.id.clone(),
            detail.approval.revision + 1,
            Decision::Approve
        )),
        (SessionFailure::Changed, false)
    );
    assert!(decisions(&env).is_empty());
    // Approve. The receipt is the authority's; the provider is answered once.
    let intent = id();
    let receipt = turns
        .decide(
            intent.clone(),
            notice.id.clone(),
            detail.approval.revision,
            Decision::Approve,
        )
        .unwrap();
    assert_eq!(
        (&receipt.id, receipt.state),
        (&notice.id, ApprovalState::Accepted)
    );
    // The same action again is the same receipt, and nothing reaches the provider.
    for _ in 0..3 {
        assert_eq!(
            turns
                .decide(
                    intent.clone(),
                    notice.id.clone(),
                    detail.approval.revision,
                    Decision::Approve
                )
                .unwrap(),
            receipt
        );
    }
    // The same identity for the other decision, or another decision afterwards: refused.
    assert_eq!(
        failed(turns.decide(
            intent,
            notice.id.clone(),
            detail.approval.revision,
            Decision::Deny
        )),
        (SessionFailure::Conflict, false)
    );
    assert_eq!(
        failed(turns.decide(
            id(),
            notice.id.clone(),
            detail.approval.revision,
            Decision::Deny
        )),
        (SessionFailure::NotAllowed, false)
    );

    // The same turn goes on: the command runs, reports, and the turn completes.
    let rest = follow_from(
        &turns,
        &started.id,
        waiting.views.last().unwrap().next,
        ended,
    );
    let last = rest.views.last().unwrap();
    assert_eq!(
        (last.turn.state, last.turn.reason),
        (TurnState::Completed, Some(TurnReason::Completed))
    );
    use TurnEventKind as K;
    for kind in [
        K::ApprovalResolved,
        K::CommandStarted,
        K::CommandOutput,
        K::CommandCompleted,
    ] {
        assert!(rest.kinds.contains(&kind), "{kind:?} in {:?}", rest.kinds);
    }
    // What the command printed stays on the authority's side of the projection.
    assert!(
        !serde_json::to_string(&rest.views)
            .unwrap()
            .contains("WYSS_FIXTURE_OUTPUT")
    );
    assert_eq!(decisions(&env), ["\"accept\""]);
    assert_eq!(tally(&env, "executions"), 1);
    assert_eq!(env.sent("turn/start"), 1);
    // Decided: it is what the authority recorded, without its action, and cannot be
    // decided again.
    let list = turns.approvals().unwrap();
    assert!(list.pending.is_empty());
    assert_eq!(
        (
            list.recent[0].state,
            list.recent[0].decision,
            list.recent[0].decidable
        ),
        (
            ApprovalState::Accepted,
            Some(ApprovalDecision::Accept),
            false
        )
    );
    let after = turns.approval(notice.id.clone(), None, None).unwrap();
    assert!(after.command.is_none());
    assert_eq!(
        failed(turns.decide(
            id(),
            notice.id.clone(),
            after.approval.revision,
            Decision::Deny
        )),
        (SessionFailure::NotAllowed, false)
    );
    assert_eq!(decisions(&env).len(), 1);
    assert_eq!(other.lease_active(session.clone()).unwrap(), None);

    other.runtime_stop(mutation()).unwrap();
    env.ready(ActualState::Stopped);
    drop(other);
    authority.stop(false);
}

#[test]
fn a_denied_command_never_runs() {
    let _serial = serial();
    let env = Sandbox::new("approval_command");
    let mut authority = env.start();
    let (sessions, turns) = env.services();
    let (session, _) = activated(&turns, &sessions, &env.workspace(), AgentMode::LocalTools);
    let started = turns
        .start(id(), session.clone(), 1, PROMPT.into())
        .unwrap();
    let waiting = follow(&turns, &started.id, |v| v.approval.is_some());
    let notice = waiting.views.last().unwrap().approval.clone().unwrap();
    let revision = turns.approvals().unwrap().pending[0].approval.revision;

    let receipt = turns
        .decide(id(), notice.id.clone(), revision, Decision::Deny)
        .unwrap();
    assert_eq!(receipt.state, ApprovalState::Declined);
    let rest = follow_from(
        &turns,
        &started.id,
        waiting.views.last().unwrap().next,
        ended,
    );
    let last = rest.views.last().unwrap();
    // The pinned provider offers no way to refuse a command and keep going: the
    // authority answers `cancel`, and that ends the turn. It is shown as what it is.
    assert_eq!(last.turn.state, TurnState::Interrupted);
    assert_eq!(decisions(&env), ["\"cancel\""]);
    assert_eq!(tally(&env, "executions"), 0);
    assert!(!rest.kinds.contains(&TurnEventKind::CommandStarted));
    let list = turns.approvals().unwrap();
    assert!(list.pending.is_empty());
    assert_eq!(
        (list.recent[0].state, list.recent[0].decision),
        (ApprovalState::Declined, Some(ApprovalDecision::Decline))
    );
    // The session is still there for another turn, and no second turn was started.
    assert_eq!(env.sent("turn/start"), 1);
    assert!(turns.list(session).unwrap().runnable);

    let mut other = env.client();
    other.runtime_stop(mutation()).unwrap();
    env.ready(ActualState::Stopped);
    drop(other);
    authority.stop(false);
}

#[test]
fn a_file_change_is_shown_whole_denied_leaves_the_file_alone_and_approved_writes_it() {
    let _serial = serial();
    // This provider proposes to add one file and asks before writing it.
    let env = Sandbox::new("approval_file");
    let mut authority = env.start();
    let (sessions, turns) = env.services();
    let workspace = env.workspace();
    let target = PathBuf::from(&workspace).join("wyss-fixture-probe.txt");
    let (session, _) = activated(&turns, &sessions, &workspace, AgentMode::LocalTools);

    let started = turns
        .start(id(), session.clone(), 1, PROMPT.into())
        .unwrap();
    let waiting = follow(&turns, &started.id, |v| v.approval.is_some());
    let notice = waiting.views.last().unwrap().approval.clone().unwrap();
    assert_eq!(notice.kind, ApprovalKind::FileChange);
    let detail = turns.approval(notice.id.clone(), None, None).unwrap();
    // Every file of the change set can be read, with its whole diff, in pages.
    assert_eq!(detail.files, Some(1));
    assert!(detail.command.is_none());
    let file = detail.file.as_ref().unwrap();
    assert_eq!(
        (
            file.index,
            file.path.as_str(),
            file.kind,
            file.move_path.as_deref()
        ),
        (0, target.to_str().unwrap(), FileChangeKind::Add, None)
    );
    assert_eq!(
        (file.chunk.as_str(), file.bytes, file.offset, file.next),
        ("+WYSS_FIXTURE_PATCH\n", 20, 0, None)
    );
    // A file the change set does not have is not a page of it.
    assert_eq!(
        failed(turns.approval(notice.id.clone(), Some(1), Some(0))),
        (SessionFailure::Invalid, false)
    );
    assert!(!target.exists());

    // Denied: the file is not written, and this provider's turn goes on to its end.
    turns
        .decide(
            id(),
            notice.id.clone(),
            detail.approval.revision,
            Decision::Deny,
        )
        .unwrap();
    let rest = follow_from(
        &turns,
        &started.id,
        waiting.views.last().unwrap().next,
        ended,
    );
    assert_eq!(rest.views.last().unwrap().turn.state, TurnState::Completed);
    assert_eq!(decisions(&env), ["\"decline\""]);
    assert_eq!(tally(&env, "mutations"), 0);
    assert!(!target.exists());
    assert!(!rest.kinds.contains(&TurnEventKind::FileChangeStarted));

    // Another turn, approved: the same change is written, once, by the provider.
    let second = turns
        .start(id(), session.clone(), 1, PROMPT.into())
        .unwrap();
    let waiting = follow(&turns, &second.id, |v| v.approval.is_some());
    let notice = waiting.views.last().unwrap().approval.clone().unwrap();
    let revision = turns
        .approval(notice.id.clone(), None, None)
        .unwrap()
        .approval
        .revision;
    assert!(!target.exists());
    turns
        .decide(id(), notice.id.clone(), revision, Decision::Approve)
        .unwrap();
    let rest = follow_from(
        &turns,
        &second.id,
        waiting.views.last().unwrap().next,
        ended,
    );
    assert_eq!(rest.views.last().unwrap().turn.state, TurnState::Completed);
    assert!(rest.kinds.contains(&TurnEventKind::FileChangeCompleted));
    assert_eq!(fs::read_to_string(&target).unwrap(), "WYSS_FIXTURE_PATCH\n");
    assert_eq!(tally(&env, "mutations"), 1);
    // Neither the path nor the diff is in what the turn's stream gave the window.
    let wire = serde_json::to_string(&rest.views).unwrap();
    assert!(!wire.contains("wyss-fixture-probe") && !wire.contains("WYSS_FIXTURE_PATCH"));
    // Both approvals, newest first, as the authority recorded them.
    let list = turns.approvals().unwrap();
    assert_eq!(
        list.recent.iter().map(|a| a.state).collect::<Vec<_>>(),
        [ApprovalState::Accepted, ApprovalState::Declined]
    );
    assert_eq!(env.sent("turn/start"), 2);

    let mut other = env.client();
    other.runtime_stop(mutation()).unwrap();
    env.ready(ActualState::Stopped);
    drop(other);
    authority.stop(false);
}

#[test]
fn one_runtime_serves_one_mode_and_is_never_switched_under_a_running_turn() {
    let _serial = serial();
    // This provider starts a turn and does not end it.
    let env = Sandbox::new("turn_long");
    let mut authority = env.start();
    let (sessions, turns) = env.services();
    let workspace = env.workspace();
    let mut other = env.client();

    // Inference is what a session gets unless local tools is asked for by name.
    let (plain, stages) = activated(&turns, &sessions, &workspace, AgentMode::Inference);
    assert_eq!(
        stages,
        [
            ActivationStage::RuntimeStarting,
            ActivationStage::Bound,
            ActivationStage::Ready
        ]
    );
    assert_eq!(
        shown(&sessions, &plain),
        (BindingState::BoundAttached, SessionMode::Inference)
    );
    let inference = other.runtime_status().unwrap();
    assert_eq!(
        inference.runtime_safety_profile,
        Some(RuntimeSafetyProfile::CodexInferenceV1)
    );
    // An attached session is in the mode it was bound in. It is not moved to the other:
    // nothing is stopped for a request that could only be refused afterwards.
    assert_eq!(
        failed(turns.enter(id(), plain.clone(), 1, AgentMode::LocalTools)),
        (SessionFailure::ModeFixed, false)
    );
    assert_eq!(other.runtime_status().unwrap().revision, inference.revision);

    // A turn runs in it. Local tools for another session would need this runtime
    // stopped: it is not asked for, and the turn and the runtime are as they were.
    let running = turns.start(id(), plain.clone(), 1, PROMPT.into()).unwrap();
    follow(&turns, &running.id, |v| v.turn.state == TurnState::Running);
    let tools = sessions.prepare(id()).unwrap().id;
    let operation = id();
    for _ in 0..3 {
        assert_eq!(
            failed(turns.activate(
                operation.clone(),
                tools.clone(),
                1,
                workspace.clone(),
                AgentMode::LocalTools
            )),
            (SessionFailure::TurnActive, false)
        );
    }
    let still = other.runtime_status().unwrap();
    assert_eq!(
        (still.revision, &still.runtime_instance_id),
        (inference.revision, &inference.runtime_instance_id)
    );
    assert_eq!(
        turns
            .read(running.id.clone(), None, false)
            .unwrap()
            .turn
            .state,
        TurnState::Running
    );
    assert_eq!(
        shown(&sessions, &tools),
        (BindingState::Unbound, SessionMode::Unknown)
    );

    // The turn is cancelled. Now the same action goes through, step by step: the
    // runtime stops, starts in local tools for this folder, and the session is bound.
    let revision = turns
        .read(running.id.clone(), None, false)
        .unwrap()
        .turn
        .revision;
    turns.cancel(id(), running.id.clone(), revision).unwrap();
    follow(&turns, &running.id, ended);
    let stages = until_ready(|| {
        turns.activate(
            operation.clone(),
            tools.clone(),
            1,
            workspace.clone(),
            AgentMode::LocalTools,
        )
    });
    assert_eq!(
        stages,
        [
            ActivationStage::RuntimeStopping,
            ActivationStage::RuntimeStarting,
            ActivationStage::Bound,
            ActivationStage::Ready
        ]
    );
    let local = other.runtime_status().unwrap();
    assert_eq!(
        (local.runtime_safety_profile, local.workspace_root.clone()),
        (
            Some(RuntimeSafetyProfile::CodexLocalToolsV1),
            Some(workspace.clone())
        )
    );
    assert_ne!(local.runtime_instance_id, inference.runtime_instance_id);
    // Every session shares that runtime: the other one is detached by the switch, and
    // what mode it has is not claimed while it is.
    assert_eq!(
        shown(&sessions, &tools),
        (BindingState::BoundAttached, SessionMode::LocalTools)
    );
    assert_eq!(
        shown(&sessions, &plain),
        (BindingState::BoundDetached, SessionMode::Unknown)
    );
    assert_eq!(
        serde_json::to_value(sessions.page(None).unwrap().runtime).unwrap(),
        serde_json::json!({"mode": "local_tools", "workspace": workspace})
    );
    // The same action again changes nothing and stops nothing.
    let settled = trail(&mut other).len();
    assert_eq!(
        turns
            .activate(
                operation,
                tools.clone(),
                1,
                workspace.clone(),
                AgentMode::LocalTools
            )
            .unwrap()
            .stage,
        ActivationStage::Ready
    );
    assert_eq!(trail(&mut other).len(), settled);

    // Back: the first session in inference again. The local-tools one is detached now.
    assert_eq!(
        entered(&turns, &plain, AgentMode::Inference),
        [
            ActivationStage::RuntimeStopping,
            ActivationStage::RuntimeStarting,
            ActivationStage::Ready
        ]
    );
    assert_eq!(
        shown(&sessions, &plain),
        (BindingState::BoundAttached, SessionMode::Inference)
    );
    assert_eq!(
        shown(&sessions, &tools),
        (BindingState::BoundDetached, SessionMode::Unknown)
    );

    // The profile does not survive an authority restart: it comes back as inference,
    // with every session detached. Local tools is entered again by asking for it.
    drop(other);
    authority.stop(false);
    let mut authority = env.start();
    env.ready(ActualState::Ready);
    let mut other = env.client();
    assert_eq!(
        other.runtime_status().unwrap().runtime_safety_profile,
        Some(RuntimeSafetyProfile::CodexInferenceV1)
    );
    assert_eq!(
        shown(&sessions, &tools),
        (BindingState::BoundDetached, SessionMode::Unknown)
    );
    assert_eq!(
        entered(&turns, &tools, AgentMode::LocalTools),
        [
            ActivationStage::RuntimeStopping,
            ActivationStage::RuntimeStarting,
            ActivationStage::Ready
        ]
    );
    assert_eq!(
        shown(&sessions, &tools),
        (BindingState::BoundAttached, SessionMode::LocalTools)
    );
    // The workspace it was started for is the one the authority held for the session:
    // the request named none.
    assert_eq!(
        other.runtime_status().unwrap().workspace_root,
        Some(workspace)
    );

    other.runtime_stop(mutation()).unwrap();
    env.ready(ActualState::Stopped);
    drop(other);
    authority.stop(false);
}

#[test]
fn an_approval_that_ends_undecided_is_shown_ended_and_is_never_answered_afterwards() {
    let _serial = serial();
    let env = Sandbox::new("approval_command");
    let mut authority = env.start();
    let (sessions, turns) = env.services();
    let workspace = env.workspace();
    let (session, _) = activated(&turns, &sessions, &workspace, AgentMode::LocalTools);
    let mut other = env.client();

    // Its deadline is the stricter of five minutes and the expiry of the lease the turn
    // was admitted under. Here that lease is somebody else's, and short.
    other
        .lease_grant(LeaseGrant {
            intent_id: id(),
            session_id: session.clone(),
            expected_session_revision: 1,
            scopes: vec![ExecutionScope::TurnStart, ExecutionScope::CommandExecute],
            ttl_seconds: 3,
        })
        .unwrap();
    let started = turns
        .start(id(), session.clone(), 1, PROMPT.into())
        .unwrap();
    let waiting = follow(&turns, &started.id, |v| v.approval.is_some());
    let notice = waiting.views.last().unwrap().approval.clone().unwrap();
    let pending = turns.approval(notice.id.clone(), None, None).unwrap();
    assert!(pending.approval.decidable);
    // Nobody decides. The authority ends it; the desktop reads that it did.
    let rest = follow_from(
        &turns,
        &started.id,
        waiting.views.last().unwrap().next,
        ended,
    );
    assert!(rest.views.last().unwrap().approval.is_none());
    let expired = turns.approval(notice.id.clone(), None, None).unwrap();
    assert!(expired.approval.state != ApprovalState::Pending && !expired.approval.decidable);
    assert!(expired.approval.ended_at.is_some() && expired.approval.reason.is_some());
    assert!(expired.command.is_none());
    // A decision that arrives after it ended is refused, whichever revision it names.
    for revision in [pending.approval.revision, expired.approval.revision] {
        let refused = failed(turns.decide(id(), notice.id.clone(), revision, Decision::Approve)).0;
        assert!(
            matches!(
                refused,
                SessionFailure::NotAllowed | SessionFailure::Changed
            ),
            "{refused:?}"
        );
    }
    assert_eq!(tally(&env, "executions"), 0);
    assert!(decisions(&env).iter().all(|d| d != "\"accept\""));

    // A stop of the runtime under a pending approval is refused by the authority: the
    // turn that waits is active. The approval is still there to decide, at the revision
    // it had, and the runtime is the one it was.
    let again = turns
        .start(id(), session.clone(), 1, PROMPT.into())
        .unwrap();
    let waiting = follow(&turns, &again.id, |v| v.approval.is_some());
    let notice = waiting.views.last().unwrap().approval.clone().unwrap();
    let revision = turns
        .approval(notice.id.clone(), None, None)
        .unwrap()
        .approval
        .revision;
    let before = other.runtime_status().unwrap();
    let answered = decisions(&env).len();
    assert_eq!(
        other.runtime_stop(mutation()).unwrap_err(),
        ControlError::ActiveTurnExists
    );
    let after = other.runtime_status().unwrap();
    assert_eq!(
        (
            after.actual_state,
            after.revision,
            after.runtime_instance_id
        ),
        (
            ActualState::Ready,
            before.revision,
            before.runtime_instance_id
        )
    );
    let held = turns.approval(notice.id.clone(), None, None).unwrap();
    assert_eq!(
        (
            held.approval.state,
            held.approval.decidable,
            held.approval.revision
        ),
        (ApprovalState::Pending, true, revision)
    );
    assert!(held.command.is_some());
    assert_eq!(decisions(&env).len(), answered);
    assert_eq!(
        turns
            .read(again.id.clone(), None, false)
            .unwrap()
            .turn
            .state,
        TurnState::Running
    );

    // The authority restarts under it. What it holds is what is shown: nothing pending,
    // nothing accepted on anyone's behalf, and no turn started again.
    drop(other);
    authority.stop(false);
    let mut authority = env.start();
    env.ready(ActualState::Ready);
    let list = turns.approvals().unwrap();
    assert!(list.pending.is_empty());
    assert_eq!(list.recent.len(), 2);
    assert!(
        list.recent
            .iter()
            .all(|a| a.decision.is_none() && !a.decidable)
    );
    assert_eq!(
        failed(turns.decide(id(), notice.id.clone(), revision, Decision::Approve)).0,
        SessionFailure::NotAllowed
    );
    assert!(decisions(&env).iter().all(|d| d != "\"accept\""));
    assert_eq!(tally(&env, "executions"), 0);
    assert_eq!(env.sent("turn/start"), 2);
    authority.stop(false);
}
