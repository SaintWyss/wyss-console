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
        Authority(
            Command::new(env!("CARGO_BIN_EXE_wyss-control"))
                .env("XDG_RUNTIME_DIR", self.run())
                .env("XDG_STATE_HOME", self.root.join("state"))
                .env("PATH", self.root.join("bin"))
                .env("CODEX_HOME", self.root.join("codex-home"))
                .env("WYSS_RUNTIME_FIXTURE", &self.root)
                .env("WYSS_RUNTIME_SCENARIO", "happy")
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
