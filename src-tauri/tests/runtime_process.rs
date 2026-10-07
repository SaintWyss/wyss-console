#![cfg(target_os = "linux")]
use nix::{
    sys::{
        signal::{Signal, kill},
        wait::{WaitPidFlag, waitpid},
    },
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
    contracts::{WireUuid, control::*},
    control::client::ControlClient,
};
fn id() -> WireUuid {
    WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap()
}
fn bootstrap() -> [&'static str; 9] {
    [
        "initialize",
        "initialized",
        "model/list",
        "config/read",
        "initialize",
        "initialized",
        "model/list",
        "config/read",
        "hooks/list",
    ]
}
fn mutation() -> RuntimeMutation {
    RuntimeMutation {
        intent_id: id(),
        adapter_id: RuntimeTarget::codex().adapter_id,
        runtime_kind: RuntimeTarget::codex().runtime_kind,
        expected_revision: None,
    }
}
struct Sandbox {
    root: PathBuf,
    scenario: &'static str,
}
impl Sandbox {
    fn new(scenario: &'static str) -> Self {
        let root = std::env::temp_dir().join(format!("wyss-rt-{}", id().as_str()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        for d in ["run", "state", "bin"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(root.join(d))
                .unwrap();
        }
        let fake = root.join("bin/codex");
        fs::write(&fake, include_str!("fixtures/codex_runtime.py")).unwrap();
        fs::set_permissions(fake, fs::Permissions::from_mode(0o700)).unwrap();
        Self { root, scenario }
    }
    fn start(&self) -> Authority {
        Authority(
            Command::new(env!("CARGO_BIN_EXE_wyss-control"))
                .env("XDG_RUNTIME_DIR", self.root.join("run"))
                .env("XDG_STATE_HOME", self.root.join("state"))
                .env("PATH", self.root.join("bin"))
                .env("WYSS_RUNTIME_FIXTURE", &self.root)
                .env("WYSS_RUNTIME_SCENARIO", self.scenario)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
    fn client(&self) -> ControlClient {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(c) = ControlClient::connect_in_runtime(&self.root.join("run")) {
                return c;
            }
            assert!(Instant::now() < deadline, "authority startup");
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn pids(&self) -> Vec<i32> {
        fs::read_to_string(self.root.join("pids"))
            .unwrap_or_default()
            .lines()
            .map(|s| s.parse().unwrap())
            .collect()
    }
    fn wait(&self, c: &mut ControlClient, state: ActualState) -> RuntimeStatus {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let s = c.runtime_status().unwrap();
            if s.actual_state == state {
                return s;
            }
            assert!(Instant::now() < deadline, "expected {state:?}, got {s:?}");
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn methods(&self) -> Vec<String> {
        fs::read_to_string(self.root.join("methods"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
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
        kill(
            Pid::from_raw(self.0.id() as i32),
            if hard {
                Signal::SIGKILL
            } else {
                Signal::SIGTERM
            },
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                if !hard {
                    assert!(status.success(), "authority shutdown: {status}");
                }
                break;
            }
            assert!(Instant::now() < deadline, "authority shutdown deadline");
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
fn dead(pid: i32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        // SIGKILL case: this test is a subreaper; wait only for our recorded owned child.
        let _ = waitpid(Pid::from_raw(pid), Some(WaitPidFlag::WNOHANG));
        if !PathBuf::from(format!("/proc/{pid}")).exists() {
            break;
        }
        assert!(Instant::now() < deadline, "owned child {pid} survived");
        thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn authority_runtime_lifecycle_idempotency_clients_restarts_and_sigkill_containment() {
    nix::sys::prctl::set_child_subreaper(true).unwrap();
    let unrelated = Sandbox::new("happy");
    let mut spectator = Authority(
        Command::new(unrelated.root.join("bin/codex"))
            .arg("app-server")
            .env("WYSS_RUNTIME_FIXTURE", &unrelated.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let env = Sandbox::new("happy");
    let mut a = env.start();
    let mut c = env.client();
    let initial = c.runtime_status().unwrap();
    assert_eq!(initial.desired_state, DesiredState::Stopped);
    assert_eq!(initial.actual_state, ActualState::Stopped);
    assert_eq!(initial.revision, 0);
    assert!(env.pids().is_empty());
    let prepared = c
        .prepare(Prepare {
            intent_id: id(),
            adapter_id: RuntimeTarget::codex().adapter_id,
            runtime_kind: RuntimeTarget::codex().runtime_kind,
        })
        .unwrap();
    let start = mutation();
    let committed = c.runtime_start(start.clone()).unwrap();
    assert_eq!(committed.revision, 1);
    let first = env.wait(&mut c, ActualState::Ready);
    let first_id = first.runtime_instance_id.clone().unwrap();
    assert_eq!(first.model_count, 45);
    assert_eq!(c.runtime_start(start.clone()).unwrap(), committed);
    for _ in 0..3 {
        let mut other = env.client();
        assert_eq!(
            other.runtime_status().unwrap().runtime_instance_id,
            Some(first_id.clone())
        );
    }
    assert_eq!(env.pids().len(), 2);
    let mut cursor = None;
    let mut models = Vec::new();
    loop {
        let page = c
            .runtime_models(RuntimeModels {
                adapter_id: RuntimeTarget::codex().adapter_id,
                runtime_kind: RuntimeTarget::codex().runtime_kind,
                cursor,
                limit: 20,
            })
            .unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() < MAX_FRAME);
        let ResultValue::RuntimeModels {
            models: part, next, ..
        } = page
        else {
            panic!()
        };
        assert!(part.len() <= 20);
        for m in &part {
            let v = serde_json::to_value(m).unwrap();
            assert_eq!(v["availability"], "unknown");
            assert!(v["provider_id"].is_null());
        }
        models.extend(part);
        cursor = next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(models.len(), 45);
    // Independent transport record enforces the complete exact outbound plan and ordering.
    assert_eq!(env.methods(), bootstrap());
    drop(c);
    let mut c = env.client();
    assert_eq!(
        c.runtime_status().unwrap().runtime_instance_id,
        Some(first_id.clone())
    );
    assert_eq!(
        c.get(prepared.managed_session_id.clone()).unwrap(),
        prepared
    );
    a.stop(false);
    dead(env.pids()[1]);
    let mut b = env.start();
    let mut c = env.client();
    let second = env.wait(&mut c, ActualState::Ready);
    let second_id = second.runtime_instance_id.unwrap();
    assert_ne!(second_id, first_id);
    assert_eq!(second.revision, 1);
    assert_eq!(second.desired_state, DesiredState::Running);
    assert!(matches!(
        c.runtime_models(RuntimeModels {
            adapter_id: RuntimeTarget::codex().adapter_id,
            runtime_kind: RuntimeTarget::codex().runtime_kind,
            cursor: Some(ModelCursor {
                runtime_instance_id: first_id,
                index: 20
            }),
            limit: 20
        }),
        Err(ControlError::StaleRevision)
    ));
    b.stop(true);
    dead(env.pids()[3]);
    assert!(
        spectator.0.try_wait().unwrap().is_none(),
        "unrelated Codex child affected"
    );
    let mut d = env.start();
    let mut c = env.client();
    let third = env.wait(&mut c, ActualState::Ready);
    assert_ne!(third.runtime_instance_id, Some(second_id));
    assert_eq!(third.revision, 1);
    assert_eq!(
        c.get(prepared.managed_session_id.clone()).unwrap(),
        prepared
    );
    let stop = mutation();
    let stopped = c.runtime_stop(stop.clone()).unwrap();
    assert_eq!(stopped.revision, 2);
    assert_eq!(c.runtime_stop(stop).unwrap(), stopped);
    env.wait(&mut c, ActualState::Stopped);
    dead(env.pids()[5]);
    assert!(matches!(
        c.runtime_models(RuntimeModels {
            adapter_id: RuntimeTarget::codex().adapter_id,
            runtime_kind: RuntimeTarget::codex().runtime_kind,
            cursor: None,
            limit: 20
        }),
        Err(ControlError::InvalidTransition)
    ));
    assert_eq!(c.runtime_start(start).unwrap(), committed);
    assert_eq!(
        c.runtime_status().unwrap().desired_state,
        DesiredState::Stopped
    );
    assert_eq!(env.pids().len(), 6);
    d.stop(false);
    let mut e = env.start();
    let mut c = env.client();
    let final_state = c.runtime_status().unwrap();
    assert_eq!(final_state.actual_state, ActualState::Stopped);
    assert_eq!(final_state.desired_state, DesiredState::Stopped);
    assert_eq!(final_state.revision, 2);
    assert_eq!(env.pids().len(), 6);
    assert_eq!(
        c.get(prepared.managed_session_id.clone()).unwrap(),
        prepared
    );
    e.stop(false);
    assert!(!env.root.join("run/wyss-console/control.sock").exists());
}
#[test]
fn runtime_fails_closed_for_invalid_protocol_exit_requests_and_catalog_without_restart_loop() {
    for scenario in [
        "malformed",
        "oversized",
        "invalid_notification",
        "duplicate",
        "server_request",
        "exit",
        "catalog_error",
        "too_many_models",
    ] {
        let env = Sandbox::new(scenario);
        let mut a = env.start();
        let mut c = env.client();
        c.runtime_start(mutation()).unwrap();
        let failed = env.wait(&mut c, ActualState::Failed);
        assert_eq!(failed.desired_state, DesiredState::Running, "{scenario}");
        assert!(failed.runtime_instance_id.is_none());
        assert_eq!(failed.model_count, 0);
        assert!(failed.diagnostic.is_some());
        let attempts = env.pids().len();
        assert!((1..=2).contains(&attempts));
        for pid in env.pids() {
            dead(pid);
        }
        assert!(env.methods().iter().all(|m| {
            [
                "initialize",
                "initialized",
                "model/list",
                "config/read",
                "hooks/list",
            ]
            .contains(&m.as_str())
        }));
        if scenario == "server_request" {
            assert_eq!(fs::read_to_string(env.root.join("responses")).unwrap(), "0");
            assert_eq!(
                failed.diagnostic,
                Some(RuntimeDiagnostic::InferenceIsolationViolation)
            );
        }
        for _ in 0..3 {
            assert_eq!(
                c.runtime_status().unwrap().actual_state,
                ActualState::Failed
            );
        }
        assert_eq!(env.pids().len(), attempts);
        // New explicit intent admits exactly one new attempt; an ambiguous retry does not.
        let retry = mutation();
        let r = c.runtime_start(retry.clone()).unwrap();
        assert_eq!(r.revision, 2);
        env.wait(&mut c, ActualState::Failed);
        assert_eq!(c.runtime_start(retry).unwrap(), r);
        assert_eq!(env.pids().len(), attempts * 2);
        dead(env.pids()[1]);
        a.stop(false);
    }
}
#[test]
fn ready_runtime_continuously_drains_both_pipes_and_shutdown_reaps() {
    let env = Sandbox::new("flood");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Ready);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !env.root.join("drained").exists() {
        assert!(Instant::now() < deadline, "pipe flood blocked");
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(c.runtime_status().unwrap().actual_state, ActualState::Ready);
    assert_eq!(env.methods(), bootstrap());
    a.stop(false);
    for pid in env.pids() {
        dead(pid);
    }
}
#[test]
fn readiness_audit_failure_reaps_child_and_never_publishes_ready() {
    let env = Sandbox::new("happy");
    let mut a = env.start();
    let mut c = env.client();
    let conn =
        rusqlite::Connection::open(env.root.join("state/wyss-console/control.sqlite3")).unwrap();
    conn.execute_batch("CREATE TRIGGER injected_audit_failure BEFORE INSERT ON control_events WHEN instr(NEW.envelope,'authority.runtime.instance_ready')>0 BEGIN SELECT RAISE(ABORT,'fixture'); END").unwrap();
    c.runtime_start(mutation()).unwrap();
    let failed = env.wait(&mut c, ActualState::Failed);
    assert_eq!(failed.diagnostic, Some(RuntimeDiagnostic::AuditUnavailable));
    assert_eq!(failed.model_count, 0);
    assert_eq!(failed.outbound_methods.initialize, 2);
    assert_eq!(failed.outbound_methods.initialized, 2);
    assert_eq!(failed.outbound_methods.model_list, 2);
    for pid in env.pids() {
        dead(pid);
    }
    let ready:i64=conn.query_row("SELECT count(*) FROM control_events WHERE instr(envelope,'authority.runtime.instance_ready')>0",[],|r|r.get(0)).unwrap();
    assert_eq!(ready, 0);
    conn.execute_batch("DROP TRIGGER injected_audit_failure")
        .unwrap();
    a.stop(false);
}
#[test]
fn stop_interrupts_pending_initialization_without_waiting_for_handshake_deadline() {
    let env = Sandbox::new("timeout");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Initializing);
    let deadline = Instant::now() + Duration::from_secs(3);
    while env.methods().is_empty() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    let started = Instant::now();
    c.runtime_stop(mutation()).unwrap();
    let stopped = env.wait(&mut c, ActualState::Stopped);
    assert_eq!(stopped.outbound_methods.initialize, 1);
    assert_eq!(stopped.outbound_methods.initialized, 0);
    assert_eq!(stopped.outbound_methods.model_list, 0);
    assert!(started.elapsed() < Duration::from_secs(3));
    dead(env.pids()[0]);
    a.stop(false);
}

#[test]
fn supervisor_outbound_method_allowlist_is_exact_on_actual_child_stdin() {
    let env = Sandbox::new("happy");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    let status = env.wait(&mut c, ActualState::Ready);
    assert_eq!(status.outbound_methods.initialize, 2);
    assert_eq!(status.outbound_methods.initialized, 2);
    assert_eq!(status.outbound_methods.model_list, 2);
    let methods = env.methods();
    assert_eq!(methods, bootstrap());
    let previous = status.runtime_instance_id;
    assert_eq!(c.runtime_start(mutation()).unwrap().revision, 2);
    assert_eq!(c.runtime_status().unwrap().runtime_instance_id, previous);
    assert_eq!(env.pids().len(), 2);
    for forbidden in [
        "thread/start",
        "thread/resume",
        "thread/read",
        "turn/start",
        "turn/interrupt",
        "review/start",
        "RESPONSE",
    ] {
        assert_eq!(
            methods.iter().filter(|m| m.as_str() == forbidden).count(),
            0
        );
    }
    c.runtime_stop(mutation()).unwrap();
    env.wait(&mut c, ActualState::Stopped);
    assert_eq!(env.methods(), methods);
    a.stop(false);
}

// Sprint 2F: lease traffic cannot extend the fixed Codex outbound allowlist.
#[test]
fn leases_gate_disconnect_expiry_concurrent_grants_and_restarts_execute_zero_work() {
    use wyss_console::contracts::identities::ManagedSessionId;
    nix::sys::prctl::set_child_subreaper(true).unwrap();
    let env = Sandbox::new("happy");
    let mut a = env.start();
    let mut c = env.client();
    let prepared = c
        .prepare(Prepare {
            intent_id: id(),
            adapter_id: RuntimeTarget::codex().adapter_id,
            runtime_kind: RuntimeTarget::codex().runtime_kind,
        })
        .unwrap();
    let session = prepared.managed_session_id.clone();
    let query = |action| GateQuery {
        session_id: session.clone(),
        action,
    };
    assert!(c.lease_active(session.clone()).unwrap().is_none());
    assert_eq!(
        c.gate_status(query(ExecutionScope::TurnStart))
            .unwrap()
            .reason,
        GateReason::LeaseMissing
    );
    let grant = LeaseGrant {
        intent_id: id(),
        session_id: session.clone(),
        expected_session_revision: 1,
        scopes: vec![ExecutionScope::SessionCreate, ExecutionScope::TurnStart],
        ttl_seconds: 60,
    };
    let lease = c.lease_grant(grant.clone()).unwrap();
    assert_eq!(
        c.gate_status(query(ExecutionScope::TurnStart))
            .unwrap()
            .reason,
        GateReason::RuntimeNotRunning
    );
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Ready);
    let methods = env.methods();
    assert_eq!(methods, bootstrap());
    assert_eq!(
        c.gate_status(query(ExecutionScope::SessionCreate))
            .unwrap()
            .reason,
        GateReason::Allowed
    );
    assert_eq!(
        c.gate_status(query(ExecutionScope::TurnStart))
            .unwrap()
            .reason,
        GateReason::BindingNotFound
    );
    assert_eq!(
        c.gate_status(query(ExecutionScope::SessionResume))
            .unwrap()
            .reason,
        GateReason::ScopeNotGranted
    );
    drop(c);
    let mut c = env.client();
    assert_eq!(c.lease_get(lease.lease_id.clone()).unwrap(), lease);
    let renewed = c
        .lease_renew(LeaseRenew {
            intent_id: id(),
            lease_id: lease.lease_id.clone(),
            expected_revision: 1,
            ttl_seconds: 120,
        })
        .unwrap();
    assert_eq!(renewed.revision, 2);
    c.runtime_stop(mutation()).unwrap();
    env.wait(&mut c, ActualState::Stopped);
    assert_eq!(c.lease_get(lease.lease_id.clone()).unwrap(), renewed);
    assert_eq!(
        c.gate_status(query(ExecutionScope::TurnStart))
            .unwrap()
            .reason,
        GateReason::RuntimeNotRunning
    );
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Ready);
    assert_eq!(
        c.gate_status(query(ExecutionScope::TurnStart))
            .unwrap()
            .reason,
        GateReason::BindingNotFound
    );
    // Graceful and hard restarts preserve permission identity/revision/deadline exactly.
    for hard in [false, true] {
        let pid = *env.pids().last().unwrap();
        drop(c);
        a.stop(hard);
        dead(pid);
        a = env.start();
        c = env.client();
        assert_eq!(c.lease_get(lease.lease_id.clone()).unwrap(), renewed);
        env.wait(&mut c, ActualState::Ready);
        assert_eq!(
            c.gate_status(query(ExecutionScope::TurnStart))
                .unwrap()
                .reason,
            GateReason::BindingNotFound
        );
    }
    let before = env.methods();
    let revoked = c
        .lease_revoke(LeaseRevoke {
            intent_id: id(),
            lease_id: lease.lease_id,
            expected_revision: 2,
        })
        .unwrap();
    assert_eq!(
        revoked.revocation_reason,
        Some(RevocationReason::UserRequest)
    );
    assert_eq!(c.runtime_status().unwrap().actual_state, ActualState::Ready);
    let short = c
        .lease_grant(LeaseGrant {
            intent_id: id(),
            ttl_seconds: 1,
            ..grant.clone()
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        let l = c.lease_get(short.lease_id.clone()).unwrap();
        if l.state == LeaseState::Expired && l.revision == 2 {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    assert!(c.lease_active(session.clone()).unwrap().is_none());
    assert_eq!(
        c.gate_status(query(ExecutionScope::TurnStart))
            .unwrap()
            .reason,
        GateReason::LeaseExpired
    );
    // Simultaneous grants serialize in the authority; only one can succeed.
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let mut client = env.client();
            let p = LeaseGrant {
                intent_id: id(),
                ..grant.clone()
            };
            thread::spawn(move || client.lease_grant(p))
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(ControlError::LeaseAlreadyActive)))
            .count(),
        1
    );
    let active = c.lease_active(session.clone()).unwrap().unwrap();
    c.close(Close {
        intent_id: id(),
        managed_session_id: session.clone(),
        expected_revision: 1,
    })
    .unwrap();
    assert_eq!(
        c.lease_get(active.lease_id).unwrap().revocation_reason,
        Some(RevocationReason::SessionClosed)
    );
    assert_eq!(
        c.gate_status(query(ExecutionScope::TurnStart))
            .unwrap()
            .reason,
        GateReason::SessionNotPrepared
    );
    assert_eq!(env.methods(), before); // All lease/gate/close operations sent zero Codex frames.
    let audit = c
        .audit(AuditRead {
            stream_id: c.hello_status().stream_id.clone(),
            after_sequence: None,
            limit: 50,
        })
        .unwrap();
    let value = serde_json::to_value(audit).unwrap();
    let events = value["value"]["events"].as_array().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e["event_type"] == "authority.execution_lease.expired")
            .count(),
        1
    );
    // Offline expiration after hard crash must be reconciled before serving clients.
    let second = c
        .prepare(Prepare {
            intent_id: id(),
            adapter_id: RuntimeTarget::codex().adapter_id,
            runtime_kind: RuntimeTarget::codex().runtime_kind,
        })
        .unwrap();
    let offline = c
        .lease_grant(LeaseGrant {
            intent_id: id(),
            session_id: second.managed_session_id.clone(),
            ttl_seconds: 1,
            ..grant
        })
        .unwrap();
    let pid = *env.pids().last().unwrap();
    drop(c);
    a.stop(true);
    dead(pid);
    thread::sleep(Duration::from_millis(1100));
    a = env.start();
    c = env.client();
    let expired = c.lease_get(offline.lease_id).unwrap();
    assert_eq!(expired.state, LeaseState::Expired);
    assert_eq!(expired.revision, 2);
    assert_eq!(expired.expires_at, offline.expires_at);
    assert_eq!(
        c.gate_status(GateQuery {
            session_id: ManagedSessionId::parse(second.managed_session_id.as_str()).unwrap(),
            action: ExecutionScope::TurnStart
        })
        .unwrap()
        .reason,
        GateReason::LeaseExpired
    );
    c.runtime_stop(mutation()).unwrap();
    env.wait(&mut c, ActualState::Stopped);
    drop(c);
    a.stop(false);
    for pid in env.pids() {
        dead(pid)
    }
    assert!(!env.root.join("run/wyss-console/control.sock").exists());
    for method in env.methods() {
        assert!(
            [
                "initialize",
                "initialized",
                "model/list",
                "config/read",
                "hooks/list"
            ]
            .contains(&method.as_str())
        );
    }
}

fn prepare_bound_session(
    c: &mut ControlClient,
    scopes: Vec<ExecutionScope>,
) -> (ManagedSessionRecord, ExecutionLease) {
    let s = c
        .prepare(Prepare {
            intent_id: id(),
            adapter_id: RuntimeTarget::codex().adapter_id,
            runtime_kind: RuntimeTarget::codex().runtime_kind,
        })
        .unwrap();
    let lease = c
        .lease_grant(LeaseGrant {
            intent_id: id(),
            session_id: s.managed_session_id.clone(),
            expected_session_revision: 1,
            scopes,
            ttl_seconds: 300,
        })
        .unwrap();
    (s, lease)
}
fn create_params(env: &Sandbox, s: &ManagedSessionRecord) -> BindingCreate {
    BindingCreate {
        intent_id: id(),
        managed_session_id: s.managed_session_id.clone(),
        expected_session_revision: 1,
        workspace_root: env.root.to_str().unwrap().to_owned(),
    }
}
fn resume_params(s: &ManagedSessionRecord) -> BindingResume {
    BindingResume {
        intent_id: id(),
        managed_session_id: s.managed_session_id.clone(),
        expected_session_revision: 1,
    }
}
fn count(env: &Sandbox, method: &str) -> usize {
    env.methods()
        .iter()
        .filter(|m| m.as_str() == method)
        .count()
}
fn wait_file(env: &Sandbox, name: &str) {
    let until = Instant::now() + Duration::from_secs(5);
    while !env.root.join(name).exists() {
        assert!(Instant::now() < until, "{name}");
        thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn successful_create_retry_uses_receipt_after_workspace_disappears() {
    let env = Sandbox::new("happy");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Ready);
    let (s, _) = prepare_bound_session(&mut c, vec![ExecutionScope::SessionCreate]);
    let workspace = env.root.join("workspace");
    fs::create_dir(&workspace).unwrap();
    let alias = env.root.join("workspace-alias");
    std::os::unix::fs::symlink(&workspace, &alias).unwrap();
    let mut p = create_params(&env, &s);
    p.workspace_root = workspace.to_str().unwrap().to_owned();
    let b = c.binding_create(p.clone()).unwrap();
    let mut aliased = p.clone();
    aliased.workspace_root = alias.to_str().unwrap().to_owned();
    assert_eq!(c.binding_create(aliased).unwrap(), b);
    let methods = env.methods();
    fs::remove_dir(&workspace).unwrap();
    assert_eq!(c.binding_create(p).unwrap(), b);
    assert_eq!(env.methods(), methods);
    assert_eq!(count(&env, "thread/start"), 1);
    assert_eq!(count(&env, "thread/archive"), 1);
    assert_eq!(count(&env, "thread/unarchive"), 1);
    assert_eq!(count(&env, "thread/resume"), 0);
    c.runtime_stop(mutation()).unwrap();
    env.wait(&mut c, ActualState::Stopped);
    drop(c);
    a.stop(false);
    for pid in env.pids() {
        dead(pid)
    }
}
#[test]
fn managed_bindings_share_runtime_survive_restarts_require_explicit_resume_and_keep_zero_turns() {
    let env = Sandbox::new("happy");
    let mut a = env.start();
    let mut c = env.client();
    let (s, l) = prepare_bound_session(
        &mut c,
        vec![ExecutionScope::SessionCreate, ExecutionScope::SessionResume],
    );
    assert_eq!(
        c.binding_create(create_params(&env, &s)).unwrap_err(),
        ControlError::GateDenied
    );
    assert_eq!(count(&env, "thread/start"), 0);
    c.runtime_start(mutation()).unwrap();
    let first = env.wait(&mut c, ActualState::Ready);
    let p = create_params(&env, &s);
    let b = c.binding_create(p.clone()).unwrap();
    assert_eq!(c.binding_create(p.clone()).unwrap(), b);
    assert_eq!(count(&env, "thread/start"), 1);
    let mut conflicting = p.clone();
    conflicting.managed_session_id =
        wyss_console::contracts::identities::ManagedSessionId::parse(id().as_str()).unwrap();
    assert_eq!(
        c.binding_create(conflicting).unwrap_err(),
        ControlError::IntentConflict
    );
    assert_eq!(
        c.binding_create(create_params(&env, &s)).unwrap_err(),
        ControlError::BindingAlreadyExists
    );
    let (s2, _) = prepare_bound_session(&mut c, vec![ExecutionScope::SessionCreate]);
    let b2 = c.binding_create(create_params(&env, &s2)).unwrap();
    assert_eq!(
        c.binding_status(s.managed_session_id.clone())
            .unwrap()
            .state,
        BindingState::BoundDetached
    );
    assert_eq!(
        c.binding_status(s2.managed_session_id.clone())
            .unwrap()
            .state,
        BindingState::BoundDetached
    );
    assert_eq!(count(&env, "thread/resume"), 0);
    assert_eq!(count(&env, "thread/archive"), 2);
    assert_eq!(count(&env, "thread/unarchive"), 2);
    assert_eq!(c.binding_resume(resume_params(&s)).unwrap(), b);

    assert_ne!(b.runtime_session_id, b2.runtime_session_id);
    assert_eq!(
        c.binding_status(s.managed_session_id.clone())
            .unwrap()
            .runtime_instance_id,
        first.runtime_instance_id
    );
    assert_eq!(
        c.binding_status(s2.managed_session_id.clone())
            .unwrap()
            .runtime_instance_id,
        None
    );
    let before = count(&env, "thread/resume");
    assert_eq!(
        c.binding_resume(resume_params(&s2)).unwrap_err(),
        ControlError::GateDenied
    );
    assert_eq!(count(&env, "thread/resume"), before);
    drop(c);
    let mut c = env.client();
    assert_eq!(c.lease_get(l.lease_id.clone()).unwrap(), l);
    assert_eq!(
        c.binding_status(s.managed_session_id.clone())
            .unwrap()
            .state,
        BindingState::BoundAttached
    );
    drop(c);
    a.stop(false);
    let mut a = env.start();
    let mut c = env.client();
    let second = env.wait(&mut c, ActualState::Ready);
    assert_ne!(first.runtime_instance_id, second.runtime_instance_id);
    assert_eq!(
        c.binding_status(s.managed_session_id.clone())
            .unwrap()
            .state,
        BindingState::BoundDetached
    );
    assert_eq!(count(&env, "thread/resume"), before);
    let r = resume_params(&s);
    assert_eq!(c.binding_resume(r.clone()).unwrap(), b);
    assert_eq!(c.binding_resume(r).unwrap(), b);
    assert_eq!(count(&env, "thread/resume"), before + 1);
    c.runtime_stop(mutation()).unwrap();
    env.wait(&mut c, ActualState::Stopped);
    assert_eq!(
        c.binding_status(s.managed_session_id.clone())
            .unwrap()
            .state,
        BindingState::BoundDetached
    );
    assert_eq!(c.lease_get(l.lease_id.clone()).unwrap(), l);
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Ready);
    assert_eq!(count(&env, "thread/resume"), before + 1);
    assert_eq!(c.binding_resume(resume_params(&s)).unwrap(), b);
    c.close(Close {
        intent_id: id(),
        managed_session_id: s.managed_session_id.clone(),
        expected_revision: 1,
    })
    .unwrap();
    let status = c.binding_status(s.managed_session_id.clone()).unwrap();
    assert_eq!(status.binding, Some(b));
    assert_eq!(status.state, BindingState::BoundDetached);
    assert_eq!(c.lease_get(l.lease_id).unwrap().state, LeaseState::Revoked);
    assert_eq!(
        c.binding_resume(resume_params(&s)).unwrap_err(),
        ControlError::GateDenied
    );
    let allowed = std::collections::BTreeSet::from([
        "initialize",
        "initialized",
        "model/list",
        "thread/start",
        "thread/resume",
        "thread/archive",
        "thread/unarchive",
        "config/read",
        "hooks/list",
    ]);
    assert_eq!(
        env.methods()
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        allowed
    );
    c.runtime_stop(mutation()).unwrap();
    env.wait(&mut c, ActualState::Stopped);
    drop(c);
    a.stop(false);
    for pid in env.pids() {
        dead(pid)
    }
}
#[test]
fn create_denied_without_lease_or_with_turn_only_scope_sends_no_frame() {
    let env = Sandbox::new("happy");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Ready);
    let s = c
        .prepare(Prepare {
            intent_id: id(),
            adapter_id: RuntimeTarget::codex().adapter_id,
            runtime_kind: RuntimeTarget::codex().runtime_kind,
        })
        .unwrap();
    assert_eq!(
        c.binding_create(create_params(&env, &s)).unwrap_err(),
        ControlError::GateDenied
    );
    c.lease_grant(LeaseGrant {
        intent_id: id(),
        session_id: s.managed_session_id.clone(),
        expected_session_revision: 1,
        scopes: vec![ExecutionScope::TurnStart],
        ttl_seconds: 300,
    })
    .unwrap();
    assert_eq!(
        c.binding_create(create_params(&env, &s)).unwrap_err(),
        ControlError::GateDenied
    );
    assert_eq!(count(&env, "thread/start"), 0);
    assert_eq!(
        c.binding_resume(resume_params(&s)).unwrap_err(),
        ControlError::BindingNotFound
    );
    drop(c);
    a.stop(false);
}
#[test]
fn malformed_mismatched_server_request_and_timeout_fail_without_binding_or_automatic_retry() {
    for scenario in [
        "thread_malformed",
        "thread_wrong_cwd",
        "thread_wrong_response",
        "thread_server_request",
        "thread_timeout",
    ] {
        let env = Sandbox::new(scenario);
        let mut a = env.start();
        let mut c = env.client();
        c.runtime_start(mutation()).unwrap();
        env.wait(&mut c, ActualState::Ready);
        let (s, _) = prepare_bound_session(&mut c, vec![ExecutionScope::SessionCreate]);
        let p = create_params(&env, &s);
        assert_eq!(
            c.binding_create(p.clone()).unwrap_err(),
            ControlError::ExternalEffectUnknown,
            "{scenario}"
        );
        env.wait(&mut c, ActualState::Failed);
        assert_eq!(
            c.binding_status(s.managed_session_id.clone())
                .unwrap()
                .state,
            BindingState::Indeterminate
        );
        assert_eq!(
            c.binding_create(p).unwrap_err(),
            ControlError::BindingIndeterminate
        );
        assert_eq!(count(&env, "thread/start"), 1);
        assert_eq!(count(&env, "RESPONSE"), 0);
        drop(c);
        a.stop(false);
    }
}
#[test]
fn provider_rejection_is_known_failure_and_never_persists_raw_message() {
    let env = Sandbox::new("thread_error");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Ready);
    let (s, _) = prepare_bound_session(&mut c, vec![ExecutionScope::SessionCreate]);
    assert_eq!(
        c.binding_create(create_params(&env, &s)).unwrap_err(),
        ControlError::ProviderRejected
    );
    assert_eq!(
        c.binding_status(s.managed_session_id.clone())
            .unwrap()
            .state,
        BindingState::Unbound
    );
    assert_eq!(c.runtime_status().unwrap().actual_state, ActualState::Ready);
    drop(c);
    a.stop(false);
    let db = fs::read(env.root.join("state/wyss-console/control.sqlite3")).unwrap();
    assert!(
        !db.windows(b"private provider error".len())
            .any(|w| w == b"private provider error")
    );
}
#[test]
fn rejected_resume_discards_previous_attachment_without_stopping_runtime() {
    let env = Sandbox::new("resume_error");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Ready);
    let (s, _) = prepare_bound_session(
        &mut c,
        vec![ExecutionScope::SessionCreate, ExecutionScope::SessionResume],
    );
    let b = c.binding_create(create_params(&env, &s)).unwrap();
    assert_eq!(c.binding_resume(resume_params(&s)).unwrap(), b);
    assert_eq!(
        c.binding_status(s.managed_session_id.clone())
            .unwrap()
            .state,
        BindingState::BoundAttached
    );
    assert_eq!(
        c.binding_resume(resume_params(&s)).unwrap_err(),
        ControlError::ProviderRejected
    );
    let status = c.binding_status(s.managed_session_id).unwrap();
    assert_eq!(status.binding, Some(b));
    assert_eq!(status.state, BindingState::BoundDetached);
    assert_eq!(c.runtime_status().unwrap().actual_state, ActualState::Ready);
    assert_eq!(count(&env, "thread/start"), 1);
    assert_eq!(count(&env, "thread/resume"), 2);
    drop(c);
    a.stop(false);
}
#[test]
fn resume_identity_substitution_fails_closed_without_rewriting_binding() {
    let env = Sandbox::new("thread_wrong_id");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Ready);
    let (s, _) = prepare_bound_session(
        &mut c,
        vec![ExecutionScope::SessionCreate, ExecutionScope::SessionResume],
    );
    let b = c.binding_create(create_params(&env, &s)).unwrap();
    assert_eq!(
        c.binding_resume(resume_params(&s)).unwrap_err(),
        ControlError::ExternalEffectUnknown
    );
    env.wait(&mut c, ActualState::Failed);
    let status = c.binding_status(s.managed_session_id).unwrap();
    assert_eq!(status.binding, Some(b));
    assert_eq!(status.state, BindingState::BoundDetached);
    drop(c);
    a.stop(false);
}
#[test]
fn sigkill_after_dispatch_and_after_response_recovers_indeterminate_without_duplicate_create() {
    nix::sys::prctl::set_child_subreaper(true).unwrap();
    for (scenario, marker) in [
        ("thread_hold", "dispatched"),
        ("thread_lock_response", "responded"),
        ("archive_hold", "archive-written"),
        ("archive_lock_response", "archive-responded"),
        ("unarchive_hold", "unarchive-written"),
        ("unarchive_lock_response", "unarchive-responded"),
    ] {
        let mut env = Sandbox::new(scenario);
        let mut a = env.start();
        let mut c = env.client();
        c.runtime_start(mutation()).unwrap();
        env.wait(&mut c, ActualState::Ready);
        let (s, _) = prepare_bound_session(&mut c, vec![ExecutionScope::SessionCreate]);
        let p = create_params(&env, &s);
        let pp = p.clone();
        let client = thread::spawn(move || c.binding_create(pp));
        wait_file(&env, marker);
        a.stop(true);
        for pid in env.pids() {
            dead(pid)
        }
        let _ = client.join();
        env.scenario = "happy";
        let mut a = env.start();
        let mut c = env.client();
        env.wait(&mut c, ActualState::Ready);
        let status = c.binding_status(s.managed_session_id.clone()).unwrap();
        assert_eq!(status.state, BindingState::Indeterminate);
        assert!(status.binding.is_none());
        assert_eq!(
            c.binding_create(p).unwrap_err(),
            ControlError::BindingIndeterminate
        );
        assert_eq!(count(&env, "thread/start"), 1);
        c.binding_abandon(BindingAbandon {
            intent_id: id(),
            managed_session_id: s.managed_session_id.clone(),
            operation_id: status.operation_id.unwrap(),
        })
        .unwrap();
        c.binding_create(create_params(&env, &s)).unwrap();
        assert_eq!(count(&env, "thread/start"), 2);
        drop(c);
        a.stop(false);
        for pid in env.pids() {
            dead(pid)
        }
    }
}
#[test]
fn concurrent_creates_have_one_outstanding_request_and_one_binding() {
    let env = Sandbox::new("thread_hold");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Ready);
    let (s, _) = prepare_bound_session(&mut c, vec![ExecutionScope::SessionCreate]);
    let p = create_params(&env, &s);
    let second = p.clone();
    let worker = thread::spawn(move || c.binding_create(p));
    wait_file(&env, "dispatched");
    let mut c = env.client();
    assert_eq!(
        c.binding_create(second).unwrap_err(),
        ControlError::RuntimeOperationInProgress
    );
    fs::write(env.root.join("release"), b"").unwrap();
    worker.join().unwrap().unwrap();
    assert_eq!(count(&env, "thread/start"), 1);
    drop(c);
    a.stop(false);
}
#[test]
fn graceful_shutdown_interrupts_inflight_create_and_recovers_uncertainty() {
    let mut env = Sandbox::new("thread_hold");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Ready);
    let (s, _) = prepare_bound_session(&mut c, vec![ExecutionScope::SessionCreate]);
    let p = create_params(&env, &s);
    let worker = thread::spawn(move || c.binding_create(p));
    wait_file(&env, "dispatched");
    let start = Instant::now();
    a.stop(false);
    assert!(start.elapsed() < Duration::from_secs(5));
    let _ = worker.join();
    env.scenario = "happy";
    let mut a = env.start();
    let mut c = env.client();
    env.wait(&mut c, ActualState::Ready);
    assert_eq!(
        c.binding_status(s.managed_session_id).unwrap().state,
        BindingState::Indeterminate
    );
    assert_eq!(count(&env, "thread/start"), 1);
    drop(c);
    a.stop(false);
}
#[test]
fn close_after_admitted_create_keeps_identity_historical_and_never_grants_attachment() {
    let env = Sandbox::new("thread_hold");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Ready);
    let (s, l) = prepare_bound_session(&mut c, vec![ExecutionScope::SessionCreate]);
    let p = create_params(&env, &s);
    let worker = thread::spawn(move || c.binding_create(p));
    wait_file(&env, "dispatched");
    let mut c = env.client();
    c.close(Close {
        intent_id: id(),
        managed_session_id: s.managed_session_id.clone(),
        expected_revision: 1,
    })
    .unwrap();
    fs::write(env.root.join("release"), b"").unwrap();
    let b = worker.join().unwrap().unwrap();
    let status = c.binding_status(s.managed_session_id.clone()).unwrap();
    assert_eq!(status.binding, Some(b));
    assert_eq!(status.state, BindingState::BoundDetached);
    assert_eq!(c.lease_get(l.lease_id).unwrap().state, LeaseState::Revoked);
    assert_eq!(
        c.binding_resume(resume_params(&s)).unwrap_err(),
        ControlError::GateDenied
    );
    drop(c);
    a.stop(false);
}

#[test]
fn materialization_errors_are_indeterminate_never_bind_or_retry_and_never_respond() {
    for scenario in [
        "archive_error",
        "unarchive_error",
        "archive_extra",
        "unarchive_extra",
        "archive_wrong_response",
        "unarchive_wrong_response",
        "unarchive_wrong_id",
        "unarchive_wrong_cwd",
        "unarchive_relative_cwd",
        "unarchive_malformed",
        "archive_server_request",
        "unarchive_server_request",
        "archive_timeout",
        "unarchive_timeout",
    ] {
        let env = Sandbox::new(scenario);
        let mut a = env.start();
        let mut c = env.client();
        c.runtime_start(mutation()).unwrap();
        env.wait(&mut c, ActualState::Ready);
        let (s, _) = prepare_bound_session(&mut c, vec![ExecutionScope::SessionCreate]);
        let p = create_params(&env, &s);
        assert_eq!(
            c.binding_create(p.clone()).unwrap_err(),
            ControlError::ExternalEffectUnknown,
            "{scenario}"
        );
        env.wait(&mut c, ActualState::Failed);
        assert_eq!(
            c.binding_status(s.managed_session_id.clone())
                .unwrap()
                .state,
            BindingState::Indeterminate
        );
        assert_eq!(
            c.binding_create(p).unwrap_err(),
            ControlError::BindingIndeterminate
        );
        assert_eq!(count(&env, "thread/start"), 1);
        assert_eq!(count(&env, "thread/resume"), 0);
        assert_eq!(count(&env, "RESPONSE"), 0);
        drop(c);
        a.stop(false);
    }
}

fn inference_session(
    env: &Sandbox,
    c: &mut ControlClient,
) -> (ManagedSessionRecord, ExecutionLease) {
    let (s, l) = prepare_bound_session(
        c,
        vec![
            ExecutionScope::SessionCreate,
            ExecutionScope::SessionResume,
            ExecutionScope::TurnStart,
        ],
    );
    c.binding_create(create_params(env, &s)).unwrap();
    assert_eq!(
        c.binding_status(s.managed_session_id.clone())
            .unwrap()
            .state,
        BindingState::BoundDetached
    );
    c.binding_resume(resume_params(&s)).unwrap();
    assert_eq!(
        c.binding_status(s.managed_session_id.clone())
            .unwrap()
            .attachment_profile,
        Some(AttachmentProfile::InferenceOnlyV1)
    );
    (s, l)
}
fn turn_params(s: &ManagedSessionRecord) -> TurnStart {
    TurnStart {
        intent_id: id(),
        managed_session_id: s.managed_session_id.clone(),
        expected_session_revision: s.revision,
        prompt: format!("WYSS_2H_SECRET_PROMPT_{}", id().as_str()),
    }
}
fn wait_turn(c: &mut ControlClient, t: &ManagedTurnRecord, terminal: bool) -> ManagedTurnRecord {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let current = c.turn_status(t.managed_turn_id.clone()).unwrap();
        if current.state.terminal() == terminal && (terminal || current.state == TurnState::Running)
        {
            return current;
        }
        assert!(Instant::now() < deadline, "turn state {current:?}");
        thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn managed_turn_stream_race_privacy_idempotency_disconnect_and_restart() {
    let env = Sandbox::new("happy");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    let ready = env.wait(&mut c, ActualState::Ready);
    assert_eq!(
        ready.runtime_safety_profile,
        Some(RuntimeSafetyProfile::CodexInferenceV1)
    );
    let (s, _) = inference_session(&env, &mut c);
    let params = turn_params(&s);
    let t = c.turn_start(params.clone()).unwrap();
    assert_eq!(t.state, TurnState::Starting);
    drop(c);
    let mut c = env.client();
    let done = wait_turn(&mut c, &t, true);
    assert_eq!(done.state, TurnState::Completed);
    assert_eq!(done.revision, 3);
    assert!(done.codex_turn_id.is_some());
    assert_eq!(c.turn_start(params.clone()).unwrap(), t);
    assert_eq!(count(&env, "turn/start"), 1);
    let mut conflict = params.clone();
    conflict.prompt.push('x');
    assert_eq!(
        c.turn_start(conflict).unwrap_err(),
        ControlError::IntentConflict
    );
    let page = c
        .turn_events(TurnEvents {
            managed_turn_id: t.managed_turn_id.clone(),
            after_sequence: None,
            limit: 50,
        })
        .unwrap();
    let raw = serde_json::to_string(&page).unwrap();
    assert!(raw.contains("ASSISTANT_SENTINEL") && raw.contains("SUMMARY_SENTINEL"));
    assert!(!raw.contains("RAW_PRIVATE_REASONING_SENTINEL"));
    assert!(
        page.events
            .windows(2)
            .all(|w| w[0].sequence < w[1].sequence)
    );
    assert!(page.terminal);
    for name in [
        "control.sqlite3",
        "control.sqlite3-wal",
        "control.sqlite3-shm",
    ] {
        let bytes = fs::read(env.root.join("state/wyss-console").join(name)).unwrap_or_default();
        for sentinel in [
            params.prompt.as_bytes(),
            b"ASSISTANT_SENTINEL",
            b"SUMMARY_SENTINEL",
            b"RAW_PRIVATE_REASONING_SENTINEL",
        ] {
            assert!(
                !bytes.windows(sentinel.len()).any(|w| w == sentinel),
                "durable content in {name}"
            );
        }
    }
    let original = done.clone();
    a.stop(false);
    a = env.start();
    let mut c = env.client();
    env.wait(&mut c, ActualState::Ready);
    assert_eq!(c.turn_status(t.managed_turn_id.clone()).unwrap(), original);
    assert_eq!(count(&env, "turn/start"), 1);
    assert_eq!(
        c.binding_status(s.managed_session_id.clone())
            .unwrap()
            .state,
        BindingState::BoundDetached
    );
    assert_eq!(
        c.turn_start(turn_params(&s)).unwrap_err(),
        ControlError::GateDenied
    );
    assert_eq!(count(&env, "turn/start"), 1);
    c.runtime_stop(mutation()).unwrap();
    env.wait(&mut c, ActualState::Stopped);
    a.stop(false);
    for pid in env.pids() {
        dead(pid);
    }
}
#[test]
fn turn_cancel_is_reductive_idempotent_and_global_concurrency_is_one() {
    let env = Sandbox::new("turn_long");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Ready);
    let (s, l) = inference_session(&env, &mut c);
    let (other, _) = inference_session(&env, &mut c);
    let t = c.turn_start(turn_params(&s)).unwrap();
    let running = wait_turn(&mut c, &t, false);
    assert_eq!(
        c.turn_start(turn_params(&other)).unwrap_err(),
        ControlError::ActiveTurnExists
    );
    assert_eq!(count(&env, "turn/start"), 1);
    let cancel = TurnCancel {
        intent_id: id(),
        managed_turn_id: t.managed_turn_id.clone(),
        expected_revision: running.revision,
    };
    let receipt = c.turn_cancel(cancel.clone()).unwrap();
    assert_eq!(c.turn_cancel(cancel).unwrap(), receipt);
    let done = wait_turn(&mut c, &t, true);
    assert_eq!(done.state, TurnState::Interrupted);
    assert_eq!(count(&env, "turn/interrupt"), 1);
    assert_eq!(c.runtime_status().unwrap().actual_state, ActualState::Ready);
    assert_eq!(
        c.turn_cancel(TurnCancel {
            intent_id: id(),
            managed_turn_id: t.managed_turn_id.clone(),
            expected_revision: done.revision
        })
        .unwrap_err(),
        ControlError::InvalidTransition
    );
    let next = c.turn_start(turn_params(&other)).unwrap();
    wait_turn(&mut c, &next, false);
    c.lease_revoke(LeaseRevoke {
        intent_id: id(),
        lease_id: l.lease_id,
        expected_revision: l.revision,
    })
    .unwrap(); // Other session remains independent.
    assert_eq!(
        c.turn_status(next.managed_turn_id.clone()).unwrap().state,
        TurnState::Running
    );
    c.close(Close {
        intent_id: id(),
        managed_session_id: other.managed_session_id,
        expected_revision: 1,
    })
    .unwrap();
    assert_eq!(
        wait_turn(&mut c, &next, true).terminal_reason,
        Some(TurnReason::SessionClosed)
    );
    c.runtime_stop(mutation()).unwrap();
    env.wait(&mut c, ActualState::Stopped);
    a.stop(false);
}
#[test]
fn admitted_lease_revocation_and_expiry_interrupt_without_client_cancel() {
    for expire in [false, true] {
        let env = Sandbox::new("turn_long");
        let mut a = env.start();
        let mut c = env.client();
        c.runtime_start(mutation()).unwrap();
        env.wait(&mut c, ActualState::Ready);
        let (s, l) = inference_session(&env, &mut c);
        let lease = if expire {
            c.lease_renew(LeaseRenew {
                intent_id: id(),
                lease_id: l.lease_id.clone(),
                expected_revision: 1,
                ttl_seconds: 2,
            })
            .unwrap()
        } else {
            l
        };
        let t = c.turn_start(turn_params(&s)).unwrap();
        wait_turn(&mut c, &t, false);
        if !expire {
            c.lease_revoke(LeaseRevoke {
                intent_id: id(),
                lease_id: lease.lease_id,
                expected_revision: lease.revision,
            })
            .unwrap();
        }
        let done = wait_turn(&mut c, &t, true);
        assert_eq!(done.state, TurnState::Interrupted);
        assert_eq!(
            done.terminal_reason,
            Some(if expire {
                TurnReason::LeaseExpired
            } else {
                TurnReason::LeaseRevoked
            })
        );
        assert_eq!(count(&env, "turn/interrupt"), 1);
        c.runtime_stop(mutation()).unwrap();
        env.wait(&mut c, ActualState::Stopped);
        a.stop(false);
    }
}
#[test]
fn interrupt_timeout_contains_runtime_and_crash_never_replays_prompt() {
    nix::sys::prctl::set_child_subreaper(true).unwrap();
    for hard in [false, true] {
        let env = Sandbox::new(if hard {
            "turn_long"
        } else {
            "interrupt_timeout"
        });
        let mut a = env.start();
        let mut c = env.client();
        c.runtime_start(mutation()).unwrap();
        env.wait(&mut c, ActualState::Ready);
        let (s, _) = inference_session(&env, &mut c);
        let t = c.turn_start(turn_params(&s)).unwrap();
        let running = wait_turn(&mut c, &t, false);
        if hard {
            a.stop(true);
            for pid in env.pids() {
                dead(pid);
            }
            a = env.start();
            c = env.client();
            env.wait(&mut c, ActualState::Ready);
        } else {
            c.turn_cancel(TurnCancel {
                intent_id: id(),
                managed_turn_id: t.managed_turn_id.clone(),
                expected_revision: running.revision,
            })
            .unwrap();
            env.wait(&mut c, ActualState::Failed);
        }
        assert_eq!(wait_turn(&mut c, &t, true).state, TurnState::Indeterminate);
        assert_eq!(count(&env, "turn/start"), 1);
        assert_eq!(
            c.binding_status(s.managed_session_id.clone())
                .unwrap()
                .state,
            BindingState::BoundDetached
        );
        c.runtime_stop(mutation()).unwrap();
        env.wait(&mut c, ActualState::Stopped);
        a.stop(false);
    }
}
#[test]
fn any_external_surface_is_contained_without_server_responses() {
    for scenario in [
        "isolation_hook",
        "isolation_commandExecution",
        "isolation_fileChange",
        "isolation_mcpToolCall",
        "isolation_webSearch",
        "isolation_collabAgentToolCall",
        "isolation_approval",
        "isolation_permission",
        "isolation_input",
        "isolation_diff",
    ] {
        let env = Sandbox::new(scenario);
        let mut a = env.start();
        let mut c = env.client();
        c.runtime_start(mutation()).unwrap();
        env.wait(&mut c, ActualState::Ready);
        let (s, _) = inference_session(&env, &mut c);
        let t = c.turn_start(turn_params(&s)).unwrap();
        env.wait(&mut c, ActualState::Failed);
        let failed = wait_turn(&mut c, &t, true);
        assert_eq!(failed.state, TurnState::Failed, "{scenario}");
        assert_eq!(
            failed.terminal_reason,
            Some(TurnReason::InferenceIsolationViolation),
            "{scenario}"
        );
        assert_eq!(
            c.binding_status(s.managed_session_id).unwrap().state,
            BindingState::BoundDetached
        );
        assert_eq!(count(&env, "turn/start"), 1);
        assert_eq!(count(&env, "RESPONSE"), 0);
        a.stop(false);
        for pid in env.pids() {
            dead(pid);
        }
    }
}

#[test]
fn safety_preflight_zero_hooks_is_reverified_and_mcp_drift_reconciles_without_work() {
    let env = Sandbox::new("hooks_enabled");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Failed);
    assert_eq!(count(&env, "thread/start"), 0);
    assert_eq!(count(&env, "turn/start"), 0);
    a.stop(false);
    let env = Sandbox::new("mcp_drift");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Ready);
    assert_eq!(env.pids().len(), 4);
    assert_eq!(count(&env, "thread/start"), 0);
    assert!(c.runtime_status().unwrap().runtime_safety_profile.is_some());
    let (s, _) = inference_session(&env, &mut c);
    fs::write(env.root.join("restore-hooks"), b"fixture").unwrap();
    let t = c.turn_start(turn_params(&s)).unwrap();
    env.wait(&mut c, ActualState::Failed);
    assert!(wait_turn(&mut c, &t, true).state.terminal());
    assert_eq!(count(&env, "turn/start"), 0);
    assert_eq!(
        c.binding_status(s.managed_session_id).unwrap().state,
        BindingState::BoundDetached
    );
    a.stop(false);
}
#[test]
fn renewal_extends_running_turn_and_replacement_never_transfers_admission() {
    let env = Sandbox::new("turn_long");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Ready);
    let (s, l) = inference_session(&env, &mut c);
    let short = c
        .lease_renew(LeaseRenew {
            intent_id: id(),
            lease_id: l.lease_id.clone(),
            expected_revision: l.revision,
            ttl_seconds: 2,
        })
        .unwrap();
    let t = c.turn_start(turn_params(&s)).unwrap();
    wait_turn(&mut c, &t, false);
    let extended = c
        .lease_renew(LeaseRenew {
            intent_id: id(),
            lease_id: l.lease_id.clone(),
            expected_revision: short.revision,
            ttl_seconds: 10,
        })
        .unwrap();
    thread::sleep(Duration::from_millis(2100));
    assert_eq!(
        c.turn_status(t.managed_turn_id.clone()).unwrap().state,
        TurnState::Running
    );
    c.lease_revoke(LeaseRevoke {
        intent_id: id(),
        lease_id: l.lease_id,
        expected_revision: extended.revision,
    })
    .unwrap();
    let replacement = c
        .lease_grant(LeaseGrant {
            intent_id: id(),
            session_id: s.managed_session_id.clone(),
            expected_session_revision: 1,
            scopes: vec![ExecutionScope::TurnStart],
            ttl_seconds: 60,
        })
        .unwrap();
    assert_ne!(replacement.lease_id, t.admitted_lease_id);
    assert_eq!(wait_turn(&mut c, &t, true).state, TurnState::Interrupted);
    assert_eq!(count(&env, "turn/start"), 1);
    assert_eq!(count(&env, "turn/interrupt"), 1);
    a.stop(false);
}
#[test]
fn crash_after_prompt_write_before_rpc_response_is_indeterminate_and_never_replayed() {
    nix::sys::prctl::set_child_subreaper(true).unwrap();
    let env = Sandbox::new("turn_timeout");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Ready);
    let (s, _) = inference_session(&env, &mut c);
    let t = c.turn_start(turn_params(&s)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !env.root.join("turn-written").exists() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(5));
    }
    a.stop(true);
    for pid in env.pids() {
        dead(pid);
    }
    let mut a = env.start();
    let mut c = env.client();
    env.wait(&mut c, ActualState::Ready);
    let prior = c.turn_status(t.managed_turn_id).unwrap();
    assert_eq!(prior.state, TurnState::Indeterminate);
    assert!(prior.codex_turn_id.is_none());
    assert_eq!(count(&env, "turn/start"), 1);
    a.stop(false);
}
#[test]
fn graceful_shutdown_and_runtime_stop_never_leave_a_running_turn_or_replay() {
    for stop_runtime in [false, true] {
        let env = Sandbox::new("turn_long");
        let mut a = env.start();
        let mut c = env.client();
        c.runtime_start(mutation()).unwrap();
        env.wait(&mut c, ActualState::Ready);
        let (s, _) = inference_session(&env, &mut c);
        let t = c.turn_start(turn_params(&s)).unwrap();
        wait_turn(&mut c, &t, false);
        if stop_runtime {
            c.runtime_stop(mutation()).unwrap();
            env.wait(&mut c, ActualState::Stopped);
            assert!(wait_turn(&mut c, &t, true).state.terminal());
        }
        a.stop(false);
        for pid in env.pids() {
            dead(pid);
        }
        let mut a = env.start();
        let mut c = env.client();
        let previous = c.turn_status(t.managed_turn_id).unwrap();
        assert!(previous.state.terminal());
        assert_eq!(count(&env, "turn/start"), 1);
        assert_eq!(
            c.binding_status(s.managed_session_id).unwrap().state,
            BindingState::BoundDetached
        );
        a.stop(false);
    }
}

