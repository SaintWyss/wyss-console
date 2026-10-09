//! The desktop's managed-turn service against the real `wyss-control` process, in
//! isolated temporary XDG directories. The runtime is the repository's Codex fixture: no
//! real Codex is launched and no production state is read.
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
    sessions::{ActivationStage, AgentMode, SessionError, SessionFailure, SessionService},
    turns::{TurnEventKind, TurnService, TurnView},
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
const PROMPT: &str = "WYSS_3D_PROMPT_SENTINEL say hello";

struct Sandbox {
    root: PathBuf,
    scenario: &'static str,
}
impl Sandbox {
    fn new(scenario: &'static str) -> Self {
        let root = std::env::temp_dir().join(format!("wyss-tu-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        for d in ["run", "state", "bin", "codex-home", "workspace"] {
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
        Self { root, scenario }
    }
    fn run(&self) -> PathBuf {
        self.root.join("run")
    }
    fn workspace(&self) -> String {
        self.root.join("workspace").to_str().unwrap().to_owned()
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
    /// A session created and activated by the desktop, as the window does it.
    fn activated(&self, sessions: &SessionService) -> ManagedSessionId {
        let session = sessions.prepare(id()).unwrap().id;
        let operation = id();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let progress = sessions
                .activate(operation.clone(), session.clone(), 1, self.workspace())
                .unwrap();
            if progress.stage == ActivationStage::Ready {
                return session;
            }
            assert!(Instant::now() < deadline, "activation at {progress:?}");
            thread::sleep(Duration::from_millis(20));
        }
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
fn kinds(trail: &[serde_json::Value]) -> Vec<String> {
    trail
        .iter()
        .map(|e| {
            e["event_type"]
                .as_str()
                .unwrap()
                .trim_start_matches("authority.")
                .to_owned()
        })
        .collect()
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

#[test]
fn a_turn_started_from_the_desktop_runs_streams_and_ends_at_the_real_authority() {
    let _serial = serial();
    let env = Sandbox::new("happy");
    let mut authority = env.start();
    let (sessions, turns) = env.services();
    let session = env.activated(&sessions);
    let mut other = env.client();
    let before = trail(&mut other).len();

    // Nothing yet, and a turn can be started.
    let empty = turns.list(session.clone()).unwrap();
    assert!(empty.turns.is_empty() && empty.runnable && !empty.partial);
    assert_eq!(empty.session.binding, BindingState::BoundAttached);

    let intent = id();
    let started = turns
        .start(intent.clone(), session.clone(), 1, PROMPT.into())
        .unwrap();
    let followed = follow(&turns, &started.id, ended);
    let last = followed.views.last().unwrap();
    assert_eq!(
        (last.turn.state, last.turn.reason, last.turn.cancellable),
        (TurnState::Completed, Some(TurnReason::Completed), false)
    );
    assert_eq!(last.turn.session, session);
    assert!(last.turn.ended_at.is_some() && last.approval.is_none());
    // The assistant's answer, whole and once, in stream order.
    assert_eq!(followed.text, "ASSISTANT_SENTINEL");
    use TurnEventKind as K;
    assert_eq!(
        followed.kinds,
        [
            K::Started,
            K::Text,
            K::Thinking,
            K::MessageEnd,
            K::Completed
        ]
    );
    assert_eq!(followed.sequences, [1, 2, 3, 4, 5]);
    // Reasoning never crosses, in any reading: not the summary the stream carries, not
    // the raw reasoning the authority already drops. Nor does the prompt come back.
    let wire = serde_json::to_string(&followed.views).unwrap();
    for private in [
        "SUMMARY_SENTINEL",
        "RAW_PRIVATE_REASONING_SENTINEL",
        "WYSS_3D_PROMPT_SENTINEL",
        "provider-turn",
        "codex_turn_id",
        "lease",
        "runtime_instance",
    ] {
        assert!(!wire.contains(private), "{private} crossed to the window");
    }

    // At the authority: a lease for exactly a turn, the turn's own lifecycle, and the
    // lease given back once the desktop read the turn as ended.
    let mut other = env.client();
    let events = trail(&mut other);
    assert_eq!(
        kinds(&events)[before..],
        [
            "execution_lease.granted",
            "managed_turn.admitted",
            "managed_turn.running",
            "managed_turn.completed",
            "execution_lease.revoked",
        ]
    );
    let granted = &events[before];
    assert_eq!(
        granted["payload"]["scopes"],
        serde_json::json!(["turn_start"])
    );
    let seconds = |v: &serde_json::Value| {
        time::OffsetDateTime::parse(
            v.as_str().unwrap(),
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap()
    };
    assert_eq!(
        (seconds(&granted["payload"]["expires_at"]) - seconds(&granted["timestamp"]))
            .whole_seconds(),
        1800
    );
    assert_eq!(other.lease_active(session.clone()).unwrap(), None);
    assert_eq!(env.sent("turn/start"), 1);

    // The same action again is the same turn: nothing reaches the provider.
    let settled = trail(&mut other).len();
    for _ in 0..3 {
        assert_eq!(
            turns
                .start(intent.clone(), session.clone(), 1, PROMPT.into())
                .unwrap(),
            started
        );
    }
    assert_eq!(
        (trail(&mut other).len(), env.sent("turn/start")),
        (settled, 1)
    );
    // The same identity for another input is the authority's refusal.
    assert_eq!(
        failed(turns.start(intent, session.clone(), 1, "another input".into())),
        (SessionFailure::Conflict, false)
    );

    // Reading again from a cursor returns what follows it and nothing before; reading
    // from the beginning returns what the authority still holds, in the same order.
    let part = turns
        .read(started.id.clone(), "3".parse().ok(), false)
        .unwrap();
    assert_eq!(
        part.events
            .iter()
            .map(|e| e.sequence.get())
            .collect::<Vec<_>>(),
        [4, 5]
    );
    let whole = turns.read(started.id.clone(), None, false).unwrap();
    assert_eq!(whole.events.len(), 5);
    assert!(!whole.gap && whole.turn.output);
    assert_eq!(
        turns
            .read(started.id.clone(), whole.next, false)
            .unwrap()
            .events,
        []
    );

    // A second turn, after the first has ended, under a lease of its own.
    let second = turns
        .start(id(), session.clone(), 1, PROMPT.into())
        .unwrap();
    assert_ne!(second.id, started.id);
    follow(&turns, &second.id, ended);
    assert_eq!(env.sent("turn/start"), 2);
    assert_eq!(sessions.stats().leases, 4);
    let listed = turns.list(session.clone()).unwrap();
    assert_eq!(
        listed
            .turns
            .iter()
            .map(|t| t.id.clone())
            .collect::<Vec<_>>(),
        [second.id.clone(), started.id.clone()]
    );
    assert!(listed.turns.iter().all(|t| t.state == TurnState::Completed));
    assert!(listed.runnable);
    assert!(
        !serde_json::to_string(&listed)
            .unwrap()
            .contains("WYSS_3D_PROMPT_SENTINEL")
    );
    // Another session has none of them.
    let elsewhere = sessions.prepare(id()).unwrap().id;
    assert!(turns.list(elsewhere).unwrap().turns.is_empty());

    // The authority restarts: the turns are still what they were, and their output,
    // which it kept in memory only, is gone. Nothing is sent to the provider for it.
    drop(other);
    authority.stop(false);
    assert_eq!(
        failed(turns.read(second.id.clone(), None, true)),
        (SessionFailure::Unavailable, true)
    );
    assert_eq!(
        failed(turns.list(session.clone())),
        (SessionFailure::Unavailable, true)
    );
    let mut authority = env.start();
    env.ready(ActualState::Ready);
    let after = turns.read(second.id.clone(), whole.next, true).unwrap();
    assert_eq!(after.turn.state, TurnState::Completed);
    assert!(after.gap && !after.turn.output);
    let listed = turns.list(session.clone()).unwrap();
    assert_eq!(listed.turns.len(), 2);
    // Detached by the restart: the hint says no, and so does the authority.
    assert_eq!(listed.session.binding, BindingState::BoundDetached);
    assert!(!listed.runnable);
    assert_eq!(
        failed(turns.start(id(), session.clone(), 1, PROMPT.into())),
        (SessionFailure::RuntimeUnavailable, false)
    );
    let mut other = env.client();
    assert_eq!(other.lease_active(session.clone()).unwrap(), None);
    assert_eq!(env.sent("turn/start"), 2);
    // Resumed, it runs a turn again.
    sessions.resume(id(), session.clone(), 1).unwrap();
    let third = turns
        .start(id(), session.clone(), 1, PROMPT.into())
        .unwrap();
    assert_eq!(follow(&turns, &third.id, ended).text, "ASSISTANT_SENTINEL");

    other.runtime_stop(mutation()).unwrap();
    env.ready(ActualState::Stopped);
    drop(other);
    authority.stop(false);
}

#[test]
fn one_turn_runs_at_a_time_and_cancel_is_the_authority_s_to_carry_out() {
    let _serial = serial();
    // This provider starts a turn, says one thing, and does not end it.
    let env = Sandbox::new("turn_long");
    let mut authority = env.start();
    let (sessions, turns) = env.services();
    let session = env.activated(&sessions);
    let other_session = env.activated(&sessions);

    let started = turns
        .start(id(), session.clone(), 1, PROMPT.into())
        .unwrap();
    let running = follow(&turns, &started.id, |v| v.turn.state == TurnState::Running);
    let view = running.views.last().unwrap();
    assert_eq!(running.text, "ASSISTANT_SENTINEL");
    assert!(view.turn.cancellable && view.turn.ended_at.is_none());
    // Still running: a waiting read comes back with nothing new, and nothing changed.
    let idle = turns.read(started.id.clone(), view.next, true).unwrap();
    assert!(idle.events.is_empty());
    assert_eq!(idle.turn.state, TurnState::Running);

    // One at a time, whichever session asks: refused by the authority, and no lease is
    // left behind for the turn that did not start.
    let mut other = env.client();
    for asking in [&session, &other_session] {
        assert_eq!(
            failed(turns.start(id(), asking.clone(), 1, PROMPT.into())),
            (SessionFailure::Busy, true)
        );
    }
    assert_eq!(other.lease_active(other_session.clone()).unwrap(), None);
    assert_eq!(env.sent("turn/start"), 1);
    let listed = turns.list(session.clone()).unwrap();
    assert!(listed.turns[0].cancellable && !listed.runnable);

    // Cancel: a stale revision is refused; the right one is a receipt, and the turn is
    // interrupted when the authority says so.
    assert_eq!(
        failed(turns.cancel(id(), started.id.clone(), 1)),
        (SessionFailure::Changed, false)
    );
    assert_eq!(
        turns
            .read(started.id.clone(), None, false)
            .unwrap()
            .turn
            .state,
        TurnState::Running
    );
    let cancel = id();
    assert_eq!(
        turns
            .cancel(cancel.clone(), started.id.clone(), view.turn.revision)
            .unwrap()
            .id,
        started.id
    );
    let stopped = follow(&turns, &started.id, ended);
    let last = stopped.views.last().unwrap();
    assert_eq!(
        (last.turn.state, last.turn.reason),
        (TurnState::Interrupted, Some(TurnReason::UserCancel))
    );
    assert!(stopped.kinds.contains(&TurnEventKind::Interrupted));
    assert_eq!(env.sent("turn/interrupt"), 1);
    // The same cancel again is its receipt; a new one for a turn that ended is refused.
    turns
        .cancel(cancel, started.id.clone(), view.turn.revision)
        .unwrap();
    assert_eq!(
        failed(turns.cancel(id(), started.id.clone(), last.turn.revision)),
        (SessionFailure::NotAllowed, false)
    );
    assert_eq!(env.sent("turn/interrupt"), 1);
    // The lease kept for the turn was given back when it was read as ended.
    assert_eq!(other.lease_active(session.clone()).unwrap(), None);
    let events = kinds(&trail(&mut other));
    assert_eq!(
        events[events.len() - 3..],
        [
            "managed_turn.cancel_requested",
            "managed_turn.interrupted",
            "execution_lease.revoked",
        ]
    );

    // The authority is killed while a turn runs. Its record comes back as what the
    // authority can say about it; the prompt is not sent again and no turn is started.
    let again = turns
        .start(id(), other_session.clone(), 1, PROMPT.into())
        .unwrap();
    let cursor = follow(&turns, &again.id, |v| v.turn.state == TurnState::Running)
        .views
        .last()
        .unwrap()
        .next;
    assert_eq!(env.sent("turn/start"), 2);
    drop(other);
    authority.stop(true);
    assert_eq!(
        failed(turns.read(again.id.clone(), cursor, true)),
        (SessionFailure::Unavailable, true)
    );
    let mut authority = env.start();
    env.ready(ActualState::Ready);
    let after = turns.read(again.id.clone(), cursor, true).unwrap();
    assert_eq!(
        (after.turn.state, after.turn.reason),
        (
            TurnState::Indeterminate,
            Some(TurnReason::AuthorityRestarted)
        )
    );
    assert!(after.gap && !after.turn.output && !after.turn.cancellable);
    assert!(after.events.is_empty());
    assert_eq!(after.next, cursor);
    assert_eq!(env.sent("turn/start"), 2);
    let listed = turns.list(other_session.clone()).unwrap();
    assert_eq!(listed.turns[0].id, again.id);
    assert_eq!(listed.turns[0].state, TurnState::Indeterminate);
    // The lease this desktop kept for it does not outlive the reading that found it over.
    let mut other = env.client();
    assert_eq!(other.lease_active(other_session.clone()).unwrap(), None);

    other.runtime_stop(mutation()).unwrap();
    env.ready(ActualState::Stopped);
    drop(other);
    authority.stop(false);
}

#[test]
fn a_turn_the_authority_fails_is_shown_as_failed_with_its_reason() {
    let _serial = serial();
    // This provider leaves the inference surface: the authority contains it.
    let env = Sandbox::new("isolation_diff");
    let mut authority = env.start();
    let (sessions, turns) = env.services();
    let session = env.activated(&sessions);
    let started = turns
        .start(id(), session.clone(), 1, PROMPT.into())
        .unwrap();
    let followed = follow(&turns, &started.id, ended);
    let last = followed.views.last().unwrap();
    assert_eq!(
        (last.turn.state, last.turn.reason),
        (
            TurnState::Failed,
            Some(TurnReason::InferenceIsolationViolation)
        )
    );
    assert!(followed.kinds.contains(&TurnEventKind::Failed));
    assert!(
        !serde_json::to_string(&followed.views)
            .unwrap()
            .contains("forbidden")
    );
    let mut other = env.client();
    assert_eq!(other.lease_active(session.clone()).unwrap(), None);
    let listed = turns.list(session).unwrap();
    assert_eq!(listed.turns[0].state, TurnState::Failed);
    drop(other);
    authority.stop(false);
}

/// What a rejected request must leave alone: the runtime, the session's attachment, the
/// turn and its lease, and the durable stream.
fn untouched(
    other: &mut ControlClient,
    session: &ManagedSessionId,
    turn: &ManagedTurnId,
) -> String {
    let runtime = other.runtime_status().unwrap();
    let binding = other.binding_status(session.clone()).unwrap();
    let record = other.turn_status(turn.clone()).unwrap();
    format!(
        "{:?}",
        (
            (
                runtime.desired_state,
                runtime.actual_state,
                runtime.revision,
                runtime.runtime_instance_id,
            ),
            (binding.state, binding.attachment_profile),
            (record.state, record.revision, record.terminal_reason),
            other.lease_active(session.clone()).unwrap(),
            trail(other).len(),
        )
    )
}

#[test]
fn a_runtime_stop_is_refused_by_the_authority_while_a_turn_is_active_whatever_the_client_last_read()
{
    let _serial = serial();
    let env = Sandbox::new("turn_long");
    let mut authority = env.start();
    let (sessions, turns) = env.services();
    let session = env.activated(&sessions);

    // Client A reads that no turn is running. That reading is about to be stale.
    let mut other = env.client();
    assert!(!turns.active(&mut other).unwrap());
    // Client B starts a turn, on its own connection, and the authority admits it.
    let started = turns
        .start(id(), session.clone(), 1, PROMPT.into())
        .unwrap();
    let cursor = follow(&turns, &started.id, |v| v.turn.state == TurnState::Running)
        .views
        .last()
        .unwrap()
        .next;
    let before = untouched(&mut other, &session, &started.id);

    // Client A acts on what it read: it asks for the runtime to be stopped. The authority
    // refuses, however often it is asked and whatever revision it is asked at.
    let at = other.runtime_status().unwrap().revision;
    for expected_revision in [None, Some(at), None] {
        assert_eq!(
            other
                .runtime_stop(RuntimeMutation {
                    expected_revision,
                    ..mutation()
                })
                .unwrap_err(),
            ControlError::ActiveTurnExists
        );
    }
    // Nothing was taken: the runtime is the one it was and is ready, the session is
    // attached, the turn runs at the revision it had under the lease it had, and the
    // durable stream holds no event more. Nothing was stopped, detached or ended.
    assert_eq!(untouched(&mut other, &session, &started.id), before);
    let status = other.runtime_status().unwrap();
    assert_eq!(
        (status.desired_state, status.actual_state),
        (DesiredState::Running, ActualState::Ready)
    );
    assert_eq!(
        other.binding_status(session.clone()).unwrap().state,
        BindingState::BoundAttached
    );
    let recorded = kinds(&trail(&mut other));
    assert_eq!(
        recorded
            .iter()
            .filter(|k| *k == "runtime.desired_state_changed")
            .count(),
        1
    );
    assert!(!recorded.iter().any(|k| k.starts_with("managed_turn.")
        && !matches!(k.as_str(), "managed_turn.admitted" | "managed_turn.running")));

    // The desktop itself, made to believe no turn runs: its own request for the other
    // mode reaches the authority, is refused there, and is said as what it is.
    let tools = sessions.prepare(id()).unwrap().id;
    let before = untouched(&mut other, &session, &started.id);
    let believed_idle = |_: &mut ControlClient| Ok(false);
    for _ in 0..2 {
        assert_eq!(
            failed(sessions.activate_in(
                id(),
                tools.clone(),
                1,
                env.workspace(),
                AgentMode::LocalTools,
                &believed_idle,
            )),
            (SessionFailure::TurnActive, false)
        );
    }
    assert_eq!(untouched(&mut other, &session, &started.id), before);

    // The turn is still the running turn, and it still answers to its owner.
    let view = turns.read(started.id.clone(), cursor, false).unwrap();
    assert_eq!(view.turn.state, TurnState::Running);
    turns
        .cancel(id(), started.id.clone(), view.turn.revision)
        .unwrap();
    let ended_view = follow_from(&turns, &started.id, cursor, ended);
    let last = ended_view.views.last().unwrap();
    // It ended as a cancelled turn. Nothing ever ended it as stopped under.
    assert_eq!(
        (last.turn.state, last.turn.reason),
        (TurnState::Interrupted, Some(TurnReason::UserCancel))
    );

    // Ended: the same stop is admitted now, and the runtime goes down normally.
    assert!(!turns.active(&mut other).unwrap());
    let stopped = other.runtime_stop(mutation()).unwrap();
    assert_eq!(stopped.desired_state, DesiredState::Stopped);
    env.ready(ActualState::Stopped);
    let listed = turns.list(session.clone()).unwrap();
    assert_eq!(listed.session.binding, BindingState::BoundDetached);
    assert_eq!(listed.turns[0].state, TurnState::Interrupted);
    assert_eq!(env.sent("turn/start"), 1);

    // Started again and resumed, the same session runs another turn.
    other.runtime_start(mutation()).unwrap();
    env.ready(ActualState::Ready);
    sessions.resume(id(), session.clone(), 1).unwrap();
    let again = turns
        .start(id(), session.clone(), 1, PROMPT.into())
        .unwrap();
    let running = follow(&turns, &again.id, |v| v.turn.state == TurnState::Running);
    assert_eq!(running.text, "ASSISTANT_SENTINEL");
    assert_eq!(env.sent("turn/start"), 2);
    // And that one is protected the same way.
    assert_eq!(
        other.runtime_stop(mutation()).unwrap_err(),
        ControlError::ActiveTurnExists
    );
    let revision = turns
        .read(again.id.clone(), None, false)
        .unwrap()
        .turn
        .revision;
    turns.cancel(id(), again.id.clone(), revision).unwrap();
    follow(&turns, &again.id, ended);
    other.runtime_stop(mutation()).unwrap();
    env.ready(ActualState::Stopped);
    drop(other);
    authority.stop(false);
}

#[test]
fn a_stop_and_a_turn_start_that_race_never_end_with_a_turn_stopped_under() {
    let _serial = serial();
    let env = Sandbox::new("turn_long");
    let mut authority = env.start();
    let (sessions, turns) = env.services();
    let session = env.activated(&sessions);
    let mut other = env.client();
    let (mut turn_first, mut stop_first) = (0, 0);
    for round in 0..10u64 {
        // Two clients, two connections, released together; on half of the rounds the stop
        // is held back a little so that both orders are taken.
        let gate = std::sync::Barrier::new(2);
        let (start, stop) = thread::scope(|scope| {
            let starting = scope.spawn(|| {
                gate.wait();
                turns.start(id(), session.clone(), 1, PROMPT.into())
            });
            let stopping = scope.spawn(|| {
                gate.wait();
                thread::sleep(Duration::from_millis((round % 2) * (4 + round)));
                other.runtime_stop(mutation())
            });
            (starting.join().unwrap(), stopping.join().unwrap())
        });
        match (start, stop) {
            // The turn was admitted first: the stop was refused and the turn runs on a
            // runtime that is up, until its owner ends it.
            (Ok(turn), Err(refused)) => {
                turn_first += 1;
                assert_eq!(refused, ControlError::ActiveTurnExists, "round {round}");
                let view = follow(&turns, &turn.id, |v| v.turn.state == TurnState::Running);
                let running = view.views.last().unwrap();
                assert_eq!(
                    other.runtime_status().unwrap().actual_state,
                    ActualState::Ready
                );
                turns
                    .cancel(id(), turn.id.clone(), running.turn.revision)
                    .unwrap();
                let over = follow(&turns, &turn.id, ended);
                let last = over.views.last().unwrap();
                assert_eq!(
                    (last.turn.state, last.turn.reason),
                    (TurnState::Interrupted, Some(TurnReason::UserCancel)),
                    "round {round}"
                );
            }
            // The stop was admitted first: no turn was admitted on the runtime it took.
            (Err(_), Ok(_)) => {
                stop_first += 1;
                env.ready(ActualState::Stopped);
                other.runtime_start(mutation()).unwrap();
                env.ready(ActualState::Ready);
                sessions.resume(id(), session.clone(), 1).unwrap();
            }
            (start, stop) => panic!("round {round}: both or neither: {start:?} / {stop:?}"),
        }
    }
    // Whatever the order, no turn the authority holds was ever ended by a stop.
    for turn in turns.list(session.clone()).unwrap().turns {
        assert_eq!(turn.state, TurnState::Interrupted, "{turn:?}");
    }
    assert!(turn_first > 0, "{turn_first} / {stop_first}");
    assert_eq!(turn_first + stop_first, 10);
    other.runtime_stop(mutation()).unwrap();
    env.ready(ActualState::Stopped);
    drop(other);
    authority.stop(false);
}

/// The layout a local-tools generation is isolated in, as the runtime tests build it.
fn local_layout(env: &Sandbox) -> String {
    let home = env.root.join("codex-home");
    fs::write(home.join("auth.json"), "FIXTURE-CREDENTIAL").unwrap();
    fs::set_permissions(home.join("auth.json"), fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(home.join("config.toml"), "approval_policy = \"never\"\n").unwrap();
    for dir in ["repo/.git", "repo/.codex", "repo/sub/ws"] {
        fs::create_dir_all(env.root.join(dir)).unwrap();
    }
    fs::write(
        env.root.join("layers.json"),
        serde_json::to_string(&[env.root.join("repo/.codex")]).unwrap(),
    )
    .unwrap();
    env.root.join("repo/sub/ws").to_str().unwrap().to_owned()
}

#[test]
fn a_turn_waiting_on_an_approval_is_shown_waiting_and_reading_it_answers_nothing() {
    let _serial = serial();
    // This provider proposes a command and asks before running it.
    let env = Sandbox::new("approval_command");
    let workspace = local_layout(&env);
    let mut authority = env.start();
    let mut other = env.client();
    // A tool-capable runtime and a lease that allows a command are the developer
    // client's doing: the desktop asks for neither.
    other
        .runtime_start(RuntimeMutation {
            safety_profile: Some(RuntimeSafetyProfile::CodexLocalToolsV1),
            workspace_root: Some(workspace.clone()),
            ..mutation()
        })
        .unwrap();
    env.ready(ActualState::Ready);
    let prepared = other
        .prepare(Prepare {
            intent_id: id(),
            adapter_id: RuntimeTarget::codex().adapter_id,
            runtime_kind: RuntimeTarget::codex().runtime_kind,
        })
        .unwrap();
    let session = prepared.managed_session_id.clone();
    let theirs = other
        .lease_grant(LeaseGrant {
            intent_id: id(),
            session_id: session.clone(),
            expected_session_revision: 1,
            scopes: vec![
                ExecutionScope::SessionCreate,
                ExecutionScope::SessionResume,
                ExecutionScope::TurnStart,
                ExecutionScope::CommandExecute,
            ],
            ttl_seconds: 300,
        })
        .unwrap();
    other
        .binding_create(BindingCreate {
            intent_id: id(),
            managed_session_id: session.clone(),
            expected_session_revision: 1,
            workspace_root: workspace.clone(),
        })
        .unwrap();
    other
        .binding_resume(BindingResume {
            intent_id: id(),
            managed_session_id: session.clone(),
            expected_session_revision: 1,
        })
        .unwrap();

    let (sessions, turns) = env.services();
    let started = turns
        .start(id(), session.clone(), 1, PROMPT.into())
        .unwrap();
    // Under their lease, as it is: the desktop asked for none.
    assert_eq!(sessions.stats().leases, 0);
    let waiting = follow(&turns, &started.id, |v| v.approval.is_some());
    let view = waiting.views.last().unwrap();
    // The turn is running and waits: that is its canonical state, and it is shown.
    assert_eq!(view.turn.state, TurnState::Running);
    let notice = view.approval.clone().unwrap();
    assert_eq!(notice.kind, ApprovalKind::CommandExecution);
    assert!(waiting.kinds.contains(&TurnEventKind::CommandProposed));
    assert!(waiting.kinds.contains(&TurnEventKind::ApprovalRequired));
    // Which approval, what kind of action and until when. The reading of a turn carries
    // a handle to its approval, never the command, its directory or its fingerprint:
    // the action is read with the approval, by whoever decides it.
    let wire = serde_json::to_string(&waiting.views).unwrap();
    let pending = other
        .approval_pending(started.id.clone())
        .unwrap()
        .expect("pending approval");
    assert_eq!(
        serde_json::to_value(&notice).unwrap(),
        serde_json::json!({
            "id": pending.approval.managed_approval_id,
            "kind": "command_execution", "expires_at": notice.expires_at,
        })
    );
    for private in [
        "WYSS_FIXTURE_COMMAND",
        "printf",
        workspace.as_str(),
        pending.approval.action_fingerprint.as_str(),
        "call-1",
    ] {
        assert!(!wire.contains(private), "{private} crossed to the window");
    }

    // The desktop keeps reading and the approval stays exactly as it was: undecided.
    for _ in 0..3 {
        let again = turns.read(started.id.clone(), view.next, true).unwrap();
        assert!(again.approval.is_some());
        assert_eq!(again.turn.state, TurnState::Running);
    }
    turns.list(session.clone()).unwrap();
    let still = other
        .approval_pending(started.id.clone())
        .unwrap()
        .expect("still pending");
    assert_eq!(
        (
            still.approval.state,
            still.approval.revision,
            still.approval.decision
        ),
        (ApprovalState::Pending, pending.approval.revision, None)
    );
    let events = kinds(&trail(&mut other));
    assert_eq!(
        events
            .iter()
            .filter(|k| k.starts_with("managed_approval."))
            .collect::<Vec<_>>(),
        ["managed_approval.requested"]
    );
    // Nothing was answered to the provider and nothing ran.
    assert!(!env.root.join("decisions").exists());
    assert!(!env.root.join("executions").exists());

    // Cancelling the turn is what the desktop can do. The authority ends the approval
    // with it; the desktop never named one.
    turns
        .cancel(id(), started.id.clone(), view.turn.revision)
        .unwrap();
    let stopped = follow(&turns, &started.id, ended);
    let last = stopped.views.last().unwrap();
    assert_eq!(
        (last.turn.state, last.turn.reason),
        (TurnState::Interrupted, Some(TurnReason::UserCancel))
    );
    assert!(last.approval.is_none());
    assert!(stopped.kinds.contains(&TurnEventKind::ApprovalResolved));
    assert!(!env.root.join("executions").exists());
    // Their lease is still theirs.
    assert_eq!(other.lease_active(session.clone()).unwrap(), Some(theirs));

    other.runtime_stop(mutation()).unwrap();
    env.ready(ActualState::Stopped);
    drop(other);
    authority.stop(false);
}
