//! The desktop's managed-session service against the real `wyss-control` process, in
//! isolated temporary XDG directories. Where a runtime is needed it is the repository's
//! Codex fixture: no real Codex is launched and no production state is read.
#![cfg(target_os = "linux")]
use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};
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
    authority::{AuthorityService, AuthorityView, Connection},
    contracts::{Sequence, WireUuid, control::*, identities::ManagedSessionId},
    control::client::ControlClient,
    sessions::{PAGE_SIZE, SessionError, SessionFailure, SessionPage, SessionService},
};

/// Descriptors and threads are counted for the whole process, and a fixture executable
/// must not be forked while it is open for writing: these tests run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());
fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

fn id() -> WireUuid {
    WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap()
}
fn sequence(n: u64) -> Sequence {
    n.to_string().parse().unwrap()
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

struct Sandbox {
    root: PathBuf,
}
impl Sandbox {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("wyss-se-{}", uuid::Uuid::new_v4()));
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
        Self { root }
    }
    fn run(&self) -> PathBuf {
        self.root.join("run")
    }
    fn workspace(&self) -> String {
        self.root.join("workspace").to_str().unwrap().to_owned()
    }
    fn start(&self) -> Authority {
        self.start_as("happy")
    }
    /// The same authority with the Codex fixture playing another scenario.
    fn start_as(&self, scenario: &str) -> Authority {
        Authority(
            Command::new(env!("CARGO_BIN_EXE_wyss-control"))
                .env("XDG_RUNTIME_DIR", self.run())
                .env("XDG_STATE_HOME", self.root.join("state"))
                .env("PATH", self.root.join("bin"))
                .env("CODEX_HOME", self.root.join("codex-home"))
                .env("WYSS_RUNTIME_FIXTURE", &self.root)
                .env("WYSS_RUNTIME_SCENARIO", scenario)
                .env("WYSS_FIXTURE_USER_HOME", self.root.join("codex-home"))
                .env("WYSS_FIXTURE_LAYERS", "[]")
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
    fn sessions(&self) -> SessionService {
        SessionService::new(Some(self.run()))
    }
    fn ready(&self, c: &mut ControlClient, state: ActualState) {
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
    fn methods(&self, method: &str) -> usize {
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

fn head(c: &mut ControlClient) -> u64 {
    c.status().unwrap().last_sequence.map_or(0, |s| s.get())
}
fn ids(page: &SessionPage) -> Vec<String> {
    page.sessions
        .iter()
        .map(|s| s.id.as_str().to_owned())
        .collect()
}
fn failed<T: std::fmt::Debug>(result: Result<T, SessionError>) -> (SessionFailure, bool) {
    let error = result.unwrap_err();
    (error.code, error.retry)
}
/// Every durable event type the authority holds, in order.
fn events(c: &mut ControlClient) -> Vec<String> {
    let stream = c.hello_status().stream_id.clone();
    let mut after = None;
    let mut kinds = Vec::new();
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
        kinds.extend(events.iter().map(|e| {
            serde_json::to_value(e).unwrap()["event_type"]
                .as_str()
                .unwrap()
                .to_owned()
        }));
        if next.is_none() {
            return kinds;
        }
        after = next;
    }
}

#[test]
fn sessions_are_prepared_read_and_closed_by_the_real_authority() {
    let _serial = serial();
    let env = Sandbox::new();
    let _authority = env.start();
    let mut other = env.client();
    let sessions = env.sessions();

    // A fresh authority: nothing to show, and reading it wrote nothing.
    let empty = sessions.page(None).unwrap();
    assert_eq!(
        (
            empty.sessions.len(),
            empty.head,
            empty.older,
            empty.truncated
        ),
        (0, None, None, false)
    );
    assert_eq!(head(&mut other), 0);

    let first_intent = id();
    let first = sessions.prepare(first_intent.clone()).unwrap();
    let page = sessions.page(None).unwrap();
    assert_eq!(page.head, Some(sequence(1)));
    assert_eq!(ids(&page), [first.id.as_str()]);
    let shown = &page.sessions[0];
    // The projection is the authority's own record: nothing was assumed.
    let record = other.get(first.id.clone()).unwrap();
    assert_eq!(
        (&shown.id, shown.revision, shown.position, shown.state),
        (
            &record.managed_session_id,
            1,
            record.created_sequence,
            ControlState::Prepared
        )
    );
    assert_eq!(
        (&shown.created_at, &shown.updated_at),
        (&record.created_at, &record.updated_at)
    );
    assert_eq!(shown.adapter.as_str(), "codex");
    assert_eq!(
        (
            shown.binding,
            &shown.workspace,
            shown.resumable,
            shown.closable
        ),
        (BindingState::Unbound, &None, false, true)
    );
    // What crosses to the window carries no identity but the session's own.
    let wire = serde_json::to_string(&page).unwrap();
    assert_eq!(wire.matches(first.id.as_str()).count(), 1);
    assert!(!wire.contains(other.hello_status().stream_id.as_str()));
    assert!(!wire.contains(other.hello_status().authority_instance_id.as_str()));
    assert!(!wire.contains(first_intent.as_str()));

    // The same action again is the same session: no second record, no second event.
    for _ in 0..3 {
        assert_eq!(sessions.prepare(first_intent.clone()).unwrap(), first);
    }
    assert_eq!(head(&mut other), 1);
    // A response that never reached the desktop: the commit is the other client's here.
    let lost = id();
    let committed = other
        .prepare(Prepare {
            intent_id: lost.clone(),
            adapter_id: RuntimeTarget::codex().adapter_id,
            runtime_kind: RuntimeTarget::codex().runtime_kind,
        })
        .unwrap();
    assert_eq!(
        sessions.prepare(lost).unwrap().id,
        committed.managed_session_id
    );
    assert_eq!(head(&mut other), 2);
    let page = sessions.page(None).unwrap();
    // Newest first.
    assert_eq!(
        ids(&page),
        [committed.managed_session_id.as_str(), first.id.as_str()]
    );

    // Close: a stale revision, an unknown session and a reused intent are the authority's
    // refusals, and none of them changed anything.
    let unknown = ManagedSessionId::parse(id().as_str()).unwrap();
    assert_eq!(
        failed(sessions.close(id(), first.id.clone(), 2)),
        (SessionFailure::Changed, false)
    );
    assert_eq!(
        failed(sessions.close(id(), unknown.clone(), 1)),
        (SessionFailure::NotFound, false)
    );
    assert_eq!(
        failed(sessions.close(first_intent.clone(), first.id.clone(), 1)),
        (SessionFailure::Conflict, false)
    );
    assert_eq!(head(&mut other), 2);
    let close_intent = id();
    assert_eq!(
        sessions
            .close(close_intent.clone(), first.id.clone(), 1)
            .unwrap(),
        first
    );
    assert_eq!(head(&mut other), 3);
    let closed = &sessions.page(None).unwrap().sessions[1];
    assert_eq!(
        (
            closed.state,
            closed.revision,
            closed.closable,
            closed.resumable
        ),
        (ControlState::Closed, 2, false, false)
    );
    assert_eq!(other.get(first.id.clone()).unwrap().revision, 2);
    // Sent again, the close is the same close.
    for _ in 0..3 {
        assert_eq!(
            sessions
                .close(close_intent.clone(), first.id.clone(), 1)
                .unwrap(),
            first
        );
    }
    // A new close of what the caller still believes prepared is refused; one that names
    // the closed revision is accepted by the authority and changes nothing.
    assert_eq!(
        failed(sessions.close(id(), first.id.clone(), 1)),
        (SessionFailure::Changed, false)
    );
    sessions.close(id(), first.id.clone(), 2).unwrap();
    assert_eq!(head(&mut other), 3);

    // Resume without a binding: refused by the authority, and no lease was asked for.
    assert_eq!(
        failed(sessions.resume(id(), committed.managed_session_id.clone(), 1)),
        (SessionFailure::NotBound, false)
    );
    // The authority looks for the binding first: a session it never heard of has none.
    assert_eq!(
        failed(sessions.resume(id(), unknown, 1)),
        (SessionFailure::NotBound, false)
    );
    assert_eq!(sessions.stats().leases, 0);
    assert_eq!(
        other
            .lease_active(committed.managed_session_id.clone())
            .unwrap(),
        None
    );
    assert_eq!(
        events(&mut other),
        [
            "authority.managed_session.prepared",
            "authority.managed_session.prepared",
            "authority.managed_session.closed",
        ]
    );
}

#[test]
fn pages_walk_the_real_list_newest_first_and_stay_bounded() {
    let _serial = serial();
    let env = Sandbox::new();
    let _authority = env.start();
    drop(env.client());
    let sessions = env.sessions();
    let total = 2 * PAGE_SIZE + 12;
    let created: Vec<String> = (0..total)
        .map(|_| sessions.prepare(id()).unwrap().id.as_str().to_owned())
        .collect();
    let before = sessions.stats().connections;

    let mut seen = Vec::new();
    let mut cursor = None;
    let mut sizes = Vec::new();
    loop {
        let page = sessions.page(cursor).unwrap();
        assert!(page.sessions.len() <= PAGE_SIZE);
        assert!(!page.truncated);
        assert_eq!(page.head, Some(sequence(total as u64)));
        // Within a page, strictly newest first.
        assert!(
            page.sessions
                .windows(2)
                .all(|w| w[0].position > w[1].position)
        );
        sizes.push(page.sessions.len());
        seen.extend(ids(&page));
        match page.older {
            Some(next) => {
                assert_eq!(next, page.sessions.last().unwrap().position);
                cursor = Some(next);
            }
            None => break,
        }
    }
    assert_eq!(sizes, [PAGE_SIZE, PAGE_SIZE, 12]);
    // Every session exactly once, in reverse creation order.
    let expected: Vec<String> = created.iter().rev().cloned().collect();
    assert_eq!(seen, expected);
    // One connection per read, each closed when the read returned.
    assert_eq!(sessions.stats().connections, before + 3);

    // A cursor before the first session, or past the last one, is an ordinary read.
    assert!(
        sessions
            .page(Some(sequence(1)))
            .unwrap()
            .sessions
            .is_empty()
    );
    let all = sessions.page(Some(sequence(u64::MAX))).unwrap();
    assert_eq!(ids(&all), expected[..PAGE_SIZE]);
    assert!(all.older.is_some());
}

#[test]
fn an_authority_that_stops_and_returns_never_duplicates_an_intent() {
    let _serial = serial();
    let env = Sandbox::new();
    let sessions = env.sessions();
    // No authority was ever here.
    let intent = id();
    assert_eq!(
        failed(sessions.prepare(intent.clone())),
        (SessionFailure::Unavailable, true)
    );
    assert_eq!(
        failed(sessions.page(None)),
        (SessionFailure::Unavailable, true)
    );

    let mut authority = env.start();
    drop(env.client());
    let session = sessions.prepare(intent.clone()).unwrap();
    let other = id();
    for hard in [false, true, false] {
        authority.stop(hard);
        // Stopped, or killed with its socket left behind: nobody answers.
        assert_eq!(
            failed(sessions.prepare(intent.clone())),
            (SessionFailure::Unavailable, true)
        );
        assert_eq!(
            failed(sessions.prepare(other.clone())),
            (SessionFailure::Unavailable, true)
        );
        assert_eq!(
            failed(sessions.close(id(), session.id.clone(), 1)),
            (SessionFailure::Unavailable, true)
        );
        assert_eq!(
            failed(sessions.resume(id(), session.id.clone(), 1)),
            (SessionFailure::Unavailable, true)
        );
        authority = env.start();
        drop(env.client());
        // The same service finds the new instance without being told.
        assert_eq!(sessions.prepare(intent.clone()).unwrap(), session);
    }
    // The action that was refused three times while nobody answered happens once.
    let second = sessions.prepare(other.clone()).unwrap();
    assert_eq!(sessions.prepare(other).unwrap(), second);
    let page = sessions.page(None).unwrap();
    assert_eq!(ids(&page), [second.id.as_str(), session.id.as_str()]);
    assert_eq!(page.head, Some(sequence(2)));
    assert!(
        page.sessions
            .iter()
            .all(|s| s.state == ControlState::Prepared)
    );
}

fn descriptors() -> usize {
    fs::read_dir("/proc/self/fd").unwrap().count()
}
fn threads() -> usize {
    fs::read_dir("/proc/self/task").unwrap().count()
}
fn until(
    service: &AuthorityService,
    what: &str,
    accept: impl Fn(&AuthorityView) -> bool,
) -> AuthorityView {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut after = None;
    loop {
        let view = service.wait_changed(after, Duration::from_millis(50));
        if accept(&view) {
            return view;
        }
        assert!(Instant::now() < deadline, "{what}: last view {view:?}");
        after = Some(view.revision);
    }
}
fn observed_head(view: &AuthorityView) -> u64 {
    view.authority
        .as_ref()
        .and_then(|a| a.last_sequence)
        .map_or(0, |s| s.get())
}

#[test]
fn the_observer_sees_every_session_change_and_dozens_of_cycles_leak_nothing() {
    let _serial = serial();
    let env = Sandbox::new();
    let _authority = env.start();
    let stream = env.client().hello_status().stream_id.clone();
    let observer = AuthorityService::start(Some(env.run()));
    until(&observer, "connected", |v| {
        v.connection == Connection::Connected
    });
    let sessions = env.sessions();
    sessions.page(None).unwrap();
    thread::sleep(Duration::from_millis(200));
    let (idle_descriptors, idle_threads) = (descriptors(), threads());

    let cycles = 40u64;
    for cycle in 1..=cycles {
        let session = sessions.prepare(id()).unwrap();
        // The one existing watch is what announces it: the page is canonical, not derived.
        let view = until(&observer, "prepared", |v| observed_head(v) == 2 * cycle - 1);
        assert_eq!(view.authority.unwrap().prepared_sessions, 1);
        let page = sessions.page(None).unwrap();
        assert_eq!(page.sessions[0].id, session.id);
        assert_eq!(page.head, Some(sequence(2 * cycle - 1)));
        sessions.close(id(), session.id, 1).unwrap();
        let view = until(&observer, "closed", |v| observed_head(v) == 2 * cycle);
        let summary = view.authority.unwrap();
        assert_eq!(
            (summary.prepared_sessions, summary.closed_sessions),
            (0, cycle)
        );
        assert_eq!(
            sessions.page(None).unwrap().sessions[0].state,
            ControlState::Closed
        );
    }
    // The observer kept its one connection and received each event once.
    let link = observer.stats();
    assert_eq!((link.connections, link.failures, link.resyncs), (1, 0, 0));
    assert_eq!(link.events, 2 * cycles);
    // Four connections a cycle, none of them left open, and no thread started.
    assert_eq!(sessions.stats().connections, 1 + 4 * cycles);
    let deadline = Instant::now() + Duration::from_secs(3);
    while (descriptors(), threads()) != (idle_descriptors, idle_threads) {
        assert!(
            Instant::now() < deadline,
            "holds {:?}, expected {:?}",
            (descriptors(), threads()),
            (idle_descriptors, idle_threads)
        );
        thread::sleep(Duration::from_millis(10));
    }

    // Session calls take no watcher slot: with the observer and three more watchers
    // parked, the fifth watch is refused and a session call is still served.
    thread::sleep(Duration::from_millis(300));
    let watch = |wait_ms| AuditWatch {
        stream_id: stream.clone(),
        after_sequence: Some(sequence(2 * cycles)),
        limit: 1,
        wait_ms,
    };
    let parked: Vec<_> = (0..3)
        .map(|_| {
            let (mut client, request) = (env.client(), watch(1_500));
            thread::spawn(move || client.audit_watch(request))
        })
        .collect();
    thread::sleep(Duration::from_millis(400));
    assert!(matches!(
        env.client().audit_watch(watch(1_500)),
        Err(ControlError::CapacityExceeded)
    ));
    assert_eq!(sessions.page(None).unwrap().sessions.len(), PAGE_SIZE);
    for handle in parked {
        handle.join().unwrap().unwrap();
    }
    assert_eq!(observer.stats().failures, 0);
    observer.shutdown();
    // A stopped service cancels and refuses; the authority is told nothing.
    sessions.shutdown();
    assert_eq!(
        failed(sessions.page(None)),
        (SessionFailure::Unavailable, true)
    );
    assert_eq!(head(&mut env.client()), 2 * cycles);
}

#[test]
fn resume_attaches_under_a_lease_the_authority_grants_for_that_one_action() {
    let _serial = serial();
    let env = Sandbox::new();
    let mut authority = env.start();
    let mut other = env.client();
    let sessions = env.sessions();
    other.runtime_start(mutation()).unwrap();
    env.ready(&mut other, ActualState::Ready);

    // A session bound by another client, as the developer client binds one. Its lease is
    // gone before the desktop is asked to resume.
    let session = sessions.prepare(id()).unwrap();
    let bind = |other: &mut ControlClient, session: &ManagedSessionId| {
        let lease = other
            .lease_grant(LeaseGrant {
                intent_id: id(),
                session_id: session.clone(),
                expected_session_revision: 1,
                scopes: vec![ExecutionScope::SessionCreate],
                ttl_seconds: 60,
            })
            .unwrap();
        let binding = other
            .binding_create(BindingCreate {
                intent_id: id(),
                managed_session_id: session.clone(),
                expected_session_revision: 1,
                workspace_root: env.workspace(),
            })
            .unwrap();
        other
            .lease_revoke(LeaseRevoke {
                intent_id: id(),
                lease_id: lease.lease_id,
                expected_revision: 1,
            })
            .unwrap();
        binding
    };
    let binding = bind(&mut other, &session.id);
    let shown = sessions.page(None).unwrap().sessions.remove(0);
    assert_eq!(
        (shown.binding, shown.resumable, shown.closable),
        (BindingState::BoundDetached, true, true)
    );
    assert_eq!(
        shown.workspace.as_deref(),
        Some(binding.workspace_root.as_str())
    );
    // The provider thread stays on the desktop's side of the window.
    let wire = serde_json::to_string(&shown).unwrap();
    assert!(!wire.contains(&serde_json::to_string(&binding.runtime_session_id).unwrap()));

    // Resume: the authority admits it under a lease it granted for this action alone.
    let intent = id();
    assert_eq!(
        sessions
            .resume(intent.clone(), session.id.clone(), 1)
            .unwrap(),
        session
    );
    assert_eq!(env.methods("thread/resume"), 1);
    assert_eq!(
        other.binding_status(session.id.clone()).unwrap().state,
        BindingState::BoundAttached
    );
    assert_eq!(other.lease_active(session.id.clone()).unwrap(), None);
    assert_eq!(sessions.stats().leases, 1);
    let attached = sessions.page(None).unwrap().sessions.remove(0);
    assert_eq!(
        (attached.binding, attached.resumable),
        (BindingState::BoundAttached, false)
    );
    let trail = events(&mut other);
    assert_eq!(
        trail[trail.len() - 3..],
        [
            "authority.execution_lease.granted",
            "authority.managed_session.runtime_attached",
            "authority.execution_lease.revoked",
        ]
    );
    // The same action again: the authority's receipt, no lease, nothing sent to Codex.
    let settled = head(&mut other);
    for _ in 0..3 {
        assert_eq!(
            sessions
                .resume(intent.clone(), session.id.clone(), 1)
                .unwrap(),
            session
        );
    }
    assert_eq!(
        (
            head(&mut other),
            env.methods("thread/resume"),
            sessions.stats().leases
        ),
        (settled, 1, 1)
    );

    // A lease somebody else holds is used as it is and left as it was.
    let granted = |other: &mut ControlClient, scopes: Vec<ExecutionScope>| {
        other
            .lease_grant(LeaseGrant {
                intent_id: id(),
                session_id: session.id.clone(),
                expected_session_revision: 1,
                scopes,
                ttl_seconds: 300,
            })
            .unwrap()
    };
    let theirs = granted(
        &mut other,
        vec![ExecutionScope::SessionResume, ExecutionScope::TurnStart],
    );
    sessions.resume(id(), session.id.clone(), 1).unwrap();
    assert_eq!(env.methods("thread/resume"), 2);
    assert_eq!(
        other.lease_active(session.id.clone()).unwrap(),
        Some(theirs.clone())
    );
    assert_eq!(sessions.stats().leases, 1);
    other
        .lease_revoke(LeaseRevoke {
            intent_id: id(),
            lease_id: theirs.lease_id,
            expected_revision: 1,
        })
        .unwrap();
    // One that does not grant resume denies it: the desktop neither widens nor replaces it.
    let narrow = granted(&mut other, vec![ExecutionScope::TurnStart]);
    assert_eq!(
        failed(sessions.resume(id(), session.id.clone(), 1)),
        (SessionFailure::PermissionDenied, false)
    );
    assert_eq!(
        other.lease_active(session.id.clone()).unwrap(),
        Some(narrow.clone())
    );
    assert_eq!(
        (env.methods("thread/resume"), sessions.stats().leases),
        (2, 1)
    );
    other
        .lease_revoke(LeaseRevoke {
            intent_id: id(),
            lease_id: narrow.lease_id,
            expected_revision: 1,
        })
        .unwrap();

    // A revision the session is not at: refused before any lease exists.
    assert_eq!(
        failed(sessions.resume(id(), session.id.clone(), 2)),
        (SessionFailure::Changed, false)
    );
    assert_eq!(sessions.stats().leases, 1);

    // The runtime stopped: the authority grants the lease, denies the resume, and the
    // lease does not outlive the attempt.
    other.runtime_stop(mutation()).unwrap();
    env.ready(&mut other, ActualState::Stopped);
    assert_eq!(
        sessions.page(None).unwrap().sessions[0].binding,
        BindingState::BoundDetached
    );
    assert_eq!(
        failed(sessions.resume(id(), session.id.clone(), 1)),
        (SessionFailure::RuntimeUnavailable, false)
    );
    assert_eq!(other.lease_active(session.id.clone()).unwrap(), None);
    assert_eq!(
        (env.methods("thread/resume"), sessions.stats().leases),
        (2, 2)
    );

    // Closed: it keeps its binding and can never be resumed again.
    other.runtime_start(mutation()).unwrap();
    env.ready(&mut other, ActualState::Ready);
    sessions.close(id(), session.id.clone(), 1).unwrap();
    let closed = sessions.page(None).unwrap().sessions.remove(0);
    assert_eq!(
        (
            closed.state,
            closed.binding,
            closed.resumable,
            closed.closable
        ),
        (
            ControlState::Closed,
            BindingState::BoundDetached,
            false,
            false
        )
    );
    assert_eq!(
        closed.workspace.as_deref(),
        Some(binding.workspace_root.as_str())
    );
    // Whatever revision the caller names, the gate answers first: it is not prepared.
    for revision in [1, 2] {
        assert_eq!(
            failed(sessions.resume(id(), session.id.clone(), revision)),
            (SessionFailure::NotAllowed, false)
        );
    }
    assert_eq!(other.lease_active(session.id.clone()).unwrap(), None);
    assert_eq!(
        (env.methods("thread/resume"), sessions.stats().leases),
        (2, 2)
    );
    // Only the methods the authority already allowed ever reached the runtime.
    let sent: BTreeSet<String> = fs::read_to_string(env.root.join("methods"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    assert!(sent.iter().all(|m| {
        [
            "initialize",
            "initialized",
            "model/list",
            "config/read",
            "hooks/list",
            "thread/start",
            "thread/archive",
            "thread/unarchive",
            "thread/resume",
        ]
        .contains(&m.as_str())
    }));

    other.runtime_stop(mutation()).unwrap();
    env.ready(&mut other, ActualState::Stopped);
    drop(other);
    authority.stop(false);
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
fn kinds(trail: &[serde_json::Value]) -> Vec<&str> {
    trail
        .iter()
        .map(|e| e["event_type"].as_str().unwrap())
        .collect()
}
fn count(trail: &[serde_json::Value], kind: &str) -> usize {
    kinds(trail).iter().filter(|k| **k == kind).count()
}
/// Seconds between the commit of a lease event and the expiry it records.
fn lifetime(event: &serde_json::Value) -> i64 {
    let parse = |v: &serde_json::Value| {
        time::OffsetDateTime::parse(
            v.as_str().unwrap(),
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap()
    };
    (parse(&event["payload"]["expires_at"]) - parse(&event["timestamp"])).whole_seconds()
}
fn progress(
    result: Result<wyss_console::sessions::ActivationProgress, SessionError>,
) -> wyss_console::sessions::ActivationStage {
    result.unwrap().stage
}

#[test]
fn a_session_is_activated_through_the_real_authority_from_a_stopped_runtime() {
    use wyss_console::sessions::ActivationStage as Stage;
    let _serial = serial();
    let env = Sandbox::new();
    let mut authority = env.start();
    let mut other = env.client();
    let sessions = env.sessions();
    // Nothing is running: the authority was never asked to start a runtime.
    assert_eq!(
        other.runtime_status().unwrap().actual_state,
        ActualState::Stopped
    );
    let session = sessions.prepare(id()).unwrap().id;
    let shown = sessions.page(None).unwrap().sessions.remove(0);
    assert_eq!(
        (
            shown.binding,
            shown.activatable,
            shown.resumable,
            shown.recoverable
        ),
        (BindingState::Unbound, true, false, false)
    );

    let operation = id();
    let step = |sessions: &SessionService| {
        sessions.activate(operation.clone(), session.clone(), 1, env.workspace())
    };

    // The first step asks the authority to start the runtime, and nothing else.
    assert_eq!(progress(step(&sessions)), Stage::RuntimeStarting);
    let after_start = trail(&mut other);
    assert_eq!(
        kinds(&after_start),
        [
            "authority.managed_session.prepared",
            "authority.runtime.desired_state_changed",
        ]
    );
    assert_eq!(env.methods("thread/start"), 0);
    // Asked again while the authority brings it up: it waits, or goes on once ready.
    // Either way no second start is asked for.
    env.ready(&mut other, ActualState::Ready);
    assert_eq!(progress(step(&sessions)), Stage::Bound);
    let bound = trail(&mut other);
    assert_eq!(count(&bound, "authority.runtime.desired_state_changed"), 1);
    assert_eq!(
        kinds(&bound)[kinds(&bound).len() - 3..],
        [
            "authority.execution_lease.granted",
            "authority.managed_session.runtime_bound",
            "authority.execution_lease.revoked",
        ]
    );
    // The lease the authority granted for it: that scope alone, for fifteen seconds.
    let granted = &bound[bound.len() - 3];
    assert_eq!(
        granted["payload"]["scopes"],
        serde_json::json!(["session_create"])
    );
    assert_eq!(lifetime(granted), 15);
    assert_eq!(other.lease_active(session.clone()).unwrap(), None);
    // One provider thread, made durable the way the authority does it, and no attachment.
    assert_eq!(
        (
            env.methods("thread/start"),
            env.methods("thread/archive"),
            env.methods("thread/unarchive"),
            env.methods("thread/resume")
        ),
        (1, 1, 1, 0)
    );
    let status = other.binding_status(session.clone()).unwrap();
    assert_eq!(status.state, BindingState::BoundDetached);
    let binding = status.binding.unwrap();
    assert_eq!(binding.workspace_root, env.workspace());
    let shown = sessions.page(None).unwrap().sessions.remove(0);
    assert_eq!(
        (shown.binding, shown.activatable, shown.resumable),
        (BindingState::BoundDetached, false, true)
    );
    assert_eq!(shown.workspace.as_deref(), Some(env.workspace().as_str()));

    // The next step attaches it, under a lease for resuming alone.
    assert_eq!(progress(step(&sessions)), Stage::Ready);
    let attached = trail(&mut other);
    assert_eq!(
        kinds(&attached)[kinds(&attached).len() - 3..],
        [
            "authority.execution_lease.granted",
            "authority.managed_session.runtime_attached",
            "authority.execution_lease.revoked",
        ]
    );
    let granted = &attached[attached.len() - 3];
    assert_eq!(
        granted["payload"]["scopes"],
        serde_json::json!(["session_resume"])
    );
    assert_eq!(lifetime(granted), 30);
    assert_eq!(other.lease_active(session.clone()).unwrap(), None);
    assert_eq!(
        other.binding_status(session.clone()).unwrap().state,
        BindingState::BoundAttached
    );
    assert_eq!(env.methods("thread/resume"), 1);
    assert_eq!(sessions.stats().leases, 2);
    let shown = sessions.page(None).unwrap().sessions.remove(0);
    assert_eq!(
        (
            shown.state,
            shown.binding,
            shown.activatable,
            shown.resumable
        ),
        (
            ControlState::Prepared,
            BindingState::BoundAttached,
            false,
            false
        )
    );
    // No identity but the session's own crosses to the window.
    let wire = serde_json::to_string(&shown).unwrap();
    assert!(!wire.contains(&serde_json::to_string(&binding.runtime_session_id).unwrap()));
    assert!(!wire.contains(operation.as_str()));

    // The same action again: nothing is written and nothing reaches Codex.
    let settled = head(&mut other);
    let sent = fs::read_to_string(env.root.join("methods")).unwrap();
    for _ in 0..3 {
        assert_eq!(progress(step(&sessions)), Stage::Ready);
    }
    assert_eq!(head(&mut other), settled);
    assert_eq!(fs::read_to_string(env.root.join("methods")).unwrap(), sent);
    assert_eq!(sessions.stats().leases, 2);

    // A second session, with the runtime already up: bound, and never a start.
    let second = sessions.prepare(id()).unwrap().id;
    let again = id();
    let next = |sessions: &SessionService| {
        sessions.activate(again.clone(), second.clone(), 1, env.workspace())
    };
    assert_eq!(progress(next(&sessions)), Stage::Bound);
    assert_eq!(
        count(
            &trail(&mut other),
            "authority.runtime.desired_state_changed"
        ),
        1
    );
    assert_eq!(env.methods("thread/start"), 2);

    // The authority stops in the middle of that activation and returns. Nothing is
    // compensated: what it kept is what there is. Both bindings are there, detached.
    drop(other);
    authority.stop(true);
    assert_eq!(failed(next(&sessions)), (SessionFailure::Unavailable, true));
    let mut authority = env.start();
    let mut other = env.client();
    env.ready(&mut other, ActualState::Ready);
    for kept in [&session, &second] {
        assert_eq!(
            other.binding_status(kept.clone()).unwrap().state,
            BindingState::BoundDetached
        );
    }
    // The interrupted action goes on where it was: the authority answers its binding
    // request with the receipt it kept, and the session is attached. No second thread.
    assert_eq!(progress(next(&sessions)), Stage::Ready);
    assert_eq!(
        (env.methods("thread/start"), env.methods("thread/resume")),
        (2, 2)
    );
    assert_eq!(
        other.binding_status(second.clone()).unwrap().state,
        BindingState::BoundAttached
    );
    // The action that had finished is not replayed into a new attachment: its attach
    // intent is one the authority already answered. The receipt comes back, the session
    // is read as still detached, and that is what is reported.
    assert_eq!(
        failed(step(&sessions)),
        (SessionFailure::RuntimeUnavailable, false)
    );
    assert_eq!(env.methods("thread/resume"), 2);
    // Resume, a new action, attaches it to the new runtime.
    sessions.resume(id(), session.clone(), 1).unwrap();
    assert_eq!(
        other.binding_status(session.clone()).unwrap().state,
        BindingState::BoundAttached
    );
    assert_eq!(other.lease_active(session.clone()).unwrap(), None);

    other.runtime_stop(mutation()).unwrap();
    env.ready(&mut other, ActualState::Stopped);
    drop(other);
    authority.stop(false);
}

#[test]
fn the_real_authority_decides_the_workspace_the_lease_and_whose_binding_it_is() {
    use wyss_console::sessions::ActivationStage as Stage;
    let _serial = serial();
    let env = Sandbox::new();
    let mut authority = env.start();
    let mut other = env.client();
    let sessions = env.sessions();
    other.runtime_start(mutation()).unwrap();
    env.ready(&mut other, ActualState::Ready);
    let prepared = |sessions: &SessionService| sessions.prepare(id()).unwrap().id;

    // A folder the authority refuses: no lease, no thread, nothing bound.
    let session = prepared(&sessions);
    let file = env.root.join("workspace/a-file");
    fs::write(&file, b"").unwrap();
    let before = head(&mut other);
    for refused in [
        env.root.join("missing").to_str().unwrap().to_owned(),
        file.to_str().unwrap().to_owned(),
        "relative/path".to_owned(),
        format!("/{}", "x".repeat(300)),
    ] {
        assert_eq!(
            failed(sessions.activate(id(), session.clone(), 1, refused.clone())),
            (SessionFailure::WorkspaceInvalid, false),
            "{refused}"
        );
    }
    assert_eq!(head(&mut other), before);
    assert_eq!(
        (env.methods("thread/start"), sessions.stats().leases),
        (0, 0)
    );
    assert_eq!(
        other.binding_status(session.clone()).unwrap().state,
        BindingState::Unbound
    );
    // A link to a workspace: what is bound is where the authority resolved it to.
    let link = env.root.join("link");
    std::os::unix::fs::symlink(env.root.join("workspace"), &link).unwrap();
    let through = id();
    let chosen = link.to_str().unwrap().to_owned();
    assert_eq!(
        progress(sessions.activate(through.clone(), session.clone(), 1, chosen.clone())),
        Stage::Bound
    );
    assert_eq!(
        other
            .binding_status(session.clone())
            .unwrap()
            .binding
            .unwrap()
            .workspace_root,
        env.workspace()
    );
    // The same action, the same folder as it was chosen: its own binding, then attached.
    assert_eq!(
        progress(sessions.activate(through, session.clone(), 1, chosen)),
        Stage::Ready
    );
    assert_eq!(env.methods("thread/start"), 1);

    // A lease somebody else holds that does not grant a binding: denied, and left as it is.
    let session = prepared(&sessions);
    let grant = |other: &mut ControlClient, session: &ManagedSessionId, scopes| {
        other
            .lease_grant(LeaseGrant {
                intent_id: id(),
                session_id: session.clone(),
                expected_session_revision: 1,
                scopes,
                ttl_seconds: 300,
            })
            .unwrap()
    };
    let narrow = grant(&mut other, &session, vec![ExecutionScope::TurnStart]);
    let leases = sessions.stats().leases;
    assert_eq!(
        failed(sessions.activate(id(), session.clone(), 1, env.workspace())),
        (SessionFailure::PermissionDenied, false)
    );
    assert_eq!(
        other.lease_active(session.clone()).unwrap(),
        Some(narrow.clone())
    );
    assert_eq!(
        (env.methods("thread/start"), sessions.stats().leases),
        (1, leases)
    );
    other
        .lease_revoke(LeaseRevoke {
            intent_id: id(),
            lease_id: narrow.lease_id,
            expected_revision: 1,
        })
        .unwrap();
    // One that grants both: used for both steps, and still theirs afterwards.
    let wide = grant(
        &mut other,
        &session,
        vec![ExecutionScope::SessionCreate, ExecutionScope::SessionResume],
    );
    let whole = id();
    assert_eq!(
        progress(sessions.activate(whole.clone(), session.clone(), 1, env.workspace())),
        Stage::Bound
    );
    assert_eq!(
        progress(sessions.activate(whole, session.clone(), 1, env.workspace())),
        Stage::Ready
    );
    assert_eq!(other.lease_active(session.clone()).unwrap(), Some(wide));
    assert_eq!(sessions.stats().leases, leases);

    // A session somebody else bound: this action did not make that binding, so it is
    // neither attached in its name nor bound again.
    let session = prepared(&sessions);
    let lease = grant(&mut other, &session, vec![ExecutionScope::SessionCreate]);
    other
        .binding_create(BindingCreate {
            intent_id: id(),
            managed_session_id: session.clone(),
            expected_session_revision: 1,
            workspace_root: env.workspace(),
        })
        .unwrap();
    let threads = env.methods("thread/start");
    // While their lease stands it even grants a binding: the authority still refuses.
    assert_eq!(
        failed(sessions.activate(id(), session.clone(), 1, env.workspace())),
        (SessionFailure::AlreadyBound, false)
    );
    other
        .lease_revoke(LeaseRevoke {
            intent_id: id(),
            lease_id: lease.lease_id,
            expected_revision: 1,
        })
        .unwrap();
    assert_eq!(
        failed(sessions.activate(id(), session.clone(), 1, env.workspace())),
        (SessionFailure::AlreadyBound, false)
    );
    assert_eq!(
        (
            env.methods("thread/start"),
            env.methods("thread/resume"),
            sessions.stats().leases
        ),
        (threads, 2, leases)
    );
    assert_eq!(
        other.binding_status(session.clone()).unwrap().state,
        BindingState::BoundDetached
    );
    assert_eq!(other.lease_active(session.clone()).unwrap(), None);
    // Resume is the action for it.
    sessions.resume(id(), session.clone(), 1).unwrap();

    // Closed, unknown, and a revision the session is not at.
    let session = prepared(&sessions);
    sessions.close(id(), session.clone(), 1).unwrap();
    for revision in [1, 2] {
        assert_eq!(
            failed(sessions.activate(id(), session.clone(), revision, env.workspace())),
            (SessionFailure::NotAllowed, false)
        );
    }
    let unknown = ManagedSessionId::parse(id().as_str()).unwrap();
    assert_eq!(
        failed(sessions.activate(id(), unknown, 1, env.workspace())),
        (SessionFailure::NotFound, false)
    );
    let session = prepared(&sessions);
    assert_eq!(
        failed(sessions.activate(id(), session.clone(), 2, env.workspace())),
        (SessionFailure::Changed, false)
    );
    assert_eq!(other.lease_active(session.clone()).unwrap(), None);
    assert_eq!(env.methods("thread/start"), threads);
    // Nothing uncertain to abandon on any of them.
    assert_eq!(
        failed(sessions.abandon(id(), session)),
        (SessionFailure::NotAllowed, false)
    );

    other.runtime_stop(mutation()).unwrap();
    env.ready(&mut other, ActualState::Stopped);
    drop(other);
    authority.stop(false);
}

#[test]
fn a_binding_the_provider_never_answered_is_uncertain_until_it_is_abandoned() {
    use wyss_console::sessions::ActivationStage as Stage;
    let _serial = serial();
    let env = Sandbox::new();
    // This Codex takes a thread request and never answers it.
    let mut authority = env.start_as("thread_timeout");
    let mut other = env.client();
    let sessions = env.sessions();
    other.runtime_start(mutation()).unwrap();
    env.ready(&mut other, ActualState::Ready);
    let session = sessions.prepare(id()).unwrap().id;

    let operation = id();
    let error = sessions
        .activate(operation.clone(), session.clone(), 1, env.workspace())
        .unwrap_err();
    // Not a refusal, and not something to send again.
    assert_eq!(
        (error.code, error.retry, error.cause),
        (
            SessionFailure::Uncertain,
            false,
            Some(ControlError::ExternalEffectUnknown)
        )
    );
    assert_eq!(env.methods("thread/start"), 1);
    // (The authority waited ten seconds for Codex: the other client reconnects.)
    let mut other = env.client();
    let status = other.binding_status(session.clone()).unwrap();
    assert_eq!(status.state, BindingState::Indeterminate);
    let attempt = status.operation_id.unwrap();
    // The lease asked for it did not outlive it.
    assert_eq!(other.lease_active(session.clone()).unwrap(), None);
    let shown = sessions.page(None).unwrap().sessions.remove(0);
    assert_eq!(
        (
            shown.binding,
            shown.recoverable,
            shown.activatable,
            shown.resumable
        ),
        (BindingState::Indeterminate, true, false, false)
    );
    assert!(
        !serde_json::to_string(&shown)
            .unwrap()
            .contains(attempt.as_str())
    );

    // The same action, a new action, and across an authority restart: no second thread
    // is ever asked for, and the answer stays "uncertain".
    let settled = head(&mut other);
    for action in [operation.clone(), id()] {
        assert_eq!(
            failed(sessions.activate(action, session.clone(), 1, env.workspace())),
            (SessionFailure::Uncertain, false)
        );
    }
    assert_eq!(
        (head(&mut other), env.methods("thread/start")),
        (settled, 1)
    );
    drop(other);
    authority.stop(false);
    // The authority returns with a Codex that answers.
    let mut authority = env.start();
    let mut other = env.client();
    env.ready(&mut other, ActualState::Ready);
    assert_eq!(
        other.binding_status(session.clone()).unwrap().state,
        BindingState::Indeterminate
    );
    assert_eq!(
        failed(sessions.activate(operation.clone(), session.clone(), 1, env.workspace())),
        (SessionFailure::Uncertain, false)
    );
    assert_eq!(env.methods("thread/start"), 1);

    // The user gives up on that attempt: the authority records it, and names the attempt.
    let intent = id();
    assert_eq!(
        sessions
            .abandon(intent.clone(), session.clone())
            .unwrap()
            .id,
        session
    );
    let events = trail(&mut other);
    let last = events.last().unwrap();
    assert_eq!(
        last["event_type"],
        "authority.managed_session.runtime_operation_abandoned"
    );
    assert_eq!(last["payload"]["operation_id"], attempt.as_str());
    assert_eq!(last["correlation_id"], intent.as_str());
    assert_eq!(
        other.binding_status(session.clone()).unwrap().state,
        BindingState::Unbound
    );
    // Asked again once it is settled: there is nothing to give up on, and nothing changes.
    let settled = head(&mut other);
    assert_eq!(
        failed(sessions.abandon(intent, session.clone())),
        (SessionFailure::NotAllowed, false)
    );
    assert_eq!(head(&mut other), settled);

    // The action that ended in uncertainty stays ended: its binding intent is one the
    // authority holds as failed. A new action binds the session.
    assert_eq!(
        failed(sessions.activate(operation, session.clone(), 1, env.workspace())),
        (SessionFailure::NotAllowed, false)
    );
    let fresh = id();
    assert_eq!(
        progress(sessions.activate(fresh.clone(), session.clone(), 1, env.workspace())),
        Stage::Bound
    );
    assert_eq!(
        progress(sessions.activate(fresh, session.clone(), 1, env.workspace())),
        Stage::Ready
    );
    assert_eq!(env.methods("thread/start"), 2);

    other.runtime_stop(mutation()).unwrap();
    env.ready(&mut other, ActualState::Stopped);
    drop(other);
    authority.stop(false);
}

#[test]
fn repeated_activations_next_to_the_observer_leak_nothing_and_add_no_watcher() {
    use wyss_console::sessions::ActivationStage as Stage;
    let _serial = serial();
    let env = Sandbox::new();
    let _authority = env.start();
    // The authority is listening before the observer makes its one connection.
    drop(env.client());
    let observer = AuthorityService::start(Some(env.run()));
    until(&observer, "connected", |v| {
        v.connection == Connection::Connected
    });
    let sessions = env.sessions();
    // The first activation starts the runtime; the observer's one watch announces it.
    let first = sessions.prepare(id()).unwrap().id;
    let operation = id();
    assert_eq!(
        progress(sessions.activate(operation.clone(), first.clone(), 1, env.workspace())),
        Stage::RuntimeStarting
    );
    until(&observer, "ready", |v| {
        v.agent
            .as_ref()
            .is_some_and(|a| a.actual == ActualState::Ready)
    });
    assert_eq!(
        progress(sessions.activate(operation.clone(), first.clone(), 1, env.workspace())),
        Stage::Bound
    );
    assert_eq!(
        progress(sessions.activate(operation, first, 1, env.workspace())),
        Stage::Ready
    );
    thread::sleep(Duration::from_millis(200));
    let (idle_descriptors, idle_threads) = (descriptors(), threads());
    let connections = sessions.stats().connections;

    let cycles = 12u64;
    for _ in 0..cycles {
        let session = sessions.prepare(id()).unwrap().id;
        let operation = id();
        let step = || sessions.activate(operation.clone(), session.clone(), 1, env.workspace());
        assert_eq!(progress(step()), Stage::Bound);
        assert_eq!(progress(step()), Stage::Ready);
        assert_eq!(progress(step()), Stage::Ready);
        assert_eq!(
            sessions.page(None).unwrap().sessions[0].binding,
            BindingState::BoundAttached
        );
        sessions.close(id(), session, 1).unwrap();
    }
    // Six connections a cycle, each closed with its call; the observer kept its one.
    assert_eq!(sessions.stats().connections, connections + 6 * cycles);
    let link = observer.stats();
    assert_eq!((link.connections, link.failures, link.resyncs), (1, 0, 0));
    let deadline = Instant::now() + Duration::from_secs(3);
    while (descriptors(), threads()) != (idle_descriptors, idle_threads) {
        assert!(
            Instant::now() < deadline,
            "holds {:?}, expected {:?}",
            (descriptors(), threads()),
            (idle_descriptors, idle_threads)
        );
        thread::sleep(Duration::from_millis(10));
    }
    // One runtime start for all of it, and no lease left standing anywhere.
    let mut other = env.client();
    let events = trail(&mut other);
    assert_eq!(count(&events, "authority.runtime.desired_state_changed"), 1);
    assert_eq!(
        count(&events, "authority.execution_lease.granted"),
        count(&events, "authority.execution_lease.revoked")
    );
    assert_eq!(count(&events, "authority.execution_lease.expired"), 0);
    observer.shutdown();
    sessions.shutdown();
    other.runtime_stop(mutation()).unwrap();
    env.ready(&mut other, ActualState::Stopped);
}