#[test]
fn cancel_waits_for_correlated_started_when_start_response_precedes_provider_activation() {
    let env = Sandbox::new("turn_response_first");
    let mut a = env.start();
    let mut c = env.client();
    c.runtime_start(mutation()).unwrap();
    env.wait(&mut c, ActualState::Ready);
    let (s, _) = inference_session(&env, &mut c);
    let t = c.turn_start(turn_params(&s)).unwrap();
    let running = wait_turn(&mut c, &t, false);
    c.turn_cancel(TurnCancel {
        intent_id: id(),
        managed_turn_id: t.managed_turn_id.clone(),
        expected_revision: running.revision,
    })
    .unwrap();
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(count(&env, "turn/interrupt"), 0);
    assert_eq!(
        c.turn_status(t.managed_turn_id.clone()).unwrap().state,
        TurnState::Running
    );
    fs::write(env.root.join("release-started"), "").unwrap();
    let done = wait_turn(&mut c, &t, true);
    assert_eq!(done.state, TurnState::Interrupted);
    assert_eq!(done.terminal_reason, Some(TurnReason::UserCancel));
    assert_eq!(count(&env, "turn/interrupt"), 1);
    assert_eq!(c.runtime_status().unwrap().actual_state, ActualState::Ready);
    c.runtime_stop(mutation()).unwrap();
    env.wait(&mut c, ActualState::Stopped);
    a.stop(false);
}
