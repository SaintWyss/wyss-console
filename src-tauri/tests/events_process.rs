//! Events and history, as the desktop reads them, against the real `wyss-control`
//! process in isolated temporary XDG directories. The runtime is the repository's Codex
//! fixture, launched by the authority inside its real `bwrap` namespace: no real Codex
//! is launched and no production state is read.
#![cfg(target_os = "linux")]
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
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
        Sequence, WireUuid,
        control::*,
        identities::{ManagedSessionId, ManagedTurnId},
    },
    control::client::ControlClient,
    events::{EVENTS_PAGE, EventService, EventsPage},
    sessions::{
        ActivationProgress, ActivationStage, AgentMode, SessionError, SessionFailure,
        SessionService,
    },
    turns::{Decision, TurnService, TurnView},
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
const PROMPT: &str = "WYSS_3F_PROMPT_SENTINEL list the files";

struct Sandbox {
    root: PathBuf,
    scenario: &'static str,
}
impl Sandbox {
    fn new(scenario: &'static str) -> Self {
        let root = std::env::temp_dir().join(format!("wyss-ev-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        for d in ["run", "state", "other-state", "bin", "codex-home"] {
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
        self.start_on("state")
    }
    /// The authority on the same socket directory, holding the store named `state`.
    fn start_on(&self, state: &str) -> Authority {
        Authority(
            Command::new(env!("CARGO_BIN_EXE_wyss-control"))
                .env("XDG_RUNTIME_DIR", self.run())
                .env("XDG_STATE_HOME", self.root.join(state))
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

/// Reads a turn the way the window follows it, until `done` says to stop.
fn follow(turns: &TurnService, turn: &ManagedTurnId, done: impl Fn(&TurnView) -> bool) -> TurnView {
    let (mut cursor, deadline) = (None, Instant::now() + Duration::from_secs(12));
    loop {
        let view = turns.read(turn.clone(), cursor, true).unwrap();
        cursor = view.next;
        if done(&view) && !view.more {
            return view;
        }
        assert!(Instant::now() < deadline, "turn at {:?}", view.turn);
    }
}
fn ended(view: &TurnView) -> bool {
    !view.turn.cancellable
}
/// A session created and activated by the desktop in local tools, as the window does it.
fn activated(turns: &TurnService, sessions: &SessionService, workspace: &str) -> ManagedSessionId {
    let session = sessions.prepare(id()).unwrap().id;
    let operation = id();
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        let progress: ActivationProgress = turns
            .activate(
                operation.clone(),
                session.clone(),
                1,
                workspace.to_owned(),
                AgentMode::LocalTools,
            )
            .unwrap();
        if progress.stage == ActivationStage::Ready {
            return session;
        }
        assert!(Instant::now() < deadline, "stuck at {:?}", progress.stage);
        thread::sleep(Duration::from_millis(20));
    }
}
/// A turn of `session` that waits on an approval, and that approval's handle and revision.
fn waiting(turns: &TurnService, session: &ManagedSessionId) -> (ManagedTurnId, Value, u64) {
    let started = turns
        .start(id(), session.clone(), 1, PROMPT.into())
        .unwrap();
    let view = follow(turns, &started.id, |v| v.approval.is_some());
    let notice = view.approval.unwrap();
    let revision = turns.approvals().unwrap().pending[0].approval.revision;
    let approval = serde_json::to_value(&notice.id).unwrap();
    (started.id, approval, revision)
}

/// What the window is given of a stream: every window of it, newest first, read the way
/// the view reads it. Returns the windows as they crossed.
fn history(events: &EventService) -> Vec<EventsPage> {
    let mut pages = vec![events.latest().unwrap()];
    while let Some(before) = pages.last().unwrap().older {
        let stream = pages[0].stream.clone();
        pages.push(events.older(stream, before).unwrap());
        assert!(pages.len() < 64, "history does not end");
    }
    pages
}
fn rows(pages: &[EventsPage]) -> Vec<Value> {
    pages
        .iter()
        .flat_map(|page| {
            serde_json::to_value(&page.events)
                .unwrap()
                .as_array()
                .unwrap()
                .clone()
        })
        .collect()
}
/// The kind the window is told for an event of this type. Written here apart from the
/// projection, from the authority's own names.
fn kind_of(event_type: &str) -> &'static str {
    match event_type {
        "authority.managed_session.prepared" => "session_prepared",
        "authority.managed_session.closed" => "session_closed",
        "authority.managed_session.runtime_bound" => "session_bound",
        "authority.managed_session.runtime_attached" => "session_attached",
        "authority.runtime.desired_state_changed" => "runtime_desired",
        "authority.runtime.instance_ready" => "runtime_ready",
        "authority.runtime.instance_exited" => "runtime_exited",
        "authority.managed_turn.admitted" => "turn_admitted",
        "authority.managed_turn.running" => "turn_running",
        "authority.managed_turn.cancel_requested" => "turn_cancel_requested",
        "authority.managed_turn.completed" => "turn_completed",
        "authority.managed_turn.interrupted" => "turn_interrupted",
        "authority.managed_turn.failed" => "turn_failed",
        "authority.managed_turn.indeterminate" => "turn_indeterminate",
        "authority.managed_approval.requested" => "approval_requested",
        "authority.managed_approval.accepted" => "approval_accepted",
        "authority.managed_approval.confirmed" => "approval_confirmed",
        "authority.managed_approval.declined" => "approval_declined",
        "authority.managed_approval.cancelled" => "approval_cancelled",
        "authority.managed_approval.expired" => "approval_expired",
        "authority.managed_approval.invalidated" => "approval_invalidated",
        "authority.managed_approval.indeterminate" => "approval_indeterminate",
        "authority.execution_lease.granted" => "lease_granted",
        "authority.execution_lease.renewed" => "lease_renewed",
        "authority.execution_lease.revoked" => "lease_revoked",
        "authority.execution_lease.expired" => "lease_expired",
        other => panic!("an event type this test does not know: {other}"),
    }
}
fn category_of(kind: &str) -> &'static str {
    match kind.split('_').next().unwrap() {
        "session" => "session",
        "runtime" => "runtime",
        "turn" => "turn",
        "approval" => "approval",
        "lease" => "lease",
        other => panic!("{other}"),
    }
}
/// What of an event's payload is the provider's or the authority's own, and has no
/// business in the window.
const PRIVATE: [&str; 5] = [
    "runtime_session_id",
    "codex_turn_id",
    "lease_id",
    "action_fingerprint",
    "runtime_instance_id",
];
/// `wanted` appears in `kinds` in this order, with anything in between.
fn in_order(kinds: &[String], wanted: &[&str]) -> bool {
    let mut rest = kinds.iter();
    wanted.iter().all(|w| rest.any(|k| k == w))
}

#[test]
fn history_is_what_the_authority_recorded_in_its_order_and_nothing_private_of_it() {
    let _serial = serial();
    let env = Sandbox::new("approval_command");
    let mut authority = env.start();
    let (sessions, turns) = env.services();
    let events = EventService::new(sessions.clone());
    let workspace = env.workspace();
    let mut other = env.client();

    // A store nothing happened in: a stream, and no event of it.
    let empty = events.latest().unwrap();
    assert_eq!(
        (empty.head, empty.events.len(), empty.older, empty.gap),
        (None, 0, None, false)
    );
    assert_eq!(empty.stream, other.hello_status().stream_id);

    // A session, the runtime, a binding, a turn that asks before it runs a command.
    let session = activated(&turns, &sessions, &workspace);
    let (denied_turn, denied, revision) = waiting(&turns, &session);

    // A stop of the runtime under that turn is refused. A refusal is an answer, not a
    // record: the authority writes no event for it, so history has nothing to show of it.
    let before = trail(&mut other).len();
    assert!(matches!(
        other.runtime_stop(mutation()),
        Err(ControlError::ActiveTurnExists)
    ));
    assert_eq!(trail(&mut other).len(), before);

    // Denied: the turn ends interrupted. Then another turn, approved: the command runs.
    let denied_id = serde_json::from_value(denied.clone()).unwrap();
    turns
        .decide(id(), denied_id, revision, Decision::Deny)
        .unwrap();
    assert_eq!(
        follow(&turns, &denied_turn, ended).turn.state,
        TurnState::Interrupted
    );
    let (approved_turn, approved, revision) = waiting(&turns, &session);
    let approved_id = serde_json::from_value(approved.clone()).unwrap();
    turns
        .decide(id(), approved_id, revision, Decision::Approve)
        .unwrap();
    assert_eq!(
        follow(&turns, &approved_turn, ended).turn.state,
        TurnState::Completed
    );
    assert_eq!(
        fs::read_to_string(env.root.join("executions"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    // The runtime stops, the session is closed, and enough sessions are prepared for the
    // history to be longer than two windows.
    other.runtime_stop(mutation()).unwrap();
    env.ready(ActualState::Stopped);
    let revision = sessions
        .page(None)
        .unwrap()
        .sessions
        .iter()
        .find(|s| s.id == session)
        .unwrap()
        .revision;
    sessions.close(id(), session.clone(), revision).unwrap();
    for _ in 0..(2 * EVENTS_PAGE + 5) {
        sessions.prepare(id()).unwrap();
    }

    // The whole stream as the authority holds it, and as the window is given it.
    let durable = trail(&mut other);
    let pages = history(&events);
    let shown = rows(&pages);
    let total = durable.len() as u64;
    assert!(total > 2 * EVENTS_PAGE, "{total} events");

    // Windows: bounded, newest first, each beginning exactly where the one after it
    // ends, down to the first event of the stream. Nothing twice and nothing missing.
    assert!(pages.len() >= 3);
    for page in &pages {
        assert!(page.events.len() as u64 <= EVENTS_PAGE && !page.gap);
        assert_eq!(page.stream, pages[0].stream);
        assert_eq!(page.head.map(Sequence::get), Some(total));
    }
    assert_eq!(pages[0].events.len() as u64, EVENTS_PAGE);
    assert_eq!(pages.last().unwrap().older, None);
    let positions: Vec<u64> = shown
        .iter()
        .map(|row| row["sequence"].as_str().unwrap().parse().unwrap())
        .collect();
    assert_eq!(positions, (1..=total).rev().collect::<Vec<_>>());

    // Each row is its event: position, time, kind, category and the session it is about.
    let mut oldest_first = shown.clone();
    oldest_first.reverse();
    let (mut turn_rows, mut approval_rows) = (0, 0);
    for (row, event) in oldest_first.iter().zip(&durable) {
        let kind = kind_of(event["event_type"].as_str().unwrap());
        assert_eq!(row["sequence"], event["sequence"]);
        assert!(row["sequence"].is_string());
        assert_eq!(row["at"], event["timestamp"]);
        assert_eq!(
            (row["kind"].as_str(), row["category"].as_str()),
            (Some(kind), Some(category_of(kind)))
        );
        assert_eq!(row["session"], event["session_id"]);
        // A turn or an approval is named where the record names it, by its Wyss identity.
        let payload = &event["payload"];
        let named = |what: &str| payload.get(what).cloned().unwrap_or(Value::Null);
        match category_of(kind) {
            "turn" => {
                turn_rows += 1;
                assert_eq!(row["turn"], named("managed_turn_id"));
                assert!(row["turn"].is_string() && row["approval"].is_null());
            }
            "approval" => {
                approval_rows += 1;
                assert_eq!(row["turn"], named("managed_turn_id"));
                assert_eq!(row["approval"], named("managed_approval_id"));
                assert!(row["turn"].is_string() && row["approval"].is_string());
                assert_eq!(row["facts"]["approval_kind"], "command_execution");
            }
            _ => assert!(row["turn"].is_null() && row["approval"].is_null()),
        }
        // Exactly these fields, whatever the event carried.
        let keys: BTreeSet<&str> = row
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            BTreeSet::from([
                "sequence",
                "at",
                "category",
                "kind",
                "disposition",
                "security",
                "session",
                "turn",
                "approval",
                "facts"
            ])
        );
    }
    assert!(
        turn_rows >= 5 && approval_rows >= 5,
        "{turn_rows} {approval_rows}"
    );

    // The story, in the order it happened.
    let kinds: Vec<String> = oldest_first
        .iter()
        .map(|row| row["kind"].as_str().unwrap().to_owned())
        .collect();
    assert!(
        in_order(
            &kinds,
            &[
                "session_prepared",
                "runtime_desired",
                "runtime_ready",
                "session_bound",
                "lease_granted",
                "turn_admitted",
                "turn_running",
                "approval_requested",
                "approval_declined",
                "turn_interrupted",
                "turn_admitted",
                "approval_requested",
                "approval_accepted",
                "approval_confirmed",
                "turn_completed",
                "runtime_desired",
                "runtime_exited",
                "session_closed",
                "session_prepared",
            ]
        ),
        "{kinds:?}"
    );
    // Which approval was denied and which approved, each with its own turn.
    let of = |kind: &str| -> Vec<&Value> {
        oldest_first
            .iter()
            .filter(|row| row["kind"] == kind)
            .collect()
    };
    let declined = of("approval_declined")[0];
    assert_eq!(
        (
            &declined["approval"],
            &declined["turn"],
            &declined["session"]
        ),
        (&denied, &json!(denied_turn), &json!(session))
    );
    let accepted = of("approval_accepted")[0];
    assert_eq!(
        (&accepted["approval"], &accepted["turn"]),
        (&approved, &json!(approved_turn))
    );
    assert_eq!(of("turn_interrupted")[0]["turn"], json!(denied_turn));
    assert_eq!(of("turn_completed")[0]["turn"], json!(approved_turn));
    // What was asked of the runtime, and how it ended, as the record says it.
    let desired: Vec<&Value> = of("runtime_desired")
        .iter()
        .map(|r| &r["facts"]["desired"])
        .collect();
    assert_eq!(desired, [&json!("running"), &json!("stopped")]);
    let exited = of("runtime_exited")[0];
    assert_eq!(
        (
            &exited["facts"]["expected"],
            &exited["disposition"],
            &exited["security"]
        ),
        (&json!(true), &json!("normal"), &json!(false))
    );
    // A lease is a security record; an ordinary one is not marked as anything else.
    for lease in of("lease_granted") {
        assert_eq!(
            (&lease["security"], &lease["disposition"]),
            (&json!(true), &json!("normal"))
        );
        assert!(
            lease["facts"]["scopes"]
                .as_array()
                .is_some_and(|s| !s.is_empty())
        );
    }

    // Nothing of the provider or of the authority's own bookkeeping crossed: not a value
    // of it, and not its name.
    let wire = serde_json::to_string(&pages).unwrap();
    let mut private: BTreeSet<String> = BTreeSet::new();
    for event in &durable {
        for key in PRIVATE {
            if let Some(value) = event["payload"].get(key).and_then(Value::as_str) {
                private.insert(value.to_owned());
            }
        }
        for key in ["event_id", "correlation_id", "causation_id"] {
            if let Some(value) = event[key].as_str() {
                private.insert(value.to_owned());
            }
        }
    }
    // The stream held every kind of private identity this test is about.
    for key in PRIVATE {
        assert!(
            durable
                .iter()
                .any(|e| e["payload"].get(key).is_some_and(Value::is_string)),
            "no event carried {key}"
        );
    }
    let shown_ids: BTreeSet<&str> = shown
        .iter()
        .flat_map(|row| [&row["session"], &row["turn"], &row["approval"]])
        .filter_map(Value::as_str)
        .collect();
    assert!(private.len() > 20);
    for value in &private {
        if !shown_ids.contains(value.as_str()) {
            assert!(
                !wire.contains(value.as_str()),
                "{value} crossed to the window"
            );
        }
    }
    for name in [
        "payload",
        "fingerprint",
        "lease_id",
        "runtime_instance",
        "runtime_session",
        "codex_turn",
        "operation_id",
        "event_id",
        "correlation",
        "causation",
        "event_type",
        "sensitivity",
        "redaction",
        PROMPT,
        "WYSS_FIXTURE_COMMAND",
        "printf",
        "FIXTURE-CREDENTIAL",
        workspace.as_str(),
    ] {
        assert!(!wire.contains(name), "{name} crossed to the window");
    }

    // A position of another history is not read against this one.
    let stream = pages[0].stream.clone();
    let beyond: Sequence = (total + 2).to_string().parse().unwrap();
    assert_eq!(
        failed(events.older(stream.clone(), beyond)),
        (SessionFailure::Changed, false)
    );
    assert_eq!(
        failed(events.older(id(), pages[0].older.unwrap())),
        (SessionFailure::Changed, false)
    );
    // The first position has nothing before it.
    let first: Sequence = "1".parse().unwrap();
    let nothing = events.older(stream, first).unwrap();
    assert_eq!(
        (nothing.events.len(), nothing.older, nothing.gap),
        (0, None, false)
    );

    drop(other);
    authority.stop(false);
}

#[test]
fn a_restart_continues_the_stream_and_another_store_is_another_stream() {
    let _serial = serial();
    let env = Sandbox::new("approval_command");
    let mut authority = env.start();
    let (sessions, turns) = env.services();
    let events = EventService::new(sessions.clone());
    let session = activated(&turns, &sessions, &env.workspace());
    let (turn, approval, _) = waiting(&turns, &session);
    for _ in 0..EVENTS_PAGE {
        sessions.prepare(id()).unwrap();
    }
    let before = events.latest().unwrap();
    let head = before.head.unwrap().get();
    let cursor = before.older.unwrap();

    // The authority is killed with a turn waiting on an approval, and started again on
    // the same store.
    authority.stop(true);
    assert_eq!(failed(events.latest()), (SessionFailure::Unavailable, true));
    let mut authority = env.start();
    env.ready(ActualState::Ready);

    // The same stream, continued: positions go on from where they were, and what the
    // window had read of it is still a place in it.
    let after = events.latest().unwrap();
    assert_eq!(after.stream, before.stream);
    let now = after.head.unwrap().get();
    assert!(now > head, "{now} after {head}");
    let pages = history(&events);
    let shown = rows(&pages);
    let positions: Vec<u64> = shown
        .iter()
        .map(|row| row["sequence"].as_str().unwrap().parse().unwrap())
        .collect();
    assert_eq!(positions, (1..=now).rev().collect::<Vec<_>>());
    let older = events.older(before.stream.clone(), cursor).unwrap();
    assert_eq!(
        older.events[0].sequence.get() + 1,
        cursor.get(),
        "the window before the one read before the restart"
    );
    // What the restart did to what was running is in the record, with the authority's
    // own reason. That is the evidence of it; there is no event for the restart itself.
    let since: Vec<&Value> = shown
        .iter()
        .filter(|row| row["sequence"].as_str().unwrap().parse::<u64>().unwrap() > head)
        .collect();
    let find = |kind: &str| since.iter().find(|row| row["kind"] == kind).copied();
    let lost = find("turn_indeterminate").expect("the turn is recorded as unknown");
    assert_eq!(
        (
            &lost["turn"],
            &lost["facts"]["turn_reason"],
            &lost["disposition"]
        ),
        (
            &json!(turn),
            &json!("authority_restarted"),
            &json!("attention")
        )
    );
    let void = find("approval_invalidated").expect("the approval is recorded as void");
    assert_eq!(
        (&void["approval"], &void["facts"]["approval_reason"]),
        (&approval, &json!("authority_restarted"))
    );
    assert!(since.iter().all(|row| row["kind"] != "other"));
    let mut other = env.client();
    other.runtime_stop(mutation()).unwrap();
    env.ready(ActualState::Stopped);
    drop(other);
    authority.stop(false);

    // Another store behind the same socket: another stream. Its positions start over,
    // and a place in the stream read before is not a place in this one.
    let mut authority = env.start_on("other-state");
    drop(env.client());
    sessions.prepare(id()).unwrap();
    let fresh = events.latest().unwrap();
    assert_ne!(fresh.stream, before.stream);
    assert_eq!(
        (
            fresh.head.map(Sequence::get),
            fresh.events.len(),
            fresh.older
        ),
        (Some(1), 1, None)
    );
    assert_eq!(
        failed(events.older(before.stream.clone(), cursor)),
        (SessionFailure::Changed, false)
    );
    authority.stop(false);
}
